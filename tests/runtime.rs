use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use domain_criome::client::{CliRequest, Client, CommandLineDispatch};
use domain_criome::{
    DaemonConfiguration, DomainCriomeDaemonCommand, DomainCriomeDaemonConfigurationFile, Store,
};
use meta_signal_domain_criome::schema::lib as meta;
use meta_signal_domain_criome::{Operation as MetaOperation, ProjectionDeclaration};
use nota_next::NotaEncode;
use signal_domain_criome::schema::lib as ordinary;
use signal_domain_criome::{
    DomainName, DomainNameSystemRecord, Operation as DomainOperation, ProjectionQuery,
    ProjectionScope, RecordKind, RecordValue,
};
use signal_frame::CommandLineSocket;

fn encode_to_text(value: &impl NotaEncode) -> String {
    value.to_nota()
}

fn configure_projection(store: &Store) {
    let register = store.handle_meta_input(meta::Input::RegisterDomain(meta::Registration::new(
        "goldragon.criome".to_owned(),
    )));
    assert!(matches!(register, meta::Output::DomainRegistered(_)));

    let policy = store.handle_meta_input(meta::Input::SetPolicy(meta::Policy::new(vec![
        meta::ProjectionPolicy {
            domain: "goldragon.criome".to_owned(),
            projection_scope: meta::ProjectionScope::Everything,
            projection_directive: meta::ProjectionDirective::Enable,
        },
    ])));
    assert!(matches!(policy, meta::Output::PolicySet(_)));

    let projection =
        store.handle_meta_input(meta::Input::SetProjection(meta::ProjectionDeclaration {
            domain: "goldragon.criome".to_owned(),
            records: vec![meta::DomainNameSystemRecord {
                name: "goldragon.criome".to_owned(),
                record_kind: meta::RecordKind::AddressV4,
                value: "203.0.113.10".to_owned(),
            }],
            redirects: vec![],
        }));
    assert!(matches!(projection, meta::Output::ProjectionSet(_)));
}

#[test]
fn meta_policy_enables_provider_neutral_projection_and_resolution() {
    let store = Store::new();
    configure_projection(&store);

    let reply = store.handle_ordinary_input(ordinary::Input::Project(ordinary::ProjectionQuery {
        domain: "goldragon.criome".to_owned(),
        projection_scope: ordinary::ProjectionScope::PublicRecords,
    }));
    let ordinary::Output::Projected(projection) = reply else {
        panic!("expected projection");
    };
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records[0].value, "203.0.113.10");

    let reply = store.handle_ordinary_input(ordinary::Input::Resolve(ordinary::ResolutionQuery {
        name: "goldragon.criome".to_owned(),
        resolution_scope: ordinary::ResolutionScope::Public,
    }));
    let ordinary::Output::Resolved(resolution) = reply else {
        panic!("expected resolution");
    };
    assert_eq!(resolution.addresses[0].address, "203.0.113.10");
}

#[test]
fn schema_observe_domains_carries_query_payload() {
    let store = Store::new();
    configure_projection(&store);

    let reply =
        store.handle_ordinary_input(ordinary::Input::Observe(ordinary::Observation::Domains(
            ordinary::DomainQuery::new(Some("goldragon.criome".to_owned())),
        )));

    let ordinary::Output::Observed(ordinary::ObservationResult::Domains(domains)) = reply else {
        panic!("expected domain observation");
    };
    assert_eq!(domains.into_payload(), vec!["goldragon.criome".to_owned()]);
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
fn daemon_process_starts_from_binary_configuration_and_rejects_unknown_resolution() {
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

    wait_for_socket(Path::new(&configuration.ordinary_socket_path));
    wait_for_socket(Path::new(&configuration.meta_socket_path));

    let client = Client::with_sockets(
        configuration.ordinary_socket_path.clone(),
        configuration.meta_socket_path.clone(),
    );
    let reply = client
        .send_working(ordinary::Input::Resolve(ordinary::ResolutionQuery {
            name: "unknown.criome".to_owned(),
            resolution_scope: ordinary::ResolutionScope::Public,
        }))
        .expect("working request succeeds");

    match reply {
        ordinary::Output::RequestRejected(rejection) => {
            assert_eq!(rejection.reason, ordinary::RejectionReason::DomainUnknown);
        }
        other => panic!("expected domain-unknown rejection, got {other:?}"),
    }

    stop_child(&mut child);
}

#[test]
fn daemon_process_accepts_meta_projection_and_answers_working_projection() {
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

    wait_for_socket(Path::new(&configuration.ordinary_socket_path));
    wait_for_socket(Path::new(&configuration.meta_socket_path));

    let client = Client::with_sockets(
        configuration.ordinary_socket_path.clone(),
        configuration.meta_socket_path.clone(),
    );
    assert!(matches!(
        client
            .send_meta(meta::Input::RegisterDomain(meta::Registration::new(
                "goldragon.criome".to_owned(),
            )))
            .expect("register domain"),
        meta::Output::DomainRegistered(_)
    ));
    assert!(matches!(
        client
            .send_meta(meta::Input::SetPolicy(meta::Policy::new(vec![
                meta::ProjectionPolicy {
                    domain: "goldragon.criome".to_owned(),
                    projection_scope: meta::ProjectionScope::Everything,
                    projection_directive: meta::ProjectionDirective::Enable,
                },
            ])))
            .expect("set policy"),
        meta::Output::PolicySet(_)
    ));
    assert!(matches!(
        client
            .send_meta(meta::Input::SetProjection(meta::ProjectionDeclaration {
                domain: "goldragon.criome".to_owned(),
                records: vec![meta::DomainNameSystemRecord {
                    name: "goldragon.criome".to_owned(),
                    record_kind: meta::RecordKind::AddressV4,
                    value: "203.0.113.10".to_owned(),
                }],
                redirects: vec![],
            }))
            .expect("set projection"),
        meta::Output::ProjectionSet(_)
    ));

    let reply = client
        .send_working(ordinary::Input::Project(ordinary::ProjectionQuery {
            domain: "goldragon.criome".to_owned(),
            projection_scope: ordinary::ProjectionScope::PublicRecords,
        }))
        .expect("project domain");

    let ordinary::Output::Projected(projection) = reply else {
        panic!("expected projection");
    };
    assert_eq!(projection.records[0].value, "203.0.113.10");

    stop_child(&mut child);
}

#[test]
fn command_line_dispatch_routes_working_and_meta_heads() {
    let working = DomainOperation::Project(ProjectionQuery {
        domain: DomainName::new("goldragon.criome"),
        scope: ProjectionScope::PublicRecords,
    });
    let meta = MetaOperation::SetProjection(ProjectionDeclaration {
        domain: DomainName::new("goldragon.criome"),
        records: vec![DomainNameSystemRecord {
            name: DomainName::new("goldragon.criome"),
            kind: RecordKind::AddressV4,
            value: RecordValue::new("203.0.113.10"),
        }],
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
            .expect("meta route"),
        CommandLineSocket::Meta
    );

    assert!(matches!(
        CliRequest::from_nota(&encode_to_text(&working)),
        Ok(CliRequest::Working(_))
    ));
    assert!(matches!(
        CliRequest::from_nota(&encode_to_text(&meta)),
        Ok(CliRequest::Meta(_))
    ));
}

#[test]
fn command_line_request_rejects_flags_and_extra_arguments() {
    assert!(domain_criome::client::CliRequest::from_arguments(["--socket"]).is_err());
    assert!(domain_criome::client::CliRequest::from_arguments(["one", "two"]).is_err());
}

fn daemon_configuration(directory: &Path) -> DaemonConfiguration {
    DaemonConfiguration {
        ordinary_socket_path: directory.join("domain-criome.sock").display().to_string(),
        ordinary_socket_mode: 0o600,
        meta_socket_path: directory
            .join("domain-criome-meta.sock")
            .display()
            .to_string(),
        meta_socket_mode: 0o600,
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
