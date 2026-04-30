use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
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

    let haiai = value
        .get("mcpServers")
        .and_then(|s| s.get("haiai"))
        .expect(".mcp.json must have mcpServers.haiai entry");

    assert_eq!(
        haiai.get("type").and_then(Value::as_str),
        Some("stdio"),
        ".mcp.json mcpServers.haiai.type must be 'stdio'"
    );

    assert_eq!(
        haiai.get("command").and_then(Value::as_str),
        Some("haiai"),
        ".mcp.json mcpServers.haiai.command must be 'haiai'"
    );

    let args = haiai
        .get("args")
        .and_then(Value::as_array)
        .expect(".mcp.json mcpServers.haiai.args must be an array");
    let arg_strings: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        arg_strings,
        vec!["mcp"],
        ".mcp.json mcpServers.haiai.args must be [\"mcp\"]"
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

/// Extract all backtick-quoted tool names from SKILL.md that match `hai_*` or `jacs_*`
/// and verify they exist in the combined MCP tool surface.
#[test]
fn skill_md_tool_names_exist_in_mcp_server() {
    // Build the set of real tool names dynamically from both servers
    let mut real_tools: HashSet<String> = HashSet::new();

    // HAI tools -- queried from the actual tool definitions
    for tool in hai_mcp::hai_tools::definitions() {
        real_tools.insert(tool.name.to_string());
    }

    // JACS tools -- queried from the actual jacs-mcp server
    for tool in jacs_mcp::JacsMcpServer::tools() {
        real_tools.insert(tool.name.to_string());
    }

    // Read SKILL.md and extract backtick-quoted tool names matching hai_* or jacs_*
    let path = repo_root().join("skills/jacs/SKILL.md");
    let content = fs::read_to_string(&path).expect("read skills/jacs/SKILL.md");

    let mut referenced_tools: HashSet<String> = HashSet::new();
    let mut in_backtick = false;
    let mut current = String::new();

    for ch in content.chars() {
        if ch == '`' {
            if in_backtick {
                // Closing backtick: check if it's a tool name
                let trimmed = current.trim().to_string();
                if (trimmed.starts_with("hai_") || trimmed.starts_with("jacs_"))
                    && !trimmed.contains(' ')
                    && !trimmed.contains('=')
                    && !trimmed.contains('/')
                {
                    referenced_tools.insert(trimmed);
                }
                current.clear();
            }
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(ch);
        }
    }

    // Every tool referenced in SKILL.md must exist in the real tool surface
    let mut missing: Vec<&String> = referenced_tools.difference(&real_tools).collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "SKILL.md references {} tool(s) that don't exist in the MCP server:\n  {}\n\n\
         Real tools ({} total): update SKILL.md or add the tool to the server.",
        missing.len(),
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        real_tools.len()
    );
}

/// Extract CLI subcommand names from haiai-cli/src/main.rs `Commands` enum
/// and verify that every `haiai <subcommand>` documented in SKILL.md actually
/// exists as a real subcommand.
#[test]
fn skill_md_cli_commands_exist_in_binary() {
    // --- Step 1: Parse real subcommands from main.rs Commands enum ---
    let main_rs_path = repo_root().join("rust/haiai-cli/src/main.rs");
    let main_rs = fs::read_to_string(&main_rs_path).expect("read haiai-cli/src/main.rs");

    // Extract the `enum Commands { ... }` block
    let enum_start = main_rs
        .find("enum Commands")
        .expect("Commands enum must exist in main.rs");
    let enum_body_start = main_rs[enum_start..]
        .find('{')
        .expect("Commands enum must have opening brace")
        + enum_start;

    // Find the matching closing brace by counting braces
    let mut depth = 0;
    let mut enum_body_end = enum_body_start;
    for (i, ch) in main_rs[enum_body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    enum_body_end = enum_body_start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    let enum_body = &main_rs[enum_body_start..=enum_body_end];

    // Extract PascalCase variant names and convert to kebab-case (clap convention)
    let variant_re = Regex::new(r"(?m)^\s+(\w+)\s*[\{,]").unwrap();
    let real_subcommands: HashSet<String> = variant_re
        .captures_iter(enum_body)
        .filter_map(|cap| {
            let name = cap.get(1)?.as_str();
            // Skip doc-comments or attributes captured accidentally
            if name.starts_with('#') || name.starts_with('/') {
                return None;
            }
            Some(pascal_to_kebab(name))
        })
        .collect();

    assert!(
        !real_subcommands.is_empty(),
        "Should have found at least one subcommand in Commands enum"
    );

    // --- Step 2: Extract documented CLI commands from SKILL.md ---
    let skill_md_path = repo_root().join("skills/jacs/SKILL.md");
    let skill_md = fs::read_to_string(&skill_md_path).expect("read skills/jacs/SKILL.md");

    // Find the "## CLI Commands" section
    let cli_section_start = skill_md
        .find("## CLI Commands")
        .expect("SKILL.md must have a '## CLI Commands' section");

    // The section ends at the next `## ` heading or EOF
    let cli_section_end = skill_md[(cli_section_start + 15)..]
        .find("\n## ")
        .map(|i| cli_section_start + 15 + i)
        .unwrap_or(skill_md.len());

    let cli_section = &skill_md[cli_section_start..cli_section_end];

    // Extract `haiai <subcommand>` patterns from backtick-quoted or bare references
    let cmd_re = Regex::new(r"`haiai\s+([\w-]+)`").unwrap();
    let documented_commands: HashSet<String> = cmd_re
        .captures_iter(cli_section)
        .filter_map(|cap| Some(cap.get(1)?.as_str().to_string()))
        .collect();

    assert!(
        !documented_commands.is_empty(),
        "Should have found at least one CLI command in SKILL.md CLI Commands section"
    );

    // --- Step 3: Every documented command must exist in the real binary ---
    let mut missing: Vec<&String> = documented_commands.difference(&real_subcommands).collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "SKILL.md documents {} CLI command(s) that don't exist in haiai-cli:\n  {}\n\n\
         Real subcommands ({} total): {}\n\n\
         Update SKILL.md or add the missing subcommands to the CLI binary.",
        missing.len(),
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        real_subcommands.len(),
        {
            let mut sorted: Vec<&String> = real_subcommands.iter().collect();
            sorted.sort();
            sorted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
}

/// Convert a PascalCase identifier to kebab-case (e.g. "UpdateUsername" -> "update-username").
fn pascal_to_kebab(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }
    result
}
