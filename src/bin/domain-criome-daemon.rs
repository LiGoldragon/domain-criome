fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("(DaemonRejected \"{error}\")");
            std::process::exit(2);
        }
    }
}

fn run() -> domain_criome::Result<()> {
    let configuration = nota_config::ConfigurationSource::from_argv()?.decode()?;
    domain_criome::daemon::Daemon::new(configuration).run()
}
