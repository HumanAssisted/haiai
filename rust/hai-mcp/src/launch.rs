#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Run,
    PrintHelp,
    PrintVersion,
}

pub fn parse_launch_mode(args: &[String]) -> Result<LaunchMode, String> {
    match args {
        [] => Ok(LaunchMode::Run),
        [arg] if matches!(arg.as_str(), "--help" | "-h" | "help") => Ok(LaunchMode::PrintHelp),
        [arg] if matches!(arg.as_str(), "--version" | "-V" | "version") => {
            Ok(LaunchMode::PrintVersion)
        }
        _ => Err(
            "hai-mcp is local-only and stdio-only. Runtime transport/listener arguments are not supported."
                .to_string(),
        ),
    }
}

pub fn help_text() -> String {
    concat!(
        "Usage: hai-mcp [--help] [--version]\n",
        "\n",
        "HAISDK MCP server extending jacs-mcp in-process.\n",
        "Transport is always local stdio.\n",
        "\n",
        "Environment:\n",
        "  JACS_CONFIG  Path to the local jacs.config.json used by the embedded jacs-mcp server\n",
        "  HAI_URL      Optional base URL for HAI API requests (default: https://hai.ai)\n",
        "  RUST_LOG     Optional tracing filter (default: info,rmcp=warn)\n"
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_launch_mode_is_stdio_only() {
        assert_eq!(parse_launch_mode(&[]), Ok(LaunchMode::Run));
        assert_eq!(
            parse_launch_mode(&["--help".to_string()]),
            Ok(LaunchMode::PrintHelp)
        );
        assert_eq!(
            parse_launch_mode(&["--version".to_string()]),
            Ok(LaunchMode::PrintVersion)
        );

        let err = parse_launch_mode(&["--transport".to_string(), "http".to_string()])
            .expect_err("runtime args should be rejected");
        assert!(err.contains("stdio-only"));
    }

    #[test]
    fn help_text_does_not_reference_bridge_env_vars() {
        let text = help_text();
        assert!(text.contains("JACS_CONFIG"));
        assert!(!text.contains("JACS_MCP_BIN"));
        assert!(!text.contains("JACS_MCP_ARGS"));
    }
}
