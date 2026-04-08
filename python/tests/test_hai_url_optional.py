"""Tests for H8: hai_url parameter should be optional on all SDK methods.

Email CRUD methods delegate to FFI and never use hai_url (dead parameter).
Registration/hello methods use hai_url but should default to DEFAULT_BASE_URL.
Module-level wrapper functions should mirror the same optional behavior.
"""

from __future__ import annotations

import json
from typing import Any

import pytest

from haiai.client import DEFAULT_BASE_URL, HaiClient
from haiai.models import SendEmailResult


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
# Group A: Email CRUD methods — hai_url is dead (never used by FFI)
# Calling without hai_url must NOT raise TypeError.
# ---------------------------------------------------------------


class TestEmailMethodsHaiUrlOptional:
    """Email methods should accept hai_url as optional (it is unused)."""

    def test_send_email_without_hai_url(self, loaded_config: None) -> None:
        """send_email() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}

        # This should NOT raise TypeError
        result = client.send_email(to="bob@hai.ai", subject="Hi", body="Hello")
        assert result.message_id == "msg-1"

    def test_send_email_with_explicit_hai_url(self, loaded_config: None) -> None:
        """send_email() with explicit hai_url still works (backward compat)."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["send_email"] = {"message_id": "msg-2", "status": "sent"}

        result = client.send_email(hai_url="https://custom.url", to="bob@hai.ai", subject="Hi", body="Hello")
        assert result.message_id == "msg-2"

    def test_sign_email_without_hai_url(self, loaded_config: None) -> None:
        """sign_email() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["sign_email_raw"] = "dGVzdA=="  # base64 "test"

        result = client.sign_email(raw_email=b"From: a@b.com\r\n\r\nBody")
        assert isinstance(result, bytes)

    def test_send_signed_email_without_hai_url(self, loaded_config: None) -> None:
        """send_signed_email() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["send_signed_email"] = {"message_id": "msg-3", "status": "sent"}

        result = client.send_signed_email(to="bob@hai.ai", subject="Hi", body="Hello")
        assert result.message_id == "msg-3"

    def test_list_messages_without_hai_url(self, loaded_config: None) -> None:
        """list_messages() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["list_messages"] = []

        result = client.list_messages()
        assert result == []

    def test_mark_read_without_hai_url(self, loaded_config: None) -> None:
        """mark_read() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()

        result = client.mark_read(message_id="msg-1")
        assert result is True

    def test_get_email_status_without_hai_url(self, loaded_config: None) -> None:
        """get_email_status() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_email_status"] = {
            "active": True,
            "email": "test@hai.ai",
        }

        result = client.get_email_status()
        assert result is not None

    def test_get_message_without_hai_url(self, loaded_config: None) -> None:
        """get_message() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_message"] = {"id": "msg-1", "subject": "Hi"}

        result = client.get_message(message_id="msg-1")
        assert result is not None

    def test_delete_message_without_hai_url(self, loaded_config: None) -> None:
        """delete_message() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()

        result = client.delete_message(message_id="msg-1")
        assert result is True

    def test_mark_unread_without_hai_url(self, loaded_config: None) -> None:
        """mark_unread() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()

        result = client.mark_unread(message_id="msg-1")
        assert result is True

    def test_verify_email_without_hai_url(self, loaded_config: None) -> None:
        """verify_email() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["verify_email_raw"] = {"valid": True, "jacs_id": "test"}

        result = client.verify_email(raw_email=b"From: a@b.com\r\n\r\nBody")
        assert result.valid is True

    def test_search_messages_without_hai_url(self, loaded_config: None) -> None:
        """search_messages() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["search_messages"] = []

        result = client.search_messages()
        assert result == []

    def test_contacts_without_hai_url(self, loaded_config: None) -> None:
        """contacts() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["get_email_status"] = {"active": True, "email": "test@hai.ai"}
        ffi.responses["contacts"] = []

        result = client.contacts()
        assert result == []


# ---------------------------------------------------------------
# Group B: Registration/hello methods — hai_url IS used, should default
# ---------------------------------------------------------------


class TestRegistrationMethodsHaiUrlOptional:
    """Registration methods should default hai_url to DEFAULT_BASE_URL."""

    def test_testconnection_without_hai_url(self, loaded_config: None) -> None:
        """testconnection() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["hello"] = {}

        result = client.testconnection()
        assert result is True

    def test_hello_world_without_hai_url(self, loaded_config: None) -> None:
        """hello_world() without hai_url should not raise TypeError."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "message": "Hello!",
        }

        result = client.hello_world()
        assert result.success is True

    def test_hello_world_uses_default_url_for_signature_verification(
        self, loaded_config: None, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """hello_world() without hai_url should pass DEFAULT_BASE_URL to verify_hai_message."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "message": "Hello!",
            "hai_signed_ack": "fake-signature",
            "hai_public_key_fingerprint": "fake-key",
        }

        captured_urls: list = []
        original_verify = client.verify_hai_message

        def spy_verify(**kwargs: Any) -> bool:
            captured_urls.append(kwargs.get("hai_url"))
            return True

        monkeypatch.setattr(client, "verify_hai_message", lambda **kw: spy_verify(**kw))

        result = client.hello_world()  # No hai_url
        assert result.success is True
        assert len(captured_urls) == 1
        assert captured_urls[0] == DEFAULT_BASE_URL

    def test_register_preview_uses_default_url(self, loaded_config: None) -> None:
        """register(preview=True) without hai_url should use DEFAULT_BASE_URL in endpoint."""
        client = HaiClient()

        result = client.register(preview=True)
        # The preview endpoint should contain DEFAULT_BASE_URL
        assert DEFAULT_BASE_URL in result.endpoint

    def test_register_without_hai_url(self, loaded_config: None) -> None:
        """register() without hai_url should not raise TypeError (PRD acceptance criterion)."""
        client = HaiClient()
        ffi = client._get_ffi()
        ffi.responses["register"] = {"agent_id": "test-123", "registered": True}

        # This is the explicit PRD acceptance criterion:
        # register_new_agent(name="test", owner_email="x@y.com") does not raise TypeError
        result = client.register()
        assert result.success is True
        assert result.agent_id == "test-123"


# ---------------------------------------------------------------
# Module-level wrappers — same optional behavior
# ---------------------------------------------------------------


class TestModuleLevelWrappersHaiUrlOptional:
    """Module-level wrapper functions should accept hai_url as optional."""

    def test_module_send_email_without_hai_url(self, loaded_config: None) -> None:
        """Module-level send_email() without hai_url should not raise TypeError."""
        from haiai.client import send_email, _get_client

        client = _get_client()
        client._agent_email = TEST_AGENT_EMAIL
        ffi = client._get_ffi()
        ffi.responses["send_email"] = {"message_id": "msg-1", "status": "sent"}

        result = send_email(to="bob@hai.ai", subject="Hi", body="Hello")
        assert result.message_id == "msg-1"

    def test_module_list_messages_without_hai_url(self, loaded_config: None) -> None:
        """Module-level list_messages() without hai_url should not raise TypeError."""
        from haiai.client import list_messages, _get_client

        client = _get_client()
        ffi = client._get_ffi()
        ffi.responses["list_messages"] = []

        result = list_messages()
        assert result == []

    def test_module_testconnection_without_hai_url(self, loaded_config: None) -> None:
        """Module-level testconnection() without hai_url should not raise TypeError."""
        from haiai.client import testconnection, _get_client

        client = _get_client()
        ffi = client._get_ffi()
        ffi.responses["hello"] = {}

        result = testconnection()
        assert result is True

    def test_module_mark_read_without_hai_url(self, loaded_config: None) -> None:
        """Module-level mark_read() without hai_url should not raise TypeError."""
        from haiai.client import mark_read, _get_client

        client = _get_client()
        ffi = client._get_ffi()

        result = mark_read(message_id="msg-1")
        assert result is True

    def test_module_get_email_status_without_hai_url(self, loaded_config: None) -> None:
        """Module-level get_email_status() without hai_url should not raise TypeError."""
        from haiai.client import get_email_status, _get_client

        client = _get_client()
        ffi = client._get_ffi()
        ffi.responses["get_email_status"] = {"active": True, "email": "test@hai.ai"}

        result = get_email_status()
        assert result is not None

    def test_module_hello_world_without_hai_url(self, loaded_config: None) -> None:
        """Module-level hello_world() without hai_url should not raise TypeError."""
        from haiai.client import hello_world, _get_client

        client = _get_client()
        ffi = client._get_ffi()
        ffi.responses["hello"] = {"timestamp": "2026-01-01", "message": "Hello!"}

        result = hello_world()
        assert result.success is True
