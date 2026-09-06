use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}

struct MiniHaiServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown_tx: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MiniHaiServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock HAI server");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let address = listener.local_addr().expect("local addr");
        let base_url = format!("http://{address}");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let thread = thread::spawn(move || loop {
            match shutdown_rx.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }

            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    if let Some(request) = read_request(&mut stream) {
                        let response = response_for_request(&request);
                        requests_for_thread
                            .lock()
                            .expect("lock requests")
                            .push(request);
                        write_response(&mut stream, response);
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("mock HAI server accept failed: {err}"),
            }
        });

        Self {
            base_url,
            requests,
            shutdown_tx,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn assert_request<F>(&self, predicate: F, description: &str)
    where
        F: Fn(&RecordedRequest) -> bool,
    {
        let requests = self.requests.lock().expect("lock requests");
        assert!(
            requests.iter().any(predicate),
            "expected request matching {description}, got {requests:?}"
        );
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("lock requests").len()
    }
}

impl Drop for MiniHaiServer {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct TestWorkspace {
    temp_dir: TempDir,
}

impl TestWorkspace {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        Self { temp_dir }
    }

    fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    fn write_embedded_jacs_config(&self) -> PathBuf {
        let workspace_root = self
            .path()
            .canonicalize()
            .expect("canonical workspace tempdir");
        let config_path = workspace_root.join("jacs.config.json");
        // Real signed configuration and encrypted keys; changing copied paths
        // without re-signing would no longer test a usable local identity.
        let output = isolated_haiai_command()
            .args([
                "init",
                "--name",
                "mcp-test-agent",
                "--register",
                "false",
                "--algorithm",
                "ring-Ed25519",
            ])
            .arg("--data-dir")
            .arg(workspace_root.join("data"))
            .arg("--key-dir")
            .arg(workspace_root.join("keys"))
            .arg("--config-path")
            .arg(&config_path)
            .current_dir(&workspace_root)
            .output()
            .expect("create encrypted JACS fixture through CLI");
        assert!(
            output.status.success(),
            "fixture creation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        config_path
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn spawn(_workspace: &TestWorkspace, hai_url: &str, jacs_config: &Path) -> Self {
        Self::spawn_inner(hai_url, jacs_config, None, "warn", None, None)
    }

    fn spawn_with_profile(hai_url: &str, jacs_config: &Path, profile: &str) -> Self {
        Self::spawn_inner(hai_url, jacs_config, None, "warn", None, Some(profile))
    }

    fn spawn_with_log(
        _workspace: &TestWorkspace,
        hai_url: &str,
        jacs_config: &Path,
        log_file: &Path,
        rust_log: &str,
        storage: Option<&str>,
    ) -> Self {
        Self::spawn_inner(
            hai_url,
            jacs_config,
            Some(log_file),
            rust_log,
            storage,
            None,
        )
    }

    fn spawn_inner(
        hai_url: &str,
        jacs_config: &Path,
        log_file: Option<&Path>,
        rust_log: &str,
        storage: Option<&str>,
        profile: Option<&str>,
    ) -> Self {
        let mut command = isolated_haiai_command();
        if let Some(path) = log_file {
            command.arg("--log-file").arg(path);
        }
        command
            .arg("mcp")
            .env("HAI_URL", hai_url)
            .env("JACS_CONFIG", jacs_config)
            .env("JACS_PRIVATE_KEY_PASSWORD", "secretpassord")
            .env("RUST_LOG", rust_log);
        if let Some(profile) = profile {
            command.args(["--profile", profile]);
        }
        if let Some(label) = storage {
            command.env("JACS_DEFAULT_STORAGE", label);
        }
        command.current_dir(jacs_config.parent().expect("JACS config dir"));
        Self::from_command(command, log_file.is_some())
    }

    fn from_command(mut command: Command, quiet_stderr: bool) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if quiet_stderr {
                Stdio::null()
            } else {
                Stdio::inherit()
            })
            .spawn()
            .expect("spawn haiai mcp");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));

        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn initialize(&mut self) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "hai-mcp-integration",
                    "version": "0.1"
                }
            }
        }));
        let response = self.read_message();
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
        response
    }

    fn list_tools(&mut self) -> Vec<String> {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }));
        let response = self.read_message();
        response["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn call_tool(&mut self, id: i64, name: &str, arguments: Value) -> Value {
        let result = self.call_tool_allow_error(id, name, arguments);
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(!is_error, "tool '{}' returned MCP error: {}", name, result);
        result
    }

    fn call_tool_allow_error(&mut self, id: i64, name: &str, arguments: Value) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }));
        let response = self.read_message();
        response["result"].clone()
    }

    fn send(&mut self, message: Value) {
        let encoded = serde_json::to_string(&message).expect("serialize request");
        self.stdin
            .write_all(encoded.as_bytes())
            .expect("write request");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().expect("flush request");
    }

    fn read_message(&mut self) -> Value {
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read response");
            assert!(read > 0, "hai-mcp closed stdout unexpectedly");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return serde_json::from_str(trimmed).unwrap_or_else(|err| {
                panic!("failed to parse MCP response '{}': {}", trimmed, err)
            });
        }
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn haiai_bin() -> PathBuf {
    let current_exe = std::env::current_exe().expect("current_exe");
    let target_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir for integration test binary");
    let candidate = target_dir.join(format!("haiai{}", std::env::consts::EXE_SUFFIX));
    assert!(
        candidate.exists(),
        "expected haiai binary at {}. Run `cargo build -p haiai-cli` first.",
        candidate.display()
    );
    candidate
}

fn isolated_haiai_command() -> Command {
    let mut command = Command::new(haiai_bin());
    for (name, _) in std::env::vars_os() {
        let label = name.to_string_lossy();
        if label.starts_with("JACS_") || label.starts_with("HAI_") {
            command.env_remove(name);
        }
    }
    command.env("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");
    command
}

fn read_request(stream: &mut TcpStream) -> Option<RecordedRequest> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end;
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(idx) = find_header_end(&buffer) {
            header_end = idx;
            break;
        }
    }

    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next()?.to_string();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next()?.to_string();
    let path = request_parts.next()?.to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    Some(RecordedRequest {
        method,
        path,
        headers,
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn response_for_request(request: &RecordedRequest) -> Value {
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/api/v1/agents/register") => {
            json!({
                "success": true,
                "agent_id": "hai-agent-registered",
                "jacs_id": "ddf35096-d212-4ca9-a299-feda597d5525",
                "dns_verified": false,
                "registrations": [],
                "registered_at": "2026-03-06T00:00:00Z",
                "message": "registered"
            })
        }
        ("GET", "/api/agents/hai-agent-123/email/status") => {
            json!({
                "email": "demo-agent@hai.ai",
                "status": "active",
                "tier": "verified",
                "billing_tier": "free",
                "messages_sent_24h": 2,
                "daily_limit": 100,
                "daily_used": 2,
                "resets_at": "2026-03-07T00:00:00Z",
                "messages_sent_total": 12,
                "external_enabled": true,
                "external_sends_today": 1
            })
        }
        _ => json!({
            "error": format!("unexpected request: {} {}", request.method, request.path)
        }),
    }
}

fn write_response(stream: &mut TcpStream, body: Value) {
    let encoded = body.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        encoded.len(),
        encoded
    );
    stream
        .write_all(response.as_bytes())
        .expect("write mock response");
    stream.flush().expect("flush mock response");
}

/// Verify the deprecated standalone hai-mcp binary prints a deprecation message.
#[test]
fn standalone_binary_prints_deprecation() {
    let current_exe = std::env::current_exe().expect("current_exe");
    let target_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir");
    let hai_mcp_bin = target_dir.join(format!("hai-mcp{}", std::env::consts::EXE_SUFFIX));
    if !hai_mcp_bin.exists() {
        // Binary not built, skip
        return;
    }

    let output = Command::new(&hai_mcp_bin).output().expect("run hai-mcp");

    assert!(
        !output.status.success(),
        "deprecated binary should exit with failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("deprecated"), "stderr was: {stderr}");
    assert!(stderr.contains("haiai mcp"), "stderr was: {stderr}");
}

#[test]
fn serves_hai_and_embedded_jacs_tools_and_calls_hai_over_stdio() {
    let workspace = TestWorkspace::new();
    let jacs_config = workspace.write_embedded_jacs_config();
    let server = MiniHaiServer::start();

    let mut session = McpSession::spawn(&workspace, server.base_url(), &jacs_config);
    let initialize = session.initialize();
    assert_eq!(
        initialize["result"]["serverInfo"]["name"].as_str(),
        Some("hai-mcp")
    );

    let tools = session.list_tools();
    assert!(tools.contains(&"hai_register_agent".to_string()));
    assert!(tools.contains(&"hai_send_email".to_string()));
    assert!(tools.contains(&"hai_save_memory".to_string()));
    assert_eq!(
        tools
            .iter()
            .filter(|name| name.starts_with("jacs_"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["jacs_verify_document"]
    );
    for tool in hai_mcp::hai_tools::definitions() {
        assert!(
            tools.contains(&tool.name.to_string()),
            "missing HAI tool {}",
            tool.name
        );
    }
    assert!(!tools.contains(&"jacs_memory_save".to_string()));
    assert!(!tools.contains(&"jacs_memory_recall".to_string()));
    assert!(!tools.contains(&"jacs_memory_list".to_string()));
    assert!(!tools.contains(&"jacs_memory_forget".to_string()));
    assert!(!tools.contains(&"jacs_memory_update".to_string()));
    assert!(!tools.contains(&"hai_create_agent".to_string()));
    assert!(tools.contains(&"hai_self_knowledge".to_string()));

    let saved_memory = session.call_tool(
        9,
        "hai_save_memory",
        json!({
            "content": "MCP routed provider saves locally when storage is fs"
        }),
    );
    assert!(saved_memory["structuredContent"]["key"].as_str().is_some());

    // Loading a HAI provider never implicitly enables JACS key/export/sign tools.
    for (index, tool) in ["jacs_sign_document", "jacs_export_agent", "jacs_sign_image"]
        .iter()
        .enumerate()
    {
        session.send(json!({"jsonrpc":"2.0","id":100 + index,"method":"tools/call","params":{"name":tool,"arguments":{}}}));
        let rejected = session.read_message();
        assert_eq!(rejected["error"]["code"], -32602, "{rejected}");
    }

    let email_status = session.call_tool(
        12,
        "hai_get_email_status",
        json!({
            "agent_id": "hai-agent-123"
        }),
    );
    assert_eq!(
        email_status["structuredContent"]["email_status"]["email"].as_str(),
        Some("demo-agent@hai.ai")
    );
    assert_eq!(
        email_status["structuredContent"]["email_status"]["status"].as_str(),
        Some("active")
    );

    // Self-knowledge tool (no auth required, no network)
    let sk_result = session.call_tool(
        13,
        "hai_self_knowledge",
        json!({
            "query": "key rotation"
        }),
    );
    let sk_text = sk_result["content"][0]["text"]
        .as_str()
        .expect("hai_self_knowledge text");
    assert!(
        sk_text.contains("Key Rotation") || sk_text.contains("[1]"),
        "self_knowledge should return ranked results: {sk_text}"
    );

    server.assert_request(
        |request| {
            request.method == "GET"
                && request.path == "/api/agents/hai-agent-123/email/status"
                && request
                    .headers
                    .get("authorization")
                    .map(|value| value.starts_with("JACS "))
                    .unwrap_or(false)
        },
        "GET /api/agents/hai-agent-123/email/status with JACS auth",
    );
}

fn jacs_tool_value(response: Value) -> Value {
    serde_json::from_str(
        response["content"][0]["text"]
            .as_str()
            .expect("JACS tool text"),
    )
    .expect("JACS tool JSON")
}

#[test]
fn explicit_local_signing_has_exact_jacs_scope_and_positive_json_agreement_workflows() {
    use jacs::agent::boilerplate::BoilerPlate;
    let workspace = TestWorkspace::new();
    let config_path = workspace.write_embedded_jacs_config();
    let config = jacs::config::Config::from_file(config_path.to_str().unwrap()).unwrap();
    assert!(config.is_signed);
    let public = jacs::agent::Agent::from_config_public_only(config).unwrap();
    let agent_id = public.get_id().unwrap();
    let hai = MiniHaiServer::start();
    let mut session = McpSession::spawn_with_profile(hai.base_url(), &config_path, "local-sign");
    session.initialize();
    let tools = session.list_tools();
    let mut jacs_tools: Vec<_> = tools
        .iter()
        .filter(|name| name.starts_with("jacs_"))
        .map(String::as_str)
        .collect();
    jacs_tools.sort_unstable();
    assert_eq!(
        jacs_tools,
        vec![
            "jacs_apply_agreement_v2",
            "jacs_create_agreement_v2",
            "jacs_detect_agreement_v2_branch_conflict",
            "jacs_merge_agreement_v2_transcript_branches",
            "jacs_resolve_agreement_v2_branch_conflict",
            "jacs_sign_agreement_v2",
            "jacs_sign_document",
            "jacs_verify_agreement_v2",
            "jacs_verify_document",
        ]
    );
    for tool in hai_mcp::hai_tools::definitions() {
        assert!(
            tools.contains(&tool.name.to_string()),
            "missing HAI tool {}",
            tool.name
        );
    }
    let supplied = json!({"hello":"HAIAI local MCP", "jacsType":"agent"});
    let signed = jacs_tool_value(session.call_tool(
        30,
        "jacs_sign_document",
        json!({"content":supplied.to_string()}),
    ));
    assert_eq!(signed["success"], true, "{signed}");
    let document: Value =
        serde_json::from_str(signed["signed_document"].as_str().unwrap()).unwrap();
    assert_eq!(document["jacsType"], "document");
    assert_eq!(document["content"], supplied);
    assert_eq!(document["jacsSignature"]["agentID"], agent_id);
    let verified = jacs_tool_value(session.call_tool(31, "jacs_verify_document", json!({
        "document":signed["signed_document"], "public_key":public.get_public_key().unwrap(), "algorithm":"ed25519"
    })));
    assert_eq!(verified["valid"], true, "{verified}");
    assert!(config_path
        .parent()
        .unwrap()
        .join(format!(
            "documents/{}:{}.json",
            document["jacsId"].as_str().unwrap(),
            document["jacsVersion"].as_str().unwrap()
        ))
        .is_file());

    let input = json!({"title":"Local MCP Agreement", "description":"Explicit local agent workflow.", "terms":"Return the borrowed book.", "termsFormat":"text/plain", "status":"proposed",
        "parties":[{"agentId":agent_id,"agentType":"ai","role":"signer"}], "controllers":[agent_id],
        "signaturePolicy":{"partyQuorum":"all","witnessRequired":0,"notaryRequired":0,"requiredAlgorithms":["ring-Ed25519"],"minimumStrength":"classical"}});
    // Keep the public input a JSON object; the embedded handler delegates it to JACS.
    let created =
        jacs_tool_value(session.call_tool(32, "jacs_create_agreement_v2", json!({"input":input})));
    assert_eq!(created["success"], true, "{created}");
    let agreed = jacs_tool_value(session.call_tool(
        33,
        "jacs_sign_agreement_v2",
        json!({"agreement":created["agreement"],"role":"signer"}),
    ));
    assert_eq!(agreed["success"], true, "{agreed}");
    let inspected = jacs_tool_value(session.call_tool(
        34,
        "jacs_verify_agreement_v2",
        json!({"agreement":agreed["agreement"]}),
    ));
    assert_eq!(inspected["success"], true, "{inspected}");
    assert_eq!(
        inspected["result"]["cryptographicResult"], "valid",
        "{inspected}"
    );
    assert_eq!(
        inspected["valid"], false,
        "mathematics is not human approval or policy acceptance"
    );
    for (index, tool) in [
        "jacs_create_agent",
        "jacs_trust_agent",
        "jacs_sign_text",
        "jacs_sign_image",
    ]
    .iter()
    .enumerate()
    {
        assert!(!tools.iter().any(|name| name == tool));
        session.send(json!({"jsonrpc":"2.0","id":200 + index,"method":"tools/call","params":{"name":tool,"arguments":{}}}));
        let rejected = session.read_message();
        assert_eq!(rejected["error"]["code"], -32602, "{rejected}");
    }
    // JACS's offline tool scope does not disable separately authorized HAI HTTP tools.
    let email = session.call_tool(
        40,
        "hai_get_email_status",
        json!({"agent_id":"hai-agent-123"}),
    );
    assert_eq!(
        email["structuredContent"]["email_status"]["status"],
        "active"
    );
    hai.assert_request(
        |request| {
            request
                .headers
                .get("authorization")
                .is_some_and(|value| value.starts_with("JACS "))
        },
        "authenticated HAI API call with local JACS scope",
    );
}

#[test]
fn local_signing_refuses_missing_unsigned_and_conflicting_operator_configuration() {
    let workspace = TestWorkspace::new();
    let config = workspace.write_embedded_jacs_config();
    let mut unsigned: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    unsigned.as_object_mut().unwrap().remove("jacsSignature");
    let unsigned_path = workspace.path().join("unsigned.json");
    std::fs::write(&unsigned_path, serde_json::to_vec(&unsigned).unwrap()).unwrap();
    for (selection, override_env, expected) in [
        (None, None, "requires explicit JACS_CONFIG"),
        (Some(workspace.path().join("missing.json")), None, "config"),
        (
            Some(unsigned_path),
            Some(("JACS_ALLOW_UNSIGNED_AGENT_CONFIG", "true")),
            "signed",
        ),
        (
            Some(config.clone()),
            Some(("JACS_DEFAULT_STORAGE", "sqlite")),
            "fs",
        ),
        (
            Some(config.clone()),
            Some(("JACS_AGENT_ID_AND_VERSION", "another:identity")),
            "overrides",
        ),
        (
            Some(config.clone()),
            Some(("JACS_KEY_DIRECTORY", "different-keys")),
            "overrides",
        ),
    ] {
        let mut command = isolated_haiai_command();
        command
            .args(["mcp", "--profile", "local-sign"])
            .current_dir(workspace.path());
        if let Some(path) = selection {
            command.env("JACS_CONFIG", path);
        }
        if let Some((key, value)) = override_env {
            command.env(key, value);
        }
        let output = command.output().expect("negative local-sign startup");
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "startup failure must not pollute MCP stdout"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "expected {expected}: {stderr}");
    }
}

#[test]
fn mcp_profile_and_explicit_config_precedence_match_jacs() {
    let workspace = TestWorkspace::new();
    let config = workspace.write_embedded_jacs_config();
    for (cli_profile, use_primary, expected_signing) in [
        (None, true, true),
        (Some("verify-only"), true, false),
        (None, false, true),
    ] {
        let mut command = isolated_haiai_command();
        command
            .arg("mcp")
            .env("JACS_MCP_PROFILE", "local-sign")
            .current_dir(workspace.path());
        if use_primary {
            command.env("JACS_CONFIG", &config).env(
                "JACS_CONFIG_PATH",
                workspace.path().join("ignored-missing.json"),
            );
        } else {
            command.env("JACS_CONFIG_PATH", &config);
        }
        if let Some(profile) = cli_profile {
            command.args(["--profile", profile]);
        }
        let mut session = McpSession::from_command(command, false);
        session.initialize();
        assert_eq!(
            session
                .list_tools()
                .contains(&"jacs_sign_document".to_string()),
            expected_signing
        );
    }
    for profile in ["full", "core", "trust-admin", "legacy-core"] {
        let output = isolated_haiai_command()
            .args(["mcp", "--profile", profile])
            .env("JACS_CONFIG", &config)
            .current_dir(workspace.path())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{profile}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("MCP profile"));
    }
}

#[test]
fn hai_save_memory_traces_tool_storage_and_outcome() {
    let workspace = TestWorkspace::new();
    let jacs_config = workspace.write_embedded_jacs_config();
    let server = MiniHaiServer::start();
    let log_file = workspace.path().join("haiai-mcp.log");

    let mut session = McpSession::spawn_with_log(
        &workspace,
        server.base_url(),
        &jacs_config,
        &log_file,
        "info,rmcp=warn",
        Some("fs"),
    );
    session.initialize();

    let saved_memory = session.call_tool(
        14,
        "hai_save_memory",
        json!({
            "content": "MCP tracing proves local routed storage"
        }),
    );
    assert!(saved_memory["structuredContent"]["key"].as_str().is_some());

    thread::sleep(Duration::from_millis(100));
    let logs = std::fs::read_to_string(&log_file).expect("read mcp log file");
    assert!(logs.contains("hai_save_memory"), "{logs}");
    assert!(
        logs.contains("storage=fs") || logs.contains("storage=\"fs\""),
        "{logs}"
    );
    assert!(logs.contains("routed memory save completed"), "{logs}");
}

#[test]
fn rejects_runtime_hai_url_override_before_network_request() {
    let workspace = TestWorkspace::new();
    let jacs_config = workspace.write_embedded_jacs_config();
    let server = MiniHaiServer::start();

    let mut session = McpSession::spawn(&workspace, server.base_url(), &jacs_config);
    session.initialize();

    let result = session.call_tool_allow_error(
        30,
        "hai_agent_status",
        json!({
            "hai_url": "http://127.0.0.1:9"
        }),
    );

    assert_eq!(result["isError"].as_bool(), Some(true));
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("HAI_URL"),
        "unexpected result: {result}"
    );
    assert_eq!(server.request_count(), 0);
}

#[test]
fn authenticated_hai_tools_keep_working_after_startup_config_is_removed() {
    let workspace = TestWorkspace::new();
    let jacs_config = workspace.write_embedded_jacs_config();
    let server = MiniHaiServer::start();

    let mut session = McpSession::spawn(&workspace, server.base_url(), &jacs_config);
    let initialize = session.initialize();
    assert_eq!(
        initialize["result"]["serverInfo"]["name"].as_str(),
        Some("hai-mcp")
    );

    std::fs::remove_file(&jacs_config).expect("remove startup config after initialization");

    let email_status = session.call_tool(
        20,
        "hai_get_email_status",
        json!({
            "agent_id": "hai-agent-123"
        }),
    );
    assert_eq!(
        email_status["structuredContent"]["email_status"]["email"].as_str(),
        Some("demo-agent@hai.ai")
    );

    let registration = session.call_tool(
        21,
        "hai_register_agent",
        json!({
            "owner_email": "owner@example.com"
        }),
    );
    assert_eq!(
        registration["structuredContent"]["registration"]["success"].as_bool(),
        Some(true)
    );
    assert_eq!(
        registration["structuredContent"]["registration"]["agent_id"].as_str(),
        Some("hai-agent-registered")
    );

    server.assert_request(
        |request| {
            request.method == "GET"
                && request.path == "/api/agents/hai-agent-123/email/status"
                && request
                    .headers
                    .get("authorization")
                    .map(|value| value.starts_with("JACS "))
                    .unwrap_or(false)
        },
        "GET /api/agents/hai-agent-123/email/status after config removal",
    );
    server.assert_request(
        |request| {
            request.method == "POST"
                && request.path == "/api/v1/agents/register"
                && !request.headers.contains_key("authorization")
        },
        "POST /api/v1/agents/register after config removal",
    );
}
