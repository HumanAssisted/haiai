use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn plugin_json_is_valid_and_has_required_fields() {
    let path = repo_root().join(".claude-plugin/plugin.json");
    let raw = fs::read_to_string(&path).expect("read .claude-plugin/plugin.json");
    let value: Value = serde_json::from_str(&raw).expect("parse plugin.json as valid JSON");

    assert_eq!(
        value.get("name").and_then(Value::as_str),
        Some("haiai"),
        "plugin.json name must be 'haiai'"
    );

    let version = value
        .get("version")
        .and_then(Value::as_str)
        .expect("plugin.json must have a version string");
    assert!(!version.is_empty(), "plugin.json version must not be empty");

    let description = value
        .get("description")
        .and_then(Value::as_str)
        .expect("plugin.json must have a description string");
    assert!(
        !description.is_empty(),
        "plugin.json description must not be empty"
    );

    assert!(
        value.get("mcpServers").is_none(),
        "plugin.json must not contain mcpServers (use .mcp.json instead)"
    );
}

#[test]
fn marketplace_json_is_valid_and_has_required_fields() {
    let path = repo_root().join(".claude-plugin/marketplace.json");
    let raw = fs::read_to_string(&path).expect("read .claude-plugin/marketplace.json");
    let value: Value = serde_json::from_str(&raw).expect("parse marketplace.json as valid JSON");

    let name = value
        .get("name")
        .and_then(Value::as_str)
        .expect("marketplace.json must have a name");
    assert!(!name.is_empty(), "marketplace.json name must not be empty");

    let owner_name = value
        .get("owner")
        .and_then(|o| o.get("name"))
        .and_then(Value::as_str)
        .expect("marketplace.json must have owner.name");
    assert!(
        !owner_name.is_empty(),
        "marketplace.json owner.name must not be empty"
    );

    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .expect("marketplace.json must have a plugins array");
    assert!(
        !plugins.is_empty(),
        "marketplace.json plugins must not be empty"
    );

    assert_eq!(
        plugins[0].get("name").and_then(Value::as_str),
        Some("haiai"),
        "marketplace.json plugins[0].name must be 'haiai'"
    );
    assert_eq!(
        plugins[0].get("source").and_then(Value::as_str),
        Some("./"),
        "marketplace.json plugins[0].source must be './'"
    );
}

#[test]
fn mcp_json_is_valid_and_points_to_haiai_binary() {
    let path = repo_root().join(".mcp.json");
    let raw = fs::read_to_string(&path).expect("read .mcp.json");
    let value: Value = serde_json::from_str(&raw).expect("parse .mcp.json as valid JSON");

    assert_eq!(
        value
            .get("haiai")
            .and_then(|h| h.get("command"))
            .and_then(Value::as_str),
        Some("haiai"),
        ".mcp.json haiai.command must be 'haiai'"
    );

    let args = value
        .get("haiai")
        .and_then(|h| h.get("args"))
        .and_then(Value::as_array)
        .expect(".mcp.json haiai.args must be an array");
    let arg_strings: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        arg_strings,
        vec!["mcp"],
        ".mcp.json haiai.args must be [\"mcp\"]"
    );
}

#[test]
fn skill_md_exists_with_valid_frontmatter() {
    let path = repo_root().join("skills/jacs/SKILL.md");
    let content = fs::read_to_string(&path).expect("read skills/jacs/SKILL.md");

    assert!(
        content.starts_with("---"),
        "SKILL.md must start with YAML frontmatter delimiter '---'"
    );

    // Find the closing frontmatter delimiter
    let rest = &content[3..];
    let end_idx = rest
        .find("\n---")
        .expect("SKILL.md must have closing frontmatter delimiter '---'");
    let frontmatter = &rest[..end_idx];

    assert!(
        frontmatter.contains("name: jacs"),
        "SKILL.md frontmatter must contain 'name: jacs'"
    );
    assert!(
        frontmatter.contains("description:"),
        "SKILL.md frontmatter must contain 'description:'"
    );
    assert!(
        !frontmatter.contains("user-invocable"),
        "SKILL.md frontmatter must NOT contain 'user-invocable' (OpenClaw-specific)"
    );
    assert!(
        !frontmatter.contains("metadata"),
        "SKILL.md frontmatter must NOT contain 'metadata' (OpenClaw-specific)"
    );
}

#[test]
fn skill_md_does_not_reference_openclaw() {
    let path = repo_root().join("skills/jacs/SKILL.md");
    let content = fs::read_to_string(&path).expect("read skills/jacs/SKILL.md");

    let lower = content.to_lowercase();
    assert!(
        !lower.contains("openclaw jacs"),
        "SKILL.md must not reference 'openclaw jacs'"
    );
    assert!(
        !lower.contains("openclaw.plugin"),
        "SKILL.md must not reference 'openclaw.plugin'"
    );
}

#[test]
fn plugin_version_matches_cargo_version() {
    let path = repo_root().join(".claude-plugin/plugin.json");
    let raw = fs::read_to_string(&path).expect("read .claude-plugin/plugin.json");
    let value: Value = serde_json::from_str(&raw).expect("parse plugin.json");

    let plugin_version = value
        .get("version")
        .and_then(Value::as_str)
        .expect("plugin.json must have a version");
    let cargo_version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        plugin_version, cargo_version,
        "plugin.json version ({}) must match hai-mcp Cargo.toml version ({})",
        plugin_version, cargo_version
    );
}
