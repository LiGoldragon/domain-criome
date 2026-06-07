use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use meta_signal_domain_criome::{ChannelRequest as MetaRequest, Reply as MetaReply};
use nota_next::{NotaEncode, NotaSource};
use signal_domain_criome::{Reply as DomainReply, Request as DomainRequest};
use signal_frame::{
    CommandLineSocket, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply,
    HandshakeRequest, LaneSequence, Reply as FrameReply, SessionEpoch, SubReply,
};

use crate::frame_io::{MetaFrameIo, OrdinaryFrameIo};
use crate::{Error, Result};

const DEFAULT_ORDINARY_SOCKET_PATH: &str = "/run/domain-criome/domain-criome.sock";
const DEFAULT_META_SOCKET_PATH: &str = "/run/domain-criome/domain-criome-meta.sock";
const ORDINARY_SOCKET_ENVIRONMENT_VARIABLE: &str = "DOMAIN_CRIOME_SOCKET_PATH";
const META_SOCKET_ENVIRONMENT_VARIABLE: &str = "DOMAIN_CRIOME_META_SOCKET_PATH";

signal_frame::signal_cli! {
    pub struct CommandLineDispatch {
        working signal_domain_criome::Operation;
        meta meta_signal_domain_criome::Operation;
    }
}

pub struct Client {
    ordinary_socket_path: PathBuf,
    meta_socket_path: PathBuf,
}

impl Client {
    pub fn with_sockets(
        ordinary_socket_path: impl Into<PathBuf>,
        meta_socket_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ordinary_socket_path: ordinary_socket_path.into(),
            meta_socket_path: meta_socket_path.into(),
        }
    }

    pub fn from_environment() -> Self {
        let ordinary_socket_path = std::env::var_os(ORDINARY_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_ORDINARY_SOCKET_PATH));
        let meta_socket_path = std::env::var_os(META_SOCKET_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_META_SOCKET_PATH));
        Self::with_sockets(ordinary_socket_path, meta_socket_path)
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

    pub fn send_meta(&self, request: MetaRequest) -> Result<MetaReply> {
        let mut stream = UnixStream::connect(&self.meta_socket_path)?;
        self.handshake_meta(&mut stream)?;
        let exchange = ExchangeFactory::new().fresh_exchange();
        let frame =
            meta_signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
        MetaFrameIo::new(&mut stream).write(&frame)?;
        stream.flush()?;

        let reply = MetaFrameIo::new(&mut stream).read()?;
        match reply.into_body() {
            ExchangeFrameBody::Reply {
                exchange: reply_exchange,
                reply,
            } if reply_exchange == exchange => MetaReplyEnvelope::new(reply).unwrap_single_reply(),
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
            CliRequest::Meta(request) => {
                let reply = client.send_meta(request)?;
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

    fn handshake_meta(&self, stream: &mut UnixStream) -> Result<()> {
        let frame = meta_signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
            HandshakeRequest::current(),
        ));
        MetaFrameIo::new(stream).write(&frame)?;
        let reply = MetaFrameIo::new(stream).read()?;
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
    Meta(MetaRequest),
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
        let trimmed = text.trim_start();
        let source = if trimmed.starts_with('(') || trimmed.starts_with('[') {
            text.to_owned()
        } else {
            std::fs::read_to_string(PathBuf::from(argument))?
        };
        Self::from_nota(&source)
    }

    pub fn from_nota(text: &str) -> Result<Self> {
        match signal_frame::RequestHead::from_text(text)?
            .route::<signal_domain_criome::Operation, meta_signal_domain_criome::Operation>()?
        {
            CommandLineSocket::Working => Self::decode_working(text),
            CommandLineSocket::Meta => Self::decode_meta(text),
        }
    }

    fn decode_working(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<DomainRequest>()?;
        Ok(Self::Working(payload))
    }

    fn decode_meta(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<MetaRequest>()?;
        Ok(Self::Meta(payload))
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

pub struct MetaReplyEnvelope {
    reply: FrameReply<MetaReply>,
}

impl MetaReplyEnvelope {
    pub fn new(reply: FrameReply<MetaReply>) -> Self {
        Self { reply }
    }

    pub fn unwrap_single_reply(self) -> Result<MetaReply> {
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
        Ok(self.reply.to_nota())
    }
}
