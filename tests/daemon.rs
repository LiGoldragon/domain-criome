use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;

use domain_criome::Store;
use domain_criome::daemon::Daemon;
use domain_criome::frame_io::{OrdinaryFrameIo, OwnerFrameIo};
use owner_signal_domain_criome::{Operation as OwnerOperation, Registration, Reply as OwnerReply};
use signal_domain_criome::{
    DomainName, Operation as DomainOperation, Reply as DomainReply, ResolutionQuery,
    ResolutionScope,
};
use signal_frame::{
    ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, HandshakeReply, HandshakeRequest,
    LaneSequence, Reply as FrameReply, RequestPayload, SessionEpoch, SubReply,
};

#[test]
fn ordinary_daemon_handshake_accepts_current_protocol() {
    let store = Arc::new(Store::new());
    store.handle_owner_request(
        OwnerOperation::RegisterDomain(Registration {
            domain: DomainName::new("goldragon.criome"),
        })
        .into_request(),
    );

    let (mut client_stream, mut daemon_stream) = UnixStream::pair().unwrap();
    let server_store = Arc::clone(&store);
    let server = thread::spawn(move || {
        Daemon::serve_ordinary_stream(&server_store, &mut daemon_stream).unwrap();
    });

    let handshake = signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OrdinaryFrameIo::write(&mut client_stream, &handshake).unwrap();
    let reply = OrdinaryFrameIo::read(&mut client_stream).unwrap();
    assert!(matches!(
        reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let exchange = exchange();
    let request = DomainOperation::Resolve(ResolutionQuery {
        name: DomainName::new("goldragon.criome"),
        scope: ResolutionScope::Public,
    })
    .into_request();
    let frame = signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OrdinaryFrameIo::write(&mut client_stream, &frame).unwrap();
    let reply = OrdinaryFrameIo::read(&mut client_stream).unwrap();
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply,
        } => {
            assert_eq!(reply_exchange, exchange);
            assert!(matches!(
                single_domain_reply(reply),
                DomainReply::NoRecords(_)
            ));
        }
        other => panic!("unexpected frame: {other:?}"),
    }

    server.join().unwrap();
}

#[test]
fn owner_request_handling_works_through_unix_stream_pair() {
    let store = Arc::new(Store::new());
    let (mut client_stream, mut daemon_stream) = UnixStream::pair().unwrap();
    let server_store = Arc::clone(&store);
    let server = thread::spawn(move || {
        Daemon::serve_owner_stream(&server_store, &mut daemon_stream).unwrap();
    });

    let handshake = owner_signal_domain_criome::Frame::new(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));
    OwnerFrameIo::write(&mut client_stream, &handshake).unwrap();
    let reply = OwnerFrameIo::read(&mut client_stream).unwrap();
    assert!(matches!(
        reply.into_body(),
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Accepted(_))
    ));

    let exchange = exchange();
    let request = OwnerOperation::RegisterDomain(Registration {
        domain: DomainName::new("goldragon.criome"),
    })
    .into_request();
    let frame =
        owner_signal_domain_criome::Frame::new(ExchangeFrameBody::Request { exchange, request });
    OwnerFrameIo::write(&mut client_stream, &frame).unwrap();
    let reply = OwnerFrameIo::read(&mut client_stream).unwrap();
    match reply.into_body() {
        ExchangeFrameBody::Reply {
            exchange: reply_exchange,
            reply,
        } => {
            assert_eq!(reply_exchange, exchange);
            assert!(matches!(
                single_owner_reply(reply),
                OwnerReply::DomainRegistered(_)
            ));
        }
        other => panic!("unexpected frame: {other:?}"),
    }

    server.join().unwrap();
}

fn exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn single_domain_reply(reply: signal_domain_criome::ChannelReply) -> DomainReply {
    match reply {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail() {
            (SubReply::Ok(reply), tail) if tail.is_empty() => reply,
            other => panic!("unexpected subreply: {other:?}"),
        },
        other => panic!("unexpected reply: {other:?}"),
    }
}

fn single_owner_reply(reply: owner_signal_domain_criome::ChannelReply) -> OwnerReply {
    match reply {
        FrameReply::Accepted { per_operation, .. } => match per_operation.into_head_and_tail() {
            (SubReply::Ok(reply), tail) if tail.is_empty() => reply,
            other => panic!("unexpected subreply: {other:?}"),
        },
        other => panic!("unexpected reply: {other:?}"),
    }
}
