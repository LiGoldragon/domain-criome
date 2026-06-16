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

fn meta_domain(value: &str) -> meta::Domain {
    value.to_owned().into()
}

fn meta_domain_name(value: &str) -> meta::DomainName {
    value.to_owned().into()
}

fn meta_record_value(value: &str) -> meta::RecordValue {
    value.to_owned().into()
}

fn ordinary_domain_name(value: &str) -> ordinary::DomainName {
    value.to_owned().into()
}

fn register_domain_input(domain: &str) -> meta::Input {
    meta::Input::RegisterDomain(meta::Registration::new(meta_domain(domain)))
}

fn set_policy_input(domain: &str) -> meta::Input {
    meta::Input::SetPolicy(meta::Policy::new(vec![meta::ProjectionPolicy {
        domain: meta_domain(domain),
        projection_scope: meta::ProjectionScope::Everything,
        projection_directive: meta::ProjectionDirective::Enable,
    }]))
}

fn set_projection_input(domain: &str, address: &str) -> meta::Input {
    meta::Input::SetProjection(meta::ProjectionDeclaration {
        domain: meta_domain(domain),
        records: vec![meta::DomainNameSystemRecord {
            name: meta_domain_name(domain),
            record_kind: meta::RecordKind::AddressV4,
            value: meta_record_value(address),
        }],
        redirects: vec![],
    })
}

fn project_input(domain: &str) -> ordinary::Input {
    ordinary::Input::Project(ordinary::ProjectionQuery {
        domain: ordinary_domain_name(domain),
        projection_scope: ordinary::ProjectionScope::PublicRecords,
    })
}

fn resolve_input(name: &str) -> ordinary::Input {
    ordinary::Input::Resolve(ordinary::ResolutionQuery {
        name: ordinary_domain_name(name),
        resolution_scope: ordinary::ResolutionScope::Public,
    })
}

fn configure_projection(store: &Store) {
    let register = store.handle_meta_input(register_domain_input("goldragon.criome"));
    assert!(matches!(register, meta::Output::DomainRegistered(_)));

    let policy = store.handle_meta_input(set_policy_input("goldragon.criome"));
    assert!(matches!(policy, meta::Output::PolicySet(_)));

    let projection =
        store.handle_meta_input(set_projection_input("goldragon.criome", "203.0.113.10"));
    assert!(matches!(projection, meta::Output::ProjectionSet(_)));
}

#[test]
fn meta_policy_enables_provider_neutral_projection_and_resolution() {
    let store = Store::new();
    configure_projection(&store);

    let reply = store.handle_ordinary_input(project_input("goldragon.criome"));
    let ordinary::Output::Projected(projection) = reply else {
        panic!("expected projection");
    };
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records[0].value.payload(), "203.0.113.10");

    let reply = store.handle_ordinary_input(resolve_input("goldragon.criome"));
    let ordinary::Output::Resolved(resolution) = reply else {
        panic!("expected resolution");
    };
    assert_eq!(resolution.addresses[0].address.payload(), "203.0.113.10");
}

#[test]
fn schema_observe_domains_carries_query_payload() {
    let store = Store::new();
    configure_projection(&store);

    let reply =
        store.handle_ordinary_input(ordinary::Input::Observe(ordinary::Observation::Domains(
            ordinary::DomainQuery::new(Some(ordinary_domain_name("goldragon.criome"))).into(),
        )));

    let ordinary::Output::Observed(observed) = reply else {
        panic!("expected domain observation");
    };
    let ordinary::ObservationResult::Domains(domains) = observed else {
        panic!("expected domain list");
    };
    let domains = domains
        .into_payload()
        .into_iter()
        .map(|domain| domain.into_payload())
        .collect::<Vec<_>>();
    assert_eq!(domains, vec!["goldragon.criome".to_owned()]);
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
        .send_working(resolve_input("unknown.criome"))
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
            .send_meta(register_domain_input("goldragon.criome"))
            .expect("register domain"),
        meta::Output::DomainRegistered(_)
    ));
    assert!(matches!(
        client
            .send_meta(set_policy_input("goldragon.criome"))
            .expect("set policy"),
        meta::Output::PolicySet(_)
    ));
    assert!(matches!(
        client
            .send_meta(set_projection_input("goldragon.criome", "203.0.113.10"))
            .expect("set projection"),
        meta::Output::ProjectionSet(_)
    ));

    let reply = client
        .send_working(project_input("goldragon.criome"))
        .expect("project domain");

    let ordinary::Output::Projected(projection) = reply else {
        panic!("expected projection");
    };
    assert_eq!(projection.records[0].value.payload(), "203.0.113.10");

    stop_child(&mut child);
}

#[test]
fn daemon_process_recovers_meta_projection_after_restart() {
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
            .send_meta(register_domain_input("goldragon.criome"))
            .expect("register domain"),
        meta::Output::DomainRegistered(_)
    ));
    assert!(matches!(
        client
            .send_meta(set_policy_input("goldragon.criome"))
            .expect("set policy"),
        meta::Output::PolicySet(_)
    ));
    assert!(matches!(
        client
            .send_meta(set_projection_input("goldragon.criome", "203.0.113.20"))
            .expect("set projection"),
        meta::Output::ProjectionSet(_)
    ));

    stop_child(&mut child);
    remove_socket(Path::new(&configuration.ordinary_socket_path));
    remove_socket(Path::new(&configuration.meta_socket_path));

    let mut restarted = Command::new(env!("CARGO_BIN_EXE_domain-criome-daemon"))
        .arg(&configuration_path)
        .spawn()
        .expect("domain-criome-daemon restarts");

    wait_for_socket(Path::new(&configuration.ordinary_socket_path));
    wait_for_socket(Path::new(&configuration.meta_socket_path));

    let client = Client::with_sockets(
        configuration.ordinary_socket_path.clone(),
        configuration.meta_socket_path.clone(),
    );
    let reply = client
        .send_working(project_input("goldragon.criome"))
        .expect("project persisted domain");

    let ordinary::Output::Projected(projection) = reply else {
        panic!("expected persisted projection");
    };
    assert_eq!(projection.records[0].value.payload(), "203.0.113.20");

    stop_child(&mut restarted);
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
        database_path: directory.join("domain-criome.sema").display().to_string(),
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

fn remove_socket(socket: &Path) {
    let _ = std::fs::remove_file(socket);
}
