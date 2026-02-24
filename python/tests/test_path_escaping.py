"""Path-escaping regression tests for URL path segments."""

from __future__ import annotations

from typing import Any

import pytest

from jacs.hai.async_client import AsyncHaiClient
from jacs.hai.client import HaiClient


class _FakeResponse:
    def __init__(self, status_code: int, payload: dict[str, Any] | list[Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""
        self.headers: dict[str, str] = {}

    def json(self) -> dict[str, Any] | list[Any]:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise RuntimeError(f"http error: {self.status_code}")


class _FakeAsyncHTTP:
    def __init__(self) -> None:
        self.last_get_url: str | None = None
        self.last_post_url: str | None = None

    async def get(self, url: str, **_kwargs: Any) -> _FakeResponse:
        self.last_get_url = url
        return _FakeResponse(
            200,
            {
                "jacs_id": "remote/agent",
                "version": "2026/01",
                "public_key": "pem",
            },
        )

    async def post(self, url: str, **_kwargs: Any) -> _FakeResponse:
        self.last_post_url = url
        return _FakeResponse(200, {})


def test_claim_username_escapes_agent_id_path_segment(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_post(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(200, {"username": "alice"})

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)

    client = HaiClient()
    client.claim_username("https://hai.ai", "agent/../with/slash", "alice")

    assert captured["url"] == "https://hai.ai/api/v1/agents/agent%2F..%2Fwith%2Fslash/username"


def test_update_username_escapes_agent_id_path_segment(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_put(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(200, {"username": "new-name", "email": "new-name@hai.ai", "previous_username": "old-name"})

    import httpx

    monkeypatch.setattr(httpx, "put", fake_put)

    client = HaiClient()
    client.update_username("https://hai.ai", "agent/../with/slash", "new-name")

    assert captured["url"] == "https://hai.ai/api/v1/agents/agent%2F..%2Fwith%2Fslash/username"


def test_delete_username_escapes_agent_id_path_segment(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_delete(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(
            200,
            {
                "released_username": "old-name",
                "cooldown_until": "2026-03-01T00:00:00Z",
                "message": "released",
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "delete", fake_delete)

    client = HaiClient()
    client.delete_username("https://hai.ai", "agent/../with/slash")

    assert captured["url"] == "https://hai.ai/api/v1/agents/agent%2F..%2Fwith%2Fslash/username"


def test_verify_document_posts_to_public_endpoint(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}

    def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["json"] = kwargs.get("json")
        captured["headers"] = kwargs.get("headers")
        return _FakeResponse(
            200,
            {
                "valid": True,
                "verified_at": "2026-01-01T00:00:00Z",
                "document_type": "JacsDocument",
                "issuer_verified": True,
                "signature_verified": True,
                "signer_id": "agent-1",
                "signed_at": "2026-01-01T00:00:00Z",
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)

    client = HaiClient()
    result = client.verify_document("https://hai.ai", {"jacsId": "agent-1"})

    assert captured["url"] == "https://hai.ai/api/jacs/verify"
    assert captured["json"] == {"document": '{"jacsId": "agent-1"}'}
    assert "Authorization" not in (captured["headers"] or {})
    assert result["valid"] is True


def test_get_verification_escapes_agent_id_and_uses_public_endpoint(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}

    def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["headers"] = kwargs.get("headers")
        return _FakeResponse(
            200,
            {
                "agent_id": "agent/with/slash",
                "verification": {
                    "jacs_valid": True,
                    "dns_valid": True,
                    "hai_registered": False,
                    "badge": "domain",
                },
                "hai_signatures": ["ed25519:abc..."],
                "verified_at": "2026-01-02T00:00:00Z",
                "errors": [],
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "get", fake_get)

    client = HaiClient()
    result = client.get_verification("https://hai.ai", "agent/with/slash")

    assert captured["url"] == "https://hai.ai/api/v1/agents/agent%2Fwith%2Fslash/verification"
    assert "Authorization" not in (captured["headers"] or {})
    assert result["verification"]["badge"] == "domain"


def test_verify_agent_document_posts_to_public_endpoint(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}

    def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["json"] = kwargs.get("json")
        captured["headers"] = kwargs.get("headers")
        return _FakeResponse(
            200,
            {
                "agent_id": "agent-1",
                "verification": {
                    "jacs_valid": True,
                    "dns_valid": True,
                    "hai_registered": True,
                    "badge": "attested",
                },
                "hai_signatures": ["ed25519:def..."],
                "verified_at": "2026-01-02T00:00:00Z",
                "errors": [],
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)

    client = HaiClient()
    result = client.verify_agent_document(
        "https://hai.ai",
        {"jacsId": "agent-1"},
        domain="example.com",
    )

    assert captured["url"] == "https://hai.ai/api/v1/agents/verify"
    assert captured["json"] == {"agent_json": '{"jacsId": "agent-1"}', "domain": "example.com"}
    assert "Authorization" not in (captured["headers"] or {})
    assert result["verification"]["badge"] == "attested"


def test_submit_benchmark_response_escapes_job_id_path_segment(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_post(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(200, {"success": True, "job_id": "job/with/slash"})

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)

    client = HaiClient()
    client.submit_benchmark_response(
        "https://hai.ai",
        job_id="job/with/slash",
        message="ok",
    )

    assert captured["url"] == "https://hai.ai/api/v1/agents/jobs/job%2Fwith%2Fslash/response"


def test_mark_read_escapes_agent_id_and_message_id_path_segments(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_post(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(200, {})

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)

    client = HaiClient()
    monkeypatch.setattr(client, "_get_jacs_id", lambda: "agent/with/slash")

    client.mark_read("https://hai.ai", "message/with/slash")

    assert captured["url"] == (
        "https://hai.ai/api/agents/agent%2Fwith%2Fslash/email/messages/message%2Fwith%2Fslash/read"
    )


def test_fetch_remote_key_escapes_jacs_id_and_version_path_segments(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        return _FakeResponse(
            200,
            {
                "jacs_id": "remote/agent",
                "version": "2026/01",
                "public_key": "pem",
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "get", fake_get)

    client = HaiClient()
    client.fetch_remote_key("https://hai.ai", "remote/agent", "2026/01")

    assert captured["url"] == "https://hai.ai/jacs/v1/agents/remote%2Fagent/keys/2026%2F01"


@pytest.mark.asyncio
async def test_async_mark_read_escapes_agent_id_and_message_id_path_segments(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

    client = AsyncHaiClient()
    monkeypatch.setattr(client, "_get_jacs_id", lambda: "agent/with/slash")
    await client.mark_read("https://hai.ai", "message/with/slash")

    assert fake_http.last_post_url == (
        "https://hai.ai/api/agents/agent%2Fwith%2Fslash/email/messages/message%2Fwith%2Fslash/read"
    )


@pytest.mark.asyncio
async def test_async_fetch_remote_key_escapes_jacs_id_and_version_path_segments(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

    client = AsyncHaiClient()
    await client.fetch_remote_key("https://hai.ai", "remote/agent", "2026/01")

    assert fake_http.last_get_url == "https://hai.ai/jacs/v1/agents/remote%2Fagent/keys/2026%2F01"


@pytest.mark.asyncio
async def test_async_get_verification_escapes_agent_id_path_segment(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

    client = AsyncHaiClient()
    await client.get_verification("https://hai.ai", "agent/with/slash")

    assert fake_http.last_get_url == "https://hai.ai/api/v1/agents/agent%2Fwith%2Fslash/verification"


@pytest.mark.asyncio
async def test_async_verify_agent_document_uses_public_endpoint(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_http = _FakeAsyncHTTP()

    async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
        return fake_http

    monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

    client = AsyncHaiClient()
    await client.verify_agent_document("https://hai.ai", {"jacsId": "agent-1"}, domain="example.com")

    assert fake_http.last_post_url == "https://hai.ai/api/v1/agents/verify"
