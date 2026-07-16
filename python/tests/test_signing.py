"""Tests for haiai.signing module.

All crypto operations delegate to JACS binding-core.
"""

from __future__ import annotations

import json
from typing import Any

import pytest

from haiai.errors import HaiError
from haiai.signing import (
    canonicalize_json,
    create_agent_document,
    fetch_server_keys,
    invalidate_key_cache,
    is_signed_event,
    sign_response,
    unwrap_signed_event,
    verify_string,
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


class TestVerifyStringStandalone:
    """Standalone verify_string is the symmetric primitive used by external
    callers (hai API, cross-language tests) to check signatures without
    instantiating an agent. Coverage gap: prior to 2026-05-05, no test
    exercised this path, and the implementation broke against jacs 0.10.1
    (called the removed jacs.jacs.extract_public_key_bytes).
    """

    def test_roundtrip_ed25519(self) -> None:
        pytest.importorskip("jacs")
        from jacs import SimpleAgent

        agent, info = SimpleAgent.ephemeral("ed25519")
        msg = "hello world"
        sig = agent.sign_string(msg)
        pub_pem = agent.get_public_key_pem()
        assert verify_string(msg, sig, pub_pem, algorithm=info["algorithm"]) is True

    def test_tampered_message_returns_false(self) -> None:
        pytest.importorskip("jacs")
        from jacs import SimpleAgent

        agent, info = SimpleAgent.ephemeral("ed25519")
        sig = agent.sign_string("original")
        pub_pem = agent.get_public_key_pem()
        assert (
            verify_string("tampered", sig, pub_pem, algorithm=info["algorithm"])
            is False
        )

    def test_wrong_key_returns_false(self) -> None:
        pytest.importorskip("jacs")
        from jacs import SimpleAgent

        agent_a, info = SimpleAgent.ephemeral("ed25519")
        agent_b, _ = SimpleAgent.ephemeral("ed25519")
        msg = "hello"
        sig = agent_a.sign_string(msg)
        wrong_pem = agent_b.get_public_key_pem()
        assert verify_string(msg, sig, wrong_pem, algorithm=info["algorithm"]) is False

    def test_accepts_standard_x509_subjectpublickeyinfo_pem(self) -> None:
        """verify_string accepts both JACS-style raw PEMs AND standard X.509
        SubjectPublicKeyInfo PEMs (e.g., produced by the cryptography library
        in tests). Regression guard: hai-side test fixtures generate Ed25519
        keys via cryptography.hazmat which emits SubjectPublicKeyInfo, not
        the JACS bare-bytes shape.
        """
        pytest.importorskip("jacs")
        from cryptography.hazmat.primitives.asymmetric.ed25519 import (
            Ed25519PrivateKey,
        )
        from cryptography.hazmat.primitives.serialization import (
            Encoding,
            PublicFormat,
        )
        import base64

        priv = Ed25519PrivateKey.generate()
        pub_pem = (
            priv.public_key()
            .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
            .decode()
        )
        msg = "x509 round-trip"
        sig_b64 = base64.b64encode(priv.sign(msg.encode())).decode()
        assert verify_string(msg, sig_b64, pub_pem, algorithm="ring-Ed25519") is True


class TestCanonicalizeJson:
    def test_sorted_keys(self, loaded_config: None) -> None:
        result = canonicalize_json({"b": 2, "a": 1})
        assert result == '{"a":1,"b":2}'

    def test_no_spaces(self, loaded_config: None) -> None:
        result = canonicalize_json({"key": "value"})
        assert " " not in result

    def test_nested(self, loaded_config: None) -> None:
        result = canonicalize_json({"z": {"b": 1, "a": 2}})
        parsed = json.loads(result)
        assert list(parsed.keys()) == ["z"]

    def test_requires_jacs_agent_loaded(self) -> None:
        """canonicalize_json raises HaiError when JACS agent is not loaded."""
        from haiai import config as config_mod

        config_mod.reset()
        with pytest.raises(HaiError) as exc_info:
            canonicalize_json({"a": 1})
        assert exc_info.value.code == "JACS_NOT_LOADED"
        assert exc_info.value.action  # non-empty action hint


class TestCreateAgentDocument:
    def test_creates_valid_doc(self, jacs_agent: Any, loaded_config: None) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="TestBot",
            version="1.0",
        )
        assert doc["jacsAgentName"] == "TestBot"
        assert "jacsSignature" in doc
        assert "jacsId" in doc

    def test_signature_is_present(self, jacs_agent: Any, loaded_config: None) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="Bot",
            version="1.0",
        )
        jacs_sig = doc["jacsSignature"]
        assert isinstance(jacs_sig, dict)
        assert "signature" in jacs_sig
        assert jacs_sig["signature"]  # non-empty

    def test_custom_jacs_id(self, jacs_agent: Any, loaded_config: None) -> None:
        doc = create_agent_document(
            agent=jacs_agent,
            name="B",
            version="1",
            jacs_id="custom-id",
        )
        assert doc["jacsId"] == "custom-id"


class TestIsSignedEvent:
    def test_fully_bound_v2_format(self) -> None:
        data = {
            "version": "2.0.0",
            "document_type": "job_response",
            "data": {},
            "metadata": {},
            "jacsSignature": {},
        }
        assert is_signed_event(data)

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
    @staticmethod
    def _canonical_event() -> dict[str, Any]:
        return {
            "version": "2.0.0",
            "document_type": "job_response",
            "data": {"type": "benchmark_job", "job_id": "j1"},
            "metadata": {
                "issuer": "server:v1",
                "document_id": "doc-1",
                "created_at": "now",
                "hash": "abc",
            },
            "jacsSignature": {
                "agentID": "server:v1",
                "date": "now",
                "signature": "sig",
            },
        }

    def test_strict_native_verification_uses_flat_exact_signer_map(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        from types import SimpleNamespace
        from haiai import config as config_mod

        calls: list[tuple[str, str]] = []

        class Agent:
            def unwrap_signed_event(self, event_json: str, keys_json: str) -> str:
                calls.append((event_json, keys_json))
                return json.dumps(
                    {
                        "data": {"type": "benchmark_job", "job_id": "j1"},
                        "verified": True,
                        "status": "verified",
                    }
                )

        monkeypatch.setattr(config_mod, "is_loaded", lambda: True)
        monkeypatch.setattr(config_mod, "get_agent", lambda: Agent())
        ffi = SimpleNamespace(
            fetch_server_keys=lambda: {
                "keys": [
                    {
                        "signer_id": "server:v1",
                        "key_id": "db-key-7",
                        "algorithm": "ed25519",
                        "public_key": "PEM",
                        "is_active": True,
                    }
                ]
            }
        )

        payload, verified = unwrap_signed_event(
            self._canonical_event(),
            "https://hai.example",
            verify=True,
            ffi=ffi,
        )

        assert payload == {"type": "benchmark_job", "job_id": "j1"}
        assert verified is True
        assert json.loads(calls[0][1]) == {"server:v1": "PEM"}

    @pytest.mark.parametrize(
        "native_result",
        [
            {"data": {"type": "benchmark_job"}, "verified": False},
            {"data": {"type": "benchmark_job"}},
            {"data": {"type": "benchmark_job"}, "verified": "true"},
            {"verified": True},
        ],
    )
    def test_strict_native_verification_rejects_false_or_malformed_provenance(
        self, monkeypatch: pytest.MonkeyPatch, native_result: dict[str, Any]
    ) -> None:
        from types import SimpleNamespace
        from haiai import config as config_mod
        import haiai.signing as signing_mod

        class Agent:
            def unwrap_signed_event(self, _event_json: str, _keys_json: str) -> str:
                return json.dumps(native_result)

        monkeypatch.setattr(config_mod, "is_loaded", lambda: True)
        monkeypatch.setattr(config_mod, "get_agent", lambda: Agent())
        monkeypatch.setattr(
            signing_mod,
            "fetch_server_keys",
            lambda _url, _ffi=None: [
                SimpleNamespace(
                    signer_id="server:v1",
                    key_id="k1",
                    algorithm="ed25519",
                    public_key_pem="PEM",
                )
            ],
        )

        with pytest.raises(HaiError, match="verified|verification|data"):
            unwrap_signed_event(
                self._canonical_event(), "https://hai.example", verify=True
            )

    def test_strict_verification_requires_url_and_native_method(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        from haiai import config as config_mod

        with pytest.raises(HaiError, match="URL"):
            unwrap_signed_event(self._canonical_event(), verify=True)

        monkeypatch.setattr(config_mod, "is_loaded", lambda: True)
        monkeypatch.setattr(config_mod, "get_agent", lambda: object())
        with pytest.raises(HaiError, match="unwrap_signed_event|upgrade"):
            unwrap_signed_event(
                self._canonical_event(), "https://hai.example", verify=True
            )

    def test_strict_verification_rejects_legacy_payload_only_event(self) -> None:
        data = {
            "payload": {"type": "benchmark_job"},
            "signature": {"signature": "abc"},
            "metadata": {},
        }
        with pytest.raises(HaiError, match="Legacy|fully bound"):
            unwrap_signed_event(data, "https://hai.example", verify=True)

    def test_explicit_verification_off_returns_canonical_data_as_unverified(
        self,
    ) -> None:
        payload, verified = unwrap_signed_event(self._canonical_event(), verify=False)
        assert payload == {"type": "benchmark_job", "job_id": "j1"}
        assert verified is False


class TestFetchServerKeys:
    class Ffi:
        def __init__(self, response: dict[str, Any]) -> None:
            self.response = response
            self.calls = 0

        def fetch_server_keys(self) -> dict[str, Any]:
            self.calls += 1
            return self.response

    def test_requires_https_except_for_loopback(self) -> None:
        invalidate_key_cache()
        ffi = self.Ffi(
            {
                "keys": [
                    {
                        "signer_id": "server:v1",
                        "public_key": "PEM",
                        "is_active": True,
                    }
                ]
            }
        )

        with pytest.raises(HaiError, match="HTTPS|loopback"):
            fetch_server_keys("http://hai.example", ffi)
        assert ffi.calls == 0
        assert fetch_server_keys("http://127.0.0.1:8080", ffi)[0].signer_id == (
            "server:v1"
        )

    def test_indexes_active_key_by_exact_signer_id(self) -> None:
        invalidate_key_cache()
        ffi = self.Ffi(
            {
                "issuer": "hai.example",
                "keys": [
                    {
                        "signer_id": "server:v7",
                        "jacs_id": "server",
                        "key_id": "db-row-7",
                        "algorithm": "ed25519",
                        "public_key": "PEM",
                        "is_active": True,
                    }
                ],
            }
        )

        keys = fetch_server_keys("https://hai.example", ffi)

        assert [(key.signer_id, key.public_key_pem) for key in keys] == [
            ("server:v7", "PEM")
        ]

    def test_rejects_conflicting_active_keys_for_one_signer(self) -> None:
        invalidate_key_cache()
        ffi = self.Ffi(
            {
                "keys": [
                    {
                        "signer_id": "server:v1",
                        "public_key": "PEM-A",
                        "is_active": True,
                    },
                    {
                        "signer_id": "server:v1",
                        "public_key": "PEM-B",
                        "is_active": True,
                    },
                ]
            }
        )

        with pytest.raises(HaiError, match="conflicting"):
            fetch_server_keys("https://hai.example", ffi)

    def test_never_reuses_cached_keys_across_origins(self) -> None:
        invalidate_key_cache()
        ffi_a = self.Ffi(
            {
                "keys": [
                    {
                        "signer_id": "server-a:v1",
                        "public_key": "PEM-A",
                        "is_active": True,
                    }
                ]
            }
        )
        ffi_b = self.Ffi(
            {
                "keys": [
                    {
                        "signer_id": "server-b:v1",
                        "public_key": "PEM-B",
                        "is_active": True,
                    }
                ]
            }
        )

        assert (
            fetch_server_keys("https://a.example/", ffi_a)[0].signer_id == "server-a:v1"
        )
        assert (
            fetch_server_keys("https://b.example", ffi_b)[0].signer_id == "server-b:v1"
        )
        assert ffi_b.calls == 1

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
    def test_cryptographically_binds_v2_contract_and_job_id(
        self, jacs_agent: Any, loaded_config: None
    ) -> None:
        job_id = "550e8400-e29b-41d4-a716-446655440000"
        request = {
            "response": {
                "message": "bound",
                "metadata": None,
                "processing_time_ms": 12,
            }
        }

        result = sign_response(job_id, request, jacs_agent, "agent-jacs-1")
        doc = json.loads(result["signed_document"])
        assert doc["data"] == {
            "contract": "hai.job-response",
            "version": 2,
            "job_id": job_id,
            "response": request["response"],
        }

    def test_produces_signed_document(
        self, jacs_agent: Any, loaded_config: None
    ) -> None:
        job_payload = {"response": {"message": "hello", "processing_time_ms": 100}}
        result = sign_response("job-1", job_payload, jacs_agent, "agent-jacs-1")
        assert "signed_document" in result
        assert result["agent_jacs_id"] == "agent-jacs-1"

    def test_signed_document_is_json(
        self, jacs_agent: Any, loaded_config: None
    ) -> None:
        result = sign_response(
            "job-1", {"response": {"message": "x"}}, jacs_agent, "a1"
        )
        doc = json.loads(result["signed_document"])
        assert doc["version"] == "2.0.0"
        assert doc["document_type"] == "job_response"
        assert "jacsSignature" in doc
        assert doc["jacsSignature"]["signatureContentVersion"] == "jacs-response-v2"

    def test_signature_is_present(self, jacs_agent: Any, loaded_config: None) -> None:
        payload = {"response": {"message": "test"}}
        result = sign_response("job-1", payload, jacs_agent, "id-1")
        doc = json.loads(result["signed_document"])
        sig = doc["jacsSignature"]["signature"]
        assert sig  # non-empty

    def test_requires_sign_string(self) -> None:
        """sign_response raises HaiError when agent lacks sign_string."""

        class NoSignAgent:
            pass

        with pytest.raises(HaiError) as exc_info:
            sign_response(
                "job-1", {"response": {"message": "test"}}, NoSignAgent(), "id-1"
            )
        assert exc_info.value.code == "JACS_NOT_LOADED"

    def test_rejects_old_jacs_without_native_v2_signer(self) -> None:
        class LegacyAgent:
            def sign_string(self, _data: str) -> str:
                raise AssertionError("legacy local signing must not be used")

        with pytest.raises(HaiError) as exc_info:
            sign_response(
                "job-1",
                {"response": {"message": "test"}},
                LegacyAgent(),
                "id-1",
            )
        assert exc_info.value.code == "JACS_TOO_OLD"
        assert "0.11.4" in exc_info.value.action


class TestCryptoDelegationContract:
    """Tests driven by fixtures/crypto_delegation_contract.json."""

    @pytest.fixture()
    def contract(self) -> dict:
        path = (
            __import__("pathlib").Path(__file__).resolve().parent.parent.parent
            / "fixtures"
            / "crypto_delegation_contract.json"
        )
        return json.loads(path.read_text())

    def test_canonicalization_vectors(
        self, contract: dict, loaded_config: None
    ) -> None:
        for vec in contract["canonicalization"]["test_vectors"]:
            result = canonicalize_json(vec["input"])
            assert result == vec["expected"], f"Failed for input {vec['input']}"

    def test_canonicalization_requires_jacs(self, contract: dict) -> None:
        from haiai import config as config_mod

        config_mod.reset()
        assert contract["canonicalization"]["jacs_required"] is True
        with pytest.raises(HaiError) as exc_info:
            canonicalize_json({"test": 1})
        assert exc_info.value.code == contract["canonicalization"]["error_when_no_jacs"]


class TestErrorContract:
    """Tests driven by fixtures/error_contract.json."""

    @pytest.fixture()
    def contract(self) -> dict:
        path = (
            __import__("pathlib").Path(__file__).resolve().parent.parent.parent
            / "fixtures"
            / "error_contract.json"
        )
        return json.loads(path.read_text())

    def test_jacs_not_loaded_error_matches_pattern(self, contract: dict) -> None:
        import re

        from haiai import config as config_mod

        config_mod.reset()
        pattern = contract["error_codes"]["JACS_NOT_LOADED"]["message_pattern"]
        action_pattern = contract["error_codes"]["JACS_NOT_LOADED"][
            "action_hint_pattern"
        ]
        with pytest.raises(HaiError) as exc_info:
            canonicalize_json({"a": 1})
        err = exc_info.value
        assert re.search(pattern, err.message, re.IGNORECASE), (
            f"Message '{err.message}' does not match pattern '{pattern}'"
        )
        assert re.search(action_pattern, err.action, re.IGNORECASE), (
            f"Action '{err.action}' does not match pattern '{action_pattern}'"
        )


class TestInvalidateKeyCache:
    def test_invalidate_does_not_raise(self) -> None:
        invalidate_key_cache()
