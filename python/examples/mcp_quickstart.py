#!/usr/bin/env python3
"""MCP quickstart using HAISDK integration wrappers.

Demonstrates:
1. JACS quickstart with required identity fields
2. MCP server bootstrap via HAISDK wrapper
3. Expanded JACS/A2A/trust MCP toolsets

Prerequisites:
    pip install "haisdk[mcp,a2a]" jacs

Usage:
    python python/examples/mcp_quickstart.py
"""

from __future__ import annotations

from jacs.client import JacsClient

from haisdk.mcp import (
    create_mcp_server,
    register_a2a_tools,
    register_jacs_tools,
    register_trust_tools,
)


def main() -> None:
    jacs = JacsClient.quickstart(
        name="hai-agent",
        domain="agent.example.com",
        description="HAISDK MCP agent",
        algorithm="pq2025",
    )

    mcp = create_mcp_server("hai-sdk-example-mcp")
    register_jacs_tools(mcp, client=jacs)
    register_a2a_tools(mcp, client=jacs)
    register_trust_tools(mcp, client=jacs)

    print("Registered MCP toolsets:")
    print("- jacs_share_public_key / jacs_share_agent")
    print("- A2A toolset (sign/verify/export/register helpers)")
    print("- jacs_trust_agent_with_key")


if __name__ == "__main__":
    main()

