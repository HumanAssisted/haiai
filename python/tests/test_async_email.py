"""Async email tests for AsyncHaiClient (server-side signing model)."""

from __future__ import annotations

import base64
from typing import Any

import pytest

from haiai.async_client import AsyncHaiClient
from haiai.errors import HaiError


BASE_URL = "https://test.hai.ai"
TEST_AGENT_EMAIL = "test-jacs-id-1234@hai.ai"


@pytest.mark.asyncio
async def test_async_send_email_requires_agent_email(
    loaded_config: None,
) -> None:
    client = AsyncHaiClient()
    with pytest.raises(HaiError, match="agent email not set"):
        await client.send_email(BASE_URL, "bob@hai.ai", "subject", "body")


@pytest.mark.asyncio
async def test_async_send_email_server_side_signing(
    loaded_config: None,
) -> None:
    """Verify send_email sends only content fields (no client-side signing)."""
    client = AsyncHaiClient()
    client._agent_email = TEST_AGENT_EMAIL  # type: ignore[attr-defined]
    mock_ffi = client._get_ffi()
    mock_ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}

    await client.send_email(BASE_URL, "bob@hai.ai", "Test Subject", "Test Body")

    assert mock_ffi.calls[0][0] == "send_email"
    options = mock_ffi.calls[0][1][0]
    assert options["to"] == "bob@hai.ai"
    assert options["subject"] == "Test Subject"
    assert options["body"] == "Test Body"
    # Server handles JACS signing -- client must NOT send these fields
    assert "jacs_signature" not in options
    assert "jacs_timestamp" not in options


@pytest.mark.asyncio
async def test_async_send_email_attachment_payload_no_client_signing(
    loaded_config: None,
) -> None:
    """Verify attachments are base64-encoded but no client-side signing."""
    attachments = [
        {
            "filename": "a.txt",
            "content_type": "text/plain",
            "data": b"alpha",
        },
        {
            "filename": "b.txt",
            "content_type": "text/plain",
            "data": b"beta",
        },
    ]

    client = AsyncHaiClient()
    client._agent_email = TEST_AGENT_EMAIL  # type: ignore[attr-defined]
    mock_ffi = client._get_ffi()
    mock_ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}

    await client.send_email(
        BASE_URL,
        "bob@hai.ai",
        "Attachment Subject",
        "Attachment Body",
        attachments=attachments,
    )

    options = mock_ffi.calls[0][1][0]
    assert "attachments" in options
    assert len(options["attachments"]) == 2
    assert base64.b64decode(options["attachments"][0]["data_base64"]) == b"alpha"
    assert base64.b64decode(options["attachments"][1]["data_base64"]) == b"beta"
    # Server handles JACS signing -- client must NOT send these fields
    assert "jacs_signature" not in options
    assert "jacs_timestamp" not in options


@pytest.mark.asyncio
async def test_async_update_and_delete_username(
    loaded_config: None,
) -> None:
    client = AsyncHaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["update_username"] = {
        "username": "new-name",
        "email": "new-name@hai.ai",
        "previous_username": "old-name",
    }
    mock_ffi.responses["delete_username"] = {
        "released_username": "old-name",
        "cooldown_until": "2026-03-01T00:00:00Z",
        "message": "released",
    }

    updated = await client.update_username(BASE_URL, "agent/with/slash", "new-name")
    assert mock_ffi.calls[0][0] == "update_username"
    assert mock_ffi.calls[0][1][0] == "agent/with/slash"
    assert updated["username"] == "new-name"

    deleted = await client.delete_username(BASE_URL, "agent/with/slash")
    assert mock_ffi.calls[1][0] == "delete_username"
    assert mock_ffi.calls[1][1][0] == "agent/with/slash"
    assert deleted["message"] == "released"
