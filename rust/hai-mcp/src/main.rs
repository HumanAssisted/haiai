fn main() {
    eprintln!("WARNING: The standalone `hai-mcp` binary is deprecated.");
    eprintln!("Use `haiai mcp` instead.");
    eprintln!();
    eprintln!("  haiai mcp    # start MCP server (stdio transport)");
    eprintln!();
    eprintln!("Install: cargo install haiai-cli");
    std::process::exit(1);
}
