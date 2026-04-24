"""Tests for `get_raw_email` sync + async on the Python SDK.

Asserts the load-bearing R2 byte-fidelity contract: bytes passing through the
FFI boundary are byte-identical to what JACS signed. No trimming, no
line-ending normalization, no UTF-8 lossy.
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import json
from pathlib import Path

import pytest

from haiai import RawEmailResult
from haiai.async_client import AsyncHaiClient
from haiai.client import HaiClient

BASE_URL = "https://test.hai.ai"


# --------------------------------------------------------------------------
# RawEmailResult.from_dict — pure unit tests
# --------------------------------------------------------------------------


class TestRawEmailResultFromDict:
    def test_available_true_decodes_bytes(self) -> None:
        raw = b"From: a\r\nTo: b\r\n\r\nbody with \x00 NUL and \xc3\xa9\r\n"
        b64 = base64.b64encode(raw).decode()
        data = {
            "message_id": "m.1",
            "rfc_message_id": "<a@b>",
            "available": True,
            "raw_email_b64": b64,
            "size_bytes": len(raw),
            "omitted_reason": None,
        }
        r = RawEmailResult.from_dict(data)
        assert isinstance(r, RawEmailResult)
        assert r.available is True
        assert r.raw_email == raw  # byte-identical
        assert r.size_bytes == len(raw)
        assert r.omitted_reason is None
        assert r.message_id == "m.1"
        assert r.rfc_message_id == "<a@b>"

    def test_available_false_not_stored(self) -> None:
        data = {
            "message_id": "m.2",
            "available": False,
            "raw_email_b64": None,
            "size_bytes": None,
            "omitted_reason": "not_stored",
        }
        r = RawEmailResult.from_dict(data)
        assert r.available is False
        assert r.raw_email is None
        assert r.size_bytes is None
        assert r.omitted_reason == "not_stored"

    def test_available_false_oversize(self) -> None:
        data = {
            "message_id": "m.3",
            "available": False,
            "raw_email_b64": None,
            "omitted_reason": "oversize",
        }
        r = RawEmailResult.from_dict(data)
        assert r.available is False
        assert r.raw_email is None
        assert r.omitted_reason == "oversize"


# --------------------------------------------------------------------------
# HaiClient.get_raw_email — sync via mock FFI
# --------------------------------------------------------------------------


class TestGetRawEmailSync:
    def test_roundtrips_bytes_through_ffi(self, loaded_config: None) -> None:
        raw = b"CRLF\r\nembed\x00NUL\r\nnon-ascii:\xc3\xa9\r\n"
        b64 = base64.b64encode(raw).decode()
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_raw_email"] = {
            "message_id": "m.100",
            "rfc_message_id": "<m.100@hai.ai>",
            "available": True,
            "raw_email_b64": b64,
            "size_bytes": len(raw),
            "omitted_reason": None,
        }

        result = client.get_raw_email(BASE_URL, "m.100")
        assert isinstance(result, RawEmailResult)
        assert result.available is True
        # R2: byte-identity
        assert result.raw_email == raw
        assert result.size_bytes == len(raw)

    def test_missing_message_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.get_raw_email(BASE_URL, "")

    def test_available_false_returns_none_bytes(self, loaded_config: None) -> None:
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_raw_email"] = {
            "message_id": "legacy",
            "available": False,
            "raw_email_b64": None,
            "omitted_reason": "not_stored",
        }
        result = client.get_raw_email(BASE_URL, "legacy")
        assert result.available is False
        assert result.raw_email is None
        assert result.omitted_reason == "not_stored"


# --------------------------------------------------------------------------
# AsyncHaiClient.get_raw_email — same shape, async
# --------------------------------------------------------------------------


class TestGetRawEmailAsync:
    def test_async_roundtrips_bytes(self, loaded_config: None) -> None:
        raw = b"async bytes \r\n\x00\xff end"
        b64 = base64.b64encode(raw).decode()

        client = AsyncHaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_raw_email"] = {
            "message_id": "m.async",
            "available": True,
            "raw_email_b64": b64,
            "size_bytes": len(raw),
            "omitted_reason": None,
        }

        result = asyncio.run(client.get_raw_email(BASE_URL, "m.async"))
        assert isinstance(result, RawEmailResult)
        assert result.available is True
        assert result.raw_email == raw

    def test_async_available_false(self, loaded_config: None) -> None:
        client = AsyncHaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_raw_email"] = {
            "message_id": "m.big",
            "available": False,
            "raw_email_b64": None,
            "omitted_reason": "oversize",
        }
        result = asyncio.run(client.get_raw_email(BASE_URL, "m.big"))
        assert result.available is False
        assert result.raw_email is None
        assert result.omitted_reason == "oversize"


# --------------------------------------------------------------------------
# Fixture-driven conformance (PRD §5.4)
# --------------------------------------------------------------------------


class TestRawEmailConformanceFixture:
    def test_raw_email_roundtrip_scenario_byte_identity(
        self, loaded_config: None,
    ) -> None:
        fixture_path = (
            Path(__file__).parent.parent.parent / "fixtures" / "email_conformance.json"
        )
        assert fixture_path.exists(), fixture_path
        fixture = json.loads(fixture_path.read_text())
        scenario = fixture["raw_email_roundtrip"]

        expected_bytes = base64.b64decode(scenario["input_raw_b64"])
        # Belt-and-braces: hash matches the declared input_sha256
        assert hashlib.sha256(expected_bytes).hexdigest() == scenario["input_sha256"]

        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_raw_email"] = {
            "message_id": "conformance-001",
            "available": scenario["expected_available"],
            "raw_email_b64": scenario["expected_raw_b64"],
            "size_bytes": scenario["expected_size_bytes"],
            "omitted_reason": scenario["expected_omitted_reason"],
        }

        result = client.get_raw_email(BASE_URL, "conformance-001")
        # Assertion 1 (PRD §5.4): fetched_bytes == expected_bytes.
        assert result.raw_email == expected_bytes
        assert result.size_bytes == scenario["expected_size_bytes"]

        # Assertion 2 (PRD §5.4): verify_email(fetched_bytes).valid == true.
        # We cannot run real JACS crypto through a Python FFI mock, so instead we
        # capture the base64 passed into verify_email_raw and assert it decodes
        # byte-identically back to expected_bytes (i.e. no normalization in the
        # Python wrapper's encode step). The real crypto verify runs in the
        # Rust conformance test (tests/email_conformance.rs), which exercises
        # `jacs::email::verify_email_document` against the same fixture bytes.
        captured: dict[str, bytes] = {}

        def verify_mock(raw_b64: str) -> dict:
            captured["bytes"] = base64.b64decode(raw_b64)
            return {
                "valid": scenario["expected_verify_valid"],
                "jacs_id": scenario["verify_registry"]["jacs_id"],
                "algorithm": scenario["verify_registry"]["algorithm"],
                "reputation_tier": scenario["verify_registry"]["reputation_tier"],
                "dns_verified": None,
                "field_results": [],
                "chain": [],
                "error": None,
                "agent_status": scenario["verify_registry"]["agent_status"],
                "benchmarks_completed": [],
            }

        ffi.responses["verify_email_raw"] = verify_mock
        verify_result = client.verify_email(BASE_URL, result.raw_email or b"")
        assert captured["bytes"] == expected_bytes, (
            "Python wrapper must pass bytes byte-identically to verify_email_raw FFI"
        )
        assert verify_result.valid is True
        assert verify_result.jacs_id == scenario["verify_registry"]["jacs_id"]
