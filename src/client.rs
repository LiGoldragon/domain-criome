use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use meta_signal_domain_criome::Operation as MetaOperation;
use meta_signal_domain_criome::schema::lib as meta;
use nota_next::NotaSource;
use signal_domain_criome::Operation as DomainOperation;
use signal_domain_criome::schema::lib as ordinary;
use signal_frame::CommandLineSocket;
use triad_runtime::{FrameBody, LengthPrefixedCodec};

use crate::schema_bridge::{SchemaMetaInput, SchemaOrdinaryInput};
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

    pub fn send_working(&self, input: ordinary::Input) -> Result<ordinary::Output> {
        let mut stream = UnixStream::connect(&self.ordinary_socket_path)?;
        SchemaConnection::new(&mut stream).exchange_working(input)
    }

    pub fn send_meta(&self, input: meta::Input) -> Result<meta::Output> {
        let mut stream = UnixStream::connect(&self.meta_socket_path)?;
        SchemaConnection::new(&mut stream).exchange_meta(input)
    }

    pub fn run_from_environment() -> Result<String> {
        let request = CliRequest::from_arguments(std::env::args_os().skip(1))?;
        let client = Self::from_environment();
        match request {
            CliRequest::Working(request) => Ok(format!("{:?}", client.send_working(request)?)),
            CliRequest::Meta(request) => Ok(format!("{:?}", client.send_meta(request)?)),
        }
    }
}

pub struct SchemaConnection<'stream> {
    stream: &'stream mut UnixStream,
}

impl<'stream> SchemaConnection<'stream> {
    pub fn new(stream: &'stream mut UnixStream) -> Self {
        Self { stream }
    }

    pub fn exchange_working(&mut self, input: ordinary::Input) -> Result<ordinary::Output> {
        let codec = LengthPrefixedCodec::default();
        codec.write_body(self.stream, &FrameBody::new(input.encode_signal_frame()?))?;
        self.stream.flush()?;
        let body = codec.read_body(self.stream)?;
        let (_route, output) = ordinary::Output::decode_signal_frame(body.bytes())?;
        Ok(output)
    }

    pub fn exchange_meta(&mut self, input: meta::Input) -> Result<meta::Output> {
        let codec = LengthPrefixedCodec::default();
        codec.write_body(self.stream, &FrameBody::new(input.encode_signal_frame()?))?;
        self.stream.flush()?;
        let body = codec.read_body(self.stream)?;
        let (_route, output) = meta::Output::decode_signal_frame(body.bytes())?;
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliRequest {
    Working(ordinary::Input),
    Meta(meta::Input),
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
            .route::<DomainOperation, MetaOperation>()?
        {
            CommandLineSocket::Working => Self::decode_working(text),
            CommandLineSocket::Meta => Self::decode_meta(text),
        }
    }

    fn decode_working(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<DomainOperation>()?;
        Ok(Self::Working(
            SchemaOrdinaryInput::from_operation(payload).into_input(),
        ))
    }

    fn decode_meta(text: &str) -> Result<Self> {
        let payload = NotaSource::new(text).parse::<MetaOperation>()?;
        Ok(Self::Meta(
            SchemaMetaInput::from_operation(payload).into_input(),
        ))
    }
}
