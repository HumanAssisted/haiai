"""Tests for email methods: JACS signing, CRUD, search, reply."""

from __future__ import annotations

import hashlib
import json
import time
from typing import Any
from unittest.mock import patch

import pytest

from jacs.hai.client import HaiClient
from jacs.hai.crypt import sign_string, verify_string
from jacs.hai.errors import (
    BodyTooLarge,
    EmailNotActive,
    HaiApiError,
    HaiAuthError,
    RateLimited,
    RecipientNotFound,
    SubjectTooLong,
)
from jacs.hai.models import EmailMessage, SendEmailResult


BASE_URL = "https://test.hai.ai"
JACS_ID = "test-jacs-id-1234"


class _FakeResponse:
    """Minimal fake httpx response for monkeypatching."""

    def __init__(
        self, status_code: int, payload: Any = None, text: str = "",
    ) -> None:
        self.status_code = status_code
        self._payload = payload or {}
        self.text = text or (json.dumps(payload) if payload else "")
        self.headers: dict[str, str] = {}

    def json(self) -> Any:
        return self._payload


# ---------------------------------------------------------------
# send_email — JACS content signing
# ---------------------------------------------------------------


class TestSendEmailJacsSigning:
    """Verify that send_email computes and includes JACS content signature."""

    def test_send_email_includes_jacs_signature_and_timestamp(
        self,
        loaded_config: None,
        ed25519_keypair: tuple,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """send_email payload must contain jacs_signature and jacs_timestamp."""
        captured: dict[str, Any] = {}
        private_key, _ = ed25519_keypair

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            captured["json"] = kwargs.get("json", {})
            captured["headers"] = kwargs.get("headers", {})
            return _FakeResponse(200, {"message_id": "msg-1", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        result = HaiClient().send_email(BASE_URL, "bob@hai.ai", "Hello", "World")

        assert result.message_id == "msg-1"
        assert result.status == "sent"

        payload = captured["json"]
        assert "jacs_signature" in payload
        assert "jacs_timestamp" in payload
        assert isinstance(payload["jacs_timestamp"], int)
        assert isinstance(payload["jacs_signature"], str)
        assert len(payload["jacs_signature"]) > 10  # base64 Ed25519 sig

    def test_send_email_signature_is_verifiable(
        self,
        loaded_config: None,
        ed25519_keypair: tuple,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """The JACS signature must verify against the content hash."""
        captured: dict[str, Any] = {}
        private_key, _ = ed25519_keypair

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, {"message_id": "msg-2", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        subject = "Test Subject"
        body = "Test body content"
        HaiClient().send_email(BASE_URL, "bob@hai.ai", subject, body)

        payload = captured["json"]
        content_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()
        sign_input = f"{content_hash}:{payload['jacs_timestamp']}"

        # Verify signature with the public key
        pub_key = private_key.public_key()
        assert verify_string(pub_key, sign_input, payload["jacs_signature"])

    def test_send_email_timestamp_is_recent(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """jacs_timestamp must be within 5 seconds of current time."""
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, {"message_id": "msg-3", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        before = int(time.time())
        HaiClient().send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")
        after = int(time.time())

        ts = captured["json"]["jacs_timestamp"]
        assert before <= ts <= after

    def test_send_email_passes_in_reply_to(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """in_reply_to must be forwarded to payload."""
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, {"message_id": "msg-4", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        HaiClient().send_email(BASE_URL, "bob@hai.ai", "Re: Hello", "Reply body", in_reply_to="orig-id")

        assert captured["json"]["in_reply_to"] == "orig-id"


# ---------------------------------------------------------------
# send_email — typed errors
# ---------------------------------------------------------------


class TestSendEmailErrors:
    """Verify typed error classes are raised for specific failure modes."""

    def test_email_not_active_on_403_allocated(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(403, text='{"error":"Email status: allocated"}')

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(EmailNotActive):
            HaiClient().send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

    def test_rate_limited_on_429(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(
                429,
                payload={"error": "rate limited", "resets_at": "2026-03-01T00:00:00Z"},
            )

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(RateLimited) as exc_info:
            HaiClient().send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

        assert exc_info.value.resets_at == "2026-03-01T00:00:00Z"

    def test_recipient_not_found_on_400_recipient(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(400, text='{"error":"Recipient not found"}')

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(RecipientNotFound):
            HaiClient().send_email(BASE_URL, "nobody@hai.ai", "Sub", "Body")

    def test_subject_too_long_on_400_subject(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(400, text='{"error":"Subject exceeds maximum length"}')

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(SubjectTooLong):
            HaiClient().send_email(BASE_URL, "bob@hai.ai", "x" * 1000, "Body")

    def test_body_too_large_on_400_body(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(400, text='{"error":"Body exceeds maximum size"}')

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(BodyTooLarge):
            HaiClient().send_email(BASE_URL, "bob@hai.ai", "Sub", "x" * 100_000)

    def test_auth_error_on_401(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(401, text="Unauthorized")

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        with pytest.raises(HaiAuthError):
            HaiClient().send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")


# ---------------------------------------------------------------
# get_message
# ---------------------------------------------------------------


class TestGetMessage:

    def test_get_message_success(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        msg_data = {
            "id": "msg-42",
            "from_address": "alice@hai.ai",
            "to_address": "bob@hai.ai",
            "subject": "Hello",
            "body_text": "World",
            "created_at": "2026-02-24T00:00:00Z",
            "direction": "inbound",
            "message_id": "<msg-42@hai.ai>",
            "in_reply_to": None,
            "is_read": False,
            "delivery_status": "delivered",
            "read_at": None,
            "jacs_verified": True,
        }

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(200, msg_data)

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        result = HaiClient().get_message(BASE_URL, "msg-42")
        assert isinstance(result, EmailMessage)
        assert result.id == "msg-42"
        assert result.from_address == "alice@hai.ai"
        assert result.subject == "Hello"
        assert result.body_text == "World"
        assert result.created_at == "2026-02-24T00:00:00Z"
        assert result.direction == "inbound"
        assert result.delivery_status == "delivered"
        assert result.jacs_verified is True

    def test_get_message_404(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, text="Not found")

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        with pytest.raises(HaiApiError) as exc_info:
            HaiClient().get_message(BASE_URL, "nonexistent")
        assert exc_info.value.status_code == 404


# ---------------------------------------------------------------
# delete_message
# ---------------------------------------------------------------


class TestDeleteMessage:

    def test_delete_message_success(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_delete(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(204)

        import httpx
        monkeypatch.setattr(httpx, "delete", fake_delete)

        result = HaiClient().delete_message(BASE_URL, "msg-42")
        assert result is True

    def test_delete_message_404(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_delete(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(404, text="Not found")

        import httpx
        monkeypatch.setattr(httpx, "delete", fake_delete)

        with pytest.raises(HaiApiError) as exc_info:
            HaiClient().delete_message(BASE_URL, "nonexistent")
        assert exc_info.value.status_code == 404


# ---------------------------------------------------------------
# mark_unread
# ---------------------------------------------------------------


class TestMarkUnread:

    def test_mark_unread_success(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200)

        import httpx
        monkeypatch.setattr(httpx, "post", fake_post)

        result = HaiClient().mark_unread(BASE_URL, "msg-42")
        assert result is True
        assert "/messages/msg-42/unread" in captured["url"]


# ---------------------------------------------------------------
# search_messages
# ---------------------------------------------------------------


class TestSearchMessages:

    def test_search_messages_basic(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        search_results = [
            {
                "id": "msg-1",
                "from_address": "alice@hai.ai",
                "to_address": "bob@hai.ai",
                "subject": "Hello",
                "body_text": "World",
                "created_at": "2026-02-24T00:00:00Z",
                "direction": "inbound",
                "message_id": "<msg-1@hai.ai>",
                "is_read": False,
                "delivery_status": "delivered",
            },
        ]
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, search_results)

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        result = HaiClient().search_messages(
            BASE_URL, q="hello", direction="inbound", limit=10,
        )
        assert len(result) == 1
        assert result[0].id == "msg-1"
        assert captured["params"]["q"] == "hello"
        assert captured["params"]["direction"] == "inbound"
        assert captured["params"]["limit"] == 10

    def test_search_messages_optional_params(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Only set params should be sent."""
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, [])

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        HaiClient().search_messages(BASE_URL, from_address="alice@hai.ai")
        params = captured["params"]
        assert params["from_address"] == "alice@hai.ai"
        assert "q" not in params
        assert "direction" not in params
        assert "to_address" not in params
        assert "since" not in params
        assert "until" not in params


# ---------------------------------------------------------------
# get_unread_count
# ---------------------------------------------------------------


class TestGetUnreadCount:

    def test_get_unread_count_success(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(200, {"count": 5})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        result = HaiClient().get_unread_count(BASE_URL)
        assert result == 5

    def test_get_unread_count_zero(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(200, {"count": 0})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        result = HaiClient().get_unread_count(BASE_URL)
        assert result == 0


# ---------------------------------------------------------------
# reply
# ---------------------------------------------------------------


class TestReply:

    def test_reply_fetches_original_and_sends(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """reply() should GET the original, then POST with Re: subject."""
        call_log: list[tuple[str, str]] = []

        original_msg = {
            "id": "msg-orig",
            "from_address": "alice@hai.ai",
            "to_address": "bob@hai.ai",
            "subject": "Question",
            "body_text": "What is 2+2?",
            "created_at": "2026-02-24T00:00:00Z",
            "direction": "inbound",
            "message_id": "<msg-orig@hai.ai>",
            "is_read": False,
            "delivery_status": "delivered",
        }

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            call_log.append(("GET", url))
            return _FakeResponse(200, original_msg)

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            call_log.append(("POST", url))
            call_log.append(("POST_JSON", kwargs.get("json", {})))
            return _FakeResponse(200, {"message_id": "msg-reply", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)
        monkeypatch.setattr(httpx, "post", fake_post)

        result = HaiClient().reply(BASE_URL, "msg-orig", "The answer is 4.")

        assert result.message_id == "msg-reply"
        # Verify GET was called first
        assert call_log[0][0] == "GET"
        assert "/messages/msg-orig" in call_log[0][1]
        # Verify POST was called with correct payload
        post_payload = call_log[2][1]
        assert post_payload["to"] == "alice@hai.ai"
        assert post_payload["subject"] == "Re: Question"
        assert post_payload["body"] == "The answer is 4."
        assert post_payload["in_reply_to"] == "msg-orig"

    def test_reply_with_custom_subject(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Custom subject should override the default Re: prefix."""
        captured: dict[str, Any] = {}

        original_msg = {
            "id": "msg-orig",
            "from_address": "alice@hai.ai",
            "to_address": "bob@hai.ai",
            "subject": "Question",
            "body_text": "What is 2+2?",
            "created_at": "2026-02-24T00:00:00Z",
            "direction": "inbound",
            "message_id": "<msg-orig@hai.ai>",
            "is_read": False,
            "delivery_status": "delivered",
        }

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            return _FakeResponse(200, original_msg)

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, {"message_id": "msg-reply", "status": "sent"})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)
        monkeypatch.setattr(httpx, "post", fake_post)

        HaiClient().reply(BASE_URL, "msg-orig", "Answer", subject="Custom Subject")
        assert captured["json"]["subject"] == "Custom Subject"


# ---------------------------------------------------------------
# URL construction
# ---------------------------------------------------------------


class TestEmailUrlConstruction:
    """Verify correct URL paths for all email methods."""

    def test_get_message_url(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, {"id": "m1", "from_address": "", "to_address": "", "subject": "", "body_text": "", "created_at": ""})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        HaiClient().get_message(BASE_URL, "msg-42")
        assert captured["url"] == f"{BASE_URL}/api/agents/{JACS_ID}/email/messages/msg-42"

    def test_delete_message_url(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_delete(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(204)

        import httpx
        monkeypatch.setattr(httpx, "delete", fake_delete)

        HaiClient().delete_message(BASE_URL, "msg-42")
        assert captured["url"] == f"{BASE_URL}/api/agents/{JACS_ID}/email/messages/msg-42"

    def test_search_messages_url(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, [])

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        HaiClient().search_messages(BASE_URL)
        assert captured["url"] == f"{BASE_URL}/api/agents/{JACS_ID}/email/search"

    def test_unread_count_url(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["url"] = url
            return _FakeResponse(200, {"count": 0})

        import httpx
        monkeypatch.setattr(httpx, "get", fake_get)

        HaiClient().get_unread_count(BASE_URL)
        assert captured["url"] == f"{BASE_URL}/api/agents/{JACS_ID}/email/unread-count"
