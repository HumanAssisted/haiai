"""Tests for the client-side agent key cache (5-minute TTL)."""

from __future__ import annotations

import time
from typing import Any
from unittest.mock import patch

import pytest

from jacs.hai.client import HaiClient
from jacs.hai.models import PublicKeyInfo


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


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


def _make_key_response(version: str = "v1") -> dict[str, Any]:
    return {
        "jacs_id": "agent-abc",
        "version": version,
        "public_key": "pem",
        "public_key_raw_b64": "",
        "algorithm": "Ed25519",
        "public_key_hash": "sha256:abc",
        "status": "active",
        "dns_verified": True,
        "created_at": "2026-01-15T10:30:00Z",
    }


# ---------------------------------------------------------------------------
# fetch_remote_key caching
# ---------------------------------------------------------------------------


class TestFetchRemoteKeyCachesResult:
    def test_second_call_returns_cached_value(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            return _FakeResponse(200, _make_key_response("v1"))

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")

        assert call_count == 1, "second call should use cache"
        assert r1.version == r2.version == "v1"


class TestFetchRemoteKeyCacheExpires:
    def test_expired_entry_causes_refetch(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        responses = iter([
            _FakeResponse(200, _make_key_response("v1")),
            _FakeResponse(200, _make_key_response("v2")),
        ])

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return next(responses)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r1.version == "v1"

        # Simulate TTL expiry by backdating the cache entry
        for key in list(client._key_cache):
            value, _ts = client._key_cache[key]
            client._key_cache[key] = (value, time.monotonic() - 400)

        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r2.version == "v2"


class TestInvalidateAgentKeyCacheForcesRefetch:
    def test_invalidate_then_fetch_calls_api(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        responses = iter([
            _FakeResponse(200, _make_key_response("v1")),
            _FakeResponse(200, _make_key_response("v2")),
        ])

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            return next(responses)

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r1.version == "v1"

        client.invalidate_key_cache()

        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r2.version == "v2"


class TestCacheKeyedByIdAndVersion:
    def test_different_keys_are_cached_separately(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            # Return different versions based on which URL was called
            if "agent-1" in url:
                return _FakeResponse(200, _make_key_response("v1"))
            else:
                return _FakeResponse(200, _make_key_response("v2"))

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        r1 = client.fetch_remote_key("https://hai.ai", "agent-1", "latest")
        r2 = client.fetch_remote_key("https://hai.ai", "agent-2", "latest")

        assert call_count == 2, "different ids should not share cache"
        assert r1.version == "v1"
        assert r2.version == "v2"

        # Calling again with same ids should use cache
        client.fetch_remote_key("https://hai.ai", "agent-1", "latest")
        client.fetch_remote_key("https://hai.ai", "agent-2", "latest")
        assert call_count == 2, "repeated calls should use cache"


# ---------------------------------------------------------------------------
# fetch_key_by_hash caching
# ---------------------------------------------------------------------------


class TestFetchKeyByHashCache:
    def test_second_call_uses_cache(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            return _FakeResponse(200, _make_key_response())

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")

        assert call_count == 1

    def test_invalidate_clears_hash_cache(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            return _FakeResponse(200, _make_key_response())

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")
        client.invalidate_key_cache()
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")

        assert call_count == 2


# ---------------------------------------------------------------------------
# fetch_key_by_email caching
# ---------------------------------------------------------------------------


class TestFetchKeyByEmailCache:
    def test_second_call_uses_cache(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            return _FakeResponse(200, _make_key_response())

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")
        client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert call_count == 1


# ---------------------------------------------------------------------------
# fetch_key_by_domain caching
# ---------------------------------------------------------------------------


class TestFetchKeyByDomainCache:
    def test_second_call_uses_cache(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        call_count = 0

        def fake_get(url: str, **_kwargs: Any) -> _FakeResponse:
            nonlocal call_count
            call_count += 1
            return _FakeResponse(200, _make_key_response())

        import httpx

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.fetch_key_by_domain("https://hai.ai", "example.com")
        client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert call_count == 1
