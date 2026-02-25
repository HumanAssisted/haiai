"""Live integration tests for HAI email CRUD operations.

Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
HAI_URL (defaults to http://localhost:3000) backed by Stalwart.

Run:
    HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 uv run pytest tests/test_email_integration.py -v
"""

import os
import time

import pytest

pytestmark = pytest.mark.skipif(
    os.environ.get("HAI_LIVE_TEST") != "1",
    reason="set HAI_LIVE_TEST=1 to run live API tests",
)

API_URL = os.environ.get("HAI_URL", "http://localhost:3000")


@pytest.fixture(scope="module")
def registered_client():
    """Register a fresh JACS agent and return a configured HaiClient."""
    import tempfile

    from jacs.hai.client import register_new_agent, HaiClient

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

        # Claim username to provision email address.
        claim = client.claim_username(API_URL, result.agent_id, agent_name)
        assert claim.get("email"), "claim should return email"

        yield client, agent_name, result


def test_email_integration_lifecycle(registered_client):
    """Full email lifecycle: send → list → get → mark read → mark unread →
    search → unread count → email status → reply → delete → verify deleted."""

    client, agent_name, reg_result = registered_client

    # ── 1. Send email ─────────────────────────────────────────────────────
    subject = f"py-integ-test-{int(time.time() * 1000)}"
    body = "Hello from Python integration test!"

    send_result = client.send_email(
        API_URL,
        to=f"{agent_name}@hai.ai",
        subject=subject,
        body=body,
    )

    message_id = send_result.message_id
    assert message_id, "message_id should not be empty"

    # Small delay for async delivery
    time.sleep(2)

    # ── 2. List messages ──────────────────────────────────────────────────
    messages = client.list_messages(API_URL, limit=10)
    assert len(messages) > 0, "should have at least one message"

    # ── 3. Get message ────────────────────────────────────────────────────
    msg = client.get_message(API_URL, message_id)
    assert msg.subject == subject
    assert body in msg.body_text, "body should contain our text"

    # ── 4. Mark read ──────────────────────────────────────────────────────
    client.mark_read(API_URL, message_id)

    # ── 5. Mark unread ────────────────────────────────────────────────────
    client.mark_unread(API_URL, message_id)

    # ── 6. Search messages ────────────────────────────────────────────────
    search_results = client.search_messages(API_URL, q=subject)
    assert len(search_results) > 0, "search should find the sent message"

    # ── 7. Unread count ───────────────────────────────────────────────────
    unread = client.get_unread_count(API_URL)
    assert isinstance(unread, int), "unread count should be an integer"

    # ── 8. Email status ───────────────────────────────────────────────────
    status = client.get_email_status(API_URL)
    assert status.email, "status should include email"

    # ── 9. Reply ──────────────────────────────────────────────────────────
    rfc_message_id = getattr(msg, "message_id", None) or message_id
    reply_result = client.reply(
        API_URL,
        message_id=rfc_message_id,
        body="Reply from Python integration test!",
    )
    assert reply_result.message_id, "reply message_id should not be empty"

    # ── 10. Delete ────────────────────────────────────────────────────────
    client.delete_message(API_URL, message_id)

    # ── 11. Verify deleted ────────────────────────────────────────────────
    with pytest.raises(Exception):
        client.get_message(API_URL, message_id)
