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
        self.last_params: dict[str, Any] | None = None

    async def post(self, url: str, **kwargs: Any) -> _FakeAsyncResponse:
        self.last_url = url
        self.last_json = kwargs.get("json")
        if "username" in (self.last_json or {}):
            return _FakeAsyncResponse(
                200,
                {
                    "username": self.last_json["username"],
                    "email": f"{self.last_json['username']}@hai.ai",
                    "agent_id": "agent-123",
                },
            )
        return _FakeAsyncResponse(200, {"message_id": "msg-1", "status": "sent"})

    async def put(self, url: str, **kwargs: Any) -> _FakeAsyncResponse:
        self.last_url = url
        self.last_json = kwargs.get("json")
        return _FakeAsyncResponse(
            200,
            {
                "username": self.last_json.get("username", ""),
                "email": f"{self.last_json.get('username', '')}@hai.ai",
                "previous_username": "old-name",
            },
        )

    async def delete(self, url: str, **kwargs: Any) -> _FakeAsyncResponse:
        self.last_url = url
        return _FakeAsyncResponse(
            200,
            {
                "released_username": "old-name",
                "cooldown_until": "2026-03-01T00:00:00Z",
                "message": "released",
            },
        )

    async def get(self, url: str, **kwargs: Any) -> _FakeAsyncResponse:
        self.last_url = url
        self.last_params = kwargs.get("params")
        return _FakeAsyncResponse(
            200,
            {
                "available": True,
                "username": (self.last_params or {}).get("username", ""),
                "reason": None,
            },
        )


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


@pytest.mark.asyncio
async def test_async_check_username_uses_public_endpoint(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)
    client = AsyncHaiClient()

    result = await client.check_username(BASE_URL, "alice")
    assert fake_http.last_url == "https://test.hai.ai/api/v1/agents/username/check"
    assert fake_http.last_params == {"username": "alice"}
    assert result["available"] is True
    assert result["username"] == "alice"


@pytest.mark.asyncio
async def test_async_claim_username_sets_agent_email_and_escapes_agent_id(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)
    client = AsyncHaiClient()

    result = await client.claim_username(BASE_URL, "agent/with/slash", "myagent")
    assert (
        fake_http.last_url
        == "https://test.hai.ai/api/v1/agents/agent%2Fwith%2Fslash/username"
    )
    assert result["email"] == "myagent@hai.ai"
    assert client.agent_email == "myagent@hai.ai"


@pytest.mark.asyncio
async def test_async_update_and_delete_username_escape_agent_id(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)
    client = AsyncHaiClient()

    updated = await client.update_username(BASE_URL, "agent/with/slash", "new-name")
    assert (
        fake_http.last_url
        == "https://test.hai.ai/api/v1/agents/agent%2Fwith%2Fslash/username"
    )
    assert updated["username"] == "new-name"

    deleted = await client.delete_username(BASE_URL, "agent/with/slash")
    assert (
        fake_http.last_url
        == "https://test.hai.ai/api/v1/agents/agent%2Fwith%2Fslash/username"
    )
    assert deleted["message"] == "released"
