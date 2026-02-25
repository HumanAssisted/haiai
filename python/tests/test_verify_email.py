"""Tests for verify_email_signature in jacs.hai.signing."""

from __future__ import annotations

import hashlib
import json
import time
from pathlib import Path
from unittest.mock import patch, MagicMock

import httpx
import pytest

from jacs.hai.signing import verify_email_signature, _parse_jacs_signature_header


CONTRACT_DIR = Path(__file__).parent.parent.parent / "contract"


def _load_fixture() -> dict:
    path = CONTRACT_DIR / "email_verification_example.json"
    with path.open() as fh:
        return json.load(fh)


def _mock_registry_response(fixture: dict, *, jacs_id: str = "test-agent-jacs-id") -> MagicMock:
    """Create a mock httpx response that returns the registry data."""
    mock_resp = MagicMock()
    mock_resp.status_code = 200
    mock_resp.raise_for_status = MagicMock()
    mock_resp.json.return_value = {
        "email": fixture["headers"]["From"],
        "jacs_id": jacs_id,
        "public_key": fixture["test_public_key_pem"],
        "algorithm": "ed25519",
        "reputation_tier": "established",
        "registered_at": "2026-01-15T00:00:00Z",
    }
    return mock_resp


class TestParseJacsSignatureHeader:
    def test_parses_all_fields(self) -> None:
        header = "v=1; a=ed25519; id=test-agent; t=1740000000; s=base64sig"
        fields = _parse_jacs_signature_header(header)
        assert fields["v"] == "1"
        assert fields["a"] == "ed25519"
        assert fields["id"] == "test-agent"
        assert fields["t"] == "1740000000"
        assert fields["s"] == "base64sig"

    def test_handles_extra_whitespace(self) -> None:
        header = "  v = 1 ;  a = ed25519 "
        fields = _parse_jacs_signature_header(header)
        assert fields["v"] == "1"
        assert fields["a"] == "ed25519"


class TestVerifyEmailSignature:
    def test_valid_signature(self) -> None:
        fixture = _load_fixture()

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = _mock_registry_response(fixture)
            # Set current time close to the fixture timestamp so it's not stale
            mock_time.time.return_value = 1740393600 + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=fixture["headers"],
                subject=fixture["subject"],
                body=fixture["body"],
                hai_url="https://hai.ai",
            )

        assert result.valid is True
        assert result.jacs_id == "test-agent-jacs-id"
        assert result.reputation_tier == "established"
        assert result.error is None

    def test_content_hash_matches_contract(self) -> None:
        fixture = _load_fixture()
        computed = "sha256:" + hashlib.sha256(
            (fixture["subject"] + "\n" + fixture["body"]).encode()
        ).hexdigest()
        assert computed == fixture["expected_content_hash"]

    def test_content_hash_mismatch(self) -> None:
        fixture = _load_fixture()
        headers = dict(fixture["headers"])
        headers["X-JACS-Content-Hash"] = "sha256:0000000000000000000000000000000000000000000000000000000000000000"

        result = verify_email_signature(
            headers=headers,
            subject=fixture["subject"],
            body=fixture["body"],
        )

        assert result.valid is False
        assert result.error == "Content hash mismatch"

    def test_missing_signature_header(self) -> None:
        result = verify_email_signature(
            headers={"X-JACS-Content-Hash": "sha256:abc", "From": "test@hai.ai"},
            subject="Test",
            body="Body",
        )
        assert result.valid is False
        assert "Missing X-JACS-Signature" in (result.error or "")

    def test_missing_content_hash_header(self) -> None:
        result = verify_email_signature(
            headers={"X-JACS-Signature": "v=1; a=ed25519; id=x; t=1; s=abc", "From": "test@hai.ai"},
            subject="Test",
            body="Body",
        )
        assert result.valid is False
        assert "Missing X-JACS-Content-Hash" in (result.error or "")

    def test_missing_from_header(self) -> None:
        result = verify_email_signature(
            headers={"X-JACS-Signature": "v=1; a=ed25519; id=x; t=1; s=abc", "X-JACS-Content-Hash": "sha256:abc"},
            subject="Test",
            body="Body",
        )
        assert result.valid is False
        assert result.error == "Missing From header"

    def test_stale_timestamp(self) -> None:
        fixture = _load_fixture()

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = _mock_registry_response(fixture)
            # Set current time far in the future (>24h after fixture timestamp)
            mock_time.time.return_value = 1740393600 + 90000
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=fixture["headers"],
                subject=fixture["subject"],
                body=fixture["body"],
            )

        assert result.valid is False
        assert "too old" in (result.error or "")

    def test_registry_fetch_failure(self) -> None:
        fixture = _load_fixture()

        with patch("jacs.hai.signing.httpx") as mock_httpx:
            mock_httpx.get.side_effect = httpx.ConnectError("connection refused")

            result = verify_email_signature(
                headers=fixture["headers"],
                subject=fixture["subject"],
                body=fixture["body"],
            )

        assert result.valid is False
        assert "Failed to fetch public key" in (result.error or "")

    def test_signature_verification_failure(self) -> None:
        fixture = _load_fixture()
        headers = dict(fixture["headers"])
        # Tamper with the signature
        sig_header = headers["X-JACS-Signature"]
        headers["X-JACS-Signature"] = sig_header.replace(
            "s=", "s=AAAA"
        )

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = _mock_registry_response(fixture)
            mock_time.time.return_value = 1740393600 + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=headers,
                subject=fixture["subject"],
                body=fixture["body"],
            )

        assert result.valid is False
        assert "Signature verification failed" in (result.error or "")

    def test_rejects_signature_id_mismatch(self) -> None:
        fixture = _load_fixture()

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = _mock_registry_response(fixture, jacs_id="different-agent-id")
            mock_time.time.return_value = 1740393600 + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=fixture["headers"],
                subject=fixture["subject"],
                body=fixture["body"],
            )

        assert result.valid is False
        assert result.error == "Signature id does not match registry jacs_id"
