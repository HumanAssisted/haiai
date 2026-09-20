from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from haiai.errors import HaiError
from haiai.signing import sign_response, unwrap_signed_event


def _fixture() -> dict[str, Any]:
    path = Path(__file__).resolve().parents[2] / "fixtures" / "protocol_parity.json"
    return json.loads(path.read_text())


def test_sign_response_consumes_shared_v2_contract() -> None:
    fixture = _fixture()

    for case in fixture["sign_response"]["cases"]:
        captured: dict[str, Any] = {}

        class Agent:
            def sign_string(self, data: str) -> str:
                return data

            def sign_response(self, payload_json: str) -> str:
                captured["data"] = json.loads(payload_json)
                structure = case["expected_structure"]
                return json.dumps(
                    {
                        "version": structure["version"],
                        "document_type": structure["document_type"],
                        "data": captured["data"],
                        "metadata": {
                            field: f"fixture-{field}"
                            for field in structure["metadata_fields"]
                        },
                        "jacsSignature": {
                            **{
                                field: f"fixture-{field}"
                                for field in structure["signature_fields"]
                            },
                            "signatureContentVersion": structure[
                                "signature_content_version"
                            ],
                        },
                    }
                )

        result = sign_response(case["job_id"], case["input"], Agent(), "agent:v1")
        document = json.loads(result["signed_document"])

        assert captured["data"] == case["expected_data"]
        assert document["version"] == case["expected_structure"]["version"]
        assert document["document_type"] == case["expected_structure"]["document_type"]


def test_sign_response_rejects_legacy_local_fallback(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fixture = _fixture()["sign_response"]
    assert fixture["native_signer_required"] is True

    class LegacyAgent:
        def sign_string(self, _data: str) -> str:
            raise AssertionError("legacy local signing must not be used")

        def canonicalize_json(self, value: str) -> str:
            return value

    legacy_agent = LegacyAgent()
    monkeypatch.setattr("haiai.config.is_loaded", lambda: True)
    monkeypatch.setattr("haiai.config.get_agent", lambda: legacy_agent)

    with pytest.raises(HaiError) as exc_info:
        sign_response(
            "job-legacy",
            {"response": {"message": "must not sign locally"}},
            legacy_agent,
            "legacy-agent",
        )
    assert exc_info.value.code == fixture["missing_native_error_code"]


def test_sign_response_rejects_native_v1_contract() -> None:
    fixture = _fixture()["sign_response"]

    class OldNativeAgent:
        def sign_string(self, data: str) -> str:
            return data

        def sign_response(self, _payload_json: str) -> str:
            return json.dumps(
                {
                    "version": "1.0.0",
                    "document_type": "job_response",
                    "data": {"response": {"message": "legacy"}},
                    "metadata": {
                        "issuer": "legacy-agent",
                        "document_id": "legacy-doc",
                        "created_at": "2024-01-01T00:00:00Z",
                        "hash": "legacy-hash",
                    },
                    "jacsSignature": {
                        "agentID": "legacy-agent",
                        "date": "2024-01-01T00:00:00Z",
                        "signature": "legacy-signature",
                    },
                }
            )

    with pytest.raises(HaiError) as exc_info:
        sign_response(
            "job-legacy",
            {"response": {"message": "must not accept v1"}},
            OldNativeAgent(),
            "legacy-agent",
        )
    assert exc_info.value.code == fixture["invalid_native_contract_error_code"]


def test_unwrap_signed_event_consumes_shared_fail_closed_contract(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fixture = _fixture()["unwrap_signed_event"]
    assert fixture["strict_by_default"] is True

    for case in fixture["cases"]:
        native_result = case.get("native_result")
        if case["expected_outcome"] == "reject" and native_result is None:
            with pytest.raises(HaiError) as exc_info:
                unwrap_signed_event(case["input"], "https://hai.example")
            assert exc_info.value.code == case["expected_error_code"]
            continue

        class Agent:
            def unwrap_signed_event(self, _event: str, _keys: str) -> str:
                result = native_result or {
                    "data": case["expected"],
                    "verified": True,
                }
                return json.dumps(result)

        monkeypatch.setattr("haiai.config.is_loaded", lambda: True)
        monkeypatch.setattr("haiai.config.get_agent", lambda: Agent())
        monkeypatch.setattr(
            "haiai.signing.fetch_server_keys",
            lambda _url, _ffi=None: [
                SimpleNamespace(signer_id="server:v1", public_key_pem="PEM")
            ],
        )

        if case["expected_outcome"] == "verified":
            payload, verified = unwrap_signed_event(
                case["input"], "https://hai.example"
            )
            assert payload == case["expected"]
            assert verified is True
        else:
            with pytest.raises(HaiError) as exc_info:
                unwrap_signed_event(case["input"], "https://hai.example")
            assert exc_info.value.code == case["expected_error_code"]
