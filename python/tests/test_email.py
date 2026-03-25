"""Tests for email methods: server-side signing, CRUD, search, reply."""

from __future__ import annotations

import base64
import json
import warnings
from typing import Any
from unittest.mock import patch

import pytest

from haiai.client import HaiClient
from haiai.errors import (
    BodyTooLarge,
    EmailNotActive,
    HaiApiError,
    HaiAuthError,
    HaiError,
    RateLimited,
    RecipientNotFound,
    SubjectTooLong,
)
from haiai.models import (
    Contact,
    EmailDeliveryInfo,
    EmailMessage,
    EmailReputationInfo,
    EmailStatus,
    EmailVolumeInfo,
    SendEmailResult,
)

BASE_URL = "https://test.hai.ai"
JACS_ID = "test-jacs-id-1234"
TEST_AGENT_EMAIL = f"{JACS_ID}@hai.ai"

_original_init = HaiClient.__init__


@pytest.fixture(autouse=True)
def _set_agent_email(monkeypatch: pytest.MonkeyPatch) -> None:
    """Ensure every HaiClient created in tests has agent_email set (v2 signing)."""

    def patched_init(self: HaiClient, *args: Any, **kwargs: Any) -> None:
        _original_init(self, *args, **kwargs)
        self._agent_email = TEST_AGENT_EMAIL  # type: ignore[attr-defined]

    monkeypatch.setattr(HaiClient, "__init__", patched_init)


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


class TestSendEmailServerSideSigning:
    """Verify send_email sends content-only payloads (server handles JACS signing)."""

    def test_send_email_no_client_side_signing(
        self,
        loaded_config: None,
    ) -> None:
        """send_email payload must NOT contain jacs_signature or jacs_timestamp."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}

        result = client.send_email(BASE_URL, "bob@hai.ai", "Hello", "World")

        assert result.message_id == "msg-1"
        assert result.status == "sent"

        # Verify FFI was called with correct options
        assert len(mock_ffi.calls) == 1
        method, args, kwargs = mock_ffi.calls[0]
        assert method == "send_email"
        options = args[0]
        assert options["to"] == "bob@hai.ai"
        assert options["subject"] == "Hello"
        assert options["body"] == "World"
        # Server handles JACS signing -- client must NOT send these
        assert "jacs_signature" not in options
        assert "jacs_timestamp" not in options

    def test_send_email_passes_in_reply_to(
        self,
        loaded_config: None,
    ) -> None:
        """in_reply_to must be forwarded to payload."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_email"] = {"message_id": "msg-4", "status": "sent"}

        client.send_email(BASE_URL, "bob@hai.ai", "Re: Hello", "Reply body", in_reply_to="orig-id")

        assert len(mock_ffi.calls) == 1
        options = mock_ffi.calls[0][1][0]
        assert options["in_reply_to"] == "orig-id"

    def test_send_email_with_attachments_includes_attachment_data(
        self,
        loaded_config: None,
    ) -> None:
        """send_email with attachments must include attachments array in payload."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_email"] = {"message_id": "msg-att", "status": "sent"}

        attachments = [
            {
                "filename": "hello.txt",
                "content_type": "text/plain",
                "data": b"Hello, world!",
            },
            {
                "filename": "image.png",
                "content_type": "image/png",
                "data": b"\x89PNG\r\n\x1a\nfakedata",
            },
        ]
        client.send_email(
            BASE_URL, "bob@hai.ai", "With attachments", "Body", attachments=attachments,
        )

        options = mock_ffi.calls[0][1][0]
        assert "attachments" in options
        assert len(options["attachments"]) == 2

        att0 = options["attachments"][0]
        assert "data_base64" in att0
        assert att0["filename"] == "hello.txt"
        assert att0["content_type"] == "text/plain"
        # Verify base64 round-trips correctly
        assert base64.b64decode(att0["data_base64"]) == b"Hello, world!"

        att1 = options["attachments"][1]
        assert att1["filename"] == "image.png"
        assert att1["content_type"] == "image/png"
        # No client-side signing fields
        assert "jacs_signature" not in options
        assert "jacs_timestamp" not in options

    def test_send_email_no_agent_email_raises(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """send_email without agent_email must raise HaiError."""
        # Create a client without the autouse fixture's email override
        client = HaiClient.__new__(HaiClient)
        _original_init(client)
        # Explicitly clear agent_email
        client._agent_email = None  # type: ignore[attr-defined]

        with pytest.raises(HaiError, match="agent email not set"):
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")


# ---------------------------------------------------------------
# send_email — typed errors
# ---------------------------------------------------------------


class TestSendEmailErrors:
    """Verify typed error classes are raised for specific failure modes."""

    def test_email_not_active_on_403_allocated(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise EmailNotActive("Email status: allocated", status_code=403)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(EmailNotActive):
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

    def test_rate_limited_on_429(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise RateLimited("rate limited", status_code=429, resets_at="2026-03-01T00:00:00Z")

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(RateLimited) as exc_info:
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

        assert exc_info.value.resets_at == "2026-03-01T00:00:00Z"

    def test_recipient_not_found_on_400_recipient(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise RecipientNotFound("Recipient not found", status_code=400)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(RecipientNotFound):
            client.send_email(BASE_URL, "nobody@hai.ai", "Sub", "Body")

    def test_subject_too_long_on_400_subject(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise SubjectTooLong("Subject exceeds maximum length", status_code=400)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(SubjectTooLong):
            client.send_email(BASE_URL, "bob@hai.ai", "x" * 1000, "Body")

    def test_body_too_large_on_400_body(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise BodyTooLarge("Body exceeds maximum size", status_code=400)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(BodyTooLarge):
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "x" * 100_000)

    def test_auth_error_on_401(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        def raise_on_send(options: Any) -> Any:
            raise HaiAuthError("Unauthorized", status_code=401)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(HaiAuthError):
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")


# ---------------------------------------------------------------
# get_message
# ---------------------------------------------------------------


class TestGetMessage:

    def test_get_message_success(
        self,
        loaded_config: None,
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

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = msg_data

        result = client.get_message(BASE_URL, "msg-42")
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
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_not_found(message_id: str) -> Any:
            raise HaiApiError("message not found", status_code=404)

        mock_ffi.responses["get_message"] = raise_not_found

        with pytest.raises(HaiApiError) as exc_info:
            client.get_message(BASE_URL, "nonexistent")
        assert exc_info.value.status_code == 404


# ---------------------------------------------------------------
# delete_message
# ---------------------------------------------------------------


class TestDeleteMessage:

    def test_delete_message_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        # delete_message returns None (void), so no response needed

        result = client.delete_message(BASE_URL, "msg-42")
        assert result is True

    def test_delete_message_404(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_not_found(message_id: str) -> Any:
            raise HaiApiError("message not found", status_code=404)

        mock_ffi.responses["delete_message"] = raise_not_found

        with pytest.raises(HaiApiError) as exc_info:
            client.delete_message(BASE_URL, "nonexistent")
        assert exc_info.value.status_code == 404


# ---------------------------------------------------------------
# mark_unread
# ---------------------------------------------------------------


class TestMarkUnread:

    def test_mark_unread_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        result = client.mark_unread(BASE_URL, "msg-42")
        assert result is True
        # Verify FFI was called with the message_id
        assert len(mock_ffi.calls) == 1
        assert mock_ffi.calls[0][0] == "mark_unread"
        assert mock_ffi.calls[0][1][0] == "msg-42"


# ---------------------------------------------------------------
# search_messages
# ---------------------------------------------------------------


class TestSearchMessages:

    def test_search_messages_basic(
        self,
        loaded_config: None,
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

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = search_results

        result = client.search_messages(
            BASE_URL, q="hello", direction="inbound", limit=10,
        )
        assert len(result) == 1
        assert result[0].id == "msg-1"

        # Verify the options passed to FFI
        options = mock_ffi.calls[0][1][0]
        assert options["q"] == "hello"
        assert options["direction"] == "inbound"
        assert options["limit"] == 10

    def test_search_messages_optional_params(
        self,
        loaded_config: None,
    ) -> None:
        """Only set params should be sent."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = []

        client.search_messages(BASE_URL, from_address="alice@hai.ai")

        options = mock_ffi.calls[0][1][0]
        assert options["from_address"] == "alice@hai.ai"
        assert "q" not in options
        assert "direction" not in options
        assert "to_address" not in options
        assert "since" not in options
        assert "until" not in options


# ---------------------------------------------------------------
# get_unread_count
# ---------------------------------------------------------------


class TestGetUnreadCount:

    def test_get_unread_count_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_unread_count"] = 5

        result = client.get_unread_count(BASE_URL)
        assert result == 5

    def test_get_unread_count_zero(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_unread_count"] = 0

        result = client.get_unread_count(BASE_URL)
        assert result == 0


# ---------------------------------------------------------------
# reply
# ---------------------------------------------------------------


class TestReply:

    def test_reply_fetches_original_and_sends(
        self,
        loaded_config: None,
    ) -> None:
        """reply() should GET the original, then POST with Re: subject."""
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

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = original_msg
        mock_ffi.responses["send_signed_email"] = {"message_id": "msg-reply", "status": "sent"}

        result = client.reply(BASE_URL, "msg-orig", "The answer is 4.")

        assert result.message_id == "msg-reply"
        # Verify get_message was called first, then send_signed_email
        assert mock_ffi.calls[0][0] == "get_message"
        assert mock_ffi.calls[0][1][0] == "msg-orig"
        assert mock_ffi.calls[1][0] == "send_signed_email"
        send_options = mock_ffi.calls[1][1][0]
        assert send_options["to"] == "alice@hai.ai"
        assert send_options["subject"] == "Re: Question"
        assert send_options["body"] == "The answer is 4."
        assert send_options["in_reply_to"] == "<msg-orig@hai.ai>"

    def test_reply_with_custom_subject(
        self,
        loaded_config: None,
    ) -> None:
        """Custom subject should override the default Re: prefix."""
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

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = original_msg
        mock_ffi.responses["send_signed_email"] = {"message_id": "msg-reply", "status": "sent"}

        client.reply(BASE_URL, "msg-orig", "Answer", subject="Custom Subject")
        send_options = mock_ffi.calls[1][1][0]
        assert send_options["subject"] == "Custom Subject"


# ---------------------------------------------------------------
# URL construction -- now tests verify FFI method args instead
# ---------------------------------------------------------------


class TestEmailUrlConstruction:
    """Verify correct FFI method calls for all email methods."""

    def test_get_message_calls_ffi(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = {
            "id": "m1", "from_address": "", "to_address": "",
            "subject": "", "body_text": "", "created_at": "",
        }

        client.get_message(BASE_URL, "msg-42")
        assert mock_ffi.calls[0][0] == "get_message"
        assert mock_ffi.calls[0][1][0] == "msg-42"

    def test_delete_message_calls_ffi(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        client.delete_message(BASE_URL, "msg-42")
        assert mock_ffi.calls[0][0] == "delete_message"
        assert mock_ffi.calls[0][1][0] == "msg-42"

    def test_search_messages_calls_ffi(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = []

        client.search_messages(BASE_URL)
        assert mock_ffi.calls[0][0] == "search_messages"

    def test_unread_count_calls_ffi(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_unread_count"] = 0

        client.get_unread_count(BASE_URL)
        assert mock_ffi.calls[0][0] == "get_unread_count"


# ---------------------------------------------------------------
# reply — threading uses message_id not id
# ---------------------------------------------------------------


class TestReplyThreading:
    """Verify reply() uses original.message_id (RFC 5322) for in_reply_to."""

    def test_reply_uses_message_id_not_id_for_threading(
        self,
        loaded_config: None,
    ) -> None:
        """reply() must set in_reply_to to original.message_id, NOT original.id."""
        original_msg = {
            "id": "db-uuid-123",
            "from_address": "alice@hai.ai",
            "to_address": "bob@hai.ai",
            "subject": "Hello",
            "body_text": "Hi there",
            "created_at": "2026-02-24T00:00:00Z",
            "direction": "inbound",
            "message_id": "<db-uuid-123.bot@hai.ai>",
            "is_read": False,
            "delivery_status": "delivered",
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = original_msg
        mock_ffi.responses["send_signed_email"] = {"message_id": "msg-reply", "status": "sent"}

        client.reply(BASE_URL, "db-uuid-123", "Thanks!")

        send_options = mock_ffi.calls[1][1][0]
        # Must use the RFC 5322 message_id, NOT the database id
        assert send_options["in_reply_to"] == "<db-uuid-123.bot@hai.ai>"
        assert send_options["in_reply_to"] != "db-uuid-123"

    def test_reply_omits_in_reply_to_when_message_id_is_none(
        self,
        loaded_config: None,
    ) -> None:
        """When original.message_id is None, in_reply_to should not be in payload."""
        original_msg = {
            "id": "db-uuid-456",
            "from_address": "alice@hai.ai",
            "to_address": "bob@hai.ai",
            "subject": "Hello",
            "body_text": "Hi there",
            "created_at": "2026-02-24T00:00:00Z",
            "direction": "inbound",
            "message_id": None,
            "is_read": False,
            "delivery_status": "delivered",
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = original_msg
        mock_ffi.responses["send_signed_email"] = {"message_id": "msg-reply", "status": "sent"}

        client.reply(BASE_URL, "db-uuid-456", "Thanks!")

        send_options = mock_ffi.calls[1][1][0]
        # message_id was None -> send_signed_email skips in_reply_to entirely
        assert "in_reply_to" not in send_options


# ---------------------------------------------------------------
# send_email — error_code-based typed errors
# ---------------------------------------------------------------


class TestSendEmailErrorCodeParsing:
    """Verify that send_email maps error_code to typed exception classes."""

    def test_email_not_active_from_error_code(
        self,
        loaded_config: None,
    ) -> None:
        """error_code=EMAIL_NOT_ACTIVE on 403 must raise EmailNotActive."""
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_send(options: Any) -> Any:
            raise EmailNotActive("Agent email is allocated and cannot send messages", status_code=403)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(EmailNotActive) as exc_info:
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

        assert exc_info.value.status_code == 403

    def test_recipient_not_found_from_error_code(
        self,
        loaded_config: None,
    ) -> None:
        """error_code=RECIPIENT_NOT_FOUND on 400 must raise RecipientNotFound."""
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_send(options: Any) -> Any:
            raise RecipientNotFound("Invalid recipient", status_code=400)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(RecipientNotFound) as exc_info:
            client.send_email(BASE_URL, "nobody@hai.ai", "Sub", "Body")

        assert exc_info.value.status_code == 400

    def test_rate_limited_from_error_code(
        self,
        loaded_config: None,
    ) -> None:
        """error_code=RATE_LIMITED on 429 must raise RateLimited."""
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_send(options: Any) -> Any:
            raise RateLimited("Daily limit reached", status_code=429)

        mock_ffi.responses["send_email"] = raise_on_send

        with pytest.raises(RateLimited) as exc_info:
            client.send_email(BASE_URL, "bob@hai.ai", "Sub", "Body")

        assert exc_info.value.status_code == 429


# ---------------------------------------------------------------
# HaiError.from_response — error_code capture
# ---------------------------------------------------------------


class TestHaiErrorFromResponseErrorCode:
    """Verify HaiError.from_response captures the error_code field."""

    def test_from_response_captures_error_code(self) -> None:
        """error_code from JSON body must be stored on the exception."""
        from haiai.errors import HaiError

        fake_resp = _FakeResponse(
            403,
            payload={"error": "test failure", "error_code": "EMAIL_NOT_ACTIVE"},
        )

        err = HaiError.from_response(fake_resp)
        assert err.error_code == "EMAIL_NOT_ACTIVE"
        assert err.status_code == 403
        assert "test failure" in str(err)

    def test_from_response_defaults_error_code_to_empty(self) -> None:
        """When error_code is absent, it defaults to empty string."""
        from haiai.errors import HaiError

        fake_resp = _FakeResponse(500, payload={"error": "internal"})

        err = HaiError.from_response(fake_resp)
        assert err.error_code == ""


# ---------------------------------------------------------------
# send_signed_email
# ---------------------------------------------------------------


class TestSendSignedEmail:
    """Verify send_signed_email delegates to send_email (deprecated)."""

    def test_send_signed_email_delegates_to_send_email(
        self,
        loaded_config: None,
    ) -> None:
        """send_signed_email calls FFI send_signed_email directly."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_signed_email"] = {"message_id": "msg-signed-1", "status": "sent"}

        result = client.send_signed_email(
            BASE_URL, "bob@hai.ai", "Hello Signed", "Signed body",
        )

        assert result.message_id == "msg-signed-1"
        assert result.status == "sent"
        # Calls FFI send_signed_email directly
        assert mock_ffi.calls[0][0] == "send_signed_email"
        options = mock_ffi.calls[0][1][0]
        assert options["to"] == "bob@hai.ai"
        assert options["subject"] == "Hello Signed"

    def test_send_signed_email_fails_without_agent_email(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """send_signed_email should raise when agent_email is not set."""
        client = HaiClient()
        client._agent_email = None  # type: ignore[attr-defined]

        with pytest.raises(HaiError, match="agent email not set"):
            client.send_signed_email(BASE_URL, "bob@hai.ai", "Hello", "World")


# ---------------------------------------------------------------
# send_email — CC/BCC/Labels
# ---------------------------------------------------------------


class TestSendEmailCcBccLabels:
    """Verify send_email passes cc, bcc, labels to payload."""

    def test_send_email_with_cc_bcc_labels(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_email"] = {"message_id": "msg-cc", "status": "sent"}

        client.send_email(
            BASE_URL, "bob@hai.ai", "Hello", "World",
            cc=["carol@hai.ai", "dave@hai.ai"],
            bcc=["eve@hai.ai"],
            labels=["important", "follow-up"],
        )

        options = mock_ffi.calls[0][1][0]
        assert options["cc"] == ["carol@hai.ai", "dave@hai.ai"]
        assert options["bcc"] == ["eve@hai.ai"]
        assert options["labels"] == ["important", "follow-up"]

    def test_send_email_omits_empty_cc_bcc_labels(
        self,
        loaded_config: None,
    ) -> None:
        """When cc/bcc/labels are None, they should not appear in payload."""
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["send_email"] = {"message_id": "msg-plain", "status": "sent"}

        client.send_email(BASE_URL, "bob@hai.ai", "Hello", "World")

        options = mock_ffi.calls[0][1][0]
        assert "cc" not in options
        assert "bcc" not in options
        assert "labels" not in options


# ---------------------------------------------------------------
# list_messages — is_read/folder/label filters
# ---------------------------------------------------------------


class TestListMessagesFilters:
    """Verify list_messages passes new filter params."""

    def test_list_messages_with_is_read_filter(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = []

        client.list_messages(BASE_URL, is_read=False)
        options = mock_ffi.calls[0][1][0]
        assert options["is_read"] is False

    def test_list_messages_with_folder_and_label(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = []

        client.list_messages(BASE_URL, folder="archive", label="important")
        options = mock_ffi.calls[0][1][0]
        assert options["folder"] == "archive"
        assert options["label"] == "important"

    def test_list_messages_omits_none_filters(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = []

        client.list_messages(BASE_URL)
        options = mock_ffi.calls[0][1][0]
        assert "is_read" not in options
        assert "folder" not in options
        assert "label" not in options


# ---------------------------------------------------------------
# search_messages — new filters
# ---------------------------------------------------------------


class TestSearchMessagesNewFilters:
    """Verify search_messages passes is_read, jacs_verified, folder, label."""

    def test_search_with_all_new_filters(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = []

        client.search_messages(
            BASE_URL,
            is_read=True,
            jacs_verified=True,
            folder="inbox",
            label="urgent",
        )
        options = mock_ffi.calls[0][1][0]
        assert options["is_read"] is True
        assert options["jacs_verified"] is True
        assert options["folder"] == "inbox"
        assert options["label"] == "urgent"

    def test_search_omits_none_filters(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = []

        client.search_messages(BASE_URL, q="hello")
        options = mock_ffi.calls[0][1][0]
        assert "is_read" not in options
        assert "jacs_verified" not in options
        assert "folder" not in options
        assert "label" not in options


# ---------------------------------------------------------------
# EmailMessage model — cc_addresses, labels, folder fields
# ---------------------------------------------------------------


class TestEmailMessageNewFields:
    """Verify EmailMessage model fields for cc_addresses, labels, folder."""

    def test_email_message_defaults(self) -> None:
        msg = EmailMessage(
            id="m1", from_address="a@hai.ai", to_address="b@hai.ai",
            subject="Hi", body_text="Body", created_at="2026-01-01T00:00:00Z",
        )
        assert msg.cc_addresses == []
        assert msg.labels == []
        assert msg.folder == "inbox"

    def test_email_message_with_new_fields(self) -> None:
        msg = EmailMessage(
            id="m1", from_address="a@hai.ai", to_address="b@hai.ai",
            subject="Hi", body_text="Body", created_at="2026-01-01T00:00:00Z",
            cc_addresses=["c@hai.ai"], labels=["urgent"], folder="archive",
        )
        assert msg.cc_addresses == ["c@hai.ai"]
        assert msg.labels == ["urgent"]
        assert msg.folder == "archive"

    def test_list_messages_parses_new_fields(
        self,
        loaded_config: None,
    ) -> None:
        """list_messages must populate cc_addresses, labels, folder from response."""
        msg_data = [{
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Test", "body_text": "Body", "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
            "cc_addresses": ["c@hai.ai", "d@hai.ai"],
            "labels": ["important"],
            "folder": "archive",
        }]

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = msg_data

        result = client.list_messages(BASE_URL)
        assert len(result) == 1
        assert result[0].cc_addresses == ["c@hai.ai", "d@hai.ai"]
        assert result[0].labels == ["important"]
        assert result[0].folder == "archive"


# ---------------------------------------------------------------
# forward
# ---------------------------------------------------------------


class TestForward:
    """Verify forward() sends correct payload."""

    def test_forward_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["forward"] = {"message_id": "msg-fwd", "status": "sent"}

        result = client.forward(BASE_URL, "msg-42", "bob@hai.ai", comment="FYI")
        assert result.message_id == "msg-fwd"
        assert result.status == "sent"
        assert mock_ffi.calls[0][0] == "forward"
        params = mock_ffi.calls[0][1][0]
        assert params["message_id"] == "msg-42"
        assert params["to"] == "bob@hai.ai"
        assert params["comment"] == "FYI"

    def test_forward_without_comment(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["forward"] = {"message_id": "msg-fwd2", "status": "sent"}

        client.forward(BASE_URL, "msg-42", "bob@hai.ai")
        params = mock_ffi.calls[0][1][0]
        assert "comment" not in params

    def test_forward_auth_error(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_forward(params: Any) -> Any:
            raise HaiAuthError("Forbidden", status_code=403)

        mock_ffi.responses["forward"] = raise_on_forward

        with pytest.raises(HaiAuthError):
            client.forward(BASE_URL, "msg-42", "bob@hai.ai")


# ---------------------------------------------------------------
# archive / unarchive
# ---------------------------------------------------------------


class TestArchiveUnarchive:
    """Verify archive and unarchive methods."""

    def test_archive_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        result = client.archive(BASE_URL, "msg-42")
        assert result is True
        assert mock_ffi.calls[0][0] == "archive"
        assert mock_ffi.calls[0][1][0] == "msg-42"

    def test_unarchive_success(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        result = client.unarchive(BASE_URL, "msg-42")
        assert result is True
        assert mock_ffi.calls[0][0] == "unarchive"
        assert mock_ffi.calls[0][1][0] == "msg-42"

    def test_archive_auth_error(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_archive(message_id: str) -> Any:
            raise HaiAuthError("Unauthorized", status_code=401)

        mock_ffi.responses["archive"] = raise_on_archive

        with pytest.raises(HaiAuthError):
            client.archive(BASE_URL, "msg-42")


# ---------------------------------------------------------------
# contacts
# ---------------------------------------------------------------


class TestContacts:
    """Verify contacts() method."""

    def test_contacts_success(
        self,
        loaded_config: None,
    ) -> None:
        contacts_data = {
            "contacts": [
                {
                    "email": "alice@hai.ai",
                    "display_name": "Alice",
                    "last_contact": "2026-03-01T00:00:00Z",
                    "jacs_verified": True,
                    "reputation_tier": "good",
                },
                {
                    "email": "bob@hai.ai",
                    "jacs_verified": False,
                },
            ]
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["contacts"] = contacts_data

        result = client.contacts(BASE_URL)
        assert len(result) == 2
        assert result[0].email == "alice@hai.ai"
        assert result[0].display_name == "Alice"
        assert result[0].jacs_verified is True
        assert result[0].reputation_tier == "good"
        assert result[1].email == "bob@hai.ai"
        assert result[1].jacs_verified is False
        assert mock_ffi.calls[0][0] == "contacts"

    def test_contacts_bare_array_response(
        self,
        loaded_config: None,
    ) -> None:
        """contacts() should handle bare array response (no wrapper)."""
        contacts_data = [
            {"email": "alice@hai.ai", "jacs_verified": True},
        ]

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["contacts"] = contacts_data

        result = client.contacts(BASE_URL)
        assert len(result) == 1
        assert result[0].email == "alice@hai.ai"

    def test_contacts_auth_error(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()


        def raise_on_contacts() -> Any:
            raise HaiAuthError("Forbidden", status_code=403)

        mock_ffi.responses["contacts"] = raise_on_contacts

        with pytest.raises(HaiAuthError):
            client.contacts(BASE_URL)


# ---------------------------------------------------------------
# Contact model
# ---------------------------------------------------------------


class TestEmailMessageReplyTextFields:
    """Verify body_text_clean, quoted_text, and thread fields on EmailMessage."""

    def test_defaults_are_none(self) -> None:
        """New optional fields default to None when not supplied."""
        msg = EmailMessage(
            id="m1", from_address="a@hai.ai", to_address="b@hai.ai",
            subject="Hi", body_text="Body", created_at="2026-01-01T00:00:00Z",
        )
        assert msg.body_text_clean is None
        assert msg.quoted_text is None
        assert msg.thread is None

    def test_construction_with_new_fields(self) -> None:
        """EmailMessage accepts body_text_clean, quoted_text, and thread."""
        child = EmailMessage(
            id="m0", from_address="b@hai.ai", to_address="a@hai.ai",
            subject="Re: Hi", body_text="Previous msg", created_at="2026-01-01T00:00:00Z",
            body_text_clean="Previous msg", quoted_text=None,
        )
        msg = EmailMessage(
            id="m1", from_address="a@hai.ai", to_address="b@hai.ai",
            subject="Re: Hi", body_text="New text\n\n> Previous msg",
            created_at="2026-01-01T01:00:00Z",
            body_text_clean="New text",
            quoted_text="Previous msg",
            thread=[child],
        )
        assert msg.body_text_clean == "New text"
        assert msg.quoted_text == "Previous msg"
        assert msg.thread is not None
        assert len(msg.thread) == 1
        assert msg.thread[0].id == "m0"

    def test_list_messages_parses_reply_text_fields(
        self,
        loaded_config: None,
    ) -> None:
        """list_messages must populate body_text_clean and quoted_text."""
        msg_data = [{
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Re: Hi", "body_text": "New\n\n> Old",
            "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
            "body_text_clean": "New",
            "quoted_text": "Old",
        }]

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = msg_data

        result = client.list_messages(BASE_URL)
        assert len(result) == 1
        assert result[0].body_text_clean == "New"
        assert result[0].quoted_text == "Old"

    def test_list_messages_missing_reply_fields_default_none(
        self,
        loaded_config: None,
    ) -> None:
        """When API omits body_text_clean/quoted_text they should be None."""
        msg_data = [{
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Hi", "body_text": "No quoting here",
            "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
        }]

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["list_messages"] = msg_data

        result = client.list_messages(BASE_URL)
        assert result[0].body_text_clean is None
        assert result[0].quoted_text is None

    def test_get_message_parses_reply_text_and_thread(
        self,
        loaded_config: None,
    ) -> None:
        """get_message must populate body_text_clean, quoted_text, and thread."""
        msg_data = {
            "id": "m2", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Re: Hi", "body_text": "Reply\n\n> Original",
            "created_at": "2026-01-01T01:00:00Z",
            "direction": "inbound", "message_id": "<m2@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
            "body_text_clean": "Reply",
            "quoted_text": "Original",
            "thread": [
                {
                    "id": "m1", "from_address": "b@hai.ai", "to_address": "a@hai.ai",
                    "subject": "Hi", "body_text": "Original",
                    "created_at": "2026-01-01T00:00:00Z",
                    "direction": "outbound", "message_id": "<m1@hai.ai>",
                    "is_read": True, "delivery_status": "delivered",
                    "body_text_clean": "Original",
                },
            ],
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = msg_data

        result = client.get_message(BASE_URL, "m2")
        assert isinstance(result, EmailMessage)
        assert result.body_text_clean == "Reply"
        assert result.quoted_text == "Original"
        assert result.thread is not None
        assert len(result.thread) == 1
        assert result.thread[0].id == "m1"
        assert result.thread[0].body_text_clean == "Original"
        assert result.thread[0].quoted_text is None

    def test_get_message_no_thread_defaults_none(
        self,
        loaded_config: None,
    ) -> None:
        """get_message with no thread key should leave thread as None."""
        msg_data = {
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Hi", "body_text": "Body",
            "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = msg_data

        result = client.get_message(BASE_URL, "m1")
        assert result.thread is None

    def test_get_message_empty_thread(
        self,
        loaded_config: None,
    ) -> None:
        """get_message with empty thread list should return empty list."""
        msg_data = {
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Hi", "body_text": "Body",
            "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
            "thread": [],
        }

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["get_message"] = msg_data

        result = client.get_message(BASE_URL, "m1")
        assert result.thread == []

    def test_search_messages_parses_reply_text_fields(
        self,
        loaded_config: None,
    ) -> None:
        """search_messages must populate body_text_clean and quoted_text."""
        msg_data = [{
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Re: Hi", "body_text": "New\n\n> Old",
            "created_at": "2026-01-01T00:00:00Z",
            "direction": "inbound", "message_id": "<m1@hai.ai>",
            "is_read": False, "delivery_status": "delivered",
            "body_text_clean": "New",
            "quoted_text": "Old",
        }]

        client = HaiClient()
        mock_ffi = client._get_ffi()

        mock_ffi.responses["search_messages"] = msg_data

        result = client.search_messages(BASE_URL, q="Hi")
        assert len(result) == 1
        assert result[0].body_text_clean == "New"
        assert result[0].quoted_text == "Old"


class TestContactModel:
    """Verify Contact dataclass."""

    def test_contact_defaults(self) -> None:
        c = Contact(email="test@hai.ai")
        assert c.display_name is None
        assert c.last_contact == ""
        assert c.jacs_verified is False
        assert c.reputation_tier is None

    def test_contact_with_all_fields(self) -> None:
        c = Contact(
            email="test@hai.ai",
            display_name="Test Agent",
            last_contact="2026-03-01T00:00:00Z",
            jacs_verified=True,
            reputation_tier="excellent",
        )
        assert c.display_name == "Test Agent"
        assert c.jacs_verified is True
        assert c.reputation_tier == "excellent"


class TestEmailStatusNestedFields:
    """Tests for EmailStatus volume, delivery, and reputation nested fields."""

    def test_email_status_with_nested_fields(self) -> None:
        status = EmailStatus(
            email="bot@hai.ai",
            status="active",
            tier="established",
            billing_tier="pro",
            messages_sent_24h=10,
            daily_limit=100,
            daily_used=10,
            resets_at="2026-03-15T00:00:00Z",
            messages_sent_total=500,
            external_enabled=True,
            external_sends_today=3,
            last_tier_change="2026-01-01T00:00:00Z",
            volume=EmailVolumeInfo(sent_total=500, received_total=300, sent_24h=10),
            delivery=EmailDeliveryInfo(bounce_count=2, spam_report_count=1, delivery_rate=0.98),
            reputation=EmailReputationInfo(score=85.5, tier="established", email_score=90.0, hai_score=80.0),
        )
        assert status.volume is not None
        assert status.volume.sent_total == 500
        assert status.volume.received_total == 300
        assert status.volume.sent_24h == 10

        assert status.delivery is not None
        assert status.delivery.bounce_count == 2
        assert status.delivery.spam_report_count == 1
        assert status.delivery.delivery_rate == 0.98

        assert status.reputation is not None
        assert status.reputation.score == 85.5
        assert status.reputation.tier == "established"
        assert status.reputation.email_score == 90.0
        assert status.reputation.hai_score == 80.0

    def test_email_status_nested_fields_default_to_none(self) -> None:
        status = EmailStatus(
            email="bot@hai.ai",
            status="active",
            tier="new",
            billing_tier="free",
            messages_sent_24h=0,
            daily_limit=10,
            daily_used=0,
            resets_at="2026-03-15T00:00:00Z",
        )
        assert status.volume is None
        assert status.delivery is None
        assert status.reputation is None

    def test_email_status_nested_fields_from_dict(self) -> None:
        """Test parsing nested fields from a JSON-like dict (simulates API response)."""
        from haiai.client import HaiClient

        data = {
            "email": "bot@hai.ai",
            "status": "active",
            "tier": "established",
            "billing_tier": "pro",
            "messages_sent_24h": 10,
            "daily_limit": 100,
            "daily_used": 10,
            "resets_at": "2026-03-15T00:00:00Z",
            "messages_sent_total": 500,
            "external_enabled": True,
            "external_sends_today": 3,
            "last_tier_change": "2026-01-01T00:00:00Z",
            "volume": {
                "sent_total": 500,
                "received_total": 300,
                "sent_24h": 10,
            },
            "delivery": {
                "bounce_count": 2,
                "spam_report_count": 1,
                "delivery_rate": 0.98,
            },
            "reputation": {
                "score": 85.5,
                "tier": "established",
                "email_score": 90.0,
                "hai_score": 80.0,
            },
        }

        status = HaiClient._parse_email_status(data)

        assert status.volume is not None
        assert status.volume.sent_total == 500
        assert status.volume.received_total == 300

        assert status.delivery is not None
        assert status.delivery.bounce_count == 2
        assert status.delivery.delivery_rate == 0.98

        assert status.reputation is not None
        assert status.reputation.score == 85.5
        assert status.reputation.hai_score == 80.0

    def test_email_status_parse_without_nested_fields(self) -> None:
        """Test that parsing works when nested fields are absent."""
        from haiai.client import HaiClient

        data = {
            "email": "bot@hai.ai",
            "status": "active",
            "tier": "new",
            "billing_tier": "free",
            "messages_sent_24h": 0,
            "daily_limit": 10,
            "daily_used": 0,
            "resets_at": "2026-03-15T00:00:00Z",
        }

        status = HaiClient._parse_email_status(data)

        assert status.volume is None
        assert status.delivery is None
        assert status.reputation is None

    def test_reputation_hai_score_null(self) -> None:
        """Test that hai_score=null maps to None."""
        from haiai.client import HaiClient

        data = {
            "email": "bot@hai.ai",
            "status": "active",
            "tier": "new",
            "billing_tier": "free",
            "messages_sent_24h": 0,
            "daily_limit": 10,
            "daily_used": 0,
            "resets_at": "2026-03-15T00:00:00Z",
            "reputation": {
                "score": 50.0,
                "tier": "new",
                "email_score": 50.0,
                "hai_score": None,
            },
        }

        status = HaiClient._parse_email_status(data)

        assert status.reputation is not None
        assert status.reputation.hai_score is None
