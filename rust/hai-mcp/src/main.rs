mod context;
mod hai_tools;
mod launch;
mod server;

use anyhow::Context as _;
use jacs_mcp::{JacsMcpServer, load_agent_from_config_env};
use rmcp::{ServiceExt, transport::stdio};

use crate::context::HaiServerContext;
use crate::launch::{LaunchMode, help_text, parse_launch_mode};
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

    let agent = load_agent_from_config_env().context("failed to load JACS agent for hai-mcp")?;
    let fallback_jacs_id = agent
        .get_agent_id()
        .ok()
        .or_else(|| std::env::var("JACS_ID").ok())
        .unwrap_or_else(|| "anonymous-agent".to_string());
    let default_config_path = std::env::var("JACS_CONFIG").ok().filter(|value| !value.is_empty());

    let context = HaiServerContext::from_process_env(fallback_jacs_id, default_config_path);
    let server = HaiMcpServer::new(JacsMcpServer::new(agent), context);

    tracing::info!("hai-mcp ready, waiting for MCP client on stdio");

    let (stdin, stdout) = stdio();
    let running = server.serve((stdin, stdout)).await?;
    running.waiting().await?;

    Ok(())
}
