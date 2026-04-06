"""Tests for ISSUE_001: Required parameters must raise ValueError when empty.

The H8 fix made hai_url optional but also gave empty-string defaults to
required params (to, subject, body, message_id, agent_id, etc.). Validation
guards now raise ValueError for these params when they are empty.
"""

from __future__ import annotations

from typing import Any

import pytest

from haiai.client import HaiClient


JACS_ID = "test-jacs-id-1234"
TEST_AGENT_EMAIL = f"{JACS_ID}@hai.ai"

_original_init = HaiClient.__init__


@pytest.fixture(autouse=True)
def _set_agent_email(monkeypatch: pytest.MonkeyPatch) -> None:
    """Ensure every HaiClient created in tests has agent_email set."""

    def patched_init(self: HaiClient, *args: Any, **kwargs: Any) -> None:
        _original_init(self, *args, **kwargs)
        self._agent_email = TEST_AGENT_EMAIL

    monkeypatch.setattr(HaiClient, "__init__", patched_init)


# ---------------------------------------------------------------
# Email CRUD validation
# ---------------------------------------------------------------


class TestSendEmailValidation:
    """send_email() must reject empty required params."""

    def test_send_email_empty_to_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'to' is required"):
            client.send_email(to="", subject="Hi", body="Hello")

    def test_send_email_empty_subject_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'subject' is required"):
            client.send_email(to="bob@hai.ai", subject="", body="Hello")

    def test_send_email_empty_body_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'body' is required"):
            client.send_email(to="bob@hai.ai", subject="Hi", body="")

    def test_send_email_no_args_raises(self, loaded_config: None) -> None:
        """Calling send_email() with no args raises ValueError (not silent FFI call)."""
        client = HaiClient()
        with pytest.raises(ValueError):
            client.send_email()


class TestSendSignedEmailValidation:
    """send_signed_email() must reject empty required params."""

    def test_empty_to_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'to' is required"):
            client.send_signed_email(to="", subject="Hi", body="Hello")

    def test_empty_subject_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'subject' is required"):
            client.send_signed_email(to="bob@hai.ai", subject="", body="Hello")

    def test_empty_body_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'body' is required"):
            client.send_signed_email(to="bob@hai.ai", subject="Hi", body="")


class TestSignEmailValidation:
    """sign_email() must reject empty raw_email."""

    def test_empty_raw_email_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'raw_email' is required"):
            client.sign_email(raw_email=b"")


class TestVerifyEmailValidation:
    """verify_email() must reject empty raw_email."""

    def test_empty_raw_email_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'raw_email' is required"):
            client.verify_email(raw_email=b"")


class TestMessageIdValidation:
    """Methods requiring message_id must reject empty strings."""

    def test_mark_read_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.mark_read(message_id="")

    def test_get_message_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.get_message(message_id="")

    def test_delete_message_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.delete_message(message_id="")

    def test_mark_unread_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.mark_unread(message_id="")

    def test_archive_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.archive(message_id="")

    def test_unarchive_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.unarchive(message_id="")

    def test_update_labels_empty_message_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.update_labels(message_id="")

    def test_mark_read_no_args_raises(self, loaded_config: None) -> None:
        """Calling mark_read() with no args raises ValueError."""
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.mark_read()


class TestReplyValidation:
    """reply() must reject empty message_id and body."""

    def test_empty_message_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.reply(message_id="", body="Hello")

    def test_empty_body_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'body' is required"):
            client.reply(message_id="msg-1", body="")


class TestForwardValidation:
    """forward() must reject empty message_id and to."""

    def test_empty_message_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'message_id' is required"):
            client.forward(message_id="", to="bob@hai.ai")

    def test_empty_to_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'to' is required"):
            client.forward(message_id="msg-1", to="")


# ---------------------------------------------------------------
# Username & identity validation
# ---------------------------------------------------------------


class TestUsernameValidation:
    """Username methods must reject empty agent_id/username."""

    def test_update_username_empty_agent_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'agent_id' is required"):
            client.update_username(agent_id="", username="newname")

    def test_update_username_empty_username(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'username' is required"):
            client.update_username(agent_id="agent-1", username="")

    def test_delete_username_empty_agent_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'agent_id' is required"):
            client.delete_username(agent_id="")


class TestVerificationValidation:
    """get_verification() must reject empty agent_id."""

    def test_empty_agent_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'agent_id' is required"):
            client.get_verification(agent_id="")


class TestBenchmarkResponseValidation:
    """submit_benchmark_response() must reject empty job_id."""

    def test_empty_job_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'job_id' is required"):
            client.submit_benchmark_response(job_id="")


class TestAttestationValidation:
    """create_attestation() must reject empty agent_id."""

    def test_empty_agent_id_raises(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'agent_id' is required"):
            client.create_attestation(agent_id="")


# ---------------------------------------------------------------
# Key fetch validation
# ---------------------------------------------------------------


class TestKeyFetchValidation:
    """Key fetch methods must reject empty required params."""

    def test_fetch_remote_key_empty_jacs_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'jacs_id' is required"):
            client.fetch_remote_key(jacs_id="")

    def test_fetch_key_by_hash_empty(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'public_key_hash' is required"):
            client.fetch_key_by_hash(public_key_hash="")

    def test_fetch_key_by_email_empty(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'email' is required"):
            client.fetch_key_by_email(email="")

    def test_fetch_key_by_domain_empty(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'domain' is required"):
            client.fetch_key_by_domain(domain="")

    def test_fetch_all_keys_empty_jacs_id(self, loaded_config: None) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="'jacs_id' is required"):
            client.fetch_all_keys(jacs_id="")


# ---------------------------------------------------------------
# Positive tests: validation passes with valid args
# ---------------------------------------------------------------


class TestValidationPassesWithValidArgs:
    """Ensure validation does not block calls with valid arguments."""

    def test_send_email_valid_args_passes_validation(self, loaded_config: None) -> None:
        """send_email with valid args does not raise ValueError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}
        # Should not raise ValueError -- validates then calls FFI
        result = client.send_email(to="bob@hai.ai", subject="Hi", body="Hello")
        assert result.message_id == "msg-1"

    def test_mark_read_valid_args_passes_validation(self, loaded_config: None) -> None:
        client = HaiClient()
        ffi = client._get_ffi()
        result = client.mark_read(message_id="msg-1")
        assert result is True

    def test_delete_message_valid_args_passes_validation(self, loaded_config: None) -> None:
        client = HaiClient()
        ffi = client._get_ffi()
        result = client.delete_message(message_id="msg-1")
        assert result is True
