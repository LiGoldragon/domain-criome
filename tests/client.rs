use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::Arc;
use std::thread;

use domain_criome::Store;
use domain_criome::client::CliRequest;
use domain_criome::daemon::Daemon;
use owner_signal_domain_criome::{Operation as OwnerOperation, Registration};
use signal_domain_criome::{DomainName, Operation as DomainOperation};
use signal_frame::RequestPayload;

#[test]
fn cli_rejects_flags_and_multiple_arguments() {
    let flag = CliRequest::from_arguments(["--help"]).unwrap_err();
    assert!(matches!(flag, domain_criome::Error::FlagArgument(argument) if argument == "--help"));

    let multiple = CliRequest::from_arguments([
        "(Resolve ([goldragon.criome] Public))",
        "(Resolve ([www.goldragon.criome] Public))",
    ])
    .unwrap_err();
    assert!(matches!(
        multiple,
        domain_criome::Error::ExpectedSingleArgument
    ));
}

#[test]
fn cli_request_routes_working_and_owner_records() {
    let working = CliRequest::from_nota("(Resolve ([goldragon.criome] Public))").unwrap();
    assert!(matches!(working, CliRequest::Working(_)));

    let owner = CliRequest::from_nota("(RegisterDomain ([goldragon.criome]))").unwrap();
    assert!(matches!(owner, CliRequest::Owner(_)));
}

#[test]
fn cli_binary_routes_working_request_to_daemon_socket() {
    let temporary = tempfile::tempdir().unwrap();
    let ordinary_socket = temporary.path().join("ordinary.sock");
    let owner_socket = temporary.path().join("owner.sock");
    let listener = UnixListener::bind(&ordinary_socket).unwrap();
    let store = Arc::new(Store::new());
    store.handle_owner_request(
        OwnerOperation::RegisterDomain(Registration {
            domain: DomainName::new("goldragon.criome"),
        })
        .into_request(),
    );

    let server_store = Arc::clone(&store);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        Daemon::serve_ordinary_stream(&server_store, &mut stream).unwrap();
    });

    let output = Command::new(env!("CARGO_BIN_EXE_domain-criome"))
        .env("DOMAIN_CRIOME_SOCKET_PATH", &ordinary_socket)
        .env("DOMAIN_CRIOME_OWNER_SOCKET_PATH", &owner_socket)
        .arg("(Resolve ([goldragon.criome] Public))")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("NoRecords"), "{stdout}");
    server.join().unwrap();
}

#[test]
fn cli_binary_routes_owner_request_to_daemon_socket() {
    let temporary = tempfile::tempdir().unwrap();
    let ordinary_socket = temporary.path().join("ordinary.sock");
    let owner_socket = temporary.path().join("owner.sock");
    let listener = UnixListener::bind(&owner_socket).unwrap();
    let store = Arc::new(Store::new());

    let server_store = Arc::clone(&store);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        Daemon::serve_owner_stream(&server_store, &mut stream).unwrap();
    });

    let output = Command::new(env!("CARGO_BIN_EXE_domain-criome"))
        .env("DOMAIN_CRIOME_SOCKET_PATH", &ordinary_socket)
        .env("DOMAIN_CRIOME_OWNER_SOCKET_PATH", &owner_socket)
        .arg("(RegisterDomain ([goldragon.criome]))")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("DomainRegistered"), "{stdout}");
    server.join().unwrap();
}

#[test]
fn cli_request_decodes_sequence_as_working_batch() {
    let request = CliRequest::from_nota(
        "[(Resolve ([goldragon.criome] Public)) (Project ([goldragon.criome] PublicRecords))]",
    )
    .unwrap();
    let CliRequest::Working(request) = request else {
        panic!("expected working request");
    };
    assert_eq!(request.payloads().len(), 2);
    assert!(matches!(
        request.payloads().head(),
        DomainOperation::Resolve(_)
    ));
}
