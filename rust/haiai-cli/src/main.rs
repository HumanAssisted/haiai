use anyhow::Context as _;
use clap::{Parser, Subcommand};
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use haiai::{
    CreateAgentOptions, HaiClient, HaiClientOptions, JacsAgentLifecycle, JacsDocumentProvider,
    JacsProvider, ListMessagesOptions, LocalJacsProvider, RegisterAgentOptions, SearchOptions,
    SendEmailOptions,
};
use jacs_mcp::JacsMcpServer;
use rmcp::{transport::stdio, ServiceExt};
use serde_json::Value;

#[derive(Parser)]
#[command(name = "haiai", version, about = "HAIAI CLI")]
struct Cli {
    /// Do not prompt for private key password; require JACS_PRIVATE_KEY_PASSWORD env var
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Document storage backend: fs, rusqlite, sqlite
    #[arg(long, global = true)]
    storage: Option<String>,

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

    /// Start the built-in HAIAI MCP server (stdio transport)
    Mcp,

    /// Ping the HAI API and verify connectivity
    Hello,

    /// Register this agent with the HAI platform
    Register {
        /// Owner email for registration notifications
        #[arg(long)]
        owner_email: String,

        /// Optional description of this agent
        #[arg(long)]
        description: Option<String>,
    },

    /// Check registration and verification status
    Status,

    /// Check if a username is available
    CheckUsername {
        /// Username to check
        username: String,
    },

    /// Claim a @hai.ai username for this agent
    ClaimUsername {
        /// Username to claim
        username: String,
    },

    /// Send a signed email from this agent
    SendEmail {
        /// Recipient email address
        #[arg(long)]
        to: String,

        /// Email subject line
        #[arg(long)]
        subject: String,

        /// Email body text
        #[arg(long)]
        body: String,
    },

    /// List email messages
    ListMessages {
        /// Maximum number of messages to return
        #[arg(long, default_value = "20")]
        limit: u32,

        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: u32,

        /// Filter by direction: inbound or outbound
        #[arg(long)]
        direction: Option<String>,
    },

    /// Search email messages
    SearchMessages {
        /// Search query string
        #[arg(long)]
        q: Option<String>,

        /// Filter by sender address
        #[arg(long)]
        from: Option<String>,

        /// Filter by recipient address
        #[arg(long)]
        to: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Update agent metadata and re-sign with existing key
    Update {
        /// JSON string with updated agent fields (merged with current doc)
        #[arg(long)]
        set: Option<String>,
    },

    /// Rotate this agent's cryptographic keys
    Rotate,

    /// Migrate a legacy agent to the current schema
    Migrate,

    /// Run a benchmark against the HAI platform
    Benchmark {
        /// Benchmark name
        #[arg(long, default_value = "cli-benchmark")]
        name: String,

        /// Benchmark tier: free, pro, or enterprise
        #[arg(long, default_value = "free")]
        tier: String,
    },

    /// Diagnose agent health, storage, and configuration
    Doctor,

    /// Store a signed document
    StoreDocument {
        /// Path to JSON file, or "-" for stdin
        #[arg()]
        path: String,
    },

    /// List stored documents
    ListDocuments {
        /// Filter by document type
        #[arg(long)]
        doc_type: Option<String>,
    },

    /// Search stored documents
    SearchDocuments {
        /// Search query
        #[arg()]
        query: String,

        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Get a document by key (id:version)
    GetDocument {
        /// Document key (id:version)
        #[arg()]
        key: String,
    },

    /// Remove a document
    RemoveDocument {
        /// Document key (id:version)
        #[arg()]
        key: String,
    },
}

fn hai_url() -> String {
    std::env::var("HAI_URL").unwrap_or_else(|_| "https://hai.ai".to_string())
}

/// Resolve password for agent creation: use JACS_PRIVATE_KEY_PASSWORD if set,
/// otherwise prompt twice on stdin when it's a TTY (hidden input). Non-interactive
/// runs must set the env var.
fn resolve_init_password() -> anyhow::Result<String> {
    if let Ok(pass) = std::env::var("JACS_PRIVATE_KEY_PASSWORD") {
        if !pass.is_empty() {
            return Ok(pass);
        }
    }
    if !atty::is(atty::Stream::Stdin) {
        anyhow::bail!(
            "Password is required for agent creation. \
            Set the JACS_PRIVATE_KEY_PASSWORD environment variable, \
            or run haiai init from a terminal to be prompted for a password."
        );
    }
    loop {
        eprintln!("Enter password (used to encrypt private key):");
        let password = rpassword::read_password().context("failed to read password")?;
        if password.is_empty() {
            eprintln!("Password cannot be empty. Try again.");
            continue;
        }
        eprintln!("Confirm password:");
        let confirm = rpassword::read_password().context("failed to read password confirmation")?;
        if password != confirm {
            eprintln!("Passwords do not match. Try again.");
            continue;
        }
        return Ok(password);
    }
}

/// If JACS_PRIVATE_KEY_PASSWORD is not set and we're not in quiet mode, prompt for it
/// (once, hidden) and set the env var so the subsequent agent load can decrypt the key.
/// Used by all commands that load an existing agent (everything except init).
fn ensure_agent_password(quiet: bool) -> anyhow::Result<()> {
    if std::env::var("JACS_PRIVATE_KEY_PASSWORD")
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    if !atty::is(atty::Stream::Stdin) {
        anyhow::bail!(
                "JACS_PRIVATE_KEY_PASSWORD is not set. \
                Set it to the password for your private key, or run haiai from a terminal to be prompted."
            );
    }
    eprintln!("Enter private key password:");
    let password = rpassword::read_password().context("failed to read password")?;
    if password.is_empty() {
        anyhow::bail!("Password cannot be empty.");
    }
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", &password);
    Ok(())
}

/// Load the local JACS provider and build a HaiClient.
///
/// The provider is loaded from `JACS_CONFIG` / `JACS_CONFIG_PATH` env vars
/// or `./jacs.config.json`. The base URL comes from `HAI_URL` env var
/// or defaults to `https://hai.ai`.
fn load_client() -> anyhow::Result<HaiClient<LocalJacsProvider>> {
    let provider = LocalJacsProvider::from_config_path(None)
        .context("failed to load JACS agent from config")?;
    let options = HaiClientOptions {
        base_url: hai_url(),
        ..Default::default()
    };
    let client = HaiClient::new(provider, options).context("failed to construct HaiClient")?;
    Ok(client)
}

/// Load a local JACS provider with document storage configured.
fn load_provider_with_storage(storage_flag: Option<&str>) -> anyhow::Result<LocalJacsProvider> {
    let label = haiai::resolve_storage_backend(storage_flag, None)
        .context("failed to resolve storage backend")?;
    LocalJacsProvider::from_config_path_with_storage(None, &label)
        .context("failed to load JACS agent with storage")
}

/// Print a table of email messages in a consistent format.
fn print_message_table(messages: &[haiai::EmailMessage]) {
    if messages.is_empty() {
        println!("No messages.");
        return;
    }
    println!(
        "{:<9} {:<28} {:<28} {:<40} {:<20} {:<5}",
        "DIRECTION", "FROM", "TO", "SUBJECT", "DATE", "READ"
    );
    println!("{}", "-".repeat(130));
    for msg in messages {
        let subject = if msg.subject.len() > 38 {
            format!("{}...", &msg.subject[..35])
        } else {
            msg.subject.clone()
        };
        let read = if msg.is_read { "yes" } else { "no" };
        println!(
            "{:<9} {:<28} {:<28} {:<40} {:<20} {:<5}",
            msg.direction, msg.from_address, msg.to_address, subject, msg.created_at, read,
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Commands that load an existing agent need the private key password. Prompt once if not set and not -q.
    if !matches!(cli.command, Commands::Init { .. }) {
        ensure_agent_password(cli.quiet).context("failed to resolve private key password")?;
    }

    match cli.command {
        Commands::Init {
            name,
            domain,
            algorithm,
            data_dir,
            key_dir,
            config_path,
        } => {
            let password_resolved = resolve_init_password()?;
            let options = CreateAgentOptions {
                name: name.clone(),
                password: password_resolved,
                algorithm: Some(algorithm),
                data_directory: Some(data_dir),
                key_directory: Some(key_dir),
                config_path: Some(config_path),
                domain: Some(domain),
                ..Default::default()
            };

            let result = LocalJacsProvider::create_agent_with_options(&options).map_err(|e| {
                let msg = e.to_string();
                if msg.contains("Password is required") {
                    anyhow::anyhow!(
                        "Password is required for agent creation. \
                        Set the JACS_PRIVATE_KEY_PASSWORD environment variable, \
                        or run haiai init from a terminal to be prompted for a password."
                    )
                } else {
                    anyhow::anyhow!("{}", msg)
                }
            })?;

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
            println!("\nStart the MCP server with: haiai mcp");
        }

        Commands::Mcp => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    std::env::var("RUST_LOG").unwrap_or_else(|_| "info,rmcp=warn".to_string()),
                )
                .with_writer(std::io::stderr)
                .init();

            let shared_agent = LoadedSharedAgent::load_from_config_env()
                .context("failed to load JACS agent for haiai mcp")?;
            let provider = shared_agent
                .embedded_provider()
                .context("failed to construct embedded HAIAI provider from JACS agent")?;
            let fallback_jacs_id = provider.jacs_id().to_string();
            let default_config_path = Some(shared_agent.config_path().display().to_string());

            let context =
                HaiServerContext::from_process_env(fallback_jacs_id, default_config_path, provider);
            let server =
                HaiMcpServer::new(JacsMcpServer::new(shared_agent.agent_wrapper()), context);

            tracing::info!("haiai mcp ready, waiting for MCP client on stdio");

            let (stdin, stdout) = stdio();
            let running = server.serve((stdin, stdout)).await?;
            running.waiting().await?;
        }

        Commands::Hello => {
            let client = load_client()?;
            let result = client.hello(false).await.context("hello failed")?;
            println!("  Timestamp: {}", result.timestamp);
            println!("  Message:   {}", result.message);
            println!("  Hello ID:  {}", result.hello_id);
        }

        Commands::Register {
            owner_email,
            description,
        } => {
            let provider = LocalJacsProvider::from_config_path(None)
                .context("failed to load JACS agent from config")?;
            let agent_json = provider
                .export_agent_json()
                .context("failed to export agent JSON")?;
            let public_key = provider
                .public_key_pem()
                .context("failed to read public key PEM")?;

            let options = HaiClientOptions {
                base_url: hai_url(),
                ..Default::default()
            };
            let client =
                HaiClient::new(provider, options).context("failed to construct HaiClient")?;

            let reg_options = RegisterAgentOptions {
                agent_json,
                public_key_pem: Some(public_key),
                owner_email: Some(owner_email.clone()),
                description,
                ..Default::default()
            };
            let result = client
                .register(&reg_options)
                .await
                .context("registration failed")?;

            println!("  Agent ID:            {}", result.agent_id);
            println!("  JACS ID:             {}", result.jacs_id);
            println!(
                "  Registration Status: {}",
                if result.success {
                    "registered"
                } else {
                    "failed"
                }
            );
            println!("  Email:               {}", owner_email);
        }

        Commands::Status => {
            let client = load_client()?;
            let jacs_id = client.jacs_id().to_string();
            let result = client
                .verify_status(Some(&jacs_id))
                .await
                .context("status check failed")?;
            println!("  JACS ID:       {}", result.jacs_id);
            println!("  Registered:    {}", result.registered);
            println!("  DNS Verified:  {}", result.dns_verified);
            println!("  Registered At: {}", result.registered_at);
        }

        Commands::CheckUsername { username } => {
            let client = load_client()?;
            let result = client
                .check_username(&username)
                .await
                .context("username check failed")?;
            println!("  Available: {}", result.available);
            println!("  Username:  {}", result.username);
            if let Some(reason) = &result.reason {
                println!("  Reason:    {}", reason);
            }
        }

        Commands::ClaimUsername { username } => {
            let mut client = load_client()?;
            let agent_id = client.jacs_id().to_string();
            let result = client
                .claim_username(&agent_id, &username)
                .await
                .context("username claim failed")?;
            println!("  Username: {}", result.username);
            println!("  Email:    {}", result.email);
            println!("  Agent ID: {}", result.agent_id);
        }

        Commands::SendEmail { to, subject, body } => {
            let client = load_client()?;
            let options = SendEmailOptions {
                to,
                subject,
                body,
                in_reply_to: None,
                attachments: vec![],
            };
            let result = client
                .send_signed_email(&options)
                .await
                .context("send email failed")?;
            println!("  Message ID: {}", result.message_id);
            println!("  Status:     {}", result.status);
        }

        Commands::ListMessages {
            limit,
            offset,
            direction,
        } => {
            let client = load_client()?;
            let options = ListMessagesOptions {
                limit: Some(limit),
                offset: Some(offset),
                direction,
            };
            let messages = client
                .list_messages(&options)
                .await
                .context("list messages failed")?;
            print_message_table(&messages);
        }

        Commands::SearchMessages { q, from, to, limit } => {
            let client = load_client()?;
            let options = SearchOptions {
                q,
                from_address: from,
                to_address: to,
                limit: Some(limit),
                ..Default::default()
            };
            let messages = client
                .search_messages(&options)
                .await
                .context("search messages failed")?;
            print_message_table(&messages);
        }

        Commands::Update { set } => {
            let client = load_client()?;

            let exported = client
                .export_agent_json()
                .context("failed to export agent JSON")?;
            let mut doc: Value =
                serde_json::from_str(&exported).context("failed to parse agent JSON")?;

            if let Some(set_json) = set {
                let overrides: Value =
                    serde_json::from_str(&set_json).context("--set must be valid JSON")?;
                if let Some(obj) = overrides.as_object() {
                    for (k, v) in obj {
                        if k == "jacsId" {
                            anyhow::bail!("jacsId MUST NOT be changed via update");
                        }
                        doc[k] = v.clone();
                    }
                }
            }

            let result = client
                .update_agent(&doc.to_string())
                .await
                .context("agent update failed")?;

            println!("Agent updated successfully!");
            println!("  Agent ID:    {}", result.jacs_id);
            println!("  Old Version: {}", result.old_version);
            println!("  New Version: {}", result.new_version);
            if result.registered_with_hai {
                println!("  Re-registered: yes");
            } else {
                println!("  Re-registered: no (run `haiai register` to register manually)");
            }
        }

        Commands::Rotate => {
            let client = load_client()?;

            let result = client
                .rotate_keys(None)
                .await
                .context("key rotation failed")?;

            println!("Keys rotated successfully!");
            println!("  Agent ID:       {}", result.jacs_id);
            println!("  Old Version:    {}", result.old_version);
            println!("  New Version:    {}", result.new_version);
            println!("  New Key Hash:   {}", result.new_public_key_hash);
            if result.registered_with_hai {
                println!("  Re-registered:  yes");
            } else {
                println!("  Re-registered:  no (run `haiai register` to register manually)");
            }
        }

        Commands::Migrate => {
            let result =
                LocalJacsProvider::migrate_agent(None).context("agent migration failed")?;

            println!("Agent migrated successfully!");
            println!("  Agent ID:    {}", result.jacs_id);
            println!("  Old Version: {}", result.old_version);
            println!("  New Version: {}", result.new_version);
            if !result.patched_fields.is_empty() {
                println!("  Patched:     {:?}", result.patched_fields);
            }
        }

        Commands::Benchmark { name, tier } => {
            let client = load_client()?;
            let result = client
                .benchmark(Some(&name), Some(&tier))
                .await
                .context("benchmark failed")?;

            let run_id = result
                .get("run_id")
                .or_else(|| result.get("runId"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let status = result.get("status").and_then(Value::as_str).unwrap_or("");
            let tier_val = result.get("tier").and_then(Value::as_str).unwrap_or(&tier);

            println!("  Run ID: {}", run_id);
            println!("  Status: {}", status);
            println!("  Tier:   {}", tier_val);
        }

        Commands::Doctor => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;

            println!("Agent Diagnostics");
            println!("{}", "=".repeat(50));

            // Identity
            println!("  JACS ID:    {}", provider.jacs_id());
            println!("  Algorithm:  {}", provider.algorithm());
            println!("  Config:     {}", provider.config_path().display());

            // Self-verification
            match provider.verify_self() {
                Ok(result) => {
                    println!(
                        "  Self-Check: {}",
                        if result.valid { "PASS" } else { "FAIL" }
                    );
                    if let Some(err) = &result.error {
                        println!("  Error:      {}", err);
                    }
                }
                Err(e) => println!("  Self-Check: ERROR ({})", e),
            }

            // Diagnostics
            match provider.diagnostics() {
                Ok(diag) => {
                    if let Some(obj) = diag.as_object() {
                        for (k, v) in obj {
                            println!("  {}: {}", k, v);
                        }
                    }
                }
                Err(e) => println!("  Diagnostics: ERROR ({})", e),
            }

            // Storage
            let storage_label = haiai::resolve_storage_backend(cli.storage.as_deref(), None)
                .unwrap_or_else(|_| "fs".to_string());
            println!("\nStorage");
            println!("{}", "-".repeat(50));
            println!("  Backend:    {}", storage_label);
            println!(
                "  Configured: {}",
                if provider.has_document_service() {
                    "yes"
                } else {
                    "no"
                }
            );
            if provider.has_document_service() {
                match provider.storage_capabilities() {
                    Ok(caps) => {
                        println!("  Fulltext:   {}", caps.fulltext);
                        println!("  Vector:     {}", caps.vector);
                        println!("  Pagination: {}", caps.pagination);
                    }
                    Err(e) => println!("  Capabilities: ERROR ({})", e),
                }

                // Document count
                match provider.list_documents(None) {
                    Ok(docs) => println!("  Documents:  {}", docs.len()),
                    Err(e) => println!("  Documents:  ERROR ({})", e),
                }
            }
        }

        Commands::StoreDocument { path } => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;
            let content = if path == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("failed to read stdin")?;
                buf
            } else {
                std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read file: {}", path))?
            };
            let data: Value = serde_json::from_str(&content)
                .with_context(|| format!("invalid JSON in {}", path))?;
            let doc = provider
                .sign_and_store(&data)
                .context("sign_and_store failed")?;
            println!("Document stored:");
            println!("  Key: {}", doc.key);
        }

        Commands::ListDocuments { doc_type } => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;
            let keys = provider
                .list_documents(doc_type.as_deref())
                .context("list_documents failed")?;
            if keys.is_empty() {
                println!("No documents found.");
            } else {
                for key in &keys {
                    println!("{}", key);
                }
                println!("\n{} document(s)", keys.len());
            }
        }

        Commands::SearchDocuments { query, limit } => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;
            let results = provider
                .search_documents(&query, limit, 0)
                .context("search failed")?;
            if results.results.is_empty() {
                println!("No results.");
            } else {
                for hit in &results.results {
                    println!("{} (score: {:.2})", hit.key, hit.score);
                }
                println!(
                    "\n{} result(s), method: {}",
                    results.total_count, results.method
                );
            }
        }

        Commands::GetDocument { key } => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;
            let json = provider
                .get_document(&key)
                .context("get_document failed")?;
            println!("{}", json);
        }

        Commands::RemoveDocument { key } => {
            let provider = load_provider_with_storage(cli.storage.as_deref())?;
            provider
                .remove_document(&key)
                .context("remove_document failed")?;
            println!("Document removed: {}", key);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_help_does_not_panic() {
        // Verify the CLI definition is well-formed and --help can render.
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_init() {
        let cli = Cli::parse_from([
            "haiai",
            "init",
            "--name",
            "myagent",
            "--domain",
            "example.com",
        ]);
        match cli.command {
            Commands::Init {
                name,
                domain,
                algorithm,
                data_dir,
                key_dir,
                config_path,
            } => {
                assert_eq!(name, "myagent");
                assert_eq!(domain, "example.com");
                assert_eq!(algorithm, "pq2025");
                assert_eq!(data_dir, "./jacs");
                assert_eq!(key_dir, "./jacs_keys");
                assert_eq!(config_path, "./jacs.config.json");
            }
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parse_mcp() {
        let cli = Cli::parse_from(["haiai", "mcp"]);
        assert!(matches!(cli.command, Commands::Mcp));
    }

    #[test]
    fn parse_hello() {
        let cli = Cli::parse_from(["haiai", "hello"]);
        assert!(matches!(cli.command, Commands::Hello));
    }

    #[test]
    fn parse_register_required_args() {
        let cli = Cli::parse_from(["haiai", "register", "--owner-email", "agent@example.com"]);
        match cli.command {
            Commands::Register {
                owner_email,
                description,
            } => {
                assert_eq!(owner_email, "agent@example.com");
                assert!(description.is_none());
            }
            _ => panic!("expected Register command"),
        }
    }

    #[test]
    fn parse_register_with_description() {
        let cli = Cli::parse_from([
            "haiai",
            "register",
            "--owner-email",
            "agent@example.com",
            "--description",
            "My test agent",
        ]);
        match cli.command {
            Commands::Register {
                owner_email,
                description,
            } => {
                assert_eq!(owner_email, "agent@example.com");
                assert_eq!(description.as_deref(), Some("My test agent"));
            }
            _ => panic!("expected Register command"),
        }
    }

    #[test]
    fn parse_register_missing_email_fails() {
        let result = Cli::try_parse_from(["haiai", "register"]);
        assert!(
            result.is_err(),
            "register without --owner-email should fail"
        );
    }

    #[test]
    fn parse_status() {
        let cli = Cli::parse_from(["haiai", "status"]);
        assert!(matches!(cli.command, Commands::Status));
    }

    #[test]
    fn parse_check_username() {
        let cli = Cli::parse_from(["haiai", "check-username", "alice"]);
        match cli.command {
            Commands::CheckUsername { username } => {
                assert_eq!(username, "alice");
            }
            _ => panic!("expected CheckUsername command"),
        }
    }

    #[test]
    fn parse_check_username_missing_arg_fails() {
        let result = Cli::try_parse_from(["haiai", "check-username"]);
        assert!(
            result.is_err(),
            "check-username without positional arg should fail"
        );
    }

    #[test]
    fn parse_claim_username() {
        let cli = Cli::parse_from(["haiai", "claim-username", "bob"]);
        match cli.command {
            Commands::ClaimUsername { username } => {
                assert_eq!(username, "bob");
            }
            _ => panic!("expected ClaimUsername command"),
        }
    }

    #[test]
    fn parse_send_email() {
        let cli = Cli::parse_from([
            "haiai",
            "send-email",
            "--to",
            "friend@hai.ai",
            "--subject",
            "Hello",
            "--body",
            "Hi there!",
        ]);
        match cli.command {
            Commands::SendEmail { to, subject, body } => {
                assert_eq!(to, "friend@hai.ai");
                assert_eq!(subject, "Hello");
                assert_eq!(body, "Hi there!");
            }
            _ => panic!("expected SendEmail command"),
        }
    }

    #[test]
    fn parse_send_email_missing_args_fails() {
        let result = Cli::try_parse_from(["haiai", "send-email", "--to", "x@hai.ai"]);
        assert!(
            result.is_err(),
            "send-email without --subject and --body should fail"
        );
    }

    #[test]
    fn parse_list_messages_defaults() {
        let cli = Cli::parse_from(["haiai", "list-messages"]);
        match cli.command {
            Commands::ListMessages {
                limit,
                offset,
                direction,
            } => {
                assert_eq!(limit, 20);
                assert_eq!(offset, 0);
                assert!(direction.is_none());
            }
            _ => panic!("expected ListMessages command"),
        }
    }

    #[test]
    fn parse_list_messages_with_args() {
        let cli = Cli::parse_from([
            "haiai",
            "list-messages",
            "--limit",
            "50",
            "--offset",
            "10",
            "--direction",
            "inbound",
        ]);
        match cli.command {
            Commands::ListMessages {
                limit,
                offset,
                direction,
            } => {
                assert_eq!(limit, 50);
                assert_eq!(offset, 10);
                assert_eq!(direction.as_deref(), Some("inbound"));
            }
            _ => panic!("expected ListMessages command"),
        }
    }

    #[test]
    fn parse_search_messages_defaults() {
        let cli = Cli::parse_from(["haiai", "search-messages"]);
        match cli.command {
            Commands::SearchMessages { q, from, to, limit } => {
                assert!(q.is_none());
                assert!(from.is_none());
                assert!(to.is_none());
                assert_eq!(limit, 20);
            }
            _ => panic!("expected SearchMessages command"),
        }
    }

    #[test]
    fn parse_search_messages_with_args() {
        let cli = Cli::parse_from([
            "haiai",
            "search-messages",
            "--q",
            "invoice",
            "--from",
            "sender@hai.ai",
            "--to",
            "me@hai.ai",
            "--limit",
            "5",
        ]);
        match cli.command {
            Commands::SearchMessages { q, from, to, limit } => {
                assert_eq!(q.as_deref(), Some("invoice"));
                assert_eq!(from.as_deref(), Some("sender@hai.ai"));
                assert_eq!(to.as_deref(), Some("me@hai.ai"));
                assert_eq!(limit, 5);
            }
            _ => panic!("expected SearchMessages command"),
        }
    }

    #[test]
    fn parse_benchmark_defaults() {
        let cli = Cli::parse_from(["haiai", "benchmark"]);
        match cli.command {
            Commands::Benchmark { name, tier } => {
                assert_eq!(name, "cli-benchmark");
                assert_eq!(tier, "free");
            }
            _ => panic!("expected Benchmark command"),
        }
    }

    #[test]
    fn parse_benchmark_with_args() {
        let cli = Cli::parse_from([
            "haiai",
            "benchmark",
            "--name",
            "stress-test",
            "--tier",
            "pro",
        ]);
        match cli.command {
            Commands::Benchmark { name, tier } => {
                assert_eq!(name, "stress-test");
                assert_eq!(tier, "pro");
            }
            _ => panic!("expected Benchmark command"),
        }
    }

    #[test]
    fn parse_update_no_args() {
        let cli = Cli::parse_from(["haiai", "update"]);
        match cli.command {
            Commands::Update { set } => {
                assert!(set.is_none());
            }
            _ => panic!("expected Update command"),
        }
    }

    #[test]
    fn parse_update_with_set() {
        let cli = Cli::parse_from(["haiai", "update", "--set", r#"{"jacsAgentType":"service"}"#]);
        match cli.command {
            Commands::Update { set } => {
                assert_eq!(set.as_deref(), Some(r#"{"jacsAgentType":"service"}"#));
            }
            _ => panic!("expected Update command"),
        }
    }

    #[test]
    fn parse_rotate() {
        let cli = Cli::parse_from(["haiai", "rotate"]);
        assert!(matches!(cli.command, Commands::Rotate));
    }

    #[test]
    fn parse_migrate() {
        let cli = Cli::parse_from(["haiai", "migrate"]);
        assert!(matches!(cli.command, Commands::Migrate));
    }

    #[test]
    fn parse_doctor() {
        let cli = Cli::parse_from(["haiai", "doctor"]);
        assert!(matches!(cli.command, Commands::Doctor));
    }

    #[test]
    fn parse_doctor_with_storage() {
        let cli = Cli::parse_from(["haiai", "--storage", "sqlite", "doctor"]);
        assert!(matches!(cli.command, Commands::Doctor));
        assert_eq!(cli.storage.as_deref(), Some("sqlite"));
    }

    #[test]
    fn parse_store_document() {
        let cli = Cli::parse_from(["haiai", "store-document", "doc.json"]);
        match cli.command {
            Commands::StoreDocument { path } => {
                assert_eq!(path, "doc.json");
            }
            _ => panic!("expected StoreDocument command"),
        }
    }

    #[test]
    fn parse_list_documents() {
        let cli = Cli::parse_from(["haiai", "list-documents"]);
        match cli.command {
            Commands::ListDocuments { doc_type } => {
                assert!(doc_type.is_none());
            }
            _ => panic!("expected ListDocuments command"),
        }
    }

    #[test]
    fn parse_list_documents_with_type() {
        let cli = Cli::parse_from(["haiai", "list-documents", "--doc-type", "invoice"]);
        match cli.command {
            Commands::ListDocuments { doc_type } => {
                assert_eq!(doc_type.as_deref(), Some("invoice"));
            }
            _ => panic!("expected ListDocuments command"),
        }
    }

    #[test]
    fn parse_search_documents() {
        let cli = Cli::parse_from(["haiai", "search-documents", "my query"]);
        match cli.command {
            Commands::SearchDocuments { query, limit } => {
                assert_eq!(query, "my query");
                assert_eq!(limit, 20);
            }
            _ => panic!("expected SearchDocuments command"),
        }
    }

    #[test]
    fn parse_get_document() {
        let cli = Cli::parse_from(["haiai", "get-document", "abc:1"]);
        match cli.command {
            Commands::GetDocument { key } => {
                assert_eq!(key, "abc:1");
            }
            _ => panic!("expected GetDocument command"),
        }
    }

    #[test]
    fn parse_remove_document() {
        let cli = Cli::parse_from(["haiai", "remove-document", "abc:1"]);
        match cli.command {
            Commands::RemoveDocument { key } => {
                assert_eq!(key, "abc:1");
            }
            _ => panic!("expected RemoveDocument command"),
        }
    }

    #[test]
    fn parse_global_storage_flag() {
        let cli = Cli::parse_from(["haiai", "--storage", "rusqlite", "list-documents"]);
        assert_eq!(cli.storage.as_deref(), Some("rusqlite"));
        assert!(matches!(cli.command, Commands::ListDocuments { .. }));
    }
}
