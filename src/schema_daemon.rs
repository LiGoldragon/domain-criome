use std::sync::Arc;
use std::time::Duration;

use meta_signal_domain_criome::schema::lib as meta;
use signal_domain_criome::schema::lib::{Input, Output};
use tokio::io::AsyncWriteExt;
use triad_runtime::{
    AcceptedConnection, ConnectionContext, FrameBody, LengthPrefixedCodec, MaximumFrameLength,
};

use crate::schema::daemon::ComponentDaemon;
use crate::{DaemonConfiguration, Error, Result, Store};

const MAXIMUM_REQUEST_FRAME_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub struct DomainCriomeDaemon;

impl ComponentDaemon for DomainCriomeDaemon {
    type Configuration = DaemonConfiguration;
    type ConfigurationError = Error;
    type Engine = Arc<Store>;
    type Error = Error;

    const PROCESS_NAME: &'static str = "domain-criome-daemon";

    fn load_configuration(
        path: &std::path::Path,
    ) -> std::result::Result<Self::Configuration, Self::ConfigurationError> {
        let bytes = std::fs::read(path)?;
        DaemonConfiguration::from_rkyv_bytes(&bytes)
    }

    fn build_runtime(_configuration: &Self::Configuration) -> Result<Self::Engine> {
        Ok(Arc::new(Store::new()))
    }

    fn handle_working_input(
        engine: &Self::Engine,
        input: Input,
        _connection: &ConnectionContext,
    ) -> Result<Output> {
        Ok(engine.handle_ordinary_input(input))
    }

    async fn handle_meta_connection(
        engine: &Self::Engine,
        mut connection: AcceptedConnection,
    ) -> Result<()> {
        let codec = LengthPrefixedCodec::new(MaximumFrameLength::new(MAXIMUM_REQUEST_FRAME_BYTES));
        let body = tokio::time::timeout(
            REQUEST_READ_TIMEOUT,
            codec.read_body_async(connection.stream_mut()),
        )
        .await
        .map_err(|_| Error::RequestReadTimedOut)??;
        let (_route, input) = meta::Input::decode_signal_frame(body.bytes())?;
        let reply = engine.handle_meta_input(input);
        codec
            .write_body_async(
                connection.stream_mut(),
                &FrameBody::new(reply.encode_signal_frame()?),
            )
            .await?;
        connection.stream_mut().flush().await?;
        Ok(())
    }
}
