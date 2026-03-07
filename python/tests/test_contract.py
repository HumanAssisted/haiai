"""Contract tests -- validate SDK deserialization against shared JSON fixtures.

Each fixture in ``haiai/contract/`` is the single source of truth shared
across all language SDKs (Python, Node, Go, Rust).  These tests ensure
the Python SDK can round-trip every fixture into its domain model with
the correct field values, and that content-hash / sign-input computations
are cross-language compatible.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

from jacs.hai.models import EmailMessage, EmailStatus, KeyRegistryResponse, EmailVerificationResult

# ---------------------------------------------------------------------------
# Fixture directory -- two levels up from tests/, then into contract/
# ---------------------------------------------------------------------------

CONTRACT_DIR = Path(__file__).parent.parent.parent / "contract"


def _load(name: str) -> dict:
    """Load a contract fixture by filename."""
    path = CONTRACT_DIR / name
    with path.open() as fh:
        return json.load(fh)


# ---------------------------------------------------------------------------
# Helpers that mirror the deserialization logic in client.py
# ---------------------------------------------------------------------------


def _email_message_from_dict(m: dict) -> EmailMessage:
    """Construct an EmailMessage the same way ``HaiClient.list_messages`` does."""
    return EmailMessage(
        id=m.get("id", ""),
        from_address=m.get("from_address", m.get("from", "")),
        to_address=m.get("to_address", m.get("to", "")),
        subject=m.get("subject", ""),
        body_text=m.get("body_text", ""),
        created_at=m.get("created_at", ""),
        direction=m.get("direction", ""),
        message_id=m.get("message_id", ""),
        in_reply_to=m.get("in_reply_to"),
        is_read=m.get("is_read", False),
        delivery_status=m.get("delivery_status", ""),
        read_at=m.get("read_at"),
        jacs_verified=m.get("jacs_verified"),
    )


def _email_status_from_dict(data: dict) -> EmailStatus:
    """Construct an EmailStatus the same way ``HaiClient.get_email_status`` does."""
    return EmailStatus(
        email=data.get("email", ""),
        status=data.get("status", ""),
        tier=data.get("tier", ""),
        billing_tier=data.get("billing_tier", ""),
        messages_sent_24h=int(data.get("messages_sent_24h", 0)),
        daily_limit=int(data.get("daily_limit", 0)),
        daily_used=int(data.get("daily_used", 0)),
        resets_at=data.get("resets_at", ""),
        messages_sent_total=int(data.get("messages_sent_total", 0)),
        external_enabled=data.get("external_enabled", False),
        external_sends_today=int(data.get("external_sends_today", 0)),
        last_tier_change=data.get("last_tier_change"),
    )


def _key_registry_from_dict(data: dict) -> KeyRegistryResponse:
    """Construct a KeyRegistryResponse from a dict."""
    return KeyRegistryResponse(
        email=data.get("email", ""),
        jacs_id=data.get("jacs_id", ""),
        public_key=data.get("public_key", ""),
        algorithm=data.get("algorithm", ""),
        reputation_tier=data.get("reputation_tier", ""),
        registered_at=data.get("registered_at", ""),
    )


def _verification_result_from_dict(data: dict) -> EmailVerificationResult:
    """Construct an EmailVerificationResult from a dict."""
    return EmailVerificationResult(
        valid=data.get("valid", False),
        jacs_id=data.get("jacs_id", ""),
        reputation_tier=data.get("reputation_tier", ""),
        error=data.get("error"),
    )


# ===================================================================
# Tests
# ===================================================================


class TestDeserializeEmailMessage:
    """Validate that a single email_message.json round-trips correctly."""

    def test_deserialize_email_message(self) -> None:
        data = _load("email_message.json")
        msg = _email_message_from_dict(data)

        assert msg.id == "550e8400-e29b-41d4-a716-446655440000"
        assert msg.from_address == "sender@hai.ai"
        assert msg.to_address == "recipient@hai.ai"
        assert msg.subject == "Test Subject"
        assert msg.body_text == "Hello, this is a test email body."
        assert msg.direction == "inbound"
        assert msg.is_read is False
        assert msg.delivery_status == "delivered"
        assert msg.created_at == "2026-02-24T12:00:00Z"
        assert msg.jacs_verified is True


class TestDeserializeListMessagesResponse:
    """Validate list_messages_response.json envelope parsing."""

    def test_deserialize_list_messages_response(self) -> None:
        data = _load("list_messages_response.json")

        # The client extracts the "messages" list from the envelope
        raw_messages = data if isinstance(data, list) else data.get("messages", [])
        messages = [_email_message_from_dict(m) for m in raw_messages]

        assert len(messages) == 1

        msg = messages[0]
        assert msg.id == "550e8400-e29b-41d4-a716-446655440000"
        assert msg.from_address == "sender@hai.ai"
        assert msg.to_address == "recipient@hai.ai"
        assert msg.subject == "Test Subject"
        assert msg.body_text == "Hello, this is a test email body."
        assert msg.direction == "inbound"
        assert msg.is_read is False
        assert msg.delivery_status == "delivered"
        assert msg.created_at == "2026-02-24T12:00:00Z"
        assert msg.jacs_verified is True


class TestDeserializeEmailStatus:
    """Validate email_status_response.json deserialization."""

    def test_deserialize_email_status(self) -> None:
        data = _load("email_status_response.json")
        status = _email_status_from_dict(data)

        assert status.email == "testbot@hai.ai"
        assert status.tier == "new"
        assert status.billing_tier == "free"
        assert status.messages_sent_24h == 5
        assert status.daily_limit == 10
        assert status.external_enabled is False
        assert status.external_sends_today == 0
        assert status.last_tier_change is None


class TestContentHashComputation:
    """Validate that the SDK produces the same content hash as other SDKs."""

    def test_content_hash_computation(self) -> None:
        data = _load("content_hash_example.json")

        subject = data["subject"]
        body = data["body"]
        expected_hash = data["expected_hash"]

        # Same code path as HaiClient.send_email
        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()

        assert content_hash == expected_hash


class TestSignInputFormat:
    """Validate that the sign-input string matches the cross-SDK fixture."""

    def test_sign_input_format(self) -> None:
        data = _load("content_hash_example.json")

        subject = data["subject"]
        body = data["body"]
        from_email = data["from_email"]
        timestamp = data["timestamp"]
        expected_sign_input = data["sign_input_example"]

        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()

        # Same v2 format as HaiClient.send_email
        sign_input = f"{content_hash}:{from_email}:{timestamp}"

        assert sign_input == expected_sign_input


class TestDeserializeKeyRegistryResponse:
    """Validate key_registry_response.json deserialization."""

    def test_deserialize_key_registry_response(self) -> None:
        data = _load("key_registry_response.json")
        resp = _key_registry_from_dict(data)

        assert resp.email == "testbot@hai.ai"
        assert resp.jacs_id == "test-agent-jacs-id"
        assert resp.public_key == "MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF"
        assert resp.algorithm == "ed25519"
        assert resp.reputation_tier == "new"
        assert resp.registered_at == "2026-01-15T00:00:00Z"


class TestDeserializeVerificationResult:
    """Validate verification_result.json deserialization."""

    def test_deserialize_verification_result(self) -> None:
        data = _load("verification_result.json")
        result = _verification_result_from_dict(data)

        assert result.valid is True
        assert result.jacs_id == "test-agent-jacs-id"
        assert result.reputation_tier == "established"
        assert result.error is None


class TestDeserializeKeyLookupVersionedResponse:
    """Validate key_lookup_versioned_response.json contract fixture.

    Ensures the Python SDK can parse the versioned key lookup response
    format into a PublicKeyInfo dataclass with all fields intact.
    """

    def test_deserialize_key_lookup_fixture(self) -> None:
        from jacs.hai.models import PublicKeyInfo

        data = _load("key_lookup_versioned_response.json")
        resp = data["response"]

        info = PublicKeyInfo(
            jacs_id=resp["jacs_id"],
            version=resp["version"],
            public_key=resp["public_key"],
            algorithm=resp["algorithm"],
            public_key_hash=resp["public_key_hash"],
            status=resp["status"],
            dns_verified=resp["dns_verified"],
            created_at=resp["created_at"],
            public_key_raw_b64=resp.get("public_key_raw_b64", ""),
        )

        assert info.jacs_id == "fixture-agent-00000000-0000-0000-0000-000000000001"
        assert info.version == "fixture-version-00000000-0000-0000-0000-000000000001"
        assert info.public_key.startswith("-----BEGIN PUBLIC KEY-----")
        assert info.public_key.endswith("-----END PUBLIC KEY-----")
        assert info.algorithm == "ed25519"
        assert info.public_key_hash.startswith("sha256:")
        assert len(info.public_key_hash) == 7 + 64  # sha256: + 64 hex chars
        assert info.status == "active"
        assert info.dns_verified is True
        assert info.created_at == "2026-01-01T00:00:00Z"
        assert info.public_key_raw_b64 != ""
        # Verify base64 fields are non-empty
        assert resp["public_key_b64"] != ""
