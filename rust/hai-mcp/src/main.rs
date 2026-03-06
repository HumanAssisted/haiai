mod context;
mod embedded_provider;
mod hai_tools;
mod launch;
mod server;

use anyhow::Context as _;
use haisdk::JacsProvider;
use jacs_mcp::JacsMcpServer;
use rmcp::{transport::stdio, ServiceExt};

use crate::context::HaiServerContext;
use crate::embedded_provider::LoadedSharedAgent;
use crate::launch::{help_text, parse_launch_mode, LaunchMode};
use crate::server::HaiMcpServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch_mode = match parse_launch_mode(&args) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    match launch_mode {
        LaunchMode::PrintHelp => {
            print!("{}", help_text());
            return Ok(());
        }
        LaunchMode::PrintVersion => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        LaunchMode::Run => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info,rmcp=warn".to_string()))
        .with_writer(std::io::stderr)
        .init();

    let shared_agent = LoadedSharedAgent::load_from_config_env()
        .context("failed to load JACS agent for hai-mcp")?;
    let provider = shared_agent
        .embedded_provider()
        .context("failed to construct embedded HAISDK provider from JACS agent")?;
    let fallback_jacs_id = provider.jacs_id().to_string();
    let default_config_path = Some(shared_agent.config_path().display().to_string());

    let context =
        HaiServerContext::from_process_env(fallback_jacs_id, default_config_path, provider);
    let server = HaiMcpServer::new(JacsMcpServer::new(shared_agent.agent_wrapper()), context);

    tracing::info!("hai-mcp ready, waiting for MCP client on stdio");

    let (stdin, stdout) = stdio();
    let running = server.serve((stdin, stdout)).await?;
    running.waiting().await?;

    Ok(())
}
