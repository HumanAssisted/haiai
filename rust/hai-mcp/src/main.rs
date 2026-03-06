fn main() {
    eprintln!("WARNING: The standalone `hai-mcp` binary is deprecated.");
    eprintln!("Use `haisdk mcp` instead.");
    eprintln!();
    eprintln!("  haisdk mcp    # start MCP server (stdio transport)");
    eprintln!();
    eprintln!("Install: cargo install haisdk-cli");
    std::process::exit(1);
}
