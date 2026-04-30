use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

// ── Binary helper ───────────────────────────────────────────────

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

// ── CLI basics ──────────────────────────────────────────────────

#[test]
fn help_flag_exits_zero() {
    let output = Command::new(haiai_bin())
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(output.status.success(), "exit code: {}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("HAIAI CLI"), "stdout: {stdout}");
    assert!(
        stdout.contains("init"),
        "stdout should list init subcommand: {stdout}"
    );
    assert!(
        stdout.contains("mcp"),
        "stdout should list mcp subcommand: {stdout}"
    );
}

#[test]
fn version_flag_exits_zero() {
    let output = Command::new(haiai_bin())
        .arg("--version")
        .output()
        .expect("run --version");
    assert!(output.status.success(), "exit code: {}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_version = env!("CARGO_PKG_VERSION");
    assert!(
        stdout.contains(expected_version),
        "expected {expected_version} in stdout: {stdout}"
    );
}

#[test]
fn no_subcommand_exits_nonzero() {
    let output = Command::new(haiai_bin())
        .output()
        .expect("run with no args");
    assert!(!output.status.success());
}

// ── Init subcommand ─────────────────────────────────────────────

#[test]
fn init_missing_required_args_exits_nonzero() {
    let output = Command::new(haiai_bin())
        .arg("init")
        .output()
        .expect("run init without args");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--name") || stderr.contains("name"),
        "should mention missing --name: {stderr}"
    );
}

#[test]
fn init_missing_key_when_register_true() {
    let output = Command::new(haiai_bin())
        .args(["init", "--name", "test-agent"])
        .output()
        .expect("run init without --key");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Registration key is required") || stderr.contains("key"),
        "should mention missing registration key: {stderr}"
    );
}

#[test]
fn init_creates_config_keys_and_prints_agent_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("jacs.config.json");
    let key_dir = temp.path().join("keys");
    let data_dir = temp.path().join("data");

    let output = Command::new(haiai_bin())
        .args([
            "init",
            "--name",
            "cli-test-agent",
            "--domain",
            "test.example.com",
            "--register=false",
            "--algorithm",
            "ring-Ed25519",
            "--config-path",
            &config_path.to_string_lossy(),
            "--key-dir",
            &key_dir.to_string_lossy(),
            "--data-dir",
            &data_dir.to_string_lossy(),
        ])
        .env("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123")
        .output()
        .expect("run init");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init failed. stdout: {stdout}\nstderr: {stderr}"
    );

    // Output should contain agent info
    assert!(
        stdout.contains("Agent created successfully"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("Agent ID:"), "stdout: {stdout}");
    assert!(
        stdout.contains("haiai mcp"),
        "should hint about mcp: {stdout}"
    );

    // Config file should exist and contain agent ID
    assert!(config_path.is_file(), "config not created");
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).expect("read config"))
            .expect("parse config");
    assert!(
        config
            .get("jacs_agent_id_and_version")
            .and_then(Value::as_str)
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        "config should have jacs_agent_id_and_version set: {config}"
    );

    // Key files should exist
    assert!(key_dir.is_dir(), "key dir not created");
    let key_files: Vec<String> = std::fs::read_dir(&key_dir)
        .expect("read key dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        key_files.iter().any(|n| n.contains("private")),
        "expected private key file, got: {key_files:?}"
    );
    assert!(
        key_files.iter().any(|n| n.contains("public")),
        "expected public key file, got: {key_files:?}"
    );
}

#[test]
fn init_without_password_env_fails_gracefully() {
    let temp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(haiai_bin())
        .args([
            "init",
            "--name",
            "no-password-agent",
            "--domain",
            "test.example.com",
            "--register=false",
            "--config-path",
            &temp.path().join("jacs.config.json").to_string_lossy(),
            "--key-dir",
            &temp.path().join("keys").to_string_lossy(),
            "--data-dir",
            &temp.path().join("data").to_string_lossy(),
        ])
        .env_remove("JACS_PRIVATE_KEY_PASSWORD")
        .output()
        .expect("run init without password");

    assert!(!output.status.success(), "should fail without password");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("password"),
        "error should mention password: {stderr}"
    );
}

#[test]
fn init_with_domain_shows_dns_record() {
    let temp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(haiai_bin())
        .args([
            "init",
            "--name",
            "dns-test-agent",
            "--domain",
            "dns-test.example.com",
            "--register=false",
            "--algorithm",
            "ring-Ed25519",
            "--config-path",
            &temp.path().join("jacs.config.json").to_string_lossy(),
            "--key-dir",
            &temp.path().join("keys").to_string_lossy(),
            "--data-dir",
            &temp.path().join("data").to_string_lossy(),
        ])
        .env("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123")
        .output()
        .expect("run init with domain");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init failed. stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("DNS") || stdout.contains("DNSSEC"),
        "should show DNS info for domain: {stdout}"
    );
}

// ── Init → MCP end-to-end ───────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}

struct MiniHaiServer {
    base_url: String,
    _shutdown_tx: mpsc::Sender<()>,
    _thread: Option<thread::JoinHandle<()>>,
}

impl MiniHaiServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock HAI server");
        listener.set_nonblocking(true).expect("set nonblocking");
        let address = listener.local_addr().expect("local addr");
        let base_url = format!("http://{address}");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let thread = thread::spawn(move || loop {
            match shutdown_rx.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if read_request(&mut stream).is_some() {
                        write_response(&mut stream, json!({"ok": true}));
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("mock server accept failed: {err}"),
            }
        });

        Self {
            base_url,
            _shutdown_tx: shutdown_tx,
            _thread: Some(thread),
        }
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpSession {
    fn spawn(hai_url: &str, jacs_config: &Path, password: &str) -> Self {
        let mut child = Command::new(haiai_bin())
            .arg("mcp")
            .env("HAI_URL", hai_url)
            .env("JACS_CONFIG", jacs_config)
            .env("JACS_PRIVATE_KEY_PASSWORD", password)
            .env("RUST_LOG", "warn")
            .current_dir(jacs_config.parent().expect("config dir"))
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
                    "name": "cli-integration-test",
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

    fn send(&mut self, message: Value) {
        let encoded = serde_json::to_string(&message).expect("serialize request");
        self.stdin.write_all(encoded.as_bytes()).expect("write");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().expect("flush");
    }

    fn read_message(&mut self) -> Value {
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read response");
            assert!(read > 0, "haiai mcp closed stdout unexpectedly");
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

/// MCP server starts with a pre-existing JACS config and serves
/// both HAI and embedded JACS tools over stdio.
#[test]
fn mcp_serves_hai_and_jacs_tools() {
    let (_temp, jacs_config) = prepare_jacs_fixture();
    let server = MiniHaiServer::start();
    let mut session = McpSession::spawn(&server.base_url, &jacs_config, "secretpassord");
    let init_resp = session.initialize();
    assert_eq!(
        init_resp["result"]["serverInfo"]["name"].as_str(),
        Some("hai-mcp"),
        "server name mismatch: {init_resp}"
    );

    let tools = session.list_tools();
    assert!(
        tools.contains(&"hai_register_agent".to_string()),
        "missing hai_register_agent in {tools:?}"
    );
    assert!(
        tools.contains(&"jacs_export_agent".to_string()),
        "missing jacs_export_agent in {tools:?}"
    );
    assert!(
        tools.contains(&"hai_send_email".to_string()),
        "missing hai_send_email in {tools:?}"
    );
}

/// JACS currently writes ring-Ed25519 public keys as raw bytes, not PEM.
/// This means `haiai init` → `haiai mcp` fails because
/// `EmbeddedJacsProvider::load_public_key_pem` expects UTF-8.
/// This test documents the issue: init succeeds but mcp can't load the key.
#[test]
fn init_then_mcp_fails_due_to_raw_key_format() {
    let temp = tempfile::tempdir().expect("tempdir");

    let init_output = Command::new(haiai_bin())
        .args([
            "init",
            "--name",
            "key-format-agent",
            "--domain",
            "test.example.com",
            "--register=false",
            "--algorithm",
            "ring-Ed25519",
            "--config-path",
            "./jacs.config.json",
            "--key-dir",
            "./jacs_keys",
            "--data-dir",
            "./jacs",
        ])
        .current_dir(temp.path())
        .env("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123")
        .output()
        .expect("run init");
    assert!(init_output.status.success(), "init should succeed");

    // MCP should fail because the public key is raw bytes, not PEM
    let mcp_output = Command::new(haiai_bin())
        .arg("mcp")
        .env("JACS_CONFIG", temp.path().join("jacs.config.json"))
        .env("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123")
        .env("RUST_LOG", "warn")
        .current_dir(temp.path())
        .stdin(Stdio::null())
        .output()
        .expect("run mcp");

    assert!(
        !mcp_output.status.success(),
        "mcp should fail with raw key format (known issue)"
    );
    let stderr = String::from_utf8_lossy(&mcp_output.stderr);
    assert!(
        stderr.contains("UTF-8")
            || stderr.contains("public key")
            || stderr.contains("connection closed"),
        "error should mention key format: {stderr}"
    );
}

/// Prepare a JACS fixture config in a temp directory with colon-named agent files.
/// Fixtures use underscores in git (colons are illegal on Windows) and get
/// converted to colons at runtime.
fn prepare_jacs_fixture() -> (tempfile::TempDir, PathBuf) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = manifest_dir
        .join("../../fixtures/jacs-agent/jacs.config.json")
        .canonicalize()
        .expect("fixtures/jacs-agent/jacs.config.json must exist in repo");
    let source_dir = source.parent().expect("fixture dir");

    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&source).expect("read config"))
            .expect("parse config");

    let temp = tempfile::tempdir().expect("tempdir");

    // Copy keys
    let src_key_dir = source_dir.join(value["jacs_key_directory"].as_str().unwrap_or("keys"));
    let tmp_key_dir = temp.path().join("keys");
    std::fs::create_dir_all(&tmp_key_dir).expect("mkdir keys");
    for entry in std::fs::read_dir(&src_key_dir).expect("read keys") {
        let entry = entry.expect("entry");
        std::fs::copy(entry.path(), tmp_key_dir.join(entry.file_name())).expect("copy key");
    }

    // Copy data with underscore→colon conversion
    let src_data_dir = source_dir.join(value["jacs_data_directory"].as_str().unwrap_or("."));
    let tmp_data_dir = temp.path().join("data");
    copy_fixture_dir(&src_data_dir, &tmp_data_dir);

    value["jacs_data_directory"] =
        serde_json::Value::String(tmp_data_dir.to_string_lossy().into_owned());
    value["jacs_key_directory"] =
        serde_json::Value::String(tmp_key_dir.to_string_lossy().into_owned());

    let config_path = temp.path().join("jacs.config.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&value).expect("encode"),
    )
    .expect("write");
    (temp, config_path)
}

fn copy_fixture_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("mkdir");
    for entry in std::fs::read_dir(src).expect("readdir") {
        let entry = entry.expect("entry");
        let src_path = entry.path();
        let name = entry.file_name().to_string_lossy().replace('_', ":");
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_fixture_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("copy");
        }
    }
}

// ── Self-knowledge subcommand ───────────────────────────────────

#[test]
fn self_knowledge_text_output() {
    let output = Command::new(haiai_bin())
        .args(["self-knowledge", "JACS"])
        .env_remove("JACS_PRIVATE_KEY_PASSWORD")
        .output()
        .expect("run self-knowledge");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "self-knowledge failed. stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("[1]"),
        "should have ranked results: {stdout}"
    );
}

#[test]
fn self_knowledge_json_output() {
    let output = Command::new(haiai_bin())
        .args(["self-knowledge", "JACS", "--json"])
        .env_remove("JACS_PRIVATE_KEY_PASSWORD")
        .output()
        .expect("run self-knowledge --json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "self-knowledge --json failed. stdout: {stdout}\nstderr: {stderr}"
    );
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON array");
    assert!(!parsed.is_empty());
}

#[test]
fn self_knowledge_limit() {
    let output = Command::new(haiai_bin())
        .args(["self-knowledge", "JACS", "--limit", "1", "--json"])
        .env_remove("JACS_PRIVATE_KEY_PASSWORD")
        .output()
        .expect("run self-knowledge --limit 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "self-knowledge --limit 1 failed. stdout: {stdout}\nstderr: {stderr}"
    );
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(parsed.len(), 1);
}

// ── MCP without config fails gracefully ─────────────────────────

#[test]
fn mcp_without_jacs_config_fails() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output = Command::new(haiai_bin())
        .arg("mcp")
        .current_dir(temp.path())
        .env_remove("JACS_CONFIG")
        .env_remove("JACS_CONFIG_PATH")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null())
        .output()
        .expect("run mcp without config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("JACS_CONFIG")
            || stderr.contains("jacs")
            || stderr.contains("JACS_PRIVATE_KEY_PASSWORD"),
        "should mention config or password: {stderr}"
    );
}

// ── HTTP helpers (minimal, for mock server) ─────────────────────

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
        if let Some(idx) = buffer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
        {
            header_end = idx;
            break;
        }
    }

    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next()?.to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Some(RecordedRequest {
        method,
        path,
        headers,
    })
}

fn write_response(stream: &mut TcpStream, body: Value) {
    let encoded = body.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        encoded.len(),
        encoded
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
