use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, Token};
use owner_signal_domain_criome::{ChannelRequest as OwnerRequest, Reply as OwnerReply};
use signal_domain_criome::{Reply as DomainReply, Request as DomainRequest};
use signal_frame::{
    CommandLineSocket, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply,
    HandshakeRequest, LaneSequence, Reply as FrameReply, SessionEpoch, SubReply,
};

use crate::frame_io::{OrdinaryFrameIo, OwnerFrameIo};
use crate::{Error, Result};

const DEFAULT_ORDINARY_SOCKET_PATH: &str = "/run/domain-criome/domain-criome.sock";
const DEFAULT_OWNER_SOCKET_PATH: &str = "/run/domain-criome/domain-criome-owner.sock";
const ORDINARY_SOCKET_ENVIRONMENT_VARIABLE: &str = "DOMAIN_CRIOME_SOCKET_PATH";
const OWNER_SOCKET_ENVIRONMENT_VARIABLE: &str = "DOMAIN_CRIOME_OWNER_SOCKET_PATH";

signal_frame::signal_cli! {
    pub struct CommandLineDispatch {
        working signal_domain_criome::Operation;
        owner owner_signal_domain_criome::Operation;
    }
}

pub struct Client {
    ordinary_socket_path: PathBuf,
    owner_socket_path: PathBuf,
}

impl Client {
    pub fn with_sockets(
        ordinary_socket_path: impl Into<PathBuf>,
        owner_socket_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ordinary_socket_path: ordinary_socket_path.into(),
            owner_socket_path: owner_socket_path.into(),
        }
    }

    pub fn from_environment() -> Self {
        let ordinary_socket_path = std::env::var_os(ORDINARY_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_ORDINARY_SOCKET_PATH));
        let owner_socket_path = std::env::var_os(OWNER_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OWNER_SOCKET_PATH));
        Self::with_sockets(ordinary_socket_path, owner_socket_path)
    }

    pub fn send_working(&self, request: DomainRequest) -> Result<DomainReply> {
        let mut stream = UnixStream::connect(&self.ordinary_socket_path)?;
        self.handshake_working(&mut stream)?;
        let exchange = ExchangeFactory::new().fresh_exchange();
        let frame =
            signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
        OrdinaryFrameIo::new(&mut stream).write(&frame)?;
        stream.flush()?;

        let reply = OrdinaryFrameIo::new(&mut stream).read()?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => ReplyEnvelope::new(reply).unwrap_single_reply(),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn send_owner(&self, request: OwnerRequest) -> Result<OwnerReply> {
        let mut stream = UnixStream::connect(&self.owner_socket_path)?;
        self.handshake_owner(&mut stream)?;
        let exchange = ExchangeFactory::new().fresh_exchange();
        let frame = owner_signal_domain_criome::Frame::new(ExchangeFrameBody::Request {
            exchange,
            request,
        });
        OwnerFrameIo::new(&mut stream).write(&frame)?;
        stream.flush()?;

        let reply = OwnerFrameIo::new(&mut stream).read()?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => OwnerReplyEnvelope::new(reply).unwrap_single_reply(),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn run_from_environment() -> Result<String> {
        let request = CliRequest::from_arguments(std::env::args_os().skip(1))?;
        let client = Self::from_environment();
        match request {
            CliRequest::Working(request) => {
                let reply = client.send_working(request)?;
                ReplyText::new(&reply).encode()
            }
            CliRequest::Owner(request) => {
                let reply = client.send_owner(request)?;
                ReplyText::new(&reply).encode()
            }
        }
    }

    fn handshake_working(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        OrdinaryFrameIo::new(stream).write(&frame)?;
        let reply = OrdinaryFrameIo::new(stream).read()?;
        match reply.into_body() {
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_)) => Ok(()),
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(_)) => {
                Err(Error::HandshakeRejected)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }

    fn handshake_owner(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = owner_signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        OwnerFrameIo::new(stream).write(&frame)?;
        let reply = OwnerFrameIo::new(stream).read()?;
        match reply.into_body() {
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_)) => Ok(()),
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(_)) => {
                Err(Error::HandshakeRejected)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliRequest {
    Working(DomainRequest),
    Owner(OwnerRequest),
}

impl CliRequest {
    pub fn from_arguments<I, S>(arguments: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let arguments: Vec<OsString> = arguments
            .into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        let [argument] = arguments.as_slice() else {
            return Err(Error::ExpectedSingleArgument);
        };
        let text = argument.to_str().ok_or(Error::ExpectedSingleArgument)?;
        if text.starts_with("--") {
            return Err(Error::FlagArgument(text.to_owned()));
        }
        let source = if text.starts_with('(') {
            text.to_owned()
        } else {
            std::fs::read_to_string(PathBuf::from(argument))?
        };
        Self::from_nota(&source)
    }

    pub fn from_nota(text: &str) -> Result<Self> {
        match RequestHead::from_text(text)?.route()? {
            CommandLineSocket::Working => Self::decode_working(text),
            CommandLineSocket::Owner => Self::decode_owner(text),
        }
    }

    fn decode_working(text: &str) -> Result<Self> {
        let mut decoder = Decoder::new(text);
        let payload = DomainRequest::decode(&mut decoder)?;
        RequestEnd::new(&mut decoder).expect()?;
        Ok(Self::Working(payload))
    }

    fn decode_owner(text: &str) -> Result<Self> {
        let mut decoder = Decoder::new(text);
        let payload = OwnerRequest::decode(&mut decoder)?;
        RequestEnd::new(&mut decoder).expect()?;
        Ok(Self::Owner(payload))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    head: String,
}

impl RequestHead {
    pub fn from_text(text: &str) -> Result<Self> {
        let mut decoder = Decoder::new(text);
        if matches!(decoder.peek_token()?, Some(Token::LBracket)) {
            decoder.expect_seq_start()?;
        }
        let head = decoder.peek_record_head()?;
        Ok(Self { head })
    }

    pub fn route(&self) -> Result<CommandLineSocket> {
        CommandLineDispatch::new()
            .route_head(&self.head)
            .map_err(Error::command_line_route)
    }
}

pub struct RequestEnd<'decoder, 'text> {
    decoder: &'decoder mut Decoder<'text>,
}

impl<'decoder, 'text> RequestEnd<'decoder, 'text> {
    pub fn new(decoder: &'decoder mut Decoder<'text>) -> Self {
        Self { decoder }
    }

    pub fn expect(self) -> Result<()> {
        if self.decoder.peek_token()?.is_some() {
            return Err(Error::TrailingInput);
        }
        Ok(())
    }
}

pub struct ExchangeFactory {
    epoch: SessionEpoch,
    lane: ExchangeLane,
}

impl ExchangeFactory {
    pub fn new() -> Self {
        Self {
            epoch: SessionEpoch::new(1),
            lane: ExchangeLane::Connector,
        }
    }

    pub fn fresh_exchange(&self) -> ExchangeIdentifier {
        ExchangeIdentifier::new(self.epoch, self.lane, LaneSequence::first())
    }
}

impl Default for ExchangeFactory {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReplyEnvelope {
    reply: FrameReply<DomainReply>,
}

impl ReplyEnvelope {
    pub fn new(reply: FrameReply<DomainReply>) -> Self {
        Self { reply }
    }

    pub fn unwrap_single_reply(self) -> Result<DomainReply> {
        match self.reply {
            FrameReply::Accepted { per_operation, .. } => {
                match per_operation.into_head_and_tail() {
                    (SubReply::Ok(payload), tail) if tail.is_empty() => Ok(payload),
                    _ => Err(Error::SignalRequestFailed),
                }
            }
            FrameReply::Rejected { .. } => Err(Error::SignalRequestRejected),
        }
    }
}

pub struct OwnerReplyEnvelope {
    reply: FrameReply<OwnerReply>,
}

impl OwnerReplyEnvelope {
    pub fn new(reply: FrameReply<OwnerReply>) -> Self {
        Self { reply }
    }

    pub fn unwrap_single_reply(self) -> Result<OwnerReply> {
        match self.reply {
            FrameReply::Accepted { per_operation, .. } => {
                match per_operation.into_head_and_tail() {
                    (SubReply::Ok(payload), tail) if tail.is_empty() => Ok(payload),
                    _ => Err(Error::SignalRequestFailed),
                }
            }
            FrameReply::Rejected { .. } => Err(Error::SignalRequestRejected),
        }
    }
}

pub struct ReplyText<'reply, Reply> {
    reply: &'reply Reply,
}

impl<'reply, Reply> ReplyText<'reply, Reply>
where
    Reply: NotaEncode,
{
    pub fn new(reply: &'reply Reply) -> Self {
        Self { reply }
    }

    pub fn encode(&self) -> Result<String> {
        let mut encoder = Encoder::new();
        self.reply.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}
