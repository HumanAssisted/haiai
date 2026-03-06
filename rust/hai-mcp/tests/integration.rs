use std::fs;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use haisdk::{CreateAgentOptions, LocalJacsProvider};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::{json, Value};
use tempfile::TempDir;

const TEST_PASSWORD: &str = "TestPass!123";

struct TestWorkspace {
    _temp_dir: TempDir,
    config_path: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let data_dir = temp_dir.path().join("data");
        let key_dir = temp_dir.path().join("keys");
        let config_path = temp_dir.path().join("jacs.config.json");
        fs::create_dir_all(&data_dir).expect("create data dir");
        fs::create_dir_all(&key_dir).expect("create key dir");

        LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
            name: "hai-mcp-test-agent".to_string(),
            password: TEST_PASSWORD.to_string(),
            algorithm: Some("ring-Ed25519".to_string()),
            data_directory: Some(data_dir.display().to_string()),
            key_directory: Some(key_dir.display().to_string()),
            config_path: Some(config_path.display().to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("hai-mcp integration test agent".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        })
        .expect("create local test agent");

        Self {
            _temp_dir: temp_dir,
            config_path,
        }
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn spawn(workspace: &TestWorkspace, hai_url: &str, jacs_mcp_bin: &Path) -> Self {
        let mut child = Command::new(hai_mcp_bin())
            .env("HAI_URL", hai_url)
            .env("JACS_CONFIG", &workspace.config_path)
            .env("JACS_PRIVATE_KEY_PASSWORD", TEST_PASSWORD)
            .env("JACS_MCP_BIN", jacs_mcp_bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
fn serves_hai_and_jacs_tools_and_reuses_cached_email_identity() {
    let workspace = TestWorkspace::new();
    let fake_jacs_mcp =
        write_fake_jacs_mcp_script(workspace.config_path.parent().expect("config dir"));
    let server = MockServer::start();

    let register_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/api/v1/agents/register")
            .header("content-type", "application/json");
        then.status(200).json_body(json!({
            "agent_id": "hai-agent-123",
            "jacs_id": "ignored-by-test"
        }));
    });

    let claim_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/api/v1/agents/hai-agent-123/username")
            .header_exists("authorization")
            .header("content-type", "application/json");
        then.status(200).json_body(json!({
            "agent_id": "hai-agent-123",
            "username": "demo-agent",
            "email": "demo-agent@hai.ai"
        }));
    });

    let send_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/api/agents/hai-agent-123/email/send")
            .header_exists("authorization")
            .header("content-type", "application/json")
            .body_includes("\"to\"")
            .body_includes("\"subject\"")
            .body_includes("\"body\"");
        then.status(200).json_body(json!({
            "message_id": "msg-001",
            "status": "queued"
        }));
    });

    let email_status_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/agents/hai-agent-123/email/status")
            .header_exists("authorization");
        then.status(200).json_body(json!({
            "email": "demo-agent@hai.ai",
            "status": "active",
            "tier": "free",
            "daily_limit": 100,
            "daily_used": 1
        }));
    });

    let mut session = McpSession::spawn(&workspace, &server.base_url(), &fake_jacs_mcp);
    eprintln!("spawned hai-mcp");
    let initialize = session.initialize();
    eprintln!("initialized hai-mcp");
    assert_eq!(
        initialize["result"]["serverInfo"]["name"].as_str(),
        Some("hai-mcp")
    );

    let tools = session.list_tools();
    eprintln!("listed tools");
    assert!(tools.contains(&"hai_register_agent".to_string()));
    assert!(tools.contains(&"hai_send_email".to_string()));
    assert!(tools.contains(&"jacs_echo".to_string()));

    let bridged = session.call_tool(10, "jacs_echo", json!({ "message": "hello" }));
    eprintln!("called bridged tool");
    assert_eq!(
        bridged["structuredContent"]["echoed"].as_str(),
        Some("hello")
    );

    let register = session.call_tool(
        11,
        "hai_register_agent",
        json!({
            "owner_email": "owner@example.com"
        }),
    );
    eprintln!("registered agent");
    assert_eq!(
        register["structuredContent"]["registration"]["agent_id"].as_str(),
        Some("hai-agent-123")
    );

    let claim = session.call_tool(
        12,
        "hai_claim_username",
        json!({
            "agent_id": "hai-agent-123",
            "username": "demo-agent"
        }),
    );
    eprintln!("claimed username");
    assert_eq!(
        claim["structuredContent"]["claim_username"]["email"].as_str(),
        Some("demo-agent@hai.ai")
    );

    let send = session.call_tool(
        13,
        "hai_send_email",
        json!({
            "to": "ops@hai.ai",
            "subject": "Integration Subject",
            "body": "Integration Body"
        }),
    );
    eprintln!("sent email");
    assert_eq!(
        send["structuredContent"]["send_email"]["message_id"].as_str(),
        Some("msg-001")
    );

    let email_status = session.call_tool(14, "hai_get_email_status", json!({}));
    eprintln!("fetched email status");
    assert_eq!(
        email_status["structuredContent"]["email_status"]["email"].as_str(),
        Some("demo-agent@hai.ai")
    );

    register_mock.assert();
    claim_mock.assert();
    send_mock.assert();
    email_status_mock.assert();
}
