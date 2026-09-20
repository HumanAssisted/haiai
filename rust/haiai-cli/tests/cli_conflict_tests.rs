//! Integration tests for `haiai conflict ...` commands.

mod common;

use common::{prepare_jacs_fixture, run_haiai_in_fixture};
use serde_json::{json, Value};

fn write_conflict_body(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("conflict.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "title": "GPU scheduling disagreement",
            "description": "Two parties disagree about who gets a shared GPU window.",
            "participants": [
                {
                    "agentId": "018ff6c4-9a42-7dc0-8bf4-bb7f3e100010",
                    "agentType": "human",
                    "displayName": "Alice",
                    "role": "party"
                },
                {
                    "agentId": "018ff6c4-9a42-7dc0-8bf4-bb7f3e100011",
                    "agentType": "human",
                    "displayName": "Bob",
                    "role": "party"
                }
            ],
            "positions": [],
            "divergences": [],
            "phase": "surfacing"
        }))
        .expect("encode conflict body"),
    )
    .expect("write conflict body");
    path
}

#[test]
fn cli_conflict_create_get_list_update_and_check_readiness() {
    let (temp, _config_path) = prepare_jacs_fixture();
    let body_path = write_conflict_body(temp.path());

    let out = run_haiai_in_fixture(
        temp.path(),
        &[
            "conflict",
            "create",
            "--body",
            body_path.to_str().expect("body path"),
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "conflict create failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let created: Value =
        serde_json::from_slice(&out.stdout).expect("create --json should emit JSON");
    let first_key = created["key"].as_str().expect("created key").to_string();
    assert!(
        first_key.contains(':'),
        "key should be id:version: {created}"
    );
    assert_eq!(created["document"]["jacsType"].as_str(), Some("conflict"));

    let out = run_haiai_in_fixture(temp.path(), &["conflict", "get", &first_key]);
    assert!(
        out.status.success(),
        "conflict get failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let fetched: Value = serde_json::from_slice(&out.stdout).expect("get should emit JSON");
    assert_eq!(fetched["jacsType"].as_str(), Some("conflict"));

    let out = run_haiai_in_fixture(temp.path(), &["conflict", "list", "--json"]);
    assert!(
        out.status.success(),
        "conflict list failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let listed: Value = serde_json::from_slice(&out.stdout).expect("list --json should emit JSON");
    assert!(
        listed["keys"]
            .as_array()
            .expect("keys array")
            .iter()
            .any(|key| key.as_str() == Some(first_key.as_str())),
        "list should include created key: {listed}"
    );

    let out = run_haiai_in_fixture(
        temp.path(),
        &[
            "conflict",
            "update",
            &first_key,
            "--mutation",
            r#"{"type":"addPosition","value":{"id":"pos-alice","participantId":"018ff6c4-9a42-7dc0-8bf4-bb7f3e100010","statement":"Alice needs the shared GPU on Monday.","kind":"resource","statedAt":"2026-06-11T12:01:00Z","confirmed":false}}"#,
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "conflict update failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let updated: Value =
        serde_json::from_slice(&out.stdout).expect("update --json should emit JSON");
    let updated_key = updated["key"].as_str().expect("updated key");
    assert_ne!(updated_key, first_key);
    assert_eq!(
        updated["document"]["positions"].as_array().map(Vec::len),
        Some(1)
    );

    let out = run_haiai_in_fixture(
        temp.path(),
        &["conflict", "check-readiness", updated_key, "--json"],
    );
    assert!(
        out.status.success(),
        "conflict check-readiness failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let readiness: Value =
        serde_json::from_slice(&out.stdout).expect("check-readiness --json should emit JSON");
    assert_eq!(readiness["ready"].as_bool(), Some(false), "{readiness}");
    assert!(
        readiness["blockers"]
            .as_array()
            .map(|blockers| !blockers.is_empty())
            .unwrap_or(false),
        "sparse conflict should have readiness blockers: {readiness}"
    );
}
