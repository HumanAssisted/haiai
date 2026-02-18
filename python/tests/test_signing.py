"""Tests for jacs.hai.signing and jacs.hai.crypt modules."""

from __future__ import annotations

import json

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from jacs.hai.crypt import (
    canonicalize_json,
    create_agent_document,
    sign_string,
    verify_string,
)
from jacs.hai.signing import (
    invalidate_key_cache,
    is_signed_event,
    sign_response,
    unwrap_signed_event,
)


class TestSignString:
    def test_round_trip(self, ed25519_keypair: tuple) -> None:
        private_key, _ = ed25519_keypair
        public_key = private_key.public_key()
        sig = sign_string(private_key, "hello world")
        assert verify_string(public_key, "hello world", sig)

    def test_wrong_message(self, ed25519_keypair: tuple) -> None:
        private_key, _ = ed25519_keypair
        public_key = private_key.public_key()
        sig = sign_string(private_key, "hello")
        assert not verify_string(public_key, "wrong", sig)

    def test_wrong_key(self) -> None:
        key1 = Ed25519PrivateKey.generate()
        key2 = Ed25519PrivateKey.generate()
        sig = sign_string(key1, "msg")
        assert not verify_string(key2.public_key(), "msg", sig)

    def test_invalid_signature(self, ed25519_keypair: tuple) -> None:
        _, _ = ed25519_keypair
        key = Ed25519PrivateKey.generate()
        assert not verify_string(key.public_key(), "msg", "not-valid-base64!!!")


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
    def test_creates_valid_doc(self, ed25519_keypair: tuple) -> None:
        private_key, public_pem = ed25519_keypair
        doc = create_agent_document(
            name="TestBot",
            version="1.0",
            public_key_pem=public_pem,
            private_key=private_key,
        )
        assert doc["jacsAgentName"] == "TestBot"
        assert "jacsSignature" in doc
        assert "jacsId" in doc

    def test_signature_verifies(self, ed25519_keypair: tuple) -> None:
        private_key, public_pem = ed25519_keypair
        doc = create_agent_document(
            name="Bot", version="1.0",
            public_key_pem=public_pem, private_key=private_key,
        )
        # jacsSignature is now a structured object
        jacs_sig = doc["jacsSignature"]
        assert isinstance(jacs_sig, dict)
        sig_b64 = jacs_sig["signature"]
        # Reconstruct canonical form: remove only .signature sub-field
        import copy
        signing_doc = copy.deepcopy(doc)
        del signing_doc["jacsSignature"]["signature"]
        canonical = canonicalize_json(signing_doc)
        assert verify_string(private_key.public_key(), canonical, sig_b64)

    def test_custom_jacs_id(self, ed25519_keypair: tuple) -> None:
        private_key, public_pem = ed25519_keypair
        doc = create_agent_document(
            name="B", version="1", public_key_pem=public_pem,
            private_key=private_key, jacs_id="custom-id",
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
    def test_produces_signed_document(self, ed25519_keypair: tuple) -> None:
        private_key, _ = ed25519_keypair
        job_payload = {"response": {"message": "hello", "processing_time_ms": 100}}
        result = sign_response(job_payload, private_key, "agent-jacs-1")
        assert "signed_document" in result
        assert result["agent_jacs_id"] == "agent-jacs-1"

    def test_signed_document_is_json(self, ed25519_keypair: tuple) -> None:
        private_key, _ = ed25519_keypair
        result = sign_response({"response": {"message": "x"}}, private_key, "a1")
        doc = json.loads(result["signed_document"])
        assert doc["version"] == "1.0.0"
        assert doc["document_type"] == "job_response"
        assert "jacsSignature" in doc
        assert doc["jacsSignature"]["agentID"] == "a1"

    def test_signature_covers_canonical_data(self, ed25519_keypair: tuple) -> None:
        private_key, _ = ed25519_keypair
        payload = {"response": {"message": "test"}}
        result = sign_response(payload, private_key, "id-1")
        doc = json.loads(result["signed_document"])

        canonical = canonicalize_json(payload)
        sig = doc["jacsSignature"]["signature"]
        assert verify_string(private_key.public_key(), canonical, sig)


class TestInvalidateKeyCache:
    def test_invalidate_does_not_raise(self) -> None:
        invalidate_key_cache()
