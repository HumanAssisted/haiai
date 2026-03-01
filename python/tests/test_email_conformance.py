"""Cross-SDK email conformance tests.

Validates the Python SDK against the shared ``email_conformance.json`` fixture
to ensure structural equivalence with Go, Node, and Rust SDKs.
"""
from __future__ import annotations

import json
import warnings
from pathlib import Path
from typing import Any
from unittest.mock import patch

import pytest

from jacs.hai.client import HaiClient
from jacs.hai.errors import EmailNotActive, RateLimited, RecipientNotFound
from jacs.hai.models import (
    ChainEntry,
    EmailVerificationResultV2,
    FieldResult,
    FieldStatus,
)

# ---------------------------------------------------------------------------
# Fixture loading
# ---------------------------------------------------------------------------

FIXTURES_DIR = Path(__file__).parent.parent.parent / "fixtures"


def _load_conformance() -> dict:
    path = FIXTURES_DIR / "email_conformance.json"
    with path.open() as fh:
        return json.load(fh)


CONFORMANCE = _load_conformance()

BASE_URL = "https://test.hai.ai"
TEST_AGENT_EMAIL = "test@hai.ai"

_original_init = HaiClient.__init__


@pytest.fixture(autouse=True)
def _set_agent_email(monkeypatch: pytest.MonkeyPatch) -> None:
    """Ensure every HaiClient created in tests has agent_email set."""

    def patched_init(self: HaiClient, *args: Any, **kwargs: Any) -> None:
        _original_init(self, *args, **kwargs)
        self._agent_email = TEST_AGENT_EMAIL  # type: ignore[attr-defined]

    monkeypatch.setattr(HaiClient, "__init__", patched_init)


class _FakeResponse:
    """Minimal fake httpx response for monkeypatching."""

    def __init__(
        self, status_code: int, payload: Any = None, text: str = "",
        content: bytes = b"",
    ) -> None:
        self.status_code = status_code
        self._payload = payload or {}
        self.text = text or (json.dumps(payload) if payload else "")
        self.content = content
        self.headers: dict[str, str] = {}

    def json(self) -> Any:
        return self._payload


# ---------------------------------------------------------------------------
# Content hash golden value conformance
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# EmailVerificationResultV2 structural conformance
# ---------------------------------------------------------------------------


class TestConformanceMockVerifyResponse:
    """Mock server response deserializes correctly into EmailVerificationResultV2."""

    def test_deserialization(self) -> None:
        mock_json = CONFORMANCE["mock_verify_response"]["json"]

        # Simulate what HaiClient.verify_email does when parsing the response
        result = EmailVerificationResultV2(
            valid=mock_json.get("valid", False),
            jacs_id=mock_json.get("jacs_id", ""),
            algorithm=mock_json.get("algorithm", ""),
            reputation_tier=mock_json.get("reputation_tier", ""),
            dns_verified=mock_json.get("dns_verified"),
            field_results=[
                FieldResult(
                    field=fr.get("field", ""),
                    status=FieldStatus(fr.get("status", "unverifiable")),
                    original_hash=fr.get("original_hash"),
                    current_hash=fr.get("current_hash"),
                    original_value=fr.get("original_value"),
                    current_value=fr.get("current_value"),
                )
                for fr in mock_json.get("field_results", [])
            ],
            chain=[
                ChainEntry(
                    signer=ce.get("signer", ""),
                    jacs_id=ce.get("jacs_id", ""),
                    valid=ce.get("valid", False),
                    forwarded=ce.get("forwarded", False),
                )
                for ce in mock_json.get("chain", [])
            ],
            error=mock_json.get("error"),
        )

        assert result.valid is True
        assert result.jacs_id == "conformance-test-agent-001"
        assert result.algorithm == "ed25519"
        assert result.reputation_tier == "established"
        assert result.dns_verified is True
        assert result.error is None

        # field_results
        assert len(result.field_results) == 4
        assert result.field_results[0].field == "subject"
        assert result.field_results[0].status == FieldStatus.PASS
        assert result.field_results[3].field == "date"
        assert result.field_results[3].status == FieldStatus.MODIFIED

        # chain
        assert len(result.chain) == 1
        assert result.chain[0].signer == "agent@hai.ai"
        assert result.chain[0].jacs_id == "conformance-test-agent-001"
        assert result.chain[0].valid is True
        assert result.chain[0].forwarded is False


# ---------------------------------------------------------------------------
# FieldStatus enum conformance
# ---------------------------------------------------------------------------


class TestConformanceFieldStatusValues:
    """FieldStatus enum must match the conformance fixture values."""

    def test_all_values_present(self) -> None:
        expected = set(CONFORMANCE["verification_result_v2_schema"]["field_status_values"])
        actual = {fs.value for fs in FieldStatus}
        assert actual == expected, f"FieldStatus mismatch: actual={actual}, expected={expected}"


# ---------------------------------------------------------------------------
# API contract conformance: SignEmail
# ---------------------------------------------------------------------------


class TestConformanceSignEmailAPIContract:
    """SignEmail must POST to /api/v1/email/sign with message/rfc822."""

    def test_api_contract(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        contract = CONFORMANCE["api_contracts"]["sign_email"]
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            captured["headers"] = kwargs.get("headers", {})
            return _FakeResponse(200, content=b"signed email bytes")

        monkeypatch.setattr("httpx.post", fake_post)

        client = HaiClient()
        client.sign_email(BASE_URL, b"raw email bytes")

        assert captured["url"].endswith(contract["path"])
        assert captured["headers"].get("Content-Type") == contract["request_content_type"]


# ---------------------------------------------------------------------------
# API contract conformance: VerifyEmail
# ---------------------------------------------------------------------------


class TestConformanceVerifyEmailAPIContract:
    """VerifyEmail must POST to /api/v1/email/verify with message/rfc822."""

    def test_api_contract(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        contract = CONFORMANCE["api_contracts"]["verify_email"]
        mock_json = CONFORMANCE["mock_verify_response"]["json"]
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            captured["headers"] = kwargs.get("headers", {})
            return _FakeResponse(200, payload=mock_json)

        monkeypatch.setattr("httpx.post", fake_post)

        client = HaiClient()
        client.verify_email(BASE_URL, b"raw email bytes")

        assert captured["url"].endswith(contract["path"])
        assert captured["headers"].get("Content-Type") == contract["request_content_type"]


# ---------------------------------------------------------------------------
# API contract conformance: SendEmail excluded fields
# ---------------------------------------------------------------------------


class TestConformanceSendEmailExcludedFields:
    """SendEmail must NOT include client-side signing fields."""

    def test_excluded_fields(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        excluded = CONFORMANCE["api_contracts"]["send_email"]["excluded_fields"]
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            content = kwargs.get("content")
            if content:
                captured["payload"] = json.loads(content)
            return _FakeResponse(200, {"message_id": "msg-conf", "status": "sent"})

        monkeypatch.setattr("httpx.post", fake_post)

        client = HaiClient()
        client.send_email(BASE_URL, "bob@hai.ai", "Test", "Body")

        for field_name in excluded:
            assert field_name not in captured.get("payload", {}), (
                f"SendEmail payload must not contain {field_name!r} (server handles signing)"
            )


# ---------------------------------------------------------------------------
# Error type conformance
# ---------------------------------------------------------------------------


class TestConformanceErrorTypes:
    """All email error sentinel types must exist."""

    def test_error_sentinels_exist(self) -> None:
        # Verify they're classes and can be instantiated
        e1 = EmailNotActive("test", status_code=403, body="")
        e2 = RecipientNotFound("test", status_code=404, body="")
        e3 = RateLimited("test", status_code=429, body="")

        assert "test" in str(e1)
        assert "test" in str(e2)
        assert "test" in str(e3)
