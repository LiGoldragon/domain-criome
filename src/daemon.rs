use crate::DaemonConfiguration;
use crate::schema::daemon::{DaemonBinder, DaemonError};
use crate::schema_daemon::DomainCriomeDaemon;

pub struct Daemon {
    configuration: DaemonConfiguration,
}

impl Daemon {
    pub fn new(configuration: DaemonConfiguration) -> Self {
        Self { configuration }
    }

    pub fn run(self) -> std::result::Result<(), DaemonError<DomainCriomeDaemon>> {
        tokio::runtime::Runtime::new()
            .map_err(DaemonError::Runtime)?
            .block_on(async {
                DomainCriomeDaemon::bind(self.configuration)?
                    .run()
                    .await
                    .map_err(DaemonError::from)
            })
    }
}
