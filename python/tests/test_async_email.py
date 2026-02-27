"""Async email signing parity tests for AsyncHaiClient."""

from __future__ import annotations

import base64
from typing import Any

import pytest

from jacs.hai.async_client import AsyncHaiClient
from jacs.hai.client import compute_content_hash
from jacs.hai.crypt import verify_string
from jacs.hai.errors import HaiError


BASE_URL = "https://test.hai.ai"
TEST_AGENT_EMAIL = "test-jacs-id-1234@hai.ai"


class _FakeAsyncResponse:
    def __init__(self, status_code: int, payload: dict[str, Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""

    def json(self) -> dict[str, Any]:
        return self._payload


class _FakeAsyncHTTP:
    def __init__(self) -> None:
        self.last_url: str | None = None
        self.last_json: dict[str, Any] | None = None

    async def post(self, url: str, **kwargs: Any) -> _FakeAsyncResponse:
        self.last_url = url
        self.last_json = kwargs.get("json")
        return _FakeAsyncResponse(200, {"message_id": "msg-1", "status": "sent"})


@pytest.mark.asyncio
async def test_async_send_email_requires_agent_email(
    loaded_config: None,
) -> None:
    client = AsyncHaiClient()
    with pytest.raises(HaiError, match="agent email not set"):
        await client.send_email(BASE_URL, "bob@hai.ai", "subject", "body")


@pytest.mark.asyncio
async def test_async_send_email_uses_v2_sign_input_with_from_email(
    loaded_config: None,
    ed25519_keypair: tuple,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    private_key, _ = ed25519_keypair
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

    client = AsyncHaiClient()
    client._agent_email = TEST_AGENT_EMAIL  # type: ignore[attr-defined]
    await client.send_email(BASE_URL, "bob@hai.ai", "Test Subject", "Test Body")

    assert fake_http.last_url == "https://test.hai.ai/api/agents/test-jacs-id-1234/email/send"
    assert fake_http.last_json is not None

    payload = fake_http.last_json
    content_hash = compute_content_hash("Test Subject", "Test Body", None)
    sign_input = f"{content_hash}:{TEST_AGENT_EMAIL}:{payload['jacs_timestamp']}"
    assert verify_string(private_key.public_key(), sign_input, payload["jacs_signature"])


@pytest.mark.asyncio
async def test_async_send_email_attachment_hash_and_payload_match_v2(
    loaded_config: None,
    ed25519_keypair: tuple,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    private_key, _ = ed25519_keypair
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

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
    await client.send_email(
        BASE_URL,
        "bob@hai.ai",
        "Attachment Subject",
        "Attachment Body",
        attachments=attachments,
    )

    assert fake_http.last_json is not None
    payload = fake_http.last_json
    assert "attachments" in payload
    assert len(payload["attachments"]) == 2
    assert base64.b64decode(payload["attachments"][0]["data_base64"]) == b"alpha"
    assert base64.b64decode(payload["attachments"][1]["data_base64"]) == b"beta"

    expected_hash = compute_content_hash(
        "Attachment Subject",
        "Attachment Body",
        attachments,
    )
    sign_input = f"{expected_hash}:{TEST_AGENT_EMAIL}:{payload['jacs_timestamp']}"
    assert verify_string(private_key.public_key(), sign_input, payload["jacs_signature"])
