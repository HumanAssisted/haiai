from __future__ import annotations

import json

import pytest

from haisdk import mcp_server


@pytest.mark.asyncio
async def test_hai_send_email_loads_config_and_delegates(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_load(path: str) -> None:
        seen["config_path"] = path

    def fake_send_email(
        hai_url: str,
        *,
        to: str,
        subject: str,
        body: str,
        in_reply_to: str | None = None,
    ) -> dict[str, object]:
        seen["hai_url"] = hai_url
        seen["payload"] = {
            "to": to,
            "subject": subject,
            "body": body,
            "in_reply_to": in_reply_to,
        }
        return {"message_id": "msg-1"}

    monkeypatch.setattr("jacs.hai.config.load", fake_load)
    monkeypatch.setattr("jacs.hai.client.send_email", fake_send_email)

    result = await mcp_server.hai_send_email(
        to="ops@hai.ai",
        subject="Subject",
        body="Body",
        config_path="/tmp/jacs.config.json",
        hai_url="https://hai.example",
    )

    assert seen["config_path"] == "/tmp/jacs.config.json"
    assert seen["hai_url"] == "https://hai.example"
    assert seen["payload"] == {
        "to": "ops@hai.ai",
        "subject": "Subject",
        "body": "Body",
        "in_reply_to": None,
    }
    assert json.loads(result) == {"message_id": "msg-1"}


@pytest.mark.asyncio
async def test_hai_reply_email_uses_subject_override(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_reply(
        hai_url: str,
        message_id: str,
        body: str,
        *,
        subject: str | None = None,
    ) -> dict[str, object]:
        seen["call"] = {
            "hai_url": hai_url,
            "message_id": message_id,
            "body": body,
            "subject": subject,
        }
        return {"message_id": "reply-1"}

    monkeypatch.setattr("jacs.hai.client.reply", fake_reply)

    result = await mcp_server.hai_reply_email(
        message_id="msg-1",
        body="Reply body",
        subject_override="Custom subject",
        hai_url="https://hai.example",
    )

    assert seen["call"] == {
        "hai_url": "https://hai.example",
        "message_id": "msg-1",
        "body": "Reply body",
        "subject": "Custom subject",
    }
    assert json.loads(result) == {"message_id": "reply-1"}
