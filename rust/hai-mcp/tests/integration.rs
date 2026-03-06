use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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
    _temp_dir: TempDir,
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        Self {
            root: temp_dir.path().to_path_buf(),
            _temp_dir: temp_dir,
        }
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn spawn(_workspace: &TestWorkspace, hai_url: &str, jacs_mcp_bin: &Path) -> Self {
        let mut child = Command::new(hai_mcp_bin())
            .env("HAI_URL", hai_url)
            .env("JACS_MCP_BIN", jacs_mcp_bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn hai-mcp");

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
        let result = response["result"].clone();
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(
            !is_error,
            "tool '{}' returned MCP error: {}",
            name, response
        );
        result
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

fn hai_mcp_bin() -> PathBuf {
    let current_exe = std::env::current_exe().expect("current_exe");
    let target_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir for integration test binary");
    let candidate = target_dir.join(format!("hai-mcp{}", std::env::consts::EXE_SUFFIX));
    assert!(
        candidate.exists(),
        "expected hai-mcp binary at {}",
        candidate.display()
    );
    candidate
}

fn write_fake_jacs_mcp_script(dir: &Path) -> PathBuf {
    let script_path = dir.join("fake-jacs-mcp.py");
    let script = r#"#!/usr/bin/env python3
import json
import sys

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    request = json.loads(line)
    method = request.get("method")

    if method == "initialize":
        response = {
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "fake-jacs-mcp", "version": "0.1.0"},
                "capabilities": {"tools": {}}
            }
        }
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        response = {
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "tools": [
                    {
                        "name": "jacs_echo",
                        "description": "Echo a message from the fake jacs-mcp bridge",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "message": {"type": "string"}
                            },
                            "required": ["message"]
                        }
                    }
                ]
            }
        }
    elif method == "tools/call":
        params = request.get("params", {})
        args = params.get("arguments", {})
        response = {
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "content": [{"type": "text", "text": f"jacs_echo {args.get('message', '')}"}],
                "structuredContent": {"echoed": args.get("message", "")}
            }
        }
    else:
        response = {
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "error": {"code": -32601, "message": f"method not found: {method}"}
        }

    print(json.dumps(response), flush=True)
"#;

    fs::write(&script_path, script).expect("write fake jacs-mcp script");
    make_executable(&script_path);
    script_path
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod +x");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

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

#[test]
fn rejects_non_stdio_runtime_arguments() {
    let output = Command::new(hai_mcp_bin())
        .arg("--transport")
        .arg("http")
        .output()
        .expect("run hai-mcp");

    assert!(
        !output.status.success(),
        "unexpected success: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdio-only"), "stderr was: {stderr}");
}

#[test]
fn serves_hai_and_jacs_tools_and_calls_hai_over_stdio() {
    let workspace = TestWorkspace::new();
    let fake_jacs_mcp = write_fake_jacs_mcp_script(&workspace.root);
    let server = MiniHaiServer::start();

    let mut session = McpSession::spawn(&workspace, server.base_url(), &fake_jacs_mcp);
    let initialize = session.initialize();
    assert_eq!(
        initialize["result"]["serverInfo"]["name"].as_str(),
        Some("hai-mcp")
    );

    let tools = session.list_tools();
    assert!(tools.contains(&"hai_register_agent".to_string()));
    assert!(tools.contains(&"hai_send_email".to_string()));
    assert!(tools.contains(&"jacs_echo".to_string()));

    let bridged = session.call_tool(10, "jacs_echo", json!({ "message": "hello" }));
    assert_eq!(
        bridged["structuredContent"]["echoed"].as_str(),
        Some("hello")
    );

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
}
