"""Tests for verify_email_signature in jacs.hai.signing."""

from __future__ import annotations

import hashlib
import json
import logging
import time
from pathlib import Path
from unittest.mock import patch, MagicMock

import httpx
import pytest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from jacs.hai.crypt import sign_string
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


def _build_v2_header(
    jacs_id: str,
    from_addr: str,
    content_hash: str,
    timestamp: int,
    signature_b64: str,
    jacs_version: str = "1.0.0",
) -> str:
    """Build a v2 X-JACS-Signature header string."""
    return (
        f"v=2; a=ed25519; id={jacs_id}; from={from_addr}; "
        f"h={content_hash}; jv={jacs_version}; t={timestamp}; s={signature_b64}"
    )


class TestVerifyEmailSignatureV2:
    """Tests for v2 email signature verification."""

    def test_verify_v2_email_signature(self) -> None:
        """A correctly signed v2 header must verify successfully."""
        private_key = Ed25519PrivateKey.generate()
        public_pem = (
            private_key.public_key()
            .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
            .decode()
        )

        jacs_id = "v2-test-agent"
        from_addr = "v2agent@hai.ai"
        subject = "V2 Test Subject"
        body = "V2 test body content."
        timestamp = 1740393600

        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()

        # v2 signing payload: {hash}:{from}:{timestamp}
        sign_input = f"{content_hash}:{from_addr}:{timestamp}"
        signature_b64 = sign_string(private_key, sign_input)

        sig_header = _build_v2_header(
            jacs_id=jacs_id,
            from_addr=from_addr,
            content_hash=content_hash,
            timestamp=timestamp,
            signature_b64=signature_b64,
        )

        headers = {
            "X-JACS-Signature": sig_header,
            "From": from_addr,
        }

        mock_registry = MagicMock()
        mock_registry.status_code = 200
        mock_registry.raise_for_status = MagicMock()
        mock_registry.json.return_value = {
            "email": from_addr,
            "jacs_id": jacs_id,
            "public_key": public_pem,
            "algorithm": "ed25519",
            "reputation_tier": "established",
            "registered_at": "2026-01-15T00:00:00Z",
        }

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = mock_registry
            mock_time.time.return_value = timestamp + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=headers,
                subject=subject,
                body=body,
                hai_url="https://hai.ai",
            )

        assert result.valid is True, f"Expected valid but got error: {result.error}"
        assert result.jacs_id == jacs_id
        assert result.reputation_tier == "established"
        assert result.error is None

    def test_verify_v2_email_from_mismatch_fails(self) -> None:
        """v2 header with from=agent@hai.ai but From: other@hai.ai should fail."""
        private_key = Ed25519PrivateKey.generate()
        public_pem = (
            private_key.public_key()
            .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
            .decode()
        )

        jacs_id = "v2-mismatch-agent"
        signed_from = "agent@hai.ai"
        actual_from = "other@hai.ai"
        subject = "Mismatch Test"
        body = "Body."
        timestamp = 1740393600

        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()

        # Sign with the correct from (agent@hai.ai)
        sign_input = f"{content_hash}:{signed_from}:{timestamp}"
        signature_b64 = sign_string(private_key, sign_input)

        sig_header = _build_v2_header(
            jacs_id=jacs_id,
            from_addr=signed_from,
            content_hash=content_hash,
            timestamp=timestamp,
            signature_b64=signature_b64,
        )

        headers = {
            "X-JACS-Signature": sig_header,
            # From header says a different address
            "From": actual_from,
        }

        mock_registry = MagicMock()
        mock_registry.status_code = 200
        mock_registry.raise_for_status = MagicMock()
        mock_registry.json.return_value = {
            "email": actual_from,
            "jacs_id": jacs_id,
            "public_key": public_pem,
            "algorithm": "ed25519",
            "reputation_tier": "established",
            "registered_at": "2026-01-15T00:00:00Z",
        }

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = mock_registry
            mock_time.time.return_value = timestamp + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=headers,
                subject=subject,
                body=body,
                hai_url="https://hai.ai",
            )

        assert result.valid is False
        assert result.error is not None
        # The from in the header doesn't match the From email header
        assert "from" in result.error.lower() or "mismatch" in result.error.lower()

    def test_v1_still_requires_content_hash_header(self) -> None:
        """v1 signatures must still require X-JACS-Content-Hash header."""
        result = verify_email_signature(
            headers={
                "X-JACS-Signature": "v=1; a=ed25519; id=x; t=1; s=abc",
                "From": "test@hai.ai",
            },
            subject="Test",
            body="Body",
        )
        assert result.valid is False
        assert "Missing X-JACS-Content-Hash" in (result.error or "")

    def test_v2_does_not_require_content_hash_header(self) -> None:
        """v2 signatures should NOT require X-JACS-Content-Hash header."""
        private_key = Ed25519PrivateKey.generate()
        public_pem = (
            private_key.public_key()
            .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
            .decode()
        )

        jacs_id = "v2-no-content-hash"
        from_addr = "noextra@hai.ai"
        subject = "No Content-Hash"
        body = "Body."
        timestamp = 1740393600

        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()

        sign_input = f"{content_hash}:{from_addr}:{timestamp}"
        signature_b64 = sign_string(private_key, sign_input)

        sig_header = _build_v2_header(
            jacs_id=jacs_id,
            from_addr=from_addr,
            content_hash=content_hash,
            timestamp=timestamp,
            signature_b64=signature_b64,
        )

        # Note: NO X-JACS-Content-Hash header
        headers = {
            "X-JACS-Signature": sig_header,
            "From": from_addr,
        }

        mock_registry = MagicMock()
        mock_registry.status_code = 200
        mock_registry.raise_for_status = MagicMock()
        mock_registry.json.return_value = {
            "email": from_addr,
            "jacs_id": jacs_id,
            "public_key": public_pem,
            "algorithm": "ed25519",
            "reputation_tier": "established",
            "registered_at": "2026-01-15T00:00:00Z",
        }

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time:
            mock_httpx.get.return_value = mock_registry
            mock_time.time.return_value = timestamp + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=headers,
                subject=subject,
                body=body,
                hai_url="https://hai.ai",
            )

        assert result.valid is True, f"Expected valid but got error: {result.error}"

    def test_v2_detection_uses_v_field_not_h_presence(self) -> None:
        """A header with v=1 and h=sha256:... must be treated as v1, not v2.

        This ensures version detection uses the explicit v= field rather
        than the mere presence of the h= field.
        """
        # Build a v1 header that also happens to include h= (unusual but valid).
        # Because v=1, the verifier should take the v1 code path and require
        # X-JACS-Content-Hash, which we deliberately omit.
        sig_header = (
            "v=1; a=ed25519; id=test-agent; "
            "h=sha256:deadbeef; t=1740393600; s=base64sig"
        )
        result = verify_email_signature(
            headers={
                "X-JACS-Signature": sig_header,
                "From": "test@hai.ai",
            },
            subject="Test",
            body="Body",
        )
        # v1 path requires X-JACS-Content-Hash header -- its absence proves
        # the verifier correctly chose the v1 path.
        assert result.valid is False
        assert "Missing X-JACS-Content-Hash" in (result.error or "")

    def test_v2_with_attachment_hash_warns_but_passes(self) -> None:
        """v2 with h= that doesn't match body-only hash (attachments) still passes.

        When the signed content hash includes attachments the verifier cannot
        recompute the full hash.  It should log a warning but the signature
        itself is still valid.
        """
        private_key = Ed25519PrivateKey.generate()
        public_pem = (
            private_key.public_key()
            .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
            .decode()
        )

        jacs_id = "v2-attach-agent"
        from_addr = "attach@hai.ai"
        subject = "Attachment Test"
        body = "Body without attachment bytes."
        timestamp = 1740393600

        # Simulate a content hash that covers subject + body + attachment data.
        # This will NOT match sha256(subject + "\n" + body) because the
        # attachment bytes are included in the original hash.
        full_content = subject + "\n" + body + "\nATTACHMENT_BYTES_HERE"
        content_hash = "sha256:" + hashlib.sha256(
            full_content.encode("utf-8")
        ).hexdigest()

        # v2 signing payload: {hash}:{from}:{timestamp}
        sign_input = f"{content_hash}:{from_addr}:{timestamp}"
        signature_b64 = sign_string(private_key, sign_input)

        sig_header = _build_v2_header(
            jacs_id=jacs_id,
            from_addr=from_addr,
            content_hash=content_hash,
            timestamp=timestamp,
            signature_b64=signature_b64,
        )

        headers = {
            "X-JACS-Signature": sig_header,
            "From": from_addr,
        }

        mock_registry = MagicMock()
        mock_registry.status_code = 200
        mock_registry.raise_for_status = MagicMock()
        mock_registry.json.return_value = {
            "email": from_addr,
            "jacs_id": jacs_id,
            "public_key": public_pem,
            "algorithm": "ed25519",
            "reputation_tier": "established",
            "registered_at": "2026-01-15T00:00:00Z",
        }

        with patch("jacs.hai.signing.httpx") as mock_httpx, \
             patch("jacs.hai.signing.time") as mock_time, \
             patch("jacs.hai.signing.logger") as mock_logger:
            mock_httpx.get.return_value = mock_registry
            mock_time.time.return_value = timestamp + 100
            mock_time.monotonic = time.monotonic

            result = verify_email_signature(
                headers=headers,
                subject=subject,
                body=body,
                hai_url="https://hai.ai",
            )

        # Signature is valid despite content hash mismatch
        assert result.valid is True, f"Expected valid but got error: {result.error}"
        assert result.jacs_id == jacs_id
        assert result.error is None

        # The warning about content hash mismatch was logged
        mock_logger.warning.assert_called_once()
        warning_msg = mock_logger.warning.call_args[0][0]
        assert "content hash mismatch" in warning_msg.lower()
