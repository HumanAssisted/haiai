"""Live integration tests for HAI email CRUD operations.

Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
HAI_URL (defaults to http://localhost:3000) backed by Stalwart.

Run:
    HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 uv run pytest tests/test_email_integration.py -v
"""

from __future__ import annotations

import os
import time

import pytest

from jacs.hai.client import HaiClient, register_new_agent
from jacs.hai.errors import HaiApiError
from jacs.hai.models import EmailMessage, EmailStatus, SendEmailResult

pytestmark = pytest.mark.skipif(
    os.environ.get("HAI_LIVE_TEST") != "1",
    reason="set HAI_LIVE_TEST=1 to run live API tests",
)

API_URL = os.environ.get("HAI_URL", "http://localhost:3000")


# ------------------------------------------------------------------
# Fixtures
# ------------------------------------------------------------------


@pytest.fixture(scope="module")
def registered_client():
    """Register a fresh JACS agent, claim a username, and return a configured HaiClient.

    Yields:
        (client, agent_name, registration_result)
    """
    import tempfile

    agent_name = f"py-integ-{int(time.time() * 1000)}"

    with tempfile.TemporaryDirectory() as tmp:
        key_dir = os.path.join(tmp, "keys")
        config_path = os.path.join(tmp, "jacs.config.json")

        owner_email = os.environ.get("HAI_OWNER_EMAIL", "jonathan@hai.io")

        result = register_new_agent(
            name=agent_name,
            owner_email=owner_email,
            hai_url=API_URL,
            key_dir=key_dir,
            config_path=config_path,
            description="Python integration test agent",
            quiet=True,
        )

        client = HaiClient()

        # Claim username to provision the @hai.ai email address.
        claim = client.claim_username(API_URL, result.agent_id, agent_name)
        assert claim.get("email"), "claim_username should return an email address"

        yield client, agent_name, result


# ------------------------------------------------------------------
# Tests
# ------------------------------------------------------------------


class TestEmailIntegrationLifecycle:
    """Full email lifecycle exercised against a live HAI API.

    Steps are ordered via pytest-dependency so earlier failures cause
    later steps to be skipped rather than producing misleading errors.
    """

    # Shared state across ordered tests within the class.
    _message_id: str = ""
    _subject: str = ""
    _body: str = ""
    _reply_message_id: str = ""

    # -- 1. Register fresh JACS agent (handled by fixture) ---------

    def test_01_send_email(self, registered_client) -> None:
        """Send an email and assert a message_id is returned."""
        client, agent_name, _reg = registered_client

        self.__class__._subject = f"py-integ-test-{int(time.time() * 1000)}"
        self.__class__._body = "Hello from Python integration test!"

        send_result = client.send_email(
            API_URL,
            to=f"{agent_name}@hai.ai",
            subject=self._subject,
            body=self._body,
        )

        assert isinstance(send_result, SendEmailResult)
        assert send_result.message_id, "send_email must return a non-empty message_id"
        self.__class__._message_id = send_result.message_id

        # Give async delivery a moment to settle.
        time.sleep(2)

    def test_02_list_messages(self, registered_client) -> None:
        """List messages and assert the sent message appears."""
        client, _name, _reg = registered_client

        messages = client.list_messages(API_URL, limit=20)
        assert isinstance(messages, list)
        assert len(messages) > 0, "list_messages should return at least one message"

        # The sent message should be among the listed messages.
        ids = [m.id for m in messages]
        subjects = [m.subject for m in messages]
        assert (
            self._message_id in ids or self._subject in subjects
        ), f"sent message (id={self._message_id}) not found in list_messages"

    def test_03_get_message(self, registered_client) -> None:
        """Get the sent message by ID and verify subject/body match."""
        client, _name, _reg = registered_client

        msg = client.get_message(API_URL, self._message_id)
        assert isinstance(msg, EmailMessage)
        assert msg.subject == self._subject, (
            f"expected subject={self._subject!r}, got {msg.subject!r}"
        )
        assert self._body in msg.body_text, (
            f"expected body to contain {self._body!r}"
        )

    def test_04_mark_read(self, registered_client) -> None:
        """Mark the message as read -- should not raise."""
        client, _name, _reg = registered_client

        result = client.mark_read(API_URL, self._message_id)
        assert result is True

    def test_05_mark_unread(self, registered_client) -> None:
        """Mark the message as unread -- should not raise."""
        client, _name, _reg = registered_client

        result = client.mark_unread(API_URL, self._message_id)
        assert result is True

    def test_06_search_messages(self, registered_client) -> None:
        """Search for the sent message by subject and assert it is found."""
        client, _name, _reg = registered_client

        search_results = client.search_messages(API_URL, q=self._subject)
        assert isinstance(search_results, list)
        assert len(search_results) > 0, (
            f"search for q={self._subject!r} should return at least one result"
        )
        found_subjects = [m.subject for m in search_results]
        assert self._subject in found_subjects, (
            f"search results should contain subject={self._subject!r}"
        )

    def test_07_unread_count(self, registered_client) -> None:
        """Get unread count and assert it returns an integer."""
        client, _name, _reg = registered_client

        unread = client.get_unread_count(API_URL)
        assert isinstance(unread, int), (
            f"get_unread_count should return int, got {type(unread).__name__}"
        )
        assert unread >= 0, "unread count must be non-negative"

    def test_08_email_status(self, registered_client) -> None:
        """Get email status and assert it includes the agent email address."""
        client, _name, _reg = registered_client

        status = client.get_email_status(API_URL)
        assert isinstance(status, EmailStatus)
        assert status.email, "email_status.email should be non-empty"
        assert status.status, "email_status.status should be non-empty"

    def test_09_reply(self, registered_client) -> None:
        """Reply to the sent message and assert a reply message_id is returned."""
        client, _name, _reg = registered_client

        reply_result = client.reply(
            API_URL,
            message_id=self._message_id,
            body="Reply from Python integration test!",
        )

        assert isinstance(reply_result, SendEmailResult)
        assert reply_result.message_id, (
            "reply must return a non-empty message_id"
        )
        self.__class__._reply_message_id = reply_result.message_id

    def test_10_delete(self, registered_client) -> None:
        """Delete the original message -- should not raise."""
        client, _name, _reg = registered_client

        result = client.delete_message(API_URL, self._message_id)
        assert result is True

    def test_11_get_deleted_message_fails(self, registered_client) -> None:
        """Get the deleted message and assert it raises an error (404)."""
        client, _name, _reg = registered_client

        with pytest.raises((HaiApiError, Exception)) as exc_info:
            client.get_message(API_URL, self._message_id)

        # If it is a HaiApiError, verify it is a 404.
        if isinstance(exc_info.value, HaiApiError):
            assert exc_info.value.status_code == 404, (
                f"expected 404, got {exc_info.value.status_code}"
            )
