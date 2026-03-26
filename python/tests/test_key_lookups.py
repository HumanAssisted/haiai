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


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_hash
# ---------------------------------------------------------------------------


class TestFetchKeyByHash:
    def test_calls_ffi_and_parses_response(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_hash"] = _KEY_RESPONSE

        result = client.fetch_key_by_hash("https://hai.ai", "sha256:abcdef1234567890")

        assert result.jacs_id == "agent-abc"
        assert result.algorithm == "Ed25519"
        assert result.public_key_hash == "sha256:abcdef1234567890"
        assert result.dns_verified is True
        # Verify FFI was called with the hash
        assert mock_ffi.calls[0][0] == "fetch_key_by_hash"
        assert mock_ffi.calls[0][1][0] == "sha256:abcdef1234567890"

    def test_raises_on_404(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_not_found(hash_val: str) -> Any:
            raise HaiApiError("No key found for hash", status_code=404)

        mock_ffi.responses["fetch_key_by_hash"] = raise_not_found

        with pytest.raises(HaiApiError, match="No key found for hash"):
            client.fetch_key_by_hash("https://hai.ai", "sha256:nonexistent")

    def test_escapes_path_traversal_in_hash(self) -> None:
        """Hash with path traversal chars should be passed to FFI as-is (FFI handles escaping)."""
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_hash"] = _KEY_RESPONSE

        client.fetch_key_by_hash("https://hai.ai", "sha256:../../etc/passwd")

        # FFI receives the raw hash value -- escaping is handled by binding-core
        assert mock_ffi.calls[0][1][0] == "sha256:../../etc/passwd"


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_email
# ---------------------------------------------------------------------------


class TestFetchKeyByEmail:
    def test_calls_ffi_and_parses_response(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_email"] = _KEY_RESPONSE

        result = client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert result.jacs_id == "agent-abc"
        assert result.version == "v1"
        assert mock_ffi.calls[0][0] == "fetch_key_by_email"
        assert mock_ffi.calls[0][1][0] == "alice@hai.ai"

    def test_raises_on_404(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_not_found(email: str) -> Any:
            raise HaiApiError("No key found for email", status_code=404)

        mock_ffi.responses["fetch_key_by_email"] = raise_not_found

        with pytest.raises(HaiApiError, match="No key found for email"):
            client.fetch_key_by_email("https://hai.ai", "nobody@hai.ai")


# ---------------------------------------------------------------------------
# Sync: fetch_key_by_domain
# ---------------------------------------------------------------------------


class TestFetchKeyByDomain:
    def test_calls_ffi_and_parses_response(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_domain"] = _KEY_RESPONSE

        result = client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert result.jacs_id == "agent-abc"
        assert result.dns_verified is True
        assert mock_ffi.calls[0][0] == "fetch_key_by_domain"
        assert mock_ffi.calls[0][1][0] == "example.com"

    def test_raises_on_404(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_not_found(domain: str) -> Any:
            raise HaiApiError("No verified agent for domain", status_code=404)

        mock_ffi.responses["fetch_key_by_domain"] = raise_not_found

        with pytest.raises(HaiApiError, match="No verified agent for domain"):
            client.fetch_key_by_domain("https://hai.ai", "nonexistent.test")


# ---------------------------------------------------------------------------
# Sync: fetch_all_keys
# ---------------------------------------------------------------------------


class TestFetchAllKeys:
    def test_calls_ffi_and_returns_dict(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_all_keys"] = _KEY_HISTORY_RESPONSE

        result = client.fetch_all_keys("https://hai.ai", "agent-abc")

        assert result["jacs_id"] == "agent-abc"
        assert result["total"] == 2
        assert len(result["keys"]) == 2
        assert result["keys"][0]["version"] == "v1"
        assert result["keys"][1]["version"] == "v0"
        assert mock_ffi.calls[0][0] == "fetch_all_keys"
        assert mock_ffi.calls[0][1][0] == "agent-abc"

    def test_passes_jacs_id_with_slashes_to_ffi(self) -> None:
        """FFI receives the raw jacs_id -- binding-core handles escaping."""
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_all_keys"] = _KEY_HISTORY_RESPONSE

        client.fetch_all_keys("https://hai.ai", "agent/with/slashes")

        assert mock_ffi.calls[0][1][0] == "agent/with/slashes"

    def test_raises_on_404(self) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_not_found(jacs_id: str) -> Any:
            raise HaiApiError("Agent not found", status_code=404)

        mock_ffi.responses["fetch_all_keys"] = raise_not_found

        with pytest.raises(HaiApiError, match="Agent not found"):
            client.fetch_all_keys("https://hai.ai", "missing-agent")


# ---------------------------------------------------------------------------
# Async: fetch_key_by_hash
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByHash:
    @pytest.mark.asyncio
    async def test_calls_ffi_and_parses_response(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_hash"] = _KEY_RESPONSE

        result = await client.fetch_key_by_hash("https://hai.ai", "sha256:abcdef1234567890")

        assert result.jacs_id == "agent-abc"
        assert result.algorithm == "Ed25519"
        assert mock_ffi.calls[0][0] == "fetch_key_by_hash"

    @pytest.mark.asyncio
    async def test_raises_on_404(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()

        def raise_not_found(*args: Any) -> Any:
            raise HaiApiError("No key found for hash", status_code=404)

        mock_ffi.responses["fetch_key_by_hash"] = raise_not_found

        with pytest.raises(HaiApiError, match="No key found for hash"):
            await client.fetch_key_by_hash("https://hai.ai", "sha256:nonexistent")


# ---------------------------------------------------------------------------
# Async: fetch_key_by_email
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByEmail:
    @pytest.mark.asyncio
    async def test_calls_ffi(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_email"] = _KEY_RESPONSE

        result = await client.fetch_key_by_email("https://hai.ai", "alice@hai.ai")

        assert result.jacs_id == "agent-abc"
        assert mock_ffi.calls[0][0] == "fetch_key_by_email"
        assert mock_ffi.calls[0][1][0] == "alice@hai.ai"


# ---------------------------------------------------------------------------
# Async: fetch_key_by_domain
# ---------------------------------------------------------------------------


class TestAsyncFetchKeyByDomain:
    @pytest.mark.asyncio
    async def test_calls_ffi(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_key_by_domain"] = _KEY_RESPONSE

        result = await client.fetch_key_by_domain("https://hai.ai", "example.com")

        assert result.dns_verified is True
        assert mock_ffi.calls[0][0] == "fetch_key_by_domain"
        assert mock_ffi.calls[0][1][0] == "example.com"


# ---------------------------------------------------------------------------
# Async: fetch_all_keys
# ---------------------------------------------------------------------------


class TestAsyncFetchAllKeys:
    @pytest.mark.asyncio
    async def test_calls_ffi_and_returns_dict(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_all_keys"] = _KEY_HISTORY_RESPONSE

        result = await client.fetch_all_keys("https://hai.ai", "agent-abc")

        assert result["jacs_id"] == "agent-abc"
        assert result["total"] == 2
        assert len(result["keys"]) == 2
        assert mock_ffi.calls[0][0] == "fetch_all_keys"

    @pytest.mark.asyncio
    async def test_passes_jacs_id_with_slashes_to_ffi(self) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_all_keys"] = _KEY_HISTORY_RESPONSE

        await client.fetch_all_keys("https://hai.ai", "agent/with/slashes")

        assert mock_ffi.calls[0][1][0] == "agent/with/slashes"
