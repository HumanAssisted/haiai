use anyhow::Context as _;
use clap::{Parser, Subcommand};
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use haiai::{
    CreateAgentOptions, CreateEmailTemplateOptions, HaiClient, HaiClientOptions,
    JacsAgentLifecycle, JacsDocumentProvider, JacsProvider, ListEmailTemplatesOptions,
    ListMessagesOptions, LocalJacsProvider, RegisterAgentOptions, SearchOptions, SendEmailOptions,
    UpdateEmailTemplateOptions,
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

    /// Read private key password from a file instead of prompting or using env var
    #[arg(long, global = true)]
    password_file: Option<String>,

    /// Document storage backend: fs, rusqlite, sqlite
    #[arg(long, global = true)]
    storage: Option<String>,

    /// Read storage backend label from the named environment variable instead of
    /// the command line. Keeps credentials out of `ps aux` process listings.
    /// Example: `--storage-env MY_STORAGE_VAR` reads `$MY_STORAGE_VAR`.
    #[arg(long, global = true, conflicts_with = "storage")]
    storage_env: Option<String>,

    /// Write log output to a file (in addition to stderr). Useful for debugging
    /// the MCP server when stderr is not visible.
    #[arg(long, global = true)]
    log_file: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new JACS agent with keys and config, optionally registering with HAI
    Init {
        /// Agent name / username (required). Must be 3-30 lowercase alphanumeric + hyphens.
        #[arg(long)]
        name: String,

        /// One-time registration key from the dashboard (required when --register=true)
        #[arg(long)]
        key: Option<String>,

        /// Agent domain for DNSSEC fingerprint (optional)
        #[arg(long)]
        domain: Option<String>,

        /// Set to false to skip HAI registration (create local identity only)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        register: bool,

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

    /// Check registration and verification status
    Status,

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

        /// CC recipients (repeatable)
        #[arg(long)]
        cc: Vec<String>,

        /// BCC recipients (repeatable)
        #[arg(long)]
        bcc: Vec<String>,

        /// Labels/tags to apply (repeatable)
        #[arg(long)]
        labels: Vec<String>,
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

        /// Filter by read status (true = read only, false = unread only)
        #[arg(long)]
        is_read: Option<bool>,

        /// Filter by folder (e.g. 'inbox', 'archive')
        #[arg(long)]
        folder: Option<String>,

        /// Filter by label/tag
        #[arg(long)]
        label: Option<String>,
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

        /// Filter by read status
        #[arg(long)]
        is_read: Option<bool>,

        /// Filter by JACS verification status
        #[arg(long)]
        jacs_verified: Option<bool>,

        /// Filter by folder
        #[arg(long)]
        folder: Option<String>,

        /// Filter by label/tag
        #[arg(long)]
        label: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Reply to an email message
    ReplyEmail {
        /// Message ID to reply to
        #[arg(long)]
        message_id: String,

        /// Reply body text
        #[arg(long)]
        body: String,

        /// Override the Re: subject line
        #[arg(long)]
        subject_override: Option<String>,
    },

    /// Forward an email message to another recipient
    ForwardEmail {
        /// Message ID to forward
        #[arg(long)]
        message_id: String,

        /// Recipient email address
        #[arg(long)]
        to: String,

        /// Optional comment to include above the forwarded message
        #[arg(long)]
        comment: Option<String>,
    },

    /// Archive an email message (move to archive folder)
    ArchiveMessage {
        /// Message ID to archive
        message_id: String,
    },

    /// Unarchive an email message (move back to inbox)
    UnarchiveMessage {
        /// Message ID to unarchive
        message_id: String,
    },

    /// List contacts derived from email history
    ListContacts,

    /// Get email account status including usage limits
    EmailStatus,

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

    /// Search embedded JACS and HAI documentation
    SelfKnowledge {
        /// Search query
        query: String,

        /// Maximum results to return
        #[arg(long, default_value = "5")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage email templates (create, list, get, update, delete)
    Template {
        #[command(subcommand)]
        command: TemplateCommands,
    },

    /// Manage the OS keychain password for your agent's private key
    Keychain {
        /// Agent ID to scope the keychain entry (e.g. your JACS ID)
        #[arg(long)]
        agent_id: String,

        #[command(subcommand)]
        action: KeychainAction,
    },
}

#[derive(Subcommand)]
enum TemplateCommands {
    /// Create a new email template
    Create {
        /// Template name (required)
        #[arg(long)]
        name: String,

        /// Instructions for how to send emails using this template
        #[arg(long)]
        how_to_send: Option<String>,

        /// Instructions for how to respond to emails matching this template
        #[arg(long)]
        how_to_respond: Option<String>,

        /// Goal or purpose of this template
        #[arg(long)]
        goal: Option<String>,

        /// Rules or constraints for this template
        #[arg(long)]
        rules: Option<String>,
    },

    /// List email templates with optional search
    List {
        /// Full-text search query
        #[arg(long)]
        query: Option<String>,

        /// Maximum number of templates to return
        #[arg(long, default_value = "20")]
        limit: u32,

        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: u32,
    },

    /// Get a single email template by ID
    Get {
        /// Template ID
        template_id: String,
    },

    /// Update an existing email template
    Update {
        /// Template ID
        template_id: String,

        /// New template name
        #[arg(long)]
        name: Option<String>,

        /// Updated instructions for how to send emails
        #[arg(long)]
        how_to_send: Option<String>,

        /// Updated instructions for how to respond to emails
        #[arg(long)]
        how_to_respond: Option<String>,

        /// Updated goal or purpose
        #[arg(long)]
        goal: Option<String>,

        /// Updated rules or constraints
        #[arg(long)]
        rules: Option<String>,
    },

    /// Delete an email template
    Delete {
        /// Template ID
        template_id: String,
    },
}

#[derive(Subcommand)]
enum KeychainAction {
    /// Store the private key password in the OS keychain
    Set {
        /// Password to store (omit to be prompted)
        #[arg(long)]
        password: Option<String>,
    },
    /// Retrieve the stored password from the OS keychain
    Get,
    /// Delete the stored password from the OS keychain
    Delete,
    /// Check if the OS keychain is available and has a stored password
    Status,
}

/// Resolve the effective `--storage` value, considering `--storage-env`.
///
/// If `--storage-env VARNAME` was passed, read the label from that env var.
/// Otherwise fall through to the explicit `--storage` flag (which may be None).
fn resolve_storage_flag(
    storage: Option<&str>,
    storage_env: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if let Some(var_name) = storage_env {
        let label = std::env::var(var_name).with_context(|| {
            format!(
                "--storage-env: environment variable '{}' is not set",
                var_name
            )
        })?;
        if label.is_empty() {
            anyhow::bail!(
                "--storage-env: environment variable '{}' is set but empty",
                var_name
            );
        }
        return Ok(Some(label));
    }
    Ok(storage.map(|s| s.to_string()))
}

fn hai_url() -> String {
    std::env::var("HAI_URL").unwrap_or_else(|_| haiai::DEFAULT_BASE_URL.to_string())
}

/// Read and trim a password from a file path.
fn read_password_file(path: &str) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read password file: {}", path))?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("password file is empty: {}", path);
    }
    Ok(trimmed)
}

/// Resolve password for agent creation: check --password-file, then
/// JACS_PRIVATE_KEY_PASSWORD env var, then prompt twice on stdin when it's a
/// TTY (hidden input). Non-interactive runs must set the env var or use
/// --password-file.
fn resolve_init_password(password_file: Option<&str>) -> anyhow::Result<String> {
    // 1. --password-file takes highest priority
    if let Some(path) = password_file {
        return read_password_file(path);
    }
    // 2. Environment variable
    if let Ok(pass) = std::env::var("JACS_PRIVATE_KEY_PASSWORD") {
        if !pass.is_empty() {
            return Ok(pass);
        }
    }
    // 3. Interactive prompt
    if !atty::is(atty::Stream::Stdin) {
        anyhow::bail!(
            "Password is required for agent creation. \
            Set the JACS_PRIVATE_KEY_PASSWORD environment variable, \
            pass --password-file /path/to/file, \
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
///
/// When `password_file` is provided, the password is read from that file and set
/// as the env var, bypassing both the env-var check and the interactive prompt.
fn ensure_agent_password(quiet: bool, password_file: Option<&str>) -> anyhow::Result<()> {
    // --password-file takes highest priority: read, set env var, done.
    if let Some(path) = password_file {
        let pass = read_password_file(path)?;
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", &pass);
        return Ok(());
    }
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
                Set it to the password for your private key, pass --password-file /path/to/file, \
                or run haiai from a terminal to be prompted."
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
/// or defaults to `https://beta.hai.ai`.
fn load_client() -> anyhow::Result<HaiClient<LocalJacsProvider>> {
    let provider = LocalJacsProvider::from_config_path(None, None)
        .context("failed to load JACS agent from config")?;
    let cached_email = provider.agent_email_from_config();
    let options = HaiClientOptions {
        base_url: hai_url(),
        client_identifier: Some(format!("haiai-cli/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    let mut client =
        HaiClient::new(provider, options).context("failed to construct HaiClient")?;
    if let Some(email) = cached_email {
        client.set_agent_email(email);
    }
    Ok(client)
}

/// Load client and resolve the agent email address.
/// Uses cached email from config; falls back to server and persists on first fetch.
async fn load_client_with_email() -> anyhow::Result<HaiClient<LocalJacsProvider>> {
    let provider = LocalJacsProvider::from_config_path(None, None)
        .context("failed to load JACS agent from config")?;
    let cached_email = provider.agent_email_from_config();
    let config_path = provider.config_path().to_path_buf();
    let options = HaiClientOptions {
        base_url: hai_url(),
        client_identifier: Some(format!("haiai-cli/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    let mut client =
        HaiClient::new(provider, options).context("failed to construct HaiClient")?;

    if let Some(email) = cached_email {
        client.set_agent_email(email);
    } else if let Ok(status) = client.get_email_status().await {
        if !status.email.is_empty() {
            let write_provider = LocalJacsProvider::from_config_path(
                Some(config_path.as_path()),
                None,
            );
            if let Ok(wp) = write_provider {
                let _ = wp.update_config_email(&status.email);
            }
            client.set_agent_email(status.email);
        }
    }
    Ok(client)
}

/// Load a local JACS provider with document storage configured.
fn load_provider_with_storage(storage_flag: Option<&str>) -> anyhow::Result<LocalJacsProvider> {
    let label = haiai::resolve_storage_backend(storage_flag, None)
        .context("failed to resolve storage backend")?;
    LocalJacsProvider::from_config_path(None, Some(&label))
        .context("failed to load JACS agent with storage")
}

/// Print a table of email messages in a consistent format.
fn print_message_table(messages: &[haiai::EmailMessage]) {
    if messages.is_empty() {
        println!("No messages.");
        return;
    }
    println!(
        "{:<38} {:<9} {:<28} {:<28} {:<40} {:<20} {:<5}",
        "ID", "DIRECTION", "FROM", "TO", "SUBJECT", "DATE", "READ"
    );
    println!("{}", "-".repeat(170));
    for msg in messages {
        let subject = if msg.subject.len() > 38 {
            format!("{}...", &msg.subject[..35])
        } else {
            msg.subject.clone()
        };
        let read = if msg.is_read { "yes" } else { "no" };
        println!(
            "{:<38} {:<9} {:<28} {:<28} {:<40} {:<20} {:<5}",
            msg.id, msg.direction, msg.from_address, msg.to_address, subject, msg.created_at, read,
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolve effective storage label from --storage or --storage-env.
    let effective_storage =
        resolve_storage_flag(cli.storage.as_deref(), cli.storage_env.as_deref())
            .context("failed to resolve storage flag")?;

    // Commands that load an existing agent need the private key password. Prompt once if not set and not -q.
    if !matches!(
        cli.command,
        Commands::Init { .. } | Commands::SelfKnowledge { .. }
    ) {
        ensure_agent_password(cli.quiet, cli.password_file.as_deref())
            .context("failed to resolve private key password")?;
    }

    match cli.command {
        Commands::Init {
            name,
            key,
            domain,
            register,
            algorithm,
            data_dir,
            key_dir,
            config_path,
        } => {
            // Validate --name against username format rules
            let name_lower = name.to_lowercase();
            if name_lower.len() < 3 || name_lower.len() > 30 {
                anyhow::bail!("Invalid username '{}': must be 3-30 lowercase alphanumeric characters or hyphens, no leading/trailing hyphens.", name);
            }
            if name_lower.starts_with('-') || name_lower.ends_with('-') {
                anyhow::bail!("Invalid username '{}': must be 3-30 lowercase alphanumeric characters or hyphens, no leading/trailing hyphens.", name);
            }
            if !name_lower.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                anyhow::bail!("Invalid username '{}': must be 3-30 lowercase alphanumeric characters or hyphens, no leading/trailing hyphens.", name);
            }

            // When register=true, --key is required
            if register {
                if key.is_none() {
                    anyhow::bail!(
                        "Registration key is required. Log in at https://hai.ai, reserve your username, and copy the registration key from your dashboard."
                    );
                }
                let k = key.as_ref().unwrap();
                if !k.starts_with("hk_") || k.len() != 67 || !k[3..].chars().all(|c| c.is_ascii_hexdigit()) {
                    anyhow::bail!(
                        "Invalid registration key format. Keys start with 'hk_' followed by 64 hex characters."
                    );
                }
            }

            let password_resolved = resolve_init_password(cli.password_file.as_deref())?;
            let mut options = CreateAgentOptions {
                name: name_lower.clone(),
                password: password_resolved,
                algorithm: Some(algorithm),
                data_directory: Some(data_dir),
                key_directory: Some(key_dir),
                config_path: Some(config_path),
                ..Default::default()
            };
            if let Some(ref d) = domain {
                options.domain = Some(d.clone());
            }

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

            if register {
                println!("\nRegistering with HAI...");
                // Load the created agent and register
                let provider = LocalJacsProvider::from_config_path(
                    Some(std::path::Path::new(&result.config_path)),
                    effective_storage.as_deref(),
                )
                .context("failed to load created agent")?;
                let agent_json = provider
                    .export_agent_json()
                    .context("failed to export agent JSON")?;
                let public_key_pem = provider
                    .public_key_pem()
                    .context("failed to read public key PEM")?;

                let hai_options = HaiClientOptions {
                    base_url: hai_url(),
                    client_identifier: Some(format!("haiai-cli/{}", env!("CARGO_PKG_VERSION"))),
                    ..Default::default()
                };
                let client = HaiClient::new(provider, hai_options)
                    .context("failed to construct HaiClient")?;

                let reg_options = RegisterAgentOptions {
                    agent_json,
                    public_key_pem: Some(public_key_pem),
                    domain: domain.clone(),
                    owner_email: None,
                    is_mediator: Some(false),
                    registration_key: key,
                    ..Default::default()
                };

                match client.register(&reg_options).await {
                    Ok(response) => {
                        println!("Agent '{}' registered. Email: {}@hai.ai", name_lower, name_lower);
                        println!("  Registration ID: {}", response.agent_id);
                        // Persist the email address to config so future
                        // invocations skip the GET /email/status round-trip.
                        if let Some(ref email) = response.email {
                            if let Ok(wp) = LocalJacsProvider::from_config_path(
                                Some(std::path::Path::new(&result.config_path)),
                                None,
                            ) {
                                let _ = wp.update_config_email(email);
                            }
                        }
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("already registered") {
                            println!("This agent is already registered. If you need new keys, use 'haiai rotate'.");
                        } else {
                            eprintln!("Registration failed: {}. Your agent was created locally. Fix connectivity and run 'haiai init --name {} --key <key>' again.", msg, name_lower);
                        }
                    }
                }
            } else {
                println!("Agent '{}' created locally.", name_lower);
                println!("\nStart the MCP server with: haiai mcp");
            }
        }

        Commands::Mcp => {
            {
                use tracing_subscriber::layer::SubscriberExt;
                use tracing_subscriber::util::SubscriberInitExt;

                let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,rmcp=warn".parse().unwrap());

                let stderr_layer = tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr);

                let registry = tracing_subscriber::registry()
                    .with(env_filter)
                    .with(stderr_layer);

                if let Some(ref log_path) = cli.log_file {
                    let file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(log_path)
                        .with_context(|| format!("failed to open log file: {log_path}"))?;
                    let file_layer = tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(std::sync::Mutex::new(file));
                    registry.with(file_layer).init();
                } else {
                    registry.with(None::<tracing_subscriber::fmt::Layer<_>>).init();
                };
            }

            // Honor JACS_STORAGE env var and --storage / --storage-env flags for MCP document
            // operations (PRD Section 5.2).
            let storage_summary =
                haiai::redacted_display(effective_storage.as_deref(), None);
            tracing::info!(
                backend = %storage_summary.backend,
                source = storage_summary.source,
                "MCP storage backend resolved"
            );

            let shared_agent = LoadedSharedAgent::load_from_config_env()
                .context("failed to load JACS agent for haiai mcp")?;
            let provider = shared_agent
                .embedded_provider()
                .context("failed to construct embedded HAIAI provider from JACS agent")?;
            let fallback_jacs_id = provider.jacs_id().to_string();
            let default_config_path = Some(shared_agent.config_path().display().to_string());

            let context =
                HaiServerContext::from_process_env(fallback_jacs_id.clone(), default_config_path, provider);
            // Pre-populate the email cache from the config file so the MCP
            // skips the GET /email/status round-trip when email is known.
            if let Some(email) = shared_agent.agent_email() {
                context.remember_agent_email(&fallback_jacs_id, email);
            }
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

        Commands::SendEmail {
            to,
            subject,
            body,
            cc,
            bcc,
            labels,
        } => {
            let client = load_client_with_email().await?;
            let options = SendEmailOptions {
                to,
                subject,
                body,
                cc,
                bcc,
                in_reply_to: None,
                attachments: vec![],
                labels,
                append_footer: None,
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
            is_read,
            folder,
            label,
        } => {
            let client = load_client()?;
            let options = ListMessagesOptions {
                limit: Some(limit),
                offset: Some(offset),
                direction,
                is_read,
                folder,
                label,
                ..Default::default()
            };
            let messages = client
                .list_messages(&options)
                .await
                .context("list messages failed")?;
            print_message_table(&messages);
        }

        Commands::SearchMessages {
            q,
            from,
            to,
            is_read,
            jacs_verified,
            folder,
            label,
            limit,
        } => {
            let client = load_client()?;
            let options = SearchOptions {
                q,
                from_address: from,
                to_address: to,
                is_read,
                jacs_verified,
                folder,
                label,
                limit: Some(limit),
                ..Default::default()
            };
            let messages = client
                .search_messages(&options)
                .await
                .context("search messages failed")?;
            print_message_table(&messages);
        }

        Commands::ReplyEmail {
            message_id,
            body,
            subject_override,
        } => {
            let client = load_client_with_email().await?;
            let result = client
                .reply(&message_id, &body, subject_override.as_deref())
                .await
                .context("reply failed")?;
            println!("  Message ID: {}", result.message_id);
            println!("  Status:     {}", result.status);
        }

        Commands::ForwardEmail {
            message_id,
            to,
            comment,
        } => {
            let client = load_client_with_email().await?;
            let result = client
                .forward(&message_id, &to, comment.as_deref())
                .await
                .context("forward failed")?;
            println!("  Message ID: {}", result.message_id);
            println!("  Status:     {}", result.status);
        }

        Commands::ArchiveMessage { message_id } => {
            let client = load_client()?;
            client
                .archive(&message_id)
                .await
                .context("archive failed")?;
            println!("  Archived: {}", message_id);
        }

        Commands::UnarchiveMessage { message_id } => {
            let client = load_client()?;
            client
                .unarchive(&message_id)
                .await
                .context("unarchive failed")?;
            println!("  Unarchived: {}", message_id);
        }

        Commands::ListContacts => {
            let client = load_client_with_email().await?;
            let contacts = client
                .contacts()
                .await
                .context("list contacts failed")?;
            if contacts.is_empty() {
                println!("No contacts.");
            } else {
                println!(
                    "{:<30} {:<25} {:<20} {:<8} {:<10}",
                    "EMAIL", "DISPLAY NAME", "LAST CONTACT", "JACS", "REPUTATION"
                );
                println!("{}", "-".repeat(93));
                for c in &contacts {
                    println!(
                        "{:<30} {:<25} {:<20} {:<8} {:<10}",
                        c.email,
                        c.display_name.as_deref().unwrap_or("-"),
                        c.last_contact,
                        if c.jacs_verified { "yes" } else { "no" },
                        c.reputation_tier.as_deref().unwrap_or("-"),
                    );
                }
            }
        }

        Commands::EmailStatus => {
            let client = load_client()?;
            let status = client
                .get_email_status()
                .await
                .context("email status failed")?;
            println!("  Email:       {}", status.email);
            println!("  Status:      {}", status.status);
            println!("  Tier:        {}", status.tier);
            println!("  Daily Used:  {}/{}", status.daily_used, status.daily_limit);
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
                println!("  Re-registered: no (run `haiai init --name <name> --key <key>` to re-register)");
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
                println!("  Re-registered:  no (run `haiai init --name <name> --key <key>` to re-register)");
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
            let provider = load_provider_with_storage(effective_storage.as_deref())?;

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
            let storage_label = haiai::resolve_storage_backend(effective_storage.as_deref(), None)
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
            let provider = load_provider_with_storage(effective_storage.as_deref())?;
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
            let provider = load_provider_with_storage(effective_storage.as_deref())?;
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
            let provider = load_provider_with_storage(effective_storage.as_deref())?;
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
            let provider = load_provider_with_storage(effective_storage.as_deref())?;
            let json = provider
                .get_document(&key)
                .context("get_document failed")?;
            println!("{}", json);
        }

        Commands::RemoveDocument { key } => {
            let provider = load_provider_with_storage(effective_storage.as_deref())?;
            provider
                .remove_document(&key)
                .context("remove_document failed")?;
            println!("Document removed: {}", key);
        }

        Commands::SelfKnowledge { query, limit, json } => {
            let results = haiai::self_knowledge::self_knowledge(&query, limit);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results)
                        .context("failed to serialize results")?
                );
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                for result in &results {
                    println!(
                        "[{}] {} (score: {:.2})",
                        result.rank, result.title, result.score
                    );
                    println!("    Source: {}", result.path);
                    println!("    ---");
                    let snippet = if result.content.len() > 500 {
                        let mut end = 497;
                        while !result.content.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &result.content[..end])
                    } else {
                        result.content.clone()
                    };
                    for line in snippet.lines() {
                        println!("    {}", line);
                    }
                    println!("    ---");
                    println!();
                }
            }
        }

        Commands::Template { command } => {
            let client = load_client()?;
            match command {
                TemplateCommands::Create {
                    name,
                    how_to_send,
                    how_to_respond,
                    goal,
                    rules,
                } => {
                    let result = client
                        .create_email_template(&CreateEmailTemplateOptions {
                            name,
                            how_to_send,
                            how_to_respond,
                            goal,
                            rules,
                        })
                        .await
                        .context("create template failed")?;
                    println!("Created template:");
                    println!("  ID:   {}", result.id);
                    println!("  Name: {}", result.name);
                }
                TemplateCommands::List {
                    query,
                    limit,
                    offset,
                } => {
                    let result = client
                        .list_email_templates(&ListEmailTemplatesOptions {
                            q: query,
                            limit: Some(limit),
                            offset: Some(offset),
                        })
                        .await
                        .context("list templates failed")?;
                    println!(
                        "Templates ({} of {}):",
                        result.templates.len(),
                        result.total
                    );
                    for t in &result.templates {
                        println!("  {} — {}", t.id, t.name);
                    }
                }
                TemplateCommands::Get { template_id } => {
                    let result = client
                        .get_email_template(&template_id)
                        .await
                        .context("get template failed")?;
                    println!("Template: {}", result.name);
                    println!("  ID:             {}", result.id);
                    if let Some(ref v) = result.how_to_send {
                        println!("  How to send:    {v}");
                    }
                    if let Some(ref v) = result.how_to_respond {
                        println!("  How to respond: {v}");
                    }
                    if let Some(ref v) = result.goal {
                        println!("  Goal:           {v}");
                    }
                    if let Some(ref v) = result.rules {
                        println!("  Rules:          {v}");
                    }
                    println!("  Created:        {}", result.created_at);
                    println!("  Updated:        {}", result.updated_at);
                }
                TemplateCommands::Update {
                    template_id,
                    name,
                    how_to_send,
                    how_to_respond,
                    goal,
                    rules,
                } => {
                    let result = client
                        .update_email_template(
                            &template_id,
                            &UpdateEmailTemplateOptions {
                                name,
                                how_to_send: how_to_send.map(Some),
                                how_to_respond: how_to_respond.map(Some),
                                goal: goal.map(Some),
                                rules: rules.map(Some),
                            },
                        )
                        .await
                        .context("update template failed")?;
                    println!("Updated template:");
                    println!("  ID:   {}", result.id);
                    println!("  Name: {}", result.name);
                }
                TemplateCommands::Delete { template_id } => {
                    client
                        .delete_email_template(&template_id)
                        .await
                        .context("delete template failed")?;
                    println!("Deleted template {template_id}");
                }
            }
        }

        Commands::Keychain { agent_id, action } => {
            use jacs::keystore::keychain;

            match action {
                KeychainAction::Set { password } => {
                    let pass = match password {
                        Some(p) => p,
                        None => {
                            if !atty::is(atty::Stream::Stdin) {
                                anyhow::bail!("No password provided. Use --password or run from a terminal.");
                            }
                            eprintln!("Enter password to store in keychain:");
                            rpassword::read_password().context("failed to read password")?
                        }
                    };
                    if pass.is_empty() {
                        anyhow::bail!("Password cannot be empty.");
                    }
                    keychain::store_password(&agent_id, &pass)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("Password stored in OS keychain.");
                }
                KeychainAction::Get => {
                    match keychain::get_password(&agent_id) {
                        Ok(Some(p)) => println!("{p}"),
                        Ok(None) => {
                            eprintln!("No password stored in OS keychain.");
                            std::process::exit(1);
                        }
                        Err(e) => anyhow::bail!("Keychain error: {e}"),
                    }
                }
                KeychainAction::Delete => {
                    keychain::delete_password(&agent_id)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("Password deleted from OS keychain.");
                }
                KeychainAction::Status => {
                    let available = keychain::is_available();
                    let has_password = keychain::get_password(&agent_id)
                        .map(|p| p.is_some())
                        .unwrap_or(false);
                    println!("Keychain available: {available}");
                    println!("Password stored:    {has_password}");
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::collections::HashSet;

    #[test]
    fn cli_help_does_not_panic() {
        // Verify the CLI definition is well-formed and --help can render.
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_commands_match_fixture() {
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/cli_command_parity.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("read cli_command_parity.json");
        let fixture: serde_json::Value =
            serde_json::from_str(&raw).expect("parse cli_command_parity.json");

        // Extract fixture command names
        let fixture_commands: HashSet<String> = fixture["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .filter_map(|c| c["name"].as_str().map(String::from))
            .collect();

        // Extract actual command names from Clap introspection
        let cli_cmd = Cli::command();
        let actual_commands: HashSet<String> = cli_cmd
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();

        // Bidirectional check: fixture -> code
        let fixture_only: Vec<&String> = fixture_commands.difference(&actual_commands).collect();
        assert!(
            fixture_only.is_empty(),
            "Commands in fixture but not in CLI binary: {:?}",
            fixture_only
        );

        // Bidirectional check: code -> fixture
        let code_only: Vec<&String> = actual_commands.difference(&fixture_commands).collect();
        assert!(
            code_only.is_empty(),
            "Commands in CLI binary but not in fixture: {:?}",
            code_only
        );

        // Sub-subcommand parity check
        for cmd in fixture["commands"].as_array().unwrap() {
            if let Some(subcmds) = cmd.get("subcommands").and_then(serde_json::Value::as_array) {
                let name = cmd["name"].as_str().unwrap();
                let fixture_subs: HashSet<String> = subcmds
                    .iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect();
                let actual_subs: HashSet<String> = cli_cmd
                    .find_subcommand(name)
                    .unwrap_or_else(|| panic!("subcommand '{name}' not found"))
                    .get_subcommands()
                    .map(|s| s.get_name().to_string())
                    .collect();
                let fixture_only_subs: Vec<&String> =
                    fixture_subs.difference(&actual_subs).collect();
                assert!(
                    fixture_only_subs.is_empty(),
                    "Sub-subcommands of '{name}' in fixture but not in CLI: {:?}",
                    fixture_only_subs
                );
                let code_only_subs: Vec<&String> =
                    actual_subs.difference(&fixture_subs).collect();
                assert!(
                    code_only_subs.is_empty(),
                    "Sub-subcommands of '{name}' in CLI but not in fixture: {:?}",
                    code_only_subs
                );
            }
        }

        // Argument-level parity check
        // Collect the names of args defined on the top-level Cli struct
        // (these are global/parent-level args that Clap propagates into
        // subcommands). We exclude them from per-subcommand comparisons.
        let top_level_arg_names: HashSet<String> = Cli::command()
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();

        for cmd in fixture["commands"].as_array().unwrap() {
            let name = cmd["name"].as_str().unwrap();
            if let Some(args) = cmd.get("args").and_then(serde_json::Value::as_array) {
                if args.is_empty() {
                    continue;
                }
                // Extract arg names from fixture (strip :type suffix, normalize
                // snake_case to kebab-case for uniform comparison)
                let fixture_arg_names: HashSet<String> = args
                    .iter()
                    .filter_map(|a| a.as_str())
                    .map(|a| {
                        a.split(':')
                            .next()
                            .unwrap_or(a)
                            .replace('_', "-")
                            .to_string()
                    })
                    .collect();

                let subcmd = cli_cmd
                    .find_subcommand(name)
                    .unwrap_or_else(|| panic!("subcommand '{name}' not found"));
                // Extract actual arg names, filtering out help/version
                // (auto-generated) and top-level Cli args (global args that
                // Clap propagates into subcommands).
                // Normalize snake_case arg IDs to kebab-case for comparison.
                let actual_arg_names: HashSet<String> = subcmd
                    .get_arguments()
                    .filter(|a| {
                        let id = a.get_id().as_str();
                        id != "help" && id != "version" && !top_level_arg_names.contains(id)
                    })
                    .map(|a| a.get_id().as_str().replace('_', "-"))
                    .collect();

                let fixture_only_args: Vec<&String> =
                    fixture_arg_names.difference(&actual_arg_names).collect();
                assert!(
                    fixture_only_args.is_empty(),
                    "Args for command '{name}' in fixture but not in CLI: {:?}",
                    fixture_only_args
                );

                let code_only_args: Vec<&String> =
                    actual_arg_names.difference(&fixture_arg_names).collect();
                assert!(
                    code_only_args.is_empty(),
                    "Args for command '{name}' in CLI but not in fixture: {:?}",
                    code_only_args
                );
            }
        }

        // Total count check
        let declared = fixture["total_command_count"]
            .as_u64()
            .expect("total_command_count");
        assert_eq!(
            declared,
            actual_commands.len() as u64,
            "total_command_count ({declared}) != actual command count ({})",
            actual_commands.len()
        );
    }

    #[test]
    fn cli_fixture_total_command_count_matches_entries() {
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/cli_command_parity.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("read cli_command_parity.json");
        let fixture: serde_json::Value =
            serde_json::from_str(&raw).expect("parse cli_command_parity.json");

        let declared = fixture["total_command_count"]
            .as_u64()
            .expect("total_command_count");
        let actual = fixture["commands"]
            .as_array()
            .expect("commands array")
            .len() as u64;
        assert_eq!(
            declared, actual,
            "cli_command_parity.json total_command_count ({declared}) != commands array length ({actual})"
        );
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
            "--register=false",
        ]);
        match cli.command {
            Commands::Init {
                name,
                domain,
                algorithm,
                data_dir,
                key_dir,
                config_path,
                register,
                key,
            } => {
                assert_eq!(name, "myagent");
                assert_eq!(domain.as_deref(), Some("example.com"));
                assert_eq!(algorithm, "pq2025");
                assert_eq!(data_dir, "./jacs");
                assert_eq!(key_dir, "./jacs_keys");
                assert_eq!(config_path, "./jacs.config.json");
                assert!(!register);
                assert!(key.is_none());
            }
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parse_mcp() {
        let cli = Cli::parse_from(["haiai", "mcp"]);
        assert!(matches!(cli.command, Commands::Mcp));
        assert!(cli.log_file.is_none());
    }

    #[test]
    fn parse_mcp_with_log_file() {
        let cli = Cli::parse_from(["haiai", "--log-file", "/tmp/haiai-mcp.log", "mcp"]);
        assert!(matches!(cli.command, Commands::Mcp));
        assert_eq!(cli.log_file.as_deref(), Some("/tmp/haiai-mcp.log"));
    }

    #[test]
    fn parse_hello() {
        let cli = Cli::parse_from(["haiai", "hello"]);
        assert!(matches!(cli.command, Commands::Hello));
    }

    #[test]
    fn parse_init_with_key_and_register() {
        let cli = Cli::parse_from([
            "haiai", "init",
            "--name", "myagent",
            "--key", "hk_a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        ]);
        match cli.command {
            Commands::Init { name, key, register, domain, .. } => {
                assert_eq!(name, "myagent");
                assert!(key.is_some());
                assert!(register); // default true
                assert!(domain.is_none());
            }
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parse_init_register_false_no_key() {
        let cli = Cli::parse_from([
            "haiai", "init",
            "--name", "myagent",
            "--register=false",
        ]);
        match cli.command {
            Commands::Init { name, key, register, .. } => {
                assert_eq!(name, "myagent");
                assert!(key.is_none());
                assert!(!register);
            }
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parse_init_domain_optional() {
        let cli = Cli::parse_from([
            "haiai", "init",
            "--name", "myagent",
            "--key", "hk_a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            "--domain", "example.com",
        ]);
        match cli.command {
            Commands::Init { domain, .. } => {
                assert_eq!(domain.as_deref(), Some("example.com"));
            }
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parse_removed_commands_fail() {
        assert!(Cli::try_parse_from(["haiai", "register", "--owner-email", "a@b.com"]).is_err());
        assert!(Cli::try_parse_from(["haiai", "claim-username", "bob"]).is_err());
        assert!(Cli::try_parse_from(["haiai", "check-username", "alice"]).is_err());
    }

    #[test]
    fn parse_status() {
        let cli = Cli::parse_from(["haiai", "status"]);
        assert!(matches!(cli.command, Commands::Status));
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
            Commands::SendEmail {
                to,
                subject,
                body,
                cc,
                bcc,
                labels,
            } => {
                assert_eq!(to, "friend@hai.ai");
                assert_eq!(subject, "Hello");
                assert_eq!(body, "Hi there!");
                assert!(cc.is_empty());
                assert!(bcc.is_empty());
                assert!(labels.is_empty());
            }
            _ => panic!("expected SendEmail command"),
        }
    }

    #[test]
    fn parse_send_email_with_cc_bcc_labels() {
        let cli = Cli::parse_from([
            "haiai",
            "send-email",
            "--to",
            "friend@hai.ai",
            "--subject",
            "Hello",
            "--body",
            "Hi",
            "--cc",
            "a@hai.ai",
            "--cc",
            "b@hai.ai",
            "--bcc",
            "secret@hai.ai",
            "--labels",
            "important",
            "--labels",
            "urgent",
        ]);
        match cli.command {
            Commands::SendEmail {
                cc,
                bcc,
                labels,
                ..
            } => {
                assert_eq!(cc, vec!["a@hai.ai", "b@hai.ai"]);
                assert_eq!(bcc, vec!["secret@hai.ai"]);
                assert_eq!(labels, vec!["important", "urgent"]);
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
                is_read,
                folder,
                label,
            } => {
                assert_eq!(limit, 20);
                assert_eq!(offset, 0);
                assert!(direction.is_none());
                assert!(is_read.is_none());
                assert!(folder.is_none());
                assert!(label.is_none());
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
                ..
            } => {
                assert_eq!(limit, 50);
                assert_eq!(offset, 10);
                assert_eq!(direction.as_deref(), Some("inbound"));
            }
            _ => panic!("expected ListMessages command"),
        }
    }

    #[test]
    fn parse_list_messages_with_filters() {
        let cli = Cli::parse_from([
            "haiai",
            "list-messages",
            "--is-read",
            "false",
            "--folder",
            "archive",
            "--label",
            "important",
        ]);
        match cli.command {
            Commands::ListMessages {
                is_read,
                folder,
                label,
                ..
            } => {
                assert_eq!(is_read, Some(false));
                assert_eq!(folder.as_deref(), Some("archive"));
                assert_eq!(label.as_deref(), Some("important"));
            }
            _ => panic!("expected ListMessages command"),
        }
    }

    #[test]
    fn parse_search_messages_defaults() {
        let cli = Cli::parse_from(["haiai", "search-messages"]);
        match cli.command {
            Commands::SearchMessages {
                q,
                from,
                to,
                is_read,
                jacs_verified,
                folder,
                label,
                limit,
            } => {
                assert!(q.is_none());
                assert!(from.is_none());
                assert!(to.is_none());
                assert!(is_read.is_none());
                assert!(jacs_verified.is_none());
                assert!(folder.is_none());
                assert!(label.is_none());
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
            Commands::SearchMessages { q, from, to, limit, .. } => {
                assert_eq!(q.as_deref(), Some("invoice"));
                assert_eq!(from.as_deref(), Some("sender@hai.ai"));
                assert_eq!(to.as_deref(), Some("me@hai.ai"));
                assert_eq!(limit, 5);
            }
            _ => panic!("expected SearchMessages command"),
        }
    }

    #[test]
    fn parse_search_messages_with_filters() {
        let cli = Cli::parse_from([
            "haiai",
            "search-messages",
            "--is-read",
            "true",
            "--jacs-verified",
            "true",
            "--folder",
            "inbox",
            "--label",
            "billing",
        ]);
        match cli.command {
            Commands::SearchMessages {
                is_read,
                jacs_verified,
                folder,
                label,
                ..
            } => {
                assert_eq!(is_read, Some(true));
                assert_eq!(jacs_verified, Some(true));
                assert_eq!(folder.as_deref(), Some("inbox"));
                assert_eq!(label.as_deref(), Some("billing"));
            }
            _ => panic!("expected SearchMessages command"),
        }
    }

    #[test]
    fn parse_reply_email() {
        let cli = Cli::parse_from([
            "haiai",
            "reply-email",
            "--message-id",
            "abc-123",
            "--body",
            "Thanks!",
        ]);
        match cli.command {
            Commands::ReplyEmail {
                message_id,
                body,
                subject_override,
            } => {
                assert_eq!(message_id, "abc-123");
                assert_eq!(body, "Thanks!");
                assert!(subject_override.is_none());
            }
            _ => panic!("expected ReplyEmail command"),
        }
    }

    #[test]
    fn parse_reply_email_with_subject_override() {
        let cli = Cli::parse_from([
            "haiai",
            "reply-email",
            "--message-id",
            "abc-123",
            "--body",
            "Thanks!",
            "--subject-override",
            "Custom Subject",
        ]);
        match cli.command {
            Commands::ReplyEmail {
                subject_override, ..
            } => {
                assert_eq!(subject_override.as_deref(), Some("Custom Subject"));
            }
            _ => panic!("expected ReplyEmail command"),
        }
    }

    #[test]
    fn parse_reply_email_missing_args_fails() {
        let result = Cli::try_parse_from(["haiai", "reply-email", "--message-id", "abc"]);
        assert!(result.is_err(), "reply-email without --body should fail");
    }

    #[test]
    fn parse_forward_email() {
        let cli = Cli::parse_from([
            "haiai",
            "forward-email",
            "--message-id",
            "abc-123",
            "--to",
            "other@hai.ai",
        ]);
        match cli.command {
            Commands::ForwardEmail {
                message_id,
                to,
                comment,
            } => {
                assert_eq!(message_id, "abc-123");
                assert_eq!(to, "other@hai.ai");
                assert!(comment.is_none());
            }
            _ => panic!("expected ForwardEmail command"),
        }
    }

    #[test]
    fn parse_forward_email_with_comment() {
        let cli = Cli::parse_from([
            "haiai",
            "forward-email",
            "--message-id",
            "abc-123",
            "--to",
            "other@hai.ai",
            "--comment",
            "FYI",
        ]);
        match cli.command {
            Commands::ForwardEmail { comment, .. } => {
                assert_eq!(comment.as_deref(), Some("FYI"));
            }
            _ => panic!("expected ForwardEmail command"),
        }
    }

    #[test]
    fn parse_forward_email_missing_args_fails() {
        let result = Cli::try_parse_from(["haiai", "forward-email", "--message-id", "abc"]);
        assert!(result.is_err(), "forward-email without --to should fail");
    }

    #[test]
    fn parse_archive_message() {
        let cli = Cli::parse_from(["haiai", "archive-message", "msg-123"]);
        match cli.command {
            Commands::ArchiveMessage { message_id } => {
                assert_eq!(message_id, "msg-123");
            }
            _ => panic!("expected ArchiveMessage command"),
        }
    }

    #[test]
    fn parse_archive_message_missing_arg_fails() {
        let result = Cli::try_parse_from(["haiai", "archive-message"]);
        assert!(
            result.is_err(),
            "archive-message without message_id should fail"
        );
    }

    #[test]
    fn parse_unarchive_message() {
        let cli = Cli::parse_from(["haiai", "unarchive-message", "msg-123"]);
        match cli.command {
            Commands::UnarchiveMessage { message_id } => {
                assert_eq!(message_id, "msg-123");
            }
            _ => panic!("expected UnarchiveMessage command"),
        }
    }

    #[test]
    fn parse_list_contacts() {
        let cli = Cli::parse_from(["haiai", "list-contacts"]);
        assert!(matches!(cli.command, Commands::ListContacts));
    }

    #[test]
    fn parse_email_status() {
        let cli = Cli::parse_from(["haiai", "email-status"]);
        assert!(matches!(cli.command, Commands::EmailStatus));
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

    #[test]
    fn parse_storage_env_flag() {
        let cli = Cli::parse_from(["haiai", "--storage-env", "MY_STORAGE", "list-documents"]);
        assert_eq!(cli.storage_env.as_deref(), Some("MY_STORAGE"));
        assert!(cli.storage.is_none());
        assert!(matches!(cli.command, Commands::ListDocuments { .. }));
    }

    #[test]
    fn storage_and_storage_env_conflict() {
        let result = Cli::try_parse_from([
            "haiai",
            "--storage",
            "fs",
            "--storage-env",
            "MY_VAR",
            "doctor",
        ]);
        assert!(
            result.is_err(),
            "--storage and --storage-env should conflict"
        );
    }

    #[test]
    fn parse_self_knowledge() {
        let cli = Cli::parse_from(["haiai", "self-knowledge", "key rotation"]);
        match cli.command {
            Commands::SelfKnowledge { query, limit, json } => {
                assert_eq!(query, "key rotation");
                assert_eq!(limit, 5);
                assert!(!json);
            }
            _ => panic!("expected SelfKnowledge command"),
        }
    }

    #[test]
    fn parse_self_knowledge_with_options() {
        let cli = Cli::parse_from([
            "haiai",
            "self-knowledge",
            "email signing",
            "--limit",
            "3",
            "--json",
        ]);
        match cli.command {
            Commands::SelfKnowledge { query, limit, json } => {
                assert_eq!(query, "email signing");
                assert_eq!(limit, 3);
                assert!(json);
            }
            _ => panic!("expected SelfKnowledge command"),
        }
    }

    #[test]
    fn resolve_storage_flag_from_env_var() {
        std::env::set_var("TEST_HAI_STORAGE_009", "rusqlite");
        let result = resolve_storage_flag(None, Some("TEST_HAI_STORAGE_009")).unwrap();
        assert_eq!(result.as_deref(), Some("rusqlite"));
        std::env::remove_var("TEST_HAI_STORAGE_009");
    }

    #[test]
    fn resolve_storage_flag_missing_env_var_errors() {
        std::env::remove_var("NONEXISTENT_STORAGE_VAR_XYZ");
        let result = resolve_storage_flag(None, Some("NONEXISTENT_STORAGE_VAR_XYZ"));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_storage_flag_explicit_wins() {
        let result = resolve_storage_flag(Some("sqlite"), None).unwrap();
        assert_eq!(result.as_deref(), Some("sqlite"));
    }

    #[test]
    fn resolve_storage_flag_none_returns_none() {
        let result = resolve_storage_flag(None, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_global_password_file_flag() {
        let cli = Cli::parse_from([
            "haiai",
            "--password-file",
            "/tmp/my-secret.txt",
            "hello",
        ]);
        assert_eq!(cli.password_file.as_deref(), Some("/tmp/my-secret.txt"));
        assert!(matches!(cli.command, Commands::Hello));
    }

    #[test]
    fn parse_password_file_with_init() {
        let cli = Cli::parse_from([
            "haiai",
            "--password-file",
            "/tmp/pw.txt",
            "init",
            "--name",
            "myagent",
            "--domain",
            "example.com",
        ]);
        assert_eq!(cli.password_file.as_deref(), Some("/tmp/pw.txt"));
        assert!(matches!(cli.command, Commands::Init { .. }));
    }

    #[test]
    fn read_password_file_reads_and_trims() {
        let dir = std::env::temp_dir().join("haiai_test_pw");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pw.txt");
        std::fs::write(&path, "  my-secret-password  \n").unwrap();
        let result = read_password_file(path.to_str().unwrap()).unwrap();
        assert_eq!(result, "my-secret-password");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_password_file_empty_file_errors() {
        let dir = std::env::temp_dir().join("haiai_test_pw");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, "  \n").unwrap();
        let result = read_password_file(path.to_str().unwrap());
        assert!(result.is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_password_file_missing_file_errors() {
        let result = read_password_file("/nonexistent/path/to/pw.txt");
        assert!(result.is_err());
    }

    #[test]
    fn mcp_cli_parity_fixture_covers_all_surfaces() {
        // Load the MCP-CLI parity fixture
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/mcp_cli_parity.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("read mcp_cli_parity.json");
        let fixture: serde_json::Value =
            serde_json::from_str(&raw).expect("parse mcp_cli_parity.json");

        // --- Collect real MCP tool names ---
        let real_mcp_tools: HashSet<String> = hai_mcp::hai_tools::definitions()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();

        // --- Collect real CLI command names ---
        let cli_cmd = Cli::command();
        let real_cli_commands: HashSet<String> = cli_cmd
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();

        // --- Extract fixture sections ---
        let paired = fixture["paired"]
            .as_array()
            .expect("paired array");
        let mcp_only = fixture["mcp_only"]
            .as_array()
            .expect("mcp_only array");
        let cli_only = fixture["cli_only"]
            .as_array()
            .expect("cli_only array");

        // Collect all MCP tools declared in the fixture (paired + mcp_only)
        let mut fixture_mcp_tools: HashSet<String> = HashSet::new();
        for entry in paired {
            fixture_mcp_tools.insert(
                entry["mcp_tool"].as_str().expect("mcp_tool string").to_string(),
            );
        }
        for entry in mcp_only {
            fixture_mcp_tools.insert(
                entry["name"].as_str().expect("name string").to_string(),
            );
        }

        // Collect all CLI commands declared in the fixture (paired + cli_only)
        let mut fixture_cli_commands: HashSet<String> = HashSet::new();
        for entry in paired {
            fixture_cli_commands.insert(
                entry["cli_command"].as_str().expect("cli_command string").to_string(),
            );
        }
        for entry in cli_only {
            fixture_cli_commands.insert(
                entry["name"].as_str().expect("name string").to_string(),
            );
        }

        // --- Verify every paired MCP tool exists in real MCP ---
        for entry in paired {
            let tool = entry["mcp_tool"].as_str().unwrap();
            assert!(
                real_mcp_tools.contains(tool),
                "Paired MCP tool '{}' does not exist in hai-mcp definitions",
                tool
            );
        }

        // --- Verify every paired CLI command exists in real CLI ---
        for entry in paired {
            let cmd = entry["cli_command"].as_str().unwrap();
            assert!(
                real_cli_commands.contains(cmd),
                "Paired CLI command '{}' does not exist in haiai-cli",
                cmd
            );
        }

        // --- Verify every mcp_only tool exists in real MCP ---
        for entry in mcp_only {
            let tool = entry["name"].as_str().unwrap();
            assert!(
                real_mcp_tools.contains(tool),
                "mcp_only tool '{}' does not exist in hai-mcp definitions",
                tool
            );
        }

        // --- Verify every cli_only command exists in real CLI ---
        for entry in cli_only {
            let cmd = entry["name"].as_str().unwrap();
            assert!(
                real_cli_commands.contains(cmd),
                "cli_only command '{}' does not exist in haiai-cli",
                cmd
            );
        }

        // --- Exhaustive coverage: no undeclared MCP tools ---
        let undeclared_mcp: Vec<&String> =
            real_mcp_tools.difference(&fixture_mcp_tools).collect();
        assert!(
            undeclared_mcp.is_empty(),
            "MCP tools exist but are not declared in mcp_cli_parity.json: {:?}\n\
             Add them to 'paired' or 'mcp_only'.",
            undeclared_mcp
        );

        // --- Exhaustive coverage: no undeclared CLI commands ---
        let undeclared_cli: Vec<&String> =
            real_cli_commands.difference(&fixture_cli_commands).collect();
        assert!(
            undeclared_cli.is_empty(),
            "CLI commands exist but are not declared in mcp_cli_parity.json: {:?}\n\
             Add them to 'paired' or 'cli_only'.",
            undeclared_cli
        );

        // --- No phantom entries in fixture ---
        let phantom_mcp: Vec<&String> =
            fixture_mcp_tools.difference(&real_mcp_tools).collect();
        assert!(
            phantom_mcp.is_empty(),
            "mcp_cli_parity.json references MCP tools that don't exist: {:?}",
            phantom_mcp
        );

        let phantom_cli: Vec<&String> =
            fixture_cli_commands.difference(&real_cli_commands).collect();
        assert!(
            phantom_cli.is_empty(),
            "mcp_cli_parity.json references CLI commands that don't exist: {:?}",
            phantom_cli
        );
    }
}
