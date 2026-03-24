"""Shared mock API contract tests for method/path/auth consistency.

Since HTTP calls now delegate to the FFI adapter, these tests verify
that the correct FFI methods are called with the right arguments.
The URL path construction and auth are handled by binding-core.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

from haiai.client import HaiClient


def _load_contract() -> dict[str, Any]:
    fixture_path = Path(__file__).resolve().parents[2] / "fixtures" / "contract_endpoints.json"
    return json.loads(fixture_path.read_text())


def test_hello_contract_calls_ffi(
    loaded_config: None,
) -> None:
    contract = _load_contract()
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["hello"] = {
        "timestamp": "2026-01-01T00:00:00Z",
        "client_ip": "127.0.0.1",
        "hai_public_key_fingerprint": "fp",
        "message": "ok",
        "hello_id": "h1",
    }

    client.hello_world(contract["base_url"])

    # Verify the FFI method was called
    assert mock_ffi.calls[0][0] == "hello"


def test_check_username_contract_calls_ffi() -> None:
    contract = _load_contract()
    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["check_username"] = {"available": True, "username": "alice"}

    client.check_username(contract["base_url"], "alice")

    assert mock_ffi.calls[0][0] == "check_username"
    assert mock_ffi.calls[0][1][0] == "alice"


def test_submit_response_contract_calls_ffi(
    loaded_config: None,
) -> None:
    contract = _load_contract()
    job_id = "job-123"

    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["submit_response"] = {
        "success": True,
        "job_id": job_id,
        "message": "ok",
    }

    client.submit_benchmark_response(contract["base_url"], job_id=job_id, message="ok")

    assert mock_ffi.calls[0][0] == "submit_response"
    params = mock_ffi.calls[0][1][0]
    assert params["job_id"] == job_id


def test_update_labels_contract_calls_ffi(
    loaded_config: None,
) -> None:
    contract = _load_contract()
    message_id = "msg-456"

    client = HaiClient()
    mock_ffi = client._get_ffi()
    mock_ffi.responses["update_labels"] = {"labels": ["urgent", "important"]}

    result = client.update_labels(
        contract["base_url"],
        message_id,
        add=["urgent"],
        remove=["spam"],
    )

    assert mock_ffi.calls[0][0] == "update_labels"
    params = mock_ffi.calls[0][1][0]
    assert params["message_id"] == message_id
    assert params["add"] == ["urgent"]
    assert params["remove"] == ["spam"]

    # Verify return type is a list of strings
    assert isinstance(result, list)
    assert result == ["urgent", "important"]
