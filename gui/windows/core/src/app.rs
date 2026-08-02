use std::{path::PathBuf, process::ExitCode};

use log::{error, info};
use paloma_binding_protocol::v1::binding_server::BindingServer;
use paloma_core::AppContext;

use crate::{service::BindingService, transport};

const DEFAULT_PIPE_NAME: &str = "paloma-core";

pub fn run() -> ExitCode {
    let env = env_logger::Env::default().default_filter_or("info,rmcp=warn");
    env_logger::Builder::from_env(env)
        .format_timestamp_millis()
        .init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("paloma-core")
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            error!("failed to start the tokio runtime: {e}");
            return ExitCode::FAILURE;
        },
    };

    match runtime.block_on(serve(pipe_name())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        },
    }
}

fn pipe_name() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--pipe"
            && let Some(name) = args.next()
        {
            return name;
        }
    }
    DEFAULT_PIPE_NAME.to_string()
}

async fn serve(pipe_name: String) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is not set")?;
    let app = AppContext::build(data_dir).await?;

    let address = format!(r"\\.\pipe\{pipe_name}");
    let incoming = transport::incoming(&address)?;
    info!("serving on {address}");

    tonic::transport::Server::builder()
        .add_service(BindingServer::new(BindingService::new(app)))
        .serve_with_incoming(incoming)
        .await?;
    Ok(())
}
