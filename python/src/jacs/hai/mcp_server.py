"""HAI SDK MCP server -- exposes SDK methods as MCP tools.

Run directly::

    python -m jacs.hai.mcp_server

Or configure in Claude Desktop / Cursor MCP settings.

Requires ``pip install haisdk[mcp]``.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Any, Optional

logger = logging.getLogger("jacs.hai.mcp_server")

DEFAULT_API_URL = os.environ.get("HAI_API_URL", "https://hai.ai")


def _load_config() -> None:
    """Try to load JACS config from env or default path."""
    from jacs.hai.config import is_loaded, load

    if is_loaded():
        return
    config_path = os.environ.get("HAI_CONFIG_PATH", os.environ.get("JACS_CONFIG_PATH"))
    try:
        load(config_path)
    except (FileNotFoundError, ValueError):
        pass


def _get_url() -> str:
    return os.environ.get("HAI_API_URL", DEFAULT_API_URL)


def create_server() -> Any:
    """Create and configure the MCP server with HAI SDK tools."""
    try:
        from mcp.server import Server
        from mcp.types import TextContent, Tool
    except ImportError:
        raise ImportError(
            "MCP dependencies not installed. Run: pip install haisdk[mcp]"
        )

    server = Server("hai-sdk")

    @server.tool()
    async def hai_register_agent(
        name: str,
        owner_email: str,
        version: str = "1.0.0",
        key_dir: str = "./keys",
    ) -> str:
        """Register a new JACS agent with HAI.AI.

        Creates a keypair, self-signs an agent document, registers with
        HAI, and saves the config locally.

        Args:
            name: Agent display name (ASCII-only).
            owner_email: Owner's email for verification.
            version: Agent version string.
            key_dir: Directory for key files.
        """
        from jacs.hai.client import register_new_agent

        result = register_new_agent(
            name=name,
            owner_email=owner_email,
            version=version,
            hai_url=_get_url(),
            key_dir=key_dir,
        )
        return json.dumps({
            "agent_id": result.agent_id,
            "jacs_id": result.jacs_id,
            "message": f"Agent registered. Check {owner_email} for verification.",
        })

    @server.tool()
    async def hai_hello() -> str:
        """Run a hello world handshake with HAI.

        Requires a loaded JACS config (run hai_register_agent first).
        """
        _load_config()
        from jacs.hai.client import hello_world

        result = hello_world(_get_url())
        return json.dumps({
            "success": result.success,
            "message": result.message,
            "timestamp": result.timestamp,
            "client_ip": result.client_ip,
        })

    @server.tool()
    async def hai_check_username(username: str) -> str:
        """Check if a @hai.ai username is available.

        Args:
            username: Desired username (3-30 chars, alphanumeric + hyphens).
        """
        from jacs.hai.client import HaiClient

        client = HaiClient()
        result = client.check_username(_get_url(), username)
        available = result.get("available", False)
        return json.dumps({
            "username": username,
            "email": f"{username}@hai.ai",
            "available": available,
            "reason": result.get("reason", ""),
        })

    @server.tool()
    async def hai_claim_username(username: str) -> str:
        """Claim a @hai.ai username for the current agent.

        Requires a loaded JACS config.

        Args:
            username: Username to claim.
        """
        _load_config()
        from jacs.hai.client import HaiClient
        from jacs.hai.config import get_config

        client = HaiClient()
        cfg = get_config()
        agent_id = cfg.jacs_id or ""
        result = client.claim_username(_get_url(), agent_id, username)
        return json.dumps(result)

    @server.tool()
    async def hai_send_email(to: str, subject: str, body: str) -> str:
        """Send an email from the agent's @hai.ai address.

        Requires a loaded JACS config with a claimed username.

        Args:
            to: Recipient address (must be @hai.ai for MVP).
            subject: Email subject line.
            body: Plain text email body.
        """
        _load_config()
        from jacs.hai.client import HaiClient

        client = HaiClient()
        result = client.send_email(_get_url(), to, subject, body)
        return json.dumps({
            "message_id": result.message_id,
            "status": result.status,
        })

    @server.tool()
    async def hai_list_messages(
        limit: int = 20,
        folder: str = "inbox",
    ) -> str:
        """List email messages for the current agent.

        Args:
            limit: Max messages to return (default: 20).
            folder: "inbox" or "sent".
        """
        _load_config()
        from jacs.hai.client import HaiClient

        client = HaiClient()
        messages = client.list_messages(_get_url(), limit=limit, folder=folder)
        return json.dumps([
            {
                "id": m.id,
                "from": m.from_address,
                "to": m.to_address,
                "subject": m.subject,
                "sent_at": m.sent_at,
                "read": m.read_at is not None,
            }
            for m in messages
        ])

    @server.tool()
    async def hai_run_benchmark(tier: str = "free") -> str:
        """Run a benchmark on HAI.

        Args:
            tier: "free", "dns_certified", or "fully_certified".
        """
        _load_config()
        from jacs.hai.client import HaiClient

        client = HaiClient()
        result = client.benchmark(_get_url(), tier=tier)
        return json.dumps({
            "success": result.success,
            "score": result.score,
            "passed": result.passed,
            "failed": result.failed,
            "total": result.total,
        })

    @server.tool()
    async def hai_verify_agent(jacs_id: str) -> str:
        """Check registration and badge level of an agent.

        Args:
            jacs_id: The agent's JACS ID to verify.
        """
        _load_config()
        from jacs.hai.client import HaiClient

        client = HaiClient()
        result = client.get_agent_attestation(_get_url(), jacs_id)
        return json.dumps({
            "registered": result.registered,
            "agent_id": result.agent_id,
            "registered_at": result.registered_at,
            "algorithms": result.hai_signatures,
        })

    @server.tool()
    async def hai_fetch_key(
        jacs_id: str,
        version: str = "latest",
    ) -> str:
        """Fetch an agent's public key from HAI.

        Args:
            jacs_id: Target agent's JACS ID.
            version: Key version ("latest" or specific).
        """
        from jacs.hai.client import HaiClient

        client = HaiClient()
        info = client.fetch_remote_key(_get_url(), jacs_id, version)
        return json.dumps({
            "jacs_id": info.jacs_id,
            "algorithm": info.algorithm,
            "public_key_hash": info.public_key_hash,
            "status": info.status,
            "dns_verified": info.dns_verified,
            "public_key": info.public_key,
        })

    return server


def main() -> None:
    """Run the MCP server."""
    import asyncio
    server = create_server()
    asyncio.run(server.run())


if __name__ == "__main__":
    main()
