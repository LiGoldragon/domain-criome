use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use signal_frame::ExchangeFrameBody;

use crate::frame_io::{HandshakeCompatibility, MetaFrameIo, OrdinaryFrameIo};
use crate::{DaemonConfiguration, Error, Result, Store};

pub struct Daemon {
    configuration: DaemonConfiguration,
}

impl Daemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> Result<()> {
        let store = Arc::new(Mutex::new(Store::new()));
        let ordinary_listener = SocketBinding::new(
            &self.configuration.ordinary_socket_path,
            self.configuration.ordinary_socket_mode,
        )
        .bind()?;
        let owner_listener = SocketBinding::new(
            &self.configuration.owner_socket_path,
            self.configuration.owner_socket_mode,
        )
        .bind()?;

        let ordinary_store = Arc::clone(&store);
        thread::spawn(move || ListenerRuntime::new(ordinary_listener).run_ordinary(ordinary_store));

        let owner_store = Arc::clone(&store);
        thread::spawn(move || ListenerRuntime::new(owner_listener).run_owner(owner_store));

        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    pub fn serve_ordinary_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        OrdinaryStreamRuntime::new(store, stream).serve()
    }

    pub fn serve_owner_stream(store: &Store, stream: &mut UnixStream) -> Result<()> {
        OwnerStreamRuntime::new(store, stream).serve()
    }
}

pub struct ListenerRuntime {
    listener: UnixListener,
}

impl ListenerRuntime {
    pub fn new(listener: UnixListener) -> Self {
        Self { listener }
    }

    pub fn run_ordinary(self, store: Arc<Mutex<Store>>) {
        for stream in self.listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) =
                        SharedStreamRuntime::new(&store, &mut stream).serve_ordinary()
                    {
                        eprintln!("(OrdinarySocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(OrdinaryAcceptError \"{error}\")"),
            }
        }
    }

    pub fn run_owner(self, store: Arc<Mutex<Store>>) {
        for stream in self.listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    if let Err(error) = SharedStreamRuntime::new(&store, &mut stream).serve_owner()
                    {
                        eprintln!("(OwnerSocketError \"{error}\")");
                    }
                }
                Err(error) => eprintln!("(OwnerAcceptError \"{error}\")"),
            }
        }
    }
}

pub struct OrdinaryStreamRuntime<'store, 'stream> {
    store: &'store Store,
    stream: &'stream mut UnixStream,
}

impl<'store, 'stream> OrdinaryStreamRuntime<'store, 'stream> {
    pub fn new(store: &'store Store, stream: &'stream mut UnixStream) -> Self {
        Self { store, stream }
    }

    pub fn serve(&mut self) -> Result<()> {
        loop {
            let frame = OrdinaryFrameIo::new(self.stream).read()?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = signal_domain_criome::Frame::new(
                        signal_domain_criome::FrameBody::HandshakeReply(
                            HandshakeCompatibility::current().reply_for(request.version()),
                        ),
                    );
                    OrdinaryFrameIo::new(self.stream).write(&reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = self.store.handle_ordinary_request(request);
                    let frame =
                        signal_domain_criome::Frame::new(signal_domain_criome::FrameBody::Reply {
                            exchange,
                            reply,
                        });
                    OrdinaryFrameIo::new(self.stream).write(&frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }
}

pub struct OwnerStreamRuntime<'store, 'stream> {
    store: &'store Store,
    stream: &'stream mut UnixStream,
}

impl<'store, 'stream> OwnerStreamRuntime<'store, 'stream> {
    pub fn new(store: &'store Store, stream: &'stream mut UnixStream) -> Self {
        Self { store, stream }
    }

    pub fn serve(&mut self) -> Result<()> {
        loop {
            let frame = MetaFrameIo::new(self.stream).read()?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = meta_signal_domain_criome::Frame::new(
                        meta_signal_domain_criome::FrameBody::HandshakeReply(
                            HandshakeCompatibility::current().reply_for(request.version()),
                        ),
                    );
                    MetaFrameIo::new(self.stream).write(&reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = self.store.handle_owner_request(request);
                    let frame = meta_signal_domain_criome::Frame::new(
                        meta_signal_domain_criome::FrameBody::Reply { exchange, reply },
                    );
                    MetaFrameIo::new(self.stream).write(&frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }
}

pub struct SharedStreamRuntime<'store, 'stream> {
    store: &'store Arc<Mutex<Store>>,
    stream: &'stream mut UnixStream,
}

impl<'store, 'stream> SharedStreamRuntime<'store, 'stream> {
    pub fn new(store: &'store Arc<Mutex<Store>>, stream: &'stream mut UnixStream) -> Self {
        Self { store, stream }
    }

    pub fn serve_ordinary(&mut self) -> Result<()> {
        loop {
            let frame = OrdinaryFrameIo::new(self.stream).read()?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = signal_domain_criome::Frame::new(
                        signal_domain_criome::FrameBody::HandshakeReply(
                            HandshakeCompatibility::current().reply_for(request.version()),
                        ),
                    );
                    OrdinaryFrameIo::new(self.stream).write(&reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = {
                        let store = self.store.lock().map_err(|_| Error::StorePoisoned)?;
                        store.handle_ordinary_request(request)
                    };
                    let frame =
                        signal_domain_criome::Frame::new(signal_domain_criome::FrameBody::Reply {
                            exchange,
                            reply,
                        });
                    OrdinaryFrameIo::new(self.stream).write(&frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }

    pub fn serve_owner(&mut self) -> Result<()> {
        loop {
            let frame = MetaFrameIo::new(self.stream).read()?;
            match frame.into_body() {
                ExchangeFrameBody::HandshakeRequest(request) => {
                    let reply = meta_signal_domain_criome::Frame::new(
                        meta_signal_domain_criome::FrameBody::HandshakeReply(
                            HandshakeCompatibility::current().reply_for(request.version()),
                        ),
                    );
                    MetaFrameIo::new(self.stream).write(&reply)?;
                }
                ExchangeFrameBody::Request { exchange, request } => {
                    let reply = {
                        let store = self.store.lock().map_err(|_| Error::StorePoisoned)?;
                        store.handle_owner_request(request)
                    };
                    let frame = meta_signal_domain_criome::Frame::new(
                        meta_signal_domain_criome::FrameBody::Reply { exchange, reply },
                    );
                    MetaFrameIo::new(self.stream).write(&frame)?;
                    return Ok(());
                }
                _ => return Err(Error::UnexpectedFrame),
            }
        }
    }
}

pub struct SocketBinding<'path> {
    path: &'path Path,
    mode: u32,
}

impl<'path> SocketBinding<'path> {
    pub fn new(path: &'path impl AsRef<Path>, mode: u32) -> Self {
        Self {
            path: path.as_ref(),
            mode,
        }
    }

    pub fn bind(&self) -> Result<UnixListener> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        if self.path.exists() {
            let metadata = fs::symlink_metadata(self.path)?;
            if !metadata.file_type().is_socket() {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "refusing to replace non-socket path {}",
                        self.path.display()
                    ),
                )));
            }
            fs::remove_file(self.path)?;
        }
        let listener = UnixListener::bind(self.path)?;
        fs::set_permissions(self.path, fs::Permissions::from_mode(self.mode))?;
        Ok(listener)
    }
}
