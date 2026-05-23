use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, Token};
use owner_signal_domain_criome::{ChannelRequest as OwnerRequest, Reply as OwnerReply};
use signal_domain_criome::{ChannelRequest as DomainRequest, Reply as DomainReply};
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
        let exchange = fresh_exchange();
        let frame =
            signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
        OrdinaryFrameIo::write(&mut stream, &frame)?;
        stream.flush()?;

        let reply = OrdinaryFrameIo::read(&mut stream)?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => Self::unwrap_single_reply(reply),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn send_owner(&self, request: OwnerRequest) -> Result<OwnerReply> {
        let mut stream = UnixStream::connect(&self.owner_socket_path)?;
        self.handshake_owner(&mut stream)?;
        let exchange = fresh_exchange();
        let frame = owner_signal_domain_criome::Frame::new(ExchangeFrameBody::Request {
            exchange,
            request,
        });
        OwnerFrameIo::write(&mut stream, &frame)?;
        stream.flush()?;

        let reply = OwnerFrameIo::read(&mut stream)?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => Self::unwrap_single_owner_reply(reply),
            _ => Err(Error::UnexpectedFrame),
        }
    }

    pub fn run_from_environment() -> Result<String> {
        let request = CliRequest::from_arguments(std::env::args_os().skip(1))?;
        let client = Self::from_environment();
        match request {
            CliRequest::Working(request) => encode_reply(&client.send_working(request)?),
            CliRequest::Owner(request) => encode_reply(&client.send_owner(request)?),
        }
    }

    fn handshake_working(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        OrdinaryFrameIo::write(stream, &frame)?;
        let reply = OrdinaryFrameIo::read(stream)?;
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
        OwnerFrameIo::write(stream, &frame)?;
        let reply = OwnerFrameIo::read(stream)?;
        match reply.into_body() {
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_)) => Ok(()),
            ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(_)) => {
                Err(Error::HandshakeRejected)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }

    fn unwrap_single_reply(reply: FrameReply<DomainReply>) -> Result<DomainReply> {
        match reply {
            FrameReply::Accepted { per_operation, .. } => {
                match per_operation.into_head_and_tail() {
                    (SubReply::Ok(payload), tail) if tail.is_empty() => Ok(payload),
                    _ => Err(Error::SignalRequestFailed),
                }
            }
            FrameReply::Rejected { .. } => Err(Error::SignalRequestRejected),
        }
    }

    fn unwrap_single_owner_reply(reply: FrameReply<OwnerReply>) -> Result<OwnerReply> {
        match reply {
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
        let source = if text.trim_start().starts_with('(') || text.trim_start().starts_with('[') {
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

struct RequestEnd<'decoder, 'text> {
    decoder: &'decoder mut Decoder<'text>,
}

impl<'decoder, 'text> RequestEnd<'decoder, 'text> {
    fn new(decoder: &'decoder mut Decoder<'text>) -> Self {
        Self { decoder }
    }

    fn expect(self) -> Result<()> {
        if self.decoder.peek_token()?.is_some() {
            return Err(Error::TrailingInput);
        }
        Ok(())
    }
}

fn fresh_exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn encode_reply(reply: &impl NotaEncode) -> Result<String> {
    let mut encoder = Encoder::new();
    reply.encode(&mut encoder)?;
    Ok(encoder.into_string())
}
