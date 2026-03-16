"""Tests for haiai.signing module.

All crypto operations delegate to JACS binding-core.
"""

from __future__ import annotations

import json
from typing import Any

from haiai.signing import (
    canonicalize_json,
    create_agent_document,
    invalidate_key_cache,
    is_signed_event,
    sign_response,
    unwrap_signed_event,
)


class TestSignString:
    def test_round_trip(self, jacs_agent: Any) -> None:
        """agent.sign_string returns a non-empty base64 signature."""
        sig = jacs_agent.sign_string("hello world")
        assert sig
        assert isinstance(sig, str)

    def test_different_messages_different_sigs(self, jacs_agent: Any) -> None:
        sig1 = jacs_agent.sign_string("hello")
        sig2 = jacs_agent.sign_string("world")
        assert sig1 != sig2


class TestCanonicalizeJson:
    def test_sorted_keys(self) -> None:
        result = canonicalize_json({"b": 2, "a": 1})
        assert result == '{"a":1,"b":2}'

    def test_no_spaces(self) -> None:
        result = canonicalize_json({"key": "value"})
        assert " " not in result

    def test_nested(self) -> None:
        result = canonicalize_json({"z": {"b": 1, "a": 2}})
        parsed = json.loads(result)
        assert list(parsed.keys()) == ["z"]


class TestCreateAgentDocument:
    def test_creates_valid_doc(self, jacs_agent: Any) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="TestBot",
            version="1.0",
        )
        assert doc["jacsAgentName"] == "TestBot"
        assert "jacsSignature" in doc
        assert "jacsId" in doc

    def test_signature_is_present(self, jacs_agent: Any) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="Bot",
            version="1.0",
        )
        jacs_sig = doc["jacsSignature"]
        assert isinstance(jacs_sig, dict)
        assert "signature" in jacs_sig
        assert jacs_sig["signature"]  # non-empty

    def test_custom_jacs_id(self, jacs_agent: Any) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="B",
            version="1",
            jacs_id="custom-id",
        )
        assert doc["jacsId"] == "custom-id"


class TestIsSignedEvent:
    def test_jacs_document_format(self) -> None:
        data = {"payload": {}, "signature": {}, "metadata": {}}
        assert is_signed_event(data)

    def test_jacs_envelope_format(self) -> None:
        data = {"jacs_envelope": True, "payload": {}}
        assert is_signed_event(data)

    def test_plain_event(self) -> None:
        data = {"type": "heartbeat"}
        assert not is_signed_event(data)

    def test_partial_match_not_signed(self) -> None:
        data = {"payload": {}, "metadata": {}}
        assert not is_signed_event(data)


class TestUnwrapSignedEvent:
    def test_unwrap_jacs_document(self) -> None:
        inner = {"type": "benchmark_job", "job_id": "j1"}
        data = {"payload": inner, "signature": {"signature": "abc"}, "metadata": {}}
        payload, verified = unwrap_signed_event(data, verify=False)
        assert payload == inner
        assert not verified

    def test_unwrap_jacs_envelope(self) -> None:
        inner = {"type": "connected"}
        data = {"jacs_envelope": True, "payload": inner}
        payload, verified = unwrap_signed_event(data, verify=False)
        assert payload == inner
        assert not verified

    def test_passthrough_plain(self) -> None:
        data = {"type": "heartbeat"}
        payload, verified = unwrap_signed_event(data, verify=False)
        assert payload == data
        assert not verified

    def test_non_dict_payload(self) -> None:
        data = {"payload": "just a string", "signature": {}, "metadata": {}}
        payload, verified = unwrap_signed_event(data, verify=False)
        assert payload == data
        assert not verified


class TestSignResponse:
    def test_produces_signed_document(self, jacs_agent: Any) -> None:
        job_payload = {"response": {"message": "hello", "processing_time_ms": 100}}
        result = sign_response(job_payload, jacs_agent, "agent-jacs-1")
        assert "signed_document" in result
        assert result["agent_jacs_id"] == "agent-jacs-1"

    def test_signed_document_is_json(self, jacs_agent: Any) -> None:
        result = sign_response({"response": {"message": "x"}}, jacs_agent, "a1")
        doc = json.loads(result["signed_document"])
        assert doc["version"] == "1.0.0"
        assert doc["document_type"] == "job_response"
        assert "jacsSignature" in doc
        assert doc["jacsSignature"]["agentID"] == "a1"

    def test_signature_is_present(self, jacs_agent: Any) -> None:
        payload = {"response": {"message": "test"}}
        result = sign_response(payload, jacs_agent, "id-1")
        doc = json.loads(result["signed_document"])
        sig = doc["jacsSignature"]["signature"]
        assert sig  # non-empty


class TestInvalidateKeyCache:
    def test_invalidate_does_not_raise(self) -> None:
        invalidate_key_cache()
