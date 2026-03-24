"""Path-escaping regression tests for URL path segments.

Since all HTTP calls now delegate to the FFI adapter (binding-core),
URL construction and escaping is handled by the Rust layer.
These tests verify that the Python client passes the correct raw
arguments to the FFI methods, and that the Python-side
urllib.parse.quote utility still works for any remaining Python-side
URL construction.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from urllib.parse import quote

import pytest

from haiai.async_client import AsyncHaiClient
from haiai.client import HaiClient


def test_claim_username_passes_raw_agent_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["claim_username"] = {"username": "alice", "email": "alice@hai.ai", "agent_id": "agent/../with/slash"}

    client.claim_username("https://hai.ai", "agent/../with/slash", "alice")

    assert mock_ffi.calls[0][0] == "claim_username"
    assert mock_ffi.calls[0][1][0] == "agent/../with/slash"
    assert mock_ffi.calls[0][1][1] == "alice"


def test_update_username_passes_raw_agent_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["update_username"] = {"username": "new-name", "email": "new-name@hai.ai", "previous_username": "old-name"}

    client.update_username("https://hai.ai", "agent/../with/slash", "new-name")

    assert mock_ffi.calls[0][0] == "update_username"
    assert mock_ffi.calls[0][1][0] == "agent/../with/slash"


def test_delete_username_passes_raw_agent_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["delete_username"] = {
        "released_username": "old-name",
        "cooldown_until": "2026-03-01T00:00:00Z",
        "message": "released",
    }

    client.delete_username("https://hai.ai", "agent/../with/slash")

    assert mock_ffi.calls[0][0] == "delete_username"
    assert mock_ffi.calls[0][1][0] == "agent/../with/slash"


def test_verify_document_calls_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["verify_document"] = {
        "valid": True,
        "verified_at": "2026-01-01T00:00:00Z",
        "document_type": "JacsDocument",
        "issuer_verified": True,
        "signature_verified": True,
        "signer_id": "agent-1",
        "signed_at": "2026-01-01T00:00:00Z",
    }

    result = client.verify_document("https://hai.ai", {"jacsId": "agent-1"})

    assert mock_ffi.calls[0][0] == "verify_document"
    assert result["valid"] is True


def test_get_verification_passes_agent_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["get_verification"] = {
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
    }

    result = client.get_verification("https://hai.ai", "agent/with/slash")

    assert mock_ffi.calls[0][0] == "get_verification"
    assert mock_ffi.calls[0][1][0] == "agent/with/slash"
    assert result["verification"]["badge"] == "domain"


def test_verify_agent_document_calls_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["verify_agent_document"] = {
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
    }

    result = client.verify_agent_document(
        "https://hai.ai",
        {"jacsId": "agent-1"},
        domain="example.com",
    )

    assert mock_ffi.calls[0][0] == "verify_agent_document"
    assert result["verification"]["badge"] == "attested"


def test_submit_benchmark_response_passes_job_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["submit_response"] = {"success": True, "job_id": "job/with/slash"}

    client.submit_benchmark_response(
        "https://hai.ai",
        job_id="job/with/slash",
        message="ok",
    )

    assert mock_ffi.calls[0][0] == "submit_response"
    params = mock_ffi.calls[0][1][0]
    assert params["job_id"] == "job/with/slash"


def test_mark_read_passes_message_id_to_ffi(
    loaded_config: None,
) -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()

    client.mark_read("https://hai.ai", "message/with/slash")

    assert mock_ffi.calls[0][0] == "mark_read"
    assert mock_ffi.calls[0][1][0] == "message/with/slash"


def test_fetch_remote_key_passes_jacs_id_and_version_to_ffi() -> None:
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["fetch_remote_key"] = {
        "jacs_id": "remote/agent",
        "version": "2026/01",
        "public_key": "pem",
    }

    client.fetch_remote_key("https://hai.ai", "remote/agent", "2026/01")

    assert mock_ffi.calls[0][0] == "fetch_remote_key"
    assert mock_ffi.calls[0][1][0] == "remote/agent"
    assert mock_ffi.calls[0][1][1] == "2026/01"


@pytest.mark.asyncio
async def test_async_mark_read_passes_message_id_to_ffi(
    loaded_config: None,
) -> None:
    client = AsyncHaiClient()
    mock_ffi = client._get_ffi()

    await client.mark_read("https://hai.ai", "message/with/slash")

    assert mock_ffi.calls[0][0] == "mark_read"
    assert mock_ffi.calls[0][1][0] == "message/with/slash"


@pytest.mark.asyncio
async def test_async_fetch_remote_key_passes_args_to_ffi() -> None:
    client = AsyncHaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["fetch_remote_key"] = {
        "jacs_id": "remote/agent",
        "version": "2026/01",
        "public_key": "pem",
    }

    await client.fetch_remote_key("https://hai.ai", "remote/agent", "2026/01")

    assert mock_ffi.calls[0][0] == "fetch_remote_key"
    assert mock_ffi.calls[0][1][0] == "remote/agent"
    assert mock_ffi.calls[0][1][1] == "2026/01"


@pytest.mark.asyncio
async def test_async_get_verification_passes_agent_id_to_ffi() -> None:
    client = AsyncHaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["get_verification"] = {
        "agent_id": "agent/with/slash",
        "verification": {},
    }

    await client.get_verification("https://hai.ai", "agent/with/slash")

    assert mock_ffi.calls[0][0] == "get_verification"
    assert mock_ffi.calls[0][1][0] == "agent/with/slash"


@pytest.mark.asyncio
async def test_async_verify_agent_document_calls_ffi() -> None:
    client = AsyncHaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["verify_agent_document"] = {
        "agent_id": "agent-1",
        "verification": {},
    }

    await client.verify_agent_document("https://hai.ai", {"jacsId": "agent-1"}, domain="example.com")

    assert mock_ffi.calls[0][0] == "verify_agent_document"


# ---------------------------------------------------------------------------
# Fixture-driven path escaping tests (T09)
# ---------------------------------------------------------------------------


def _load_path_escaping_fixture() -> dict:
    path = Path(__file__).resolve().parent.parent.parent / "fixtures" / "path_escaping_contract.json"
    return json.loads(path.read_text())


class TestPathEscapingContract:
    """Tests driven by fixtures/path_escaping_contract.json."""

    def test_all_vectors(self) -> None:
        fixture = _load_path_escaping_fixture()
        for vec in fixture["test_vectors"]:
            result = quote(vec["raw"], safe="")
            assert result == vec["escaped"], (
                f"Escaping {vec['raw']!r}: expected {vec['escaped']!r}, got {result!r}"
            )

    def test_path_traversal_escaped(self) -> None:
        malicious = "../../../etc/passwd"
        escaped = quote(malicious, safe="")
        # Slashes must be encoded to prevent path traversal
        assert "/" not in escaped
