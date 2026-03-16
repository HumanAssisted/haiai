"""Tests for key lookup methods: fetch_key_by_hash, fetch_key_by_email,
fetch_key_by_domain, and fetch_all_keys (sync + async)."""

from __future__ import annotations

from typing import Any

import pytest

from haiai.async_client import AsyncHaiClient
from haiai.client import HaiClient
from haiai.errors import HaiApiError


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------

_KEY_RESPONSE: dict[str, Any] = {
    "jacs_id": "agent-abc",
    "version": "v1",
    "public_key": "-----BEGIN PUBLIC KEY-----\nZm9v\n-----END PUBLIC KEY-----\n",
    "public_key_raw_b64": "Zm9v",
    "algorithm": "Ed25519",
    "public_key_hash": "sha256:abcdef1234567890",
    "status": "active",
    "dns_verified": True,
    "created_at": "2026-01-15T10:30:00Z",
}

_KEY_HISTORY_RESPONSE: dict[str, Any] = {
    "jacs_id": "agent-abc",
    "keys": [
        _KEY_RESPONSE,
        {**_KEY_RESPONSE, "version": "v0", "status": "rotated"},
    ],
    "total": 2,
}


class _FakeResponse:
    def __init__(self, status_code: int, payload: dict[str, Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""
        self.headers: dict[str, str] = {}

    def json(self) -> dict[str, Any]:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise RuntimeError(f"http error: {self.status_code}")


class _FakeAsyncHTTP:
    """Captures GET calls for async client tests."""

    def __init__(self, status_code: int = 200, payload: dict[str, Any] | None = None) -> None:
        self.last_get_url: str | None = None
        self._status = status_code
        self._payload = payload or _KEY_RESPONSE

    async def get(self, url: str, **_kwargs: Any) -> _FakeResponse:
        self.last_get_url = url
        return _FakeResponse(self._status, self._payload)

    async def post(self, url: str, **_kwargs: Any) -> _FakeResponse:
        return _FakeResponse(200, {})


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_hash
# ---------------------------------------------------------------------------


class TestFetchKeyByHash:
    def test_calls_correct_url_and_parses_response(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        result = client.fetch_key_by_hash("https://hai.ai", "sha256:abcdef1234567890")

        assert captured["url"] == "https://hai.ai/jacs/v1/keys/by-hash/sha256%3Aabcdef1234567890"
        assert result.jacs_id == "agent-abc"
        assert result.algorithm == "Ed25519"
        assert result.public_key_hash == "sha256:abcdef1234567890"
        assert result.dns_verified is True

    def test_raises_on_404(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, {})

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        with pytest.raises(HaiApiError, match="No key found for hash"):
            client.fetch_key_by_hash("https://hai.ai", "sha256:nonexistent")

    def test_escapes_path_traversal_in_hash(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_key_by_hash("https://hai.ai", "sha256:../../etc/passwd")

        assert "..%2F" in captured["url"]
        assert "/../" not in captured["url"]


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_email
# ---------------------------------------------------------------------------


class TestFetchKeyByEmail:
    def test_calls_correct_url_and_parses_response(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        result = client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert captured["url"] == "https://hai.ai/api/agents/keys/alice%40hai.ai"
        assert result.jacs_id == "agent-abc"
        assert result.version == "v1"

    def test_raises_on_404(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, {})

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        with pytest.raises(HaiApiError, match="No key found for email"):
            client.fetch_key_by_email("https://hai.ai", "nobody@hai.ai")


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_domain
# ---------------------------------------------------------------------------


class TestFetchKeyByDomain:
    def test_calls_correct_url_and_parses_response(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        result = client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert captured["url"] == "https://hai.ai/jacs/v1/agents/by-domain/example.com"
        assert result.jacs_id == "agent-abc"
        assert result.dns_verified is True

    def test_raises_on_404(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, {})

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        with pytest.raises(HaiApiError, match="No verified agent for domain"):
            client.fetch_key_by_domain("https://hai.ai", "nonexistent.test")


# ---------------------------------------------------------------------------
# Sync: fetch_all_keys
# ---------------------------------------------------------------------------


class TestFetchAllKeys:
    def test_calls_correct_url_and_returns_dict(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_HISTORY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        result = client.fetch_all_keys("https://hai.ai", "agent-abc")

        assert captured["url"] == "https://hai.ai/jacs/v1/agents/agent-abc/keys"
        assert result["jacs_id"] == "agent-abc"
        assert result["total"] == 2
        assert len(result["keys"]) == 2
        assert result["keys"][0]["version"] == "v1"
        assert result["keys"][1]["version"] == "v0"

    def test_escapes_jacs_id_with_slashes(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, str] = {}

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, _KEY_HISTORY_RESPONSE)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_all_keys("https://hai.ai", "agent/with/slashes")

        assert captured["url"] == "https://hai.ai/jacs/v1/agents/agent%2Fwith%2Fslashes/keys"

    def test_raises_on_404(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, {})

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        with pytest.raises(HaiApiError, match="Agent not found"):
            client.fetch_all_keys("https://hai.ai", "missing-agent")


# ---------------------------------------------------------------------------
# Async: fetch_key_by_hash
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByHash:
    @pytest.mark.asyncio
    async def test_calls_correct_url_and_parses_response(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP()

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        result = await client.fetch_key_by_hash("https://hai.ai", "sha256:abcdef1234567890")

        assert fake_http.last_get_url == "https://hai.ai/jacs/v1/keys/by-hash/sha256%3Aabcdef1234567890"
        assert result.jacs_id == "agent-abc"
        assert result.algorithm == "Ed25519"

    @pytest.mark.asyncio
    async def test_raises_on_404(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP(status_code=404)

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        with pytest.raises(HaiApiError, match="No key found for hash"):
            await client.fetch_key_by_hash("https://hai.ai", "sha256:nonexistent")


# ---------------------------------------------------------------------------
# Async: fetch_key_by_email
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByEmail:
    @pytest.mark.asyncio
    async def test_calls_correct_url(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP()

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        result = await client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert fake_http.last_get_url == "https://hai.ai/api/agents/keys/alice%40hai.ai"
        assert result.jacs_id == "agent-abc"


# ---------------------------------------------------------------------------
# Async: fetch_key_by_domain
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByDomain:
    @pytest.mark.asyncio
    async def test_calls_correct_url(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP()

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        result = await client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert fake_http.last_get_url == "https://hai.ai/jacs/v1/agents/by-domain/example.com"
        assert result.dns_verified is True


# ---------------------------------------------------------------------------
# Async: fetch_all_keys
# ---------------------------------------------------------------------------


class TestAsyncFetchAllKeys:
    @pytest.mark.asyncio
    async def test_calls_correct_url_and_returns_dict(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP(payload=_KEY_HISTORY_RESPONSE)

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        result = await client.fetch_all_keys("https://hai.ai", "agent-abc")

        assert fake_http.last_get_url == "https://hai.ai/jacs/v1/agents/agent-abc/keys"
        assert result["jacs_id"] == "agent-abc"
        assert result["total"] == 2
        assert len(result["keys"]) == 2

    @pytest.mark.asyncio
    async def test_escapes_jacs_id_with_slashes(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        fake_http = _FakeAsyncHTTP(payload=_KEY_HISTORY_RESPONSE)

        async def fake_get_http(_self: AsyncHaiClient) -> _FakeAsyncHTTP:
            return fake_http

        monkeypatch.setattr(AsyncHaiClient, "_get_http", fake_get_http)

        client = AsyncHaiClient()
        await client.fetch_all_keys("https://hai.ai", "agent/with/slashes")

        assert fake_http.last_get_url == "https://hai.ai/jacs/v1/agents/agent%2Fwith%2Fslashes/keys"
