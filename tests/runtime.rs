use std::os::unix::net::UnixStream;
use std::thread;

use domain_criome::Store;
use domain_criome::client::{CliRequest, CommandLineDispatch};
use domain_criome::daemon::Daemon;
use domain_criome::frame_io::OrdinaryFrameIo;
use nota_codec::NotaEncode;
use owner_signal_domain_criome::{
    Operation as OwnerOperation, ProjectionDeclaration, ProjectionDirective, ProjectionPolicy,
    Registration,
};
use signal_domain_criome::{
    DomainName, DomainNameSystemRecord, Operation as DomainOperation, Projection, ProjectionQuery,
    ProjectionScope, RecordKind, RecordValue, Reply as DomainReply, ResolutionQuery,
    ResolutionScope,
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
    let register = OwnerOperation::RegisterDomain(Registration {
        domain: DomainName::new("goldragon.criome"),
    })
    .into_request();
    assert!(matches!(
        store.handle_owner_request(register),
        FrameReply::Accepted { .. }
    ));

    let policy = OwnerOperation::SetPolicy(owner_signal_domain_criome::Policy {
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

    let projection = OwnerOperation::SetProjection(ProjectionDeclaration {
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
fn command_line_dispatch_routes_working_and_owner_heads() {
    let working = DomainOperation::Project(ProjectionQuery {
        domain: DomainName::new("goldragon.criome"),
        scope: ProjectionScope::PublicRecords,
    });
    let owner = OwnerOperation::SetProjection(ProjectionDeclaration {
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
