// Copyright (c) 2026 Human Assisted Intelligence, Inc.
//
// Use of this software is governed by the Business Source License 1.1
// included in the LICENSE file.
//
// SPDX-License-Identifier: BUSL-1.1

fn main() {
    eprintln!("WARNING: The standalone `hai-mcp` binary is deprecated.");
    eprintln!("Use `haiai mcp` instead.");
    eprintln!();
    eprintln!("  haiai mcp    # start MCP server (stdio transport)");
    eprintln!();
    eprintln!("Install: cargo install haiai-cli");
    std::process::exit(1);
}
