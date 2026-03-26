"""Tests for the client-side agent key cache (5-minute TTL)."""

from __future__ import annotations

import time
from typing import Any
from unittest.mock import patch

import pytest

from haiai.client import HaiClient
from haiai.models import PublicKeyInfo


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


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
    def test_second_call_returns_cached_value(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(*args: Any, **kwargs: Any) -> dict:
            nonlocal call_count
            call_count += 1
            return _make_key_response("v1")

        mock_ffi.responses["fetch_remote_key"] = counting_fetch

        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")

        assert call_count == 1, "second call should use cache"
        assert r1.version == r2.version == "v1"


class TestFetchRemoteKeyCacheExpires:
    def test_expired_entry_causes_refetch(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        versions = iter(["v1", "v2"])
        def version_fetch(*args: Any, **kwargs: Any) -> dict:
            return _make_key_response(next(versions))

        mock_ffi.responses["fetch_remote_key"] = version_fetch

        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r1.version == "v1"

        # Simulate TTL expiry by backdating the cache entry
        for key in list(client._key_cache):
            value, _ts = client._key_cache[key]
            client._key_cache[key] = (value, time.monotonic() - 400)

        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r2.version == "v2"


class TestInvalidateAgentKeyCacheForcesRefetch:
    def test_invalidate_then_fetch_calls_api(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        versions = iter(["v1", "v2"])
        def version_fetch(*args: Any, **kwargs: Any) -> dict:
            return _make_key_response(next(versions))

        mock_ffi.responses["fetch_remote_key"] = version_fetch

        r1 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r1.version == "v1"

        client.invalidate_key_cache()

        r2 = client.fetch_remote_key("https://hai.ai", "agent-abc", "latest")
        assert r2.version == "v2"


class TestCacheKeyedByIdAndVersion:
    def test_different_keys_are_cached_separately(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(jacs_id: str, version: str = "latest") -> dict:
            nonlocal call_count
            call_count += 1
            if jacs_id == "agent-1" or (isinstance(jacs_id, str) and "agent-1" in str(jacs_id)):
                return _make_key_response("v1")
            return _make_key_response("v2")

        mock_ffi.responses["fetch_remote_key"] = counting_fetch

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
    def test_second_call_uses_cache(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(*args: Any, **kwargs: Any) -> dict:
            nonlocal call_count
            call_count += 1
            return _make_key_response()

        mock_ffi.responses["fetch_key_by_hash"] = counting_fetch

        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")

        assert call_count == 1

    def test_invalidate_clears_hash_cache(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(*args: Any, **kwargs: Any) -> dict:
            nonlocal call_count
            call_count += 1
            return _make_key_response()

        mock_ffi.responses["fetch_key_by_hash"] = counting_fetch

        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")
        client.invalidate_key_cache()
        client.fetch_key_by_hash("https://hai.ai", "sha256:abc123")

        assert call_count == 2


# ---------------------------------------------------------------------------
# fetch_key_by_email caching
# ---------------------------------------------------------------------------


class TestFetchKeyByEmailCache:
    def test_second_call_uses_cache(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(*args: Any, **kwargs: Any) -> dict:
            nonlocal call_count
            call_count += 1
            return _make_key_response()

        mock_ffi.responses["fetch_key_by_email"] = counting_fetch

        client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")
        client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert call_count == 1


# ---------------------------------------------------------------------------
# fetch_key_by_domain caching
# ---------------------------------------------------------------------------


class TestFetchKeyByDomainCache:
    def test_second_call_uses_cache(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        call_count = 0
        def counting_fetch(*args: Any, **kwargs: Any) -> dict:
            nonlocal call_count
            call_count += 1
            return _make_key_response()

        mock_ffi.responses["fetch_key_by_domain"] = counting_fetch

        client.fetch_key_by_domain("https://hai.ai", "example.com")
        client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert call_count == 1
