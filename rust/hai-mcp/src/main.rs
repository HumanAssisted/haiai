use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use haisdk::{
    CreateAgentOptions, HaiClient, HaiClientOptions, JacsProvider, ListMessagesOptions,
    LocalJacsProvider, NoopJacsProvider, RegisterAgentOptions, SearchOptions, SendEmailOptions,
    generate_verify_link, generate_verify_link_hosted,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Clone)]
struct HaiServerContext {
    base_url: String,
    fallback_jacs_id: String,
}

impl HaiServerContext {
    fn from_env() -> Self {
        let base_url = std::env::var("HAI_URL").unwrap_or_else(|_| "https://hai.ai".to_string());
        let fallback_jacs_id =
            std::env::var("JACS_ID").unwrap_or_else(|_| "anonymous-agent".to_string());
        Self {
            base_url,
            fallback_jacs_id,
        }
    }

    fn noop_client_with_url(
        &self,
        base_url_override: Option<&str>,
    ) -> std::result::Result<HaiClient<NoopJacsProvider>, String> {
        let provider = NoopJacsProvider::new(self.fallback_jacs_id.clone());
        self.client_with_provider(provider, base_url_override)
    }

    fn local_provider(
        &self,
        config_path: Option<&str>,
    ) -> std::result::Result<LocalJacsProvider, String> {
        LocalJacsProvider::from_config_path(config_path.map(Path::new)).map_err(|e| {
            format!("failed to load local JACS agent; set JACS_CONFIG or pass config_path: {e}")
        })
    }

    fn local_client_with_url(
        &self,
        config_path: Option<&str>,
        base_url_override: Option<&str>,
    ) -> std::result::Result<HaiClient<LocalJacsProvider>, String> {
        let provider = self.local_provider(config_path)?;
        self.client_with_provider(provider, base_url_override)
    }

    fn client_with_provider<P: JacsProvider>(
        &self,
        provider: P,
        base_url_override: Option<&str>,
    ) -> std::result::Result<HaiClient<P>, String> {
        let base_url = base_url_override.unwrap_or(&self.base_url).to_string();
        HaiClient::new(
            provider,
            HaiClientOptions {
                base_url,
                ..HaiClientOptions::default()
            },
        )
        .map_err(|e| e.to_string())
    }
}

#[async_trait]
trait JacsmcpBridge: Send + Sync {
    async fn list_tools(&self) -> Vec<Value>;
    async fn call_tool(
        &self,
        name: &str,
        args: &Value,
    ) -> Option<std::result::Result<Value, String>>;
}

struct NoopJacsmcpBridge;

#[async_trait]
impl JacsmcpBridge for NoopJacsmcpBridge {
    async fn list_tools(&self) -> Vec<Value> {
        Vec::new()
    }

    async fn call_tool(
        &self,
        _name: &str,
        _args: &Value,
    ) -> Option<std::result::Result<Value, String>> {
        None
    }
}

struct BridgeCandidate {
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
}

struct ChildSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl ChildSession {
    async fn spawn(candidate: &BridgeCandidate) -> std::result::Result<Self, String> {
        let mut command = Command::new(&candidate.command);
        command
            .args(&candidate.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        if let Some(cwd) = &candidate.cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|e| {
            format!(
                "failed to spawn bridge command '{} {}': {e}",
                candidate.command,
                candidate.args.join(" ")
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to capture jacs-mcp stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture jacs-mcp stdout".to_string())?;

        let mut session = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        };
        session.initialize().await?;

        Ok(session)
    }

    async fn initialize(&mut self) -> std::result::Result<(), String> {
        let _ = self
            .request(
                "initialize",
                Some(json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "hai-mcp-bridge",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                })),
            )
            .await?;

        self.notify("notifications/initialized", None).await
    }

    async fn notify(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<(), String> {
        let mut request = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(params) = params {
            request["params"] = params;
        }

        let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("failed writing notification to jacs-mcp: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("failed writing newline to jacs-mcp: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("failed flushing jacs-mcp stdin: {e}"))?;
        Ok(())
    }

    async fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> std::result::Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let mut request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(params) = params {
            request["params"] = params;
        }

        let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("failed writing request to jacs-mcp: {e}"))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("failed writing newline to jacs-mcp: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("failed flushing jacs-mcp stdin: {e}"))?;

        loop {
            let maybe_line = self
                .stdout
                .next_line()
                .await
                .map_err(|e| format!("failed reading jacs-mcp response: {e}"))?;
            let line = maybe_line.ok_or_else(|| "jacs-mcp closed stdio".to_string())?;
            if line.trim().is_empty() {
                continue;
            }

            let value = match serde_json::from_str::<Value>(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if !id_matches(&value, id) {
                continue;
            }

            if let Some(error) = value.get("error") {
                return Err(rpc_error_message(error));
            }

            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

struct SubprocessJacsmcpBridge {
    session: Mutex<ChildSession>,
}

impl SubprocessJacsmcpBridge {
    async fn connect_from_env() -> std::result::Result<Self, String> {
        let mut failures = Vec::new();

        for candidate in bridge_candidates() {
            match ChildSession::spawn(&candidate).await {
                Ok(session) => {
                    return Ok(Self {
                        session: Mutex::new(session),
                    });
                }
                Err(err) => {
                    failures.push(format!(
                        "{} {} ({err})",
                        candidate.command,
                        candidate.args.join(" ")
                    ));
                }
            }
        }

        Err(format!(
            "unable to start jacs-mcp bridge; tried: {}",
            failures.join(" | ")
        ))
    }
}

#[async_trait]
impl JacsmcpBridge for SubprocessJacsmcpBridge {
    async fn list_tools(&self) -> Vec<Value> {
        let mut session = self.session.lock().await;
        match session.request("tools/list", Some(json!({}))).await {
            Ok(result) => result
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            Err(err) => {
                eprintln!("failed to list jacs-mcp tools: {err}");
                Vec::new()
            }
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        args: &Value,
    ) -> Option<std::result::Result<Value, String>> {
        if !name.starts_with("jacs_") {
            return None;
        }

        let mut session = self.session.lock().await;
        Some(
            session
                .request(
                    "tools/call",
                    Some(json!({
                        "name": name,
                        "arguments": args
                    })),
                )
                .await,
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let context = HaiServerContext::from_env();

    let bridge: Arc<dyn JacsmcpBridge> = match SubprocessJacsmcpBridge::connect_from_env().await {
        Ok(bridge) => Arc::new(bridge),
        Err(err) => {
            eprintln!("warning: {err}");
            Arc::new(NoopJacsmcpBridge)
        }
    };

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(req) => req,
            Err(err) => {
                let response = RpcResponse {
                    jsonrpc: "2.0",
                    id: Value::Null,
                    result: None,
                    error: Some(RpcError {
                        code: -32700,
                        message: format!("parse error: {err}"),
                    }),
                };
                let out = serde_json::to_string(&response)?;
                stdout.write_all(out.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        if let Some(response) = handle_request(&context, bridge.as_ref(), request).await {
            let out = serde_json::to_string(&response)?;
            stdout.write_all(out.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

async fn handle_request(
    context: &HaiServerContext,
    bridge: &dyn JacsmcpBridge,
    request: RpcRequest,
) -> Option<RpcResponse> {
    if request.id.is_none() && request.method.starts_with("notifications/") {
        return None;
    }

    let id = request.id.unwrap_or(Value::Null);

    match request.method.as_str() {
        "initialize" => Some(RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "serverInfo": {
                    "name": "hai-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            error: None,
        }),
        "ping" => Some(RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({})),
            error: None,
        }),
        "tools/list" => {
            let mut dedup = BTreeMap::new();
            for tool in hai_tool_definitions() {
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    dedup.insert(name.to_string(), tool);
                }
            }
            for tool in bridge.list_tools().await {
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    dedup.insert(name.to_string(), tool);
                }
            }

            let tools: Vec<Value> = dedup.into_values().collect();
            Some(RpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(json!({ "tools": tools })),
                error: None,
            })
        }
        "tools/call" => {
            let params = request.params.unwrap_or(Value::Null);
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);

            let result = match name {
                "hai_check_username" => call_check_username(context, &args).await,
                "hai_hello" => call_hello(context, &args).await,
                "hai_agent_status" => call_verify_status(context, &args).await,
                "hai_verify_status" => call_verify_status(context, &args).await,
                "hai_claim_username" => call_claim_username(context, &args).await,
                "hai_create_agent" => call_create_agent(context, &args).await,
                "hai_register_agent" => call_register_agent(context, &args).await,
                "hai_generate_verify_link" => call_generate_verify_link(&args).await,
                "hai_send_email" => call_send_email(context, &args).await,
                "hai_list_messages" => call_list_messages(context, &args).await,
                "hai_get_message" => call_get_message(context, &args).await,
                "hai_delete_message" => call_delete_message(context, &args).await,
                "hai_mark_read" => call_mark_read(context, &args).await,
                "hai_mark_unread" => call_mark_unread(context, &args).await,
                "hai_search_messages" => call_search_messages(context, &args).await,
                "hai_get_unread_count" => call_get_unread_count(context, &args).await,
                "hai_get_email_status" => call_get_email_status(context, &args).await,
                "hai_reply_email" => call_reply_email(context, &args).await,
                _ => {
                    if let Some(result) = bridge.call_tool(name, &args).await {
                        result
                    } else {
                        Err(format!("unknown tool: {name}"))
                    }
                }
            };

            Some(match result {
                Ok(tool_result) => RpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(tool_result),
                    error: None,
                },
                Err(message) => RpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(error_tool_result(message)),
                    error: None,
                },
            })
        }
        _ => Some(RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: format!("method not found: {}", request.method),
            }),
        }),
    }
}

fn hai_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "hai_check_username",
            "description": "Check if a hai.ai username is available",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "username": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["username"]
            }
        }),
        json!({
            "name": "hai_hello",
            "description": "Run authenticated hello handshake with HAI using local JACS config",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_test": { "type": "boolean" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_agent_status",
            "description": "Get the current agent's verification status",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_verify_status",
            "description": "Get verification status for the current or provided agent",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_claim_username",
            "description": "Claim a username for an agent ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "username": { "type": "string" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["agent_id", "username"]
            }
        }),
        json!({
            "name": "hai_create_agent",
            "description": "Create a new JACS agent locally and optionally register with HAI",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "password": { "type": "string" },
                    "algorithm": { "type": "string" },
                    "data_directory": { "type": "string" },
                    "key_directory": { "type": "string" },
                    "config_path": { "type": "string" },
                    "agent_type": { "type": "string" },
                    "description": { "type": "string" },
                    "domain": { "type": "string" },
                    "default_storage": { "type": "string" },
                    "register_with_hai": { "type": "boolean" },
                    "owner_email": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["name", "password"]
            }
        }),
        json!({
            "name": "hai_register_agent",
            "description": "Register an existing local JACS agent with HAI",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "owner_email": { "type": "string" },
                    "domain": { "type": "string" },
                    "description": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_generate_verify_link",
            "description": "Generate a HAI verify link from a signed JACS document",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document": { "type": "string" },
                    "base_url": { "type": "string" },
                    "hosted": { "type": "boolean" }
                },
                "required": ["document"]
            }
        }),
        // ----- Email tools -----
        json!({
            "name": "hai_send_email",
            "description": "Send an email from the agent's @hai.ai address",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Recipient email address" },
                    "subject": { "type": "string", "description": "Email subject line" },
                    "body": { "type": "string", "description": "Plain text email body" },
                    "in_reply_to": { "type": "string", "description": "Message-ID to reply to (for threading)" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["to", "subject", "body"]
            }
        }),
        json!({
            "name": "hai_list_messages",
            "description": "List email messages in the agent's inbox/outbox",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max messages to return (default 20)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "direction": { "type": "string", "description": "Filter: 'inbound' or 'outbound'" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_message",
            "description": "Get a single email message by ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_delete_message",
            "description": "Delete an email message",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_mark_read",
            "description": "Mark an email message as read",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_mark_unread",
            "description": "Mark an email message as unread",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_search_messages",
            "description": "Search email messages by query, sender, recipient, or date range",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Search query text" },
                    "direction": { "type": "string", "description": "Filter: 'inbound' or 'outbound'" },
                    "from_address": { "type": "string", "description": "Filter by sender address" },
                    "to_address": { "type": "string", "description": "Filter by recipient address" },
                    "since": { "type": "string", "description": "Filter: messages after this ISO date" },
                    "until": { "type": "string", "description": "Filter: messages before this ISO date" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_unread_count",
            "description": "Get the count of unread email messages",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_email_status",
            "description": "Get email account status including usage limits and daily stats",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_reply_email",
            "description": "Reply to an email message (fetches original, sends reply with threading)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "ID of the message to reply to" },
                    "body": { "type": "string", "description": "Reply body text" },
                    "subject_override": { "type": "string", "description": "Override the Re: subject line" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["message_id", "body"]
            }
        }),
    ]
}

async fn call_check_username(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let username = required_string(args, "username")?;
    let hai_url = optional_string(args, "hai_url");

    let client = context.noop_client_with_url(hai_url)?;
    let result = client
        .check_username(username)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "username={} available={} reason={}",
            result.username,
            result.available,
            result.reason.clone().unwrap_or_default()
        ),
        json!({ "check_username": result }),
    ))
}

async fn call_hello(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let include_test = args
        .get("include_test")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let client = context.local_client_with_url(config_path, hai_url)?;
    let result = client
        .hello(include_test)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("hello_id={} message={}", result.hello_id, result.message),
        json!({ "hello": result }),
    ))
}

async fn call_verify_status(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let agent_id = optional_string(args, "agent_id");
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let client = context.local_client_with_url(config_path, hai_url)?;
    let result = client
        .verify_status(agent_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "jacs_id={} registered={} dns_verified={}",
            result.jacs_id, result.registered, result.dns_verified
        ),
        json!({ "verify_status": result }),
    ))
}

async fn call_claim_username(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let agent_id = required_string(args, "agent_id")?;
    let username = required_string(args, "username")?;
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let mut client = context.local_client_with_url(config_path, hai_url)?;
    let result = client
        .claim_username(agent_id, username)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "claimed username={} for agent_id={}",
            result.username, result.agent_id
        ),
        json!({ "claim_username": result }),
    ))
}

async fn call_create_agent(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let options = CreateAgentOptions {
        name: required_string(args, "name")?.to_string(),
        password: required_string(args, "password")?.to_string(),
        algorithm: optional_string(args, "algorithm").map(ToString::to_string),
        data_directory: optional_string(args, "data_directory").map(ToString::to_string),
        key_directory: optional_string(args, "key_directory").map(ToString::to_string),
        config_path: optional_string(args, "config_path").map(ToString::to_string),
        agent_type: optional_string(args, "agent_type").map(ToString::to_string),
        description: optional_string(args, "description").map(ToString::to_string),
        domain: optional_string(args, "domain").map(ToString::to_string),
        default_storage: optional_string(args, "default_storage").map(ToString::to_string),
    };

    let created =
        LocalJacsProvider::create_agent_with_options(&options).map_err(|e| e.to_string())?;

    let register_with_hai = args
        .get("register_with_hai")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let registration = if register_with_hai {
        let config_path = if created.config_path.is_empty() {
            None
        } else {
            Some(created.config_path.as_str())
        };

        let provider = context.local_provider(config_path)?;
        let agent_json = provider.export_agent_json().map_err(|e| e.to_string())?;
        let public_key_pem = provider.public_key_pem().map_err(|e| e.to_string())?;

        let client = context.client_with_provider(provider, optional_string(args, "hai_url"))?;
        let register_result = client
            .register(&RegisterAgentOptions {
                agent_json,
                public_key_pem: Some(public_key_pem),
                owner_email: optional_string(args, "owner_email").map(ToString::to_string),
                domain: optional_string(args, "domain").map(ToString::to_string),
                description: optional_string(args, "description").map(ToString::to_string),
            })
            .await
            .map_err(|e| e.to_string())?;

        Some(register_result)
    } else {
        None
    };

    Ok(success_tool_result(
        if registration.is_some() {
            format!(
                "created agent_id={} and registered with HAI",
                created.agent_id
            )
        } else {
            format!("created agent_id={}", created.agent_id)
        },
        json!({
            "created_agent": created,
            "registration": registration
        }),
    ))
}

async fn call_register_agent(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let config_path = optional_string(args, "config_path");
    let provider = context.local_provider(config_path)?;

    let agent_json = provider.export_agent_json().map_err(|e| e.to_string())?;
    let public_key_pem = provider.public_key_pem().map_err(|e| e.to_string())?;

    let client = context.client_with_provider(provider, optional_string(args, "hai_url"))?;
    let result = client
        .register(&RegisterAgentOptions {
            agent_json,
            public_key_pem: Some(public_key_pem),
            owner_email: optional_string(args, "owner_email").map(ToString::to_string),
            domain: optional_string(args, "domain").map(ToString::to_string),
            description: optional_string(args, "description").map(ToString::to_string),
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "registered jacs_id={} agent_id={}",
            result.jacs_id, result.agent_id
        ),
        json!({ "registration": result }),
    ))
}

async fn call_generate_verify_link(args: &Value) -> std::result::Result<Value, String> {
    let document = required_string(args, "document")?;
    let base_url = optional_string(args, "base_url");
    let hosted = args.get("hosted").and_then(Value::as_bool).unwrap_or(false);

    let url = if hosted {
        generate_verify_link_hosted(document, base_url)
    } else {
        generate_verify_link(document, base_url)
    }
    .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("verify_url={url}"),
        json!({ "verify_url": url }),
    ))
}

// ---------------------------------------------------------------------------
// Email tool handlers
// ---------------------------------------------------------------------------

async fn call_send_email(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client
        .send_email(&SendEmailOptions {
            to: required_string(args, "to")?.to_string(),
            subject: required_string(args, "subject")?.to_string(),
            body: required_string(args, "body")?.to_string(),
            in_reply_to: optional_string(args, "in_reply_to").map(ToString::to_string),
            attachments: vec![],
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "sent message_id={} status={}",
            result.message_id, result.status
        ),
        json!({ "send_email": result }),
    ))
}

async fn call_list_messages(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client
        .list_messages(&ListMessagesOptions {
            limit: optional_u32(args, "limit"),
            offset: optional_u32(args, "offset"),
            direction: optional_string(args, "direction").map(ToString::to_string),
        })
        .await
        .map_err(|e| e.to_string())?;

    let count = result.len();
    Ok(success_tool_result(
        format!("found {count} messages"),
        json!({ "messages": result }),
    ))
}

async fn call_get_message(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let message_id = required_string(args, "message_id")?;
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client
        .get_message(message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "message from={} to={} subject={}",
            result.from_address, result.to_address, result.subject
        ),
        json!({ "message": result }),
    ))
}

async fn call_delete_message(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let message_id = required_string(args, "message_id")?;
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    client
        .delete_message(message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("deleted message_id={message_id}"),
        json!({ "deleted": true, "message_id": message_id }),
    ))
}

async fn call_mark_read(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let message_id = required_string(args, "message_id")?;
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    client
        .mark_read(message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("marked read message_id={message_id}"),
        json!({ "message_id": message_id, "is_read": true }),
    ))
}

async fn call_mark_unread(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let message_id = required_string(args, "message_id")?;
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    client
        .mark_unread(message_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("marked unread message_id={message_id}"),
        json!({ "message_id": message_id, "is_read": false }),
    ))
}

async fn call_search_messages(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client
        .search_messages(&SearchOptions {
            q: optional_string(args, "q").map(ToString::to_string),
            direction: optional_string(args, "direction").map(ToString::to_string),
            from_address: optional_string(args, "from_address").map(ToString::to_string),
            to_address: optional_string(args, "to_address").map(ToString::to_string),
            since: optional_string(args, "since").map(ToString::to_string),
            until: optional_string(args, "until").map(ToString::to_string),
            limit: optional_u32(args, "limit"),
            offset: optional_u32(args, "offset"),
        })
        .await
        .map_err(|e| e.to_string())?;

    let count = result.len();
    Ok(success_tool_result(
        format!("found {count} messages"),
        json!({ "messages": result }),
    ))
}

async fn call_get_unread_count(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let count = client.get_unread_count().await.map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!("unread_count={count}"),
        json!({ "count": count }),
    ))
}

async fn call_get_email_status(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client.get_email_status().await.map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "email={} status={} tier={} daily_used={}/{}",
            result.email, result.status, result.tier, result.daily_used, result.daily_limit
        ),
        json!({ "email_status": result }),
    ))
}

async fn call_reply_email(
    context: &HaiServerContext,
    args: &Value,
) -> std::result::Result<Value, String> {
    let message_id = required_string(args, "message_id")?;
    let body = required_string(args, "body")?;
    let subject_override = optional_string(args, "subject_override");
    let client = context.local_client_with_url(
        optional_string(args, "config_path"),
        optional_string(args, "hai_url"),
    )?;
    let result = client
        .reply(message_id, body, subject_override)
        .await
        .map_err(|e| e.to_string())?;

    Ok(success_tool_result(
        format!(
            "replied message_id={} status={}",
            result.message_id, result.status
        ),
        json!({ "reply": result }),
    ))
}

fn success_tool_result(text: String, structured: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
    })
}

fn error_tool_result(message: String) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

fn required_string<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn optional_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key).and_then(Value::as_u64).map(|v| v as u32)
}

fn id_matches(value: &Value, id: u64) -> bool {
    value.get("id").and_then(Value::as_u64) == Some(id)
        || value.get("id").and_then(Value::as_i64) == Some(id as i64)
}

fn rpc_error_message(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    format!("rpc error {code}: {message}")
}

fn bridge_candidates() -> Vec<BridgeCandidate> {
    let mut out = Vec::new();

    if let Ok(bin) = std::env::var("JACS_MCP_BIN") {
        if !bin.is_empty() {
            if let Ok(raw_args) = std::env::var("JACS_MCP_ARGS") {
                if !raw_args.trim().is_empty() {
                    eprintln!("warning: ignoring JACS_MCP_ARGS; stdio-only local mode is enforced");
                }
            }
            let cwd = std::env::var("JACS_MCP_CWD").ok().filter(|v| !v.is_empty());
            out.push(BridgeCandidate {
                command: bin,
                args: Vec::new(),
                cwd,
            });
            return out;
        }
    }

    out.push(BridgeCandidate {
        command: "jacs-mcp".to_string(),
        args: Vec::new(),
        cwd: std::env::var("JACS_MCP_CWD").ok().filter(|v| !v.is_empty()),
    });

    if let Ok(home) = std::env::var("HOME") {
        let manifest = format!("{home}/personal/JACS/jacs-mcp/Cargo.toml");
        if Path::new(&manifest).exists() {
            out.push(BridgeCandidate {
                command: "cargo".to_string(),
                args: vec![
                    "run".to_string(),
                    "--quiet".to_string(),
                    "--manifest-path".to_string(),
                    manifest,
                ],
                cwd: None,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_env_var(key: &str, value: Option<&str>) {
        match value {
            Some(v) => {
                // SAFETY: tests serialize env mutations with ENV_LOCK.
                unsafe { std::env::set_var(key, v) };
            }
            None => {
                // SAFETY: tests serialize env mutations with ENV_LOCK.
                unsafe { std::env::remove_var(key) };
            }
        }
    }

    #[test]
    fn bridge_candidates_ignores_jacs_mcp_args_when_custom_bin_is_set() {
        let _guard = env_lock().lock().expect("lock");
        let old_bin = std::env::var("JACS_MCP_BIN").ok();
        let old_args = std::env::var("JACS_MCP_ARGS").ok();
        let old_cwd = std::env::var("JACS_MCP_CWD").ok();

        set_env_var("JACS_MCP_BIN", Some("/tmp/custom-jacs-mcp"));
        set_env_var("JACS_MCP_ARGS", Some("--transport http --port 8080"));
        set_env_var("JACS_MCP_CWD", Some("/tmp/mcp-cwd"));

        let candidates = bridge_candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].command, "/tmp/custom-jacs-mcp");
        assert!(candidates[0].args.is_empty());
        assert_eq!(candidates[0].cwd.as_deref(), Some("/tmp/mcp-cwd"));

        set_env_var("JACS_MCP_BIN", old_bin.as_deref());
        set_env_var("JACS_MCP_ARGS", old_args.as_deref());
        set_env_var("JACS_MCP_CWD", old_cwd.as_deref());
    }

    #[test]
    fn bridge_candidates_default_to_stdio_local_command() {
        let _guard = env_lock().lock().expect("lock");
        let old_bin = std::env::var("JACS_MCP_BIN").ok();
        let old_args = std::env::var("JACS_MCP_ARGS").ok();
        let old_cwd = std::env::var("JACS_MCP_CWD").ok();

        set_env_var("JACS_MCP_BIN", None);
        set_env_var("JACS_MCP_ARGS", Some("--transport http"));
        set_env_var("JACS_MCP_CWD", None);

        let candidates = bridge_candidates();
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].command, "jacs-mcp");
        assert!(candidates[0].args.is_empty());

        set_env_var("JACS_MCP_BIN", old_bin.as_deref());
        set_env_var("JACS_MCP_ARGS", old_args.as_deref());
        set_env_var("JACS_MCP_CWD", old_cwd.as_deref());
    }

    #[test]
    fn hai_tool_definitions_include_core_identity_and_email_tools() {
        let definitions = hai_tool_definitions();
        let names: Vec<&str> = definitions
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();

        assert!(names.contains(&"hai_hello"));
        assert!(names.contains(&"hai_register_agent"));
        assert!(names.contains(&"hai_send_email"));
        assert!(names.contains(&"hai_reply_email"));
    }

    #[derive(Debug, Deserialize)]
    struct MCPToolContractFixture {
        required_tools: Vec<RequiredTool>,
    }

    #[derive(Debug, Deserialize)]
    struct RequiredTool {
        name: String,
        properties: std::collections::BTreeMap<String, String>,
        required: Vec<String>,
    }

    fn load_mcp_tool_contract_fixture() -> MCPToolContractFixture {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mcp_tool_contract.json");
        let raw = fs::read_to_string(path).expect("read mcp tool contract fixture");
        serde_json::from_str(&raw).expect("decode mcp tool contract fixture")
    }

    #[test]
    fn hai_tool_definitions_match_shared_mcp_contract() {
        let fixture = load_mcp_tool_contract_fixture();
        let actual: std::collections::BTreeMap<String, (std::collections::BTreeMap<String, String>, Vec<String>)> =
            hai_tool_definitions()
                .into_iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    let properties = tool
                        .get("inputSchema")?
                        .get("properties")?
                        .as_object()?
                        .iter()
                        .map(|(key, value)| {
                            let type_name = value
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("string")
                                .to_string();
                            (key.clone(), if type_name == "integer" { "number".to_string() } else { type_name })
                        })
                        .collect();
                    let mut required = tool
                        .get("inputSchema")?
                        .get("required")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    required.sort();
                    Some((name, (properties, required)))
                })
                .collect();

        for expected in fixture.required_tools {
            let (properties, required) = actual
                .get(&expected.name)
                .unwrap_or_else(|| panic!("missing tool {}", expected.name));
            let mut expected_required = expected.required.clone();
            expected_required.sort();
            assert_eq!(required, &expected_required, "tool {}", expected.name);
            for (key, value) in &expected.properties {
                assert_eq!(
                    properties.get(key),
                    Some(value),
                    "tool {} property {}",
                    expected.name,
                    key
                );
            }
        }
    }

    #[test]
    fn required_string_reports_missing_fields() {
        let args = json!({});
        let err = required_string(&args, "message_id").expect_err("missing field");
        assert_eq!(err, "message_id is required");
    }

    #[tokio::test]
    async fn call_generate_verify_link_returns_text_and_structured_content() {
        let result = call_generate_verify_link(&json!({
            "document": r#"{"signed":true}"#,
            "base_url": "https://example.com"
        }))
        .await
        .expect("verify link result");

        let url = result
            .get("structuredContent")
            .and_then(|value: &Value| value.get("verify_url"))
            .and_then(Value::as_str)
            .expect("verify_url");
        let expected_text = format!("verify_url={url}");

        assert!(url.starts_with("https://example.com/jacs/verify?s="));
        assert_eq!(
            result["content"][0]["text"].as_str(),
            Some(expected_text.as_str())
        );
    }

    #[tokio::test]
    async fn handle_request_lists_hai_tools_without_bridge_tools() {
        let request = RpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };
        let context = HaiServerContext {
            base_url: "https://hai.example".to_string(),
            fallback_jacs_id: "anonymous-agent".to_string(),
        };

        let response = handle_request(&context, &NoopJacsmcpBridge, request)
            .await
            .expect("tools/list response");

        let result = response
            .result
            .expect("tools/list result");
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();

        assert!(names.contains(&"hai_hello"));
        assert!(names.contains(&"hai_check_username"));
        assert!(names.contains(&"hai_get_email_status"));
    }
}
