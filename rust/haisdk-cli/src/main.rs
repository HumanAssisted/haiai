use anyhow::Context as _;
use clap::{Parser, Subcommand};
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use haisdk::{CreateAgentOptions, JacsProvider, LocalJacsProvider};
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
    /// Initialize a new JACS agent with keys and config
    Init {
        /// Agent name (required)
        #[arg(long)]
        name: String,

        /// Agent domain for DNSSEC fingerprint (required)
        #[arg(long)]
        domain: String,

        /// Signing algorithm (default: pq2025)
        #[arg(long, default_value = "pq2025")]
        algorithm: String,

        /// Directory for data storage
        #[arg(long, default_value = "./jacs")]
        data_dir: String,

        /// Directory for keys
        #[arg(long, default_value = "./jacs_keys")]
        key_dir: String,

        /// Path to config file
        #[arg(long, default_value = "./jacs.config.json")]
        config_path: String,
    },

    /// Start the built-in HAISDK MCP server (stdio transport)
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            name,
            domain,
            algorithm,
            data_dir,
            key_dir,
            config_path,
        } => {
            let options = CreateAgentOptions {
                name: name.clone(),
                password: String::new(), // resolved from JACS_PRIVATE_KEY_PASSWORD env var
                algorithm: Some(algorithm),
                data_directory: Some(data_dir),
                key_directory: Some(key_dir),
                config_path: Some(config_path),
                domain: Some(domain),
                ..Default::default()
            };

            let result = LocalJacsProvider::create_agent_with_options(&options)
                .context("failed to create JACS agent")?;

            println!("Agent created successfully!");
            println!("  Agent ID: {}", result.agent_id);
            println!("  Version:  {}", result.version);
            println!("  Algorithm: {}", result.algorithm);
            println!("  Config:   {}", result.config_path);
            println!("  Keys:     {}", result.key_directory);
            if !result.dns_record.is_empty() {
                println!("\nDNS (BIND):\n{}", result.dns_record);
                println!("Reminder: enable DNSSEC for the zone and publish DS at the registrar.");
            }
            println!("\nStart the MCP server with: haisdk mcp");
        }

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
