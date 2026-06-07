fn main() {
    match domain_criome::DomainCriomeDaemonCommand::from_environment().run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("(DaemonRejected [{error}])");
            std::process::exit(2);
        }
    }
}
