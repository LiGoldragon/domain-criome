use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use signal_frame::ExchangeFrameBody;

use crate::frame_io::{OrdinaryFrameIo, OwnerFrameIo, handshake_reply_for};
use crate::{DaemonConfiguration, Error, Result, Store};

pub struct Daemon {
    configuration: DaemonConfiguration,
}

impl Daemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> Result<()> {
        let store = Arc::new(Store::new());
        let ordinary_listener = Self::bind_socket(
            &self.configuration.ordinary_socket_path,
            self.configuration.ordinary_socket_mode,
        )?;
        let owner_listener = Self::bind_socket(
            &self.configuration.owner_socket_path,
            self.configuration.owner_socket_mode,
        )?;

        let ordinary_store = Arc::clone(&store);
        thread::spawn(move || Self::run_ordinary_listener(ordinary_listener, ordinary_store));

        let owner_store = Arc::clone(&store);
        thread::spawn(move || Self::run_owner_listener(owner_listener, owner_store));

        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    pub fn serve_ordinary_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        loop {
            let frame = OrdinaryFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = signal_domain_criome::Frame::new(
                        signal_domain_criome::FrameBody::HandshakeReply(handshake_reply_for(
                            request.version(),
                        )),
                    );
                    OrdinaryFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = store.handle_ordinary_request(request);
                    let frame =
                        signal_domain_criome::Frame::new(signal_domain_criome::FrameBody::Reply {
                            exchange,
                            reply,
                        });
                    OrdinaryFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    pub fn serve_owner_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        loop {
            let frame = OwnerFrameIo::read(stream)?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = owner_signal_domain_criome::Frame::new(
                        owner_signal_domain_criome::FrameBody::HandshakeReply(handshake_reply_for(
                            request.version(),
                        )),
                    );
                    OwnerFrameIo::write(stream, &reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = store.handle_owner_request(request);
                    let frame = owner_signal_domain_criome::Frame::new(
                        owner_signal_domain_criome::FrameBody::Reply { exchange, reply },
                    );
                    OwnerFrameIo::write(stream, &frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    fn run_ordinary_listener(listener: UnixListener, store: Arc<Store>) {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) = Self::serve_ordinary_stream(&store, &mut stream) {
                        eprintln!("(OrdinarySocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(OrdinaryAcceptError \"{error}\")"),
            }
        }
    }

    fn run_owner_listener(listener: UnixListener, store: Arc<Store>) {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) = Self::serve_owner_stream(&store, &mut stream) {
                        eprintln!("(OwnerSocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(OwnerAcceptError \"{error}\")"),
            }
        }
    }

    fn bind_socket(path: impl AsRef<Path>, mode: u32) -> Result<UnixListener> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_socket() {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("refusing to replace non-socket path {}", path.display()),
                )));
            }
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(listener)
    }
}
