use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use domain_criome::client::{CliRequest, CommandLineDispatch};
use domain_criome::daemon::Daemon;
use domain_criome::frame_io::OrdinaryFrameIo;
use domain_criome::{
    DaemonConfiguration, DomainCriomeDaemonCommand, DomainCriomeDaemonConfigurationFile, Store,
};
use meta_signal_domain_criome::{
    Operation as MetaOperation, ProjectionDeclaration, ProjectionDirective, ProjectionPolicy,
    Registration,
};
use nota_codec::NotaEncode;
use signal_domain_criome::{
    DomainName, DomainNameSystemRecord, Operation as DomainOperation, Projection, ProjectionQuery,
    ProjectionScope, RecordKind, RecordValue, RejectionReason, Reply as DomainReply,
    ResolutionQuery, ResolutionScope,
};
use signal_frame::{
    CommandLineSocket, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply,
    HandshakeRequest, LaneSequence, Reply as FrameReply, RequestPayload, SessionEpoch, SubReply,
};

fn encode_to_text(value: &impl NotaEncode) -> String {
    let mut encoder = nota_codec::Encoder::new();
    value.encode(&mut encoder).expect("encode");
    encoder.into_string()
}

fn exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn accepted_domain_reply(reply: signal_domain_criome::ChannelReply) -> DomainReply {
    match reply {
        FrameReply::Accepted { per_operation, .. } => {
            let (head, tail) = per_operation.into_head_and_tail();
            assert!(tail.is_empty());
            match head {
                SubReply::Ok(reply) => reply,
                _ => panic!("operation rejected"),
            }
        }
        FrameReply::Rejected { .. } => panic!("request rejected"),
    }
}

fn configure_projection(store: &Store) {
    let register = MetaOperation::RegisterDomain(Registration {
        domain: DomainName::new("goldragon.criome"),
    })
    .into_request();
    assert!(matches!(
        store.handle_owner_request(register),
        FrameReply::Accepted { .. }
    ));

    let policy = MetaOperation::SetPolicy(meta_signal_domain_criome::Policy {
        projections: vec![ProjectionPolicy {
            domain: DomainName::new("goldragon.criome"),
            scope: ProjectionScope::Everything,
            directive: ProjectionDirective::Enable,
        }],
    })
    .into_request();
    assert!(matches!(
        store.handle_owner_request(policy),
        FrameReply::Accepted { .. }
    ));

    let projection = MetaOperation::SetProjection(ProjectionDeclaration {
        domain: DomainName::new("goldragon.criome"),
        records: vec![DomainNameSystemRecord {
            name: DomainName::new("goldragon.criome"),
            kind: RecordKind::AddressV4,
            value: RecordValue::new("203.0.113.10"),
        }],
        redirects: vec![],
    })
    .into_request();
    assert!(matches!(
        store.handle_owner_request(projection),
        FrameReply::Accepted { .. }
    ));
}

#[test]
fn owner_policy_enables_provider_neutral_projection_and_resolution() {
    let store = Store::new();
    configure_projection(&store);

    let reply = accepted_domain_reply(
        store.handle_ordinary_request(
            DomainOperation::Project(ProjectionQuery {
                domain: DomainName::new("goldragon.criome"),
                scope: ProjectionScope::PublicRecords,
            })
            .into_request(),
        ),
    );
    let DomainReply::Projected(Projection { records, .. }) = reply else {
        panic!("expected projection");
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].value.as_str(), "203.0.113.10");

    let reply = accepted_domain_reply(
        store.handle_ordinary_request(
            DomainOperation::Resolve(ResolutionQuery {
                name: DomainName::new("goldragon.criome"),
                scope: ResolutionScope::Public,
            })
            .into_request(),
        ),
    );
    let DomainReply::Resolved(resolution) = reply else {
        panic!("expected resolution");
    };
    assert_eq!(resolution.addresses[0].address.as_str(), "203.0.113.10");
}

#[test]
fn daemon_answers_projection_over_frame_socket() {
    let store = Store::new();
    configure_projection(&store);
    let (mut client_stream, mut daemon_stream) = UnixStream::pair().expect("socket pair");

    thread::spawn(move || {
        Daemon::serve_ordinary_stream(&store, &mut daemon_stream).expect("daemon serves");
    });

    let handshake = signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OrdinaryFrameIo::new(&mut client_stream)
        .write(&handshake)
        .expect("write handshake");
    let handshake_reply = OrdinaryFrameIo::new(&mut client_stream)
        .read()
        .expect("read handshake");
    assert!(matches!(
        handshake_reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let exchange = exchange();
    let request = DomainOperation::Project(ProjectionQuery {
        domain: DomainName::new("goldragon.criome"),
        scope: ProjectionScope::PublicRecords,
    })
    .into_request();
    let frame = signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OrdinaryFrameIo::new(&mut client_stream)
        .write(&frame)
        .expect("write request");
    let reply = OrdinaryFrameIo::new(&mut client_stream)
        .read()
        .expect("read reply");
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply,
        } if reply_exchange == exchange => {
            let DomainReply::Projected(projection) = accepted_domain_reply(reply) else {
                panic!("expected projection");
            };
            assert_eq!(projection.records.len(), 1);
        }
        _ => panic!("unexpected frame"),
    }
}

#[test]
fn daemon_configuration_accepts_binary_file_argument() {
    let directory = tempfile::tempdir().expect("temp dir");
    let configuration_path = directory.path().join("domain-criome-daemon.rkyv");
    let configuration = daemon_configuration(directory.path());

    DomainCriomeDaemonConfigurationFile::new(&configuration_path)
        .write_configuration(&configuration)
        .expect("write domain-criome daemon configuration");

    let decoded =
        DomainCriomeDaemonCommand::from_arguments([configuration_path.display().to_string()])
            .configuration()
            .expect("read domain-criome daemon configuration");

    assert_eq!(decoded, configuration);
}

#[test]
fn daemon_configuration_rejects_nota_arguments() {
    let directory = tempfile::tempdir().expect("temp dir");
    let nota_path = directory.path().join("domain-criome-daemon.nota");
    std::fs::write(&nota_path, "(DaemonConfiguration)").expect("write nota fixture");

    let inline = DomainCriomeDaemonCommand::from_arguments(["(DaemonConfiguration)"])
        .configuration()
        .expect_err("inline NOTA is rejected");
    let file = DomainCriomeDaemonCommand::from_arguments([nota_path.display().to_string()])
        .configuration()
        .expect_err(".nota file is rejected");

    assert!(matches!(inline, domain_criome::Error::Argument(_)));
    assert!(matches!(file, domain_criome::Error::Argument(_)));
}

#[test]
fn daemon_process_starts_from_binary_configuration_and_answers_working_request() {
    let directory = tempfile::tempdir().expect("temp dir");
    let configuration_path = directory.path().join("domain-criome-daemon.rkyv");
    let configuration = daemon_configuration(directory.path());

    DomainCriomeDaemonConfigurationFile::new(&configuration_path)
        .write_configuration(&configuration)
        .expect("write domain-criome daemon configuration");

    let mut child = Command::new(env!("CARGO_BIN_EXE_domain-criome-daemon"))
        .arg(&configuration_path)
        .spawn()
        .expect("domain-criome-daemon starts");

    let ordinary_socket = directory.path().join("domain-criome.sock");
    let owner_socket = directory.path().join("domain-criome-owner.sock");
    wait_for_socket(&ordinary_socket);
    wait_for_socket(&owner_socket);

    let mut stream = UnixStream::connect(&ordinary_socket).expect("client connects");
    let reply = resolution_reply_from_stream(&mut stream, "unknown.criome");
    match reply {
        DomainReply::RequestRejected(rejection) => {
            assert_eq!(rejection.reason, RejectionReason::DomainUnknown);
        }
        other => panic!("expected domain-unknown rejection, got {other:?}"),
    }

    stop_child(&mut child);
}

#[test]
fn command_line_dispatch_routes_working_and_owner_heads() {
    let working = DomainOperation::Project(ProjectionQuery {
        domain: DomainName::new("goldragon.criome"),
        scope: ProjectionScope::PublicRecords,
    });
    let owner = MetaOperation::SetProjection(ProjectionDeclaration {
        domain: DomainName::new("goldragon.criome"),
        records: vec![],
        redirects: vec![],
    });

    assert_eq!(
        CommandLineDispatch::new()
            .route_head("Project")
            .expect("working route"),
        CommandLineSocket::Working
    );
    assert_eq!(
        CommandLineDispatch::new()
            .route_head("SetProjection")
            .expect("owner route"),
        CommandLineSocket::Owner
    );

    assert!(matches!(
        CliRequest::from_nota(&encode_to_text(&working)),
        Ok(CliRequest::Working(_))
    ));
    assert!(matches!(
        CliRequest::from_nota(&encode_to_text(&owner)),
        Ok(CliRequest::Owner(_))
    ));
}

#[test]
fn command_line_request_rejects_flags_and_extra_arguments() {
    assert!(domain_criome::client::CliRequest::from_arguments(["--socket"]).is_err());
    assert!(domain_criome::client::CliRequest::from_arguments(["one", "two"]).is_err());
}

fn resolution_reply_from_stream(stream: &mut UnixStream, name: &str) -> DomainReply {
    let handshake = signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OrdinaryFrameIo::new(&mut *stream)
        .write(&handshake)
        .expect("write handshake");
    let handshake_reply = OrdinaryFrameIo::new(&mut *stream)
        .read()
        .expect("read handshake");
    assert!(matches!(
        handshake_reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let exchange = exchange();
    let request = DomainOperation::Resolve(ResolutionQuery {
        name: DomainName::new(name),
        scope: ResolutionScope::Public,
    })
    .into_request();
    let frame = signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OrdinaryFrameIo::new(&mut *stream)
        .write(&frame)
        .expect("write request");
    let reply = OrdinaryFrameIo::new(&mut *stream)
        .read()
        .expect("read reply");
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply,
        } if reply_exchange == exchange => accepted_domain_reply(reply),
        other => panic!("unexpected frame {other:?}"),
    }
}

fn daemon_configuration(directory: &Path) -> DaemonConfiguration {
    DaemonConfiguration {
        ordinary_socket_path: directory.join("domain-criome.sock").display().to_string(),
        ordinary_socket_mode: 0o600,
        owner_socket_path: directory
            .join("domain-criome-owner.sock")
            .display()
            .to_string(),
        owner_socket_mode: 0o600,
    }
}

fn wait_for_socket(socket: &Path) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if socket.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("socket was not created: {}", socket.display());
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
