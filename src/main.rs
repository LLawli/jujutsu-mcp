//! jujutsu-mcp: MCP server for jj over stdio. stdout carries JSON-RPC only;
//! logs go to stderr, filtered by `RUST_LOG`.

use std::process::ExitCode;

use jujutsu_mcp::jj::{JjRunner, TESTED_JJ_VERSION};
use jujutsu_mcp::server::JjServer;
use jujutsu_mcp::setup::{Environment, SetupError, parse_setup_args, run_setup};
use rmcp::ServiceExt;
use rmcp::service::ServerInitializeError;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

#[derive(Debug, thiserror::Error)]
enum FatalError {
    #[error("could not start the async runtime: {0}")]
    Runtime(#[source] std::io::Error),
    #[error("MCP handshake failed: {0}")]
    Handshake(String),
    #[error("server task failed: {0}")]
    Serve(#[source] tokio::task::JoinError),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    match args.get(1).map(String::as_str) {
        None => serve(),
        Some("setup") => setup(&args[2..]),
        Some(other) => {
            eprintln!("jujutsu-mcp: unknown argument: {other}");
            eprintln!("{}", jujutsu_mcp::setup::USAGE);
            ExitCode::from(2)
        }
    }
}

/// `jujutsu-mcp setup`: no tracing and no async runtime, it only runs a few
/// short commands.
fn setup(args: &[String]) -> ExitCode {
    let result = parse_setup_args(args).and_then(|options| {
        let env = Environment::from_process()?;
        run_setup(&env, &options, &mut std::io::stdout())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("jujutsu-mcp: {err}");
            ExitCode::from(if matches!(err, SetupError::Usage(_)) {
                2
            } else {
                1
            })
        }
    }
}

fn serve() -> ExitCode {
    init_tracing();
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(FatalError::Runtime)
        .and_then(|runtime| runtime.block_on(run()));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("jujutsu-mcp: {err}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // try_init: a second global subscriber is not worth failing the server for.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

async fn run() -> Result<(), FatalError> {
    let runner = JjRunner::new();
    log_versions(&runner).await;

    let service = match JjServer::new(runner).serve(stdio()).await {
        Ok(service) => service,
        // stdin closed before the client sent `initialize`: the client went
        // away, which is a normal shutdown, not a protocol failure.
        Err(ServerInitializeError::ConnectionClosed(reason)) => {
            tracing::info!("stdin closed before the handshake: {reason}");
            return Ok(());
        }
        Err(err) => return Err(FatalError::Handshake(err.to_string())),
    };
    service.waiting().await.map_err(FatalError::Serve)?;
    Ok(())
}

/// Logs the startup banner and warns about a jj older than the tested one.
/// Never fails: an unknown version must not keep the server from starting.
async fn log_versions(runner: &JjRunner) {
    let crate_version = env!("CARGO_PKG_VERSION");
    match runner.version().await {
        Ok(Some(jj)) => {
            tracing::info!("jujutsu-mcp {crate_version} starting, jj {jj}");
            if jj < TESTED_JJ_VERSION {
                tracing::warn!(
                    "jj {jj} is older than {TESTED_JJ_VERSION}, the version jujutsu-mcp was tested against; some tools may fail"
                );
            }
        }
        Ok(None) => {
            tracing::info!("jujutsu-mcp {crate_version} starting");
            tracing::warn!("could not parse the jj version from `jj --version`");
        }
        Err(err) => {
            tracing::info!("jujutsu-mcp {crate_version} starting");
            tracing::warn!("could not determine the jj version: {err}");
        }
    }
}
