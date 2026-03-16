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

    fn write_embedded_jacs_config(&self) -> Option<PathBuf> {
        let source = jacs_fixture_config()?;
        let source_dir = source.parent().expect("fixture config dir");
        let mut value: Value =
            serde_json::from_str(&std::fs::read_to_string(&source).expect("read fixture config"))
                .expect("parse fixture config");

        for field in ["jacs_data_directory", "jacs_key_directory"] {
            let path = value.get(field).and_then(Value::as_str).map(PathBuf::from);
            if let Some(path) = path {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    source_dir.join(path)
                };
                value[field] = Value::String(resolved.to_string_lossy().into_owned());
            }
        }

        let config_path = self.path().join("embedded-jacs.config.json");
        std::fs::write(
            &config_path,
            serde_json::to_vec_pretty(&value).expect("encode temp config"),
        )
        .expect("write temp config");
        Some(config_path)
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn spawn(_workspace: &TestWorkspace, hai_url: &str, jacs_config: &Path) -> Self {
        let mut child = Command::new(haiai_bin())
            .arg("mcp")
            .env("HAI_URL", hai_url)
            .env("JACS_CONFIG", jacs_config)
            .env("JACS_PRIVATE_KEY_PASSWORD", "secretpassord")
            .env("RUST_LOG", "warn")
            .current_dir(jacs_config.parent().expect("JACS config dir"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
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

fn jacs_fixture_config() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../../../JACS/jacs/jacs.config.json");
    path.canonicalize().ok()
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
        ("GET", path) if path.starts_with("/api/v1/agents/username/check?username=demo-agent") => {
            json!({
                "username": "demo-agent",
                "available": true
            })
        }
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
    let jacs_config = match workspace.write_embedded_jacs_config() {
        Some(p) => p,
        None => { eprintln!("SKIP: JACS sibling checkout not found"); return; }
    };
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
    assert!(tools.contains(&"jacs_export_agent".to_string()));
    assert!(!tools.contains(&"hai_create_agent".to_string()));

    let exported = session.call_tool(10, "jacs_export_agent", json!({}));
    let export_text = exported["content"][0]["text"]
        .as_str()
        .expect("jacs_export_agent text");
    let export_json: Value = serde_json::from_str(export_text).expect("decode export result");
    assert_eq!(export_json["success"].as_bool(), Some(true));
    assert!(export_json["agent_id"].as_str().is_some());

    let check_username = session.call_tool(
        11,
        "hai_check_username",
        json!({
            "username": "demo-agent"
        }),
    );
    assert_eq!(
        check_username["structuredContent"]["check_username"]["available"].as_bool(),
        Some(true)
    );

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

    server.assert_request(
        |request| {
            request.method == "GET"
                && request
                    .path
                    .starts_with("/api/v1/agents/username/check?username=demo-agent")
                && !request.headers.contains_key("authorization")
        },
        "GET /api/v1/agents/username/check?username=demo-agent",
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

#[test]
fn rejects_runtime_hai_url_override_before_network_request() {
    let workspace = TestWorkspace::new();
    let jacs_config = match workspace.write_embedded_jacs_config() {
        Some(p) => p,
        None => { eprintln!("SKIP: JACS sibling checkout not found"); return; }
    };
    let server = MiniHaiServer::start();

    let mut session = McpSession::spawn(&workspace, server.base_url(), &jacs_config);
    session.initialize();

    let result = session.call_tool_allow_error(
        30,
        "hai_check_username",
        json!({
            "username": "demo-agent",
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
    let jacs_config = match workspace.write_embedded_jacs_config() {
        Some(p) => p,
        None => { eprintln!("SKIP: JACS sibling checkout not found"); return; }
    };
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
