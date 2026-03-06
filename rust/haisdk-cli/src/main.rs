use anyhow::Context as _;
use clap::{Parser, Subcommand};
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use haisdk::JacsProvider;
use jacs_mcp::JacsMcpServer;
use rmcp::{transport::stdio, ServiceExt};

#[derive(Parser)]
#[command(name = "haisdk", version, about = "HAISDK CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the built-in HAISDK MCP server (stdio transport)
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Mcp => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    std::env::var("RUST_LOG")
                        .unwrap_or_else(|_| "info,rmcp=warn".to_string()),
                )
                .with_writer(std::io::stderr)
                .init();

            let shared_agent = LoadedSharedAgent::load_from_config_env()
                .context("failed to load JACS agent for haisdk mcp")?;
            let provider = shared_agent
                .embedded_provider()
                .context("failed to construct embedded HAISDK provider from JACS agent")?;
            let fallback_jacs_id = provider.jacs_id().to_string();
            let default_config_path =
                Some(shared_agent.config_path().display().to_string());

            let context = HaiServerContext::from_process_env(
                fallback_jacs_id,
                default_config_path,
                provider,
            );
            let server = HaiMcpServer::new(
                JacsMcpServer::new(shared_agent.agent_wrapper()),
                context,
            );

            tracing::info!("haisdk mcp ready, waiting for MCP client on stdio");

            let (stdin, stdout) = stdio();
            let running = server.serve((stdin, stdout)).await?;
            running.waiting().await?;
        }
    }

    Ok(())
}
