"""Standalone MCP server exposing HAI SDK operations as tools.

Usage:
    haisdk-mcp                           # stdio transport
    python -m haisdk.mcp_server          # alternative
    HAI_URL=https://hai.ai haisdk-mcp    # override API endpoint
"""

from __future__ import annotations

import json
import os
from dataclasses import asdict
from typing import Optional

from mcp.server.fastmcp import FastMCP

server = FastMCP("hai-sdk")

_DEFAULT_URL = os.environ.get("HAI_URL", "https://hai.ai")


def _url(hai_url: str | None) -> str:
    return hai_url or _DEFAULT_URL


def _to_json(obj: object) -> str:
    if hasattr(obj, "__dataclass_fields__"):
        return json.dumps(asdict(obj), default=str)  # type: ignore[arg-type]
    if isinstance(obj, (dict, list)):
        return json.dumps(obj, default=str)
    return str(obj)


# ---------------------------------------------------------------------------
# Identity tools
# ---------------------------------------------------------------------------


@server.tool()
async def hai_hello(
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Run authenticated hello handshake with HAI."""
    from jacs.hai.client import hello_world

    result = hello_world(_url(hai_url))
    return _to_json(result)


@server.tool()
async def hai_register_agent(
    owner_email: str | None = None,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Register the local JACS agent with HAI."""
    from jacs.hai.client import register

    result = register(_url(hai_url), owner_email=owner_email)
    return _to_json(result)


@server.tool()
async def hai_agent_status(
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Get the current agent's verification status."""
    from jacs.hai.client import status

    result = status(_url(hai_url))
    return _to_json(result)


@server.tool()
async def hai_check_username(
    username: str,
    hai_url: str | None = None,
) -> str:
    """Check if a hai.ai username is available."""
    from jacs.hai.client import check_username

    result = check_username(_url(hai_url), username)
    return _to_json(result)


@server.tool()
async def hai_claim_username(
    agent_id: str,
    username: str,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Claim a hai.ai username for an agent."""
    from jacs.hai.client import claim_username

    result = claim_username(_url(hai_url), agent_id, username)
    return _to_json(result)


@server.tool()
async def hai_verify_agent(
    agent_document: str,
    hai_url: str | None = None,
) -> str:
    """Verify another agent's JACS document."""
    from jacs.hai.client import verify_agent

    result = verify_agent(agent_document, hai_url=_url(hai_url))
    return _to_json(result)


@server.tool()
async def hai_generate_verify_link(
    document: str,
    base_url: str | None = None,
    hosted: bool = False,
) -> str:
    """Generate a HAI verify link from a signed JACS document."""
    from jacs.hai.client import generate_verify_link

    url = generate_verify_link(document, base_url=base_url or "https://hai.ai", hosted=hosted)
    return json.dumps({"verify_url": url})


# ---------------------------------------------------------------------------
# Email tools
# ---------------------------------------------------------------------------


@server.tool()
async def hai_send_email(
    to: str,
    subject: str,
    body: str,
    in_reply_to: str | None = None,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Send an email from the agent's @hai.ai address."""
    from jacs.hai.client import send_email

    result = send_email(_url(hai_url), to=to, subject=subject, body=body, in_reply_to=in_reply_to)
    return _to_json(result)


@server.tool()
async def hai_list_messages(
    limit: int = 20,
    offset: int = 0,
    direction: str | None = None,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """List email messages in the agent's inbox/outbox."""
    from jacs.hai.client import list_messages

    result = list_messages(_url(hai_url), limit=limit, offset=offset, direction=direction)
    return _to_json(result)


@server.tool()
async def hai_get_message(
    message_id: str,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Get a single email message by ID."""
    from jacs.hai.client import get_message

    result = get_message(_url(hai_url), message_id)
    return _to_json(result)


@server.tool()
async def hai_delete_message(
    message_id: str,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Delete an email message."""
    from jacs.hai.client import delete_message

    delete_message(_url(hai_url), message_id)
    return json.dumps({"deleted": True, "message_id": message_id})


@server.tool()
async def hai_mark_read(
    message_id: str,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Mark an email message as read."""
    from jacs.hai.client import mark_read

    mark_read(_url(hai_url), message_id)
    return json.dumps({"message_id": message_id, "is_read": True})


@server.tool()
async def hai_mark_unread(
    message_id: str,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Mark an email message as unread."""
    from jacs.hai.client import mark_unread

    mark_unread(_url(hai_url), message_id)
    return json.dumps({"message_id": message_id, "is_read": False})


@server.tool()
async def hai_search_messages(
    q: str | None = None,
    direction: str | None = None,
    from_address: str | None = None,
    to_address: str | None = None,
    since: str | None = None,
    until: str | None = None,
    limit: int = 20,
    offset: int = 0,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Search email messages by query, sender, recipient, or date range."""
    from jacs.hai.client import search_messages

    result = search_messages(
        _url(hai_url),
        q=q,
        direction=direction,
        from_address=from_address,
        to_address=to_address,
        since=since,
        until=until,
        limit=limit,
        offset=offset,
    )
    return _to_json(result)


@server.tool()
async def hai_get_unread_count(
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Get the count of unread email messages."""
    from jacs.hai.client import get_unread_count

    count = get_unread_count(_url(hai_url))
    return json.dumps({"count": count})


@server.tool()
async def hai_get_email_status(
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Get email account status including usage limits and daily stats."""
    from jacs.hai.client import get_email_status

    result = get_email_status(_url(hai_url))
    return _to_json(result)


@server.tool()
async def hai_reply_email(
    message_id: str,
    body: str,
    subject_override: str | None = None,
    config_path: str | None = None,
    hai_url: str | None = None,
) -> str:
    """Reply to an email message (fetches original, sends reply with threading)."""
    from jacs.hai.client import reply

    result = reply(_url(hai_url), message_id, body, subject=subject_override)
    return _to_json(result)


def main() -> None:
    """Entry point for haisdk-mcp CLI command."""
    server.run(transport="stdio")


if __name__ == "__main__":
    main()
