fn main() {
    match nota_config::ConfigurationSource::from_argv()
        .and_then(|source| source.decode())
        .map_err(domain_criome::Error::from)
        .and_then(|configuration| domain_criome::daemon::Daemon::new(configuration).run())
    {
        Ok(()) => {}
        Err(error) => {
            eprintln!("(DaemonRejected [{error}])");
            std::process::exit(2);
        }
    }
}
