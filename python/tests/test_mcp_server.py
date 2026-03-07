from __future__ import annotations

import json
from dataclasses import dataclass

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


def test_to_json_serializes_dataclasses() -> None:
    @dataclass
    class _Payload:
        count: int
        label: str

    assert json.loads(mcp_server._to_json(_Payload(count=2, label="demo"))) == {
        "count": 2,
        "label": "demo",
    }


@pytest.mark.asyncio
async def test_hai_generate_verify_link_wraps_result(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_generate_verify_link(
        document: str,
        *,
        base_url: str,
        hosted: bool = False,
    ) -> str:
        seen["call"] = {
            "document": document,
            "base_url": base_url,
            "hosted": hosted,
        }
        return "https://hai.example/jacs/verify?s=abc"

    monkeypatch.setattr("jacs.hai.client.generate_verify_link", fake_generate_verify_link)

    result = await mcp_server.hai_generate_verify_link(
        document='{"signed":true}',
        base_url="https://hai.example",
        hosted=True,
    )

    assert seen["call"] == {
        "document": '{"signed":true}',
        "base_url": "https://hai.example",
        "hosted": True,
    }
    assert json.loads(result) == {"verify_url": "https://hai.example/jacs/verify?s=abc"}


@pytest.mark.asyncio
async def test_hai_search_messages_loads_config_and_maps_filters(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    seen: dict[str, object] = {}

    def fake_load(path: str) -> None:
        seen["config_path"] = path

    def fake_search_messages(
        hai_url: str,
        *,
        q: str | None = None,
        direction: str | None = None,
        from_address: str | None = None,
        to_address: str | None = None,
        since: str | None = None,
        until: str | None = None,
        limit: int = 20,
        offset: int = 0,
    ) -> list[dict[str, object]]:
        seen["call"] = {
            "hai_url": hai_url,
            "q": q,
            "direction": direction,
            "from_address": from_address,
            "to_address": to_address,
            "since": since,
            "until": until,
            "limit": limit,
            "offset": offset,
        }
        return [{"message_id": "msg-1"}]

    monkeypatch.setattr("jacs.hai.config.load", fake_load)
    monkeypatch.setattr("jacs.hai.client.search_messages", fake_search_messages)

    result = await mcp_server.hai_search_messages(
        q="subject",
        direction="outbound",
        from_address="sender@hai.ai",
        to_address="dest@hai.ai",
        since="2026-03-01T00:00:00Z",
        until="2026-03-05T00:00:00Z",
        limit=15,
        offset=3,
        config_path="/tmp/jacs.config.json",
        hai_url="https://hai.example",
    )

    assert seen["config_path"] == "/tmp/jacs.config.json"
    assert seen["call"] == {
        "hai_url": "https://hai.example",
        "q": "subject",
        "direction": "outbound",
        "from_address": "sender@hai.ai",
        "to_address": "dest@hai.ai",
        "since": "2026-03-01T00:00:00Z",
        "until": "2026-03-05T00:00:00Z",
        "limit": 15,
        "offset": 3,
    }
    assert json.loads(result) == [{"message_id": "msg-1"}]


@pytest.mark.asyncio
async def test_hai_delete_message_returns_deleted_payload(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_delete_message(hai_url: str, message_id: str) -> None:
        seen["call"] = {"hai_url": hai_url, "message_id": message_id}

    monkeypatch.setattr("jacs.hai.client.delete_message", fake_delete_message)

    result = await mcp_server.hai_delete_message(
        message_id="msg-9",
        hai_url="https://hai.example",
    )

    assert seen["call"] == {
        "hai_url": "https://hai.example",
        "message_id": "msg-9",
    }
    assert json.loads(result) == {"deleted": True, "message_id": "msg-9"}


def test_main_runs_stdio_transport(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_run(*, transport: str) -> None:
        seen["transport"] = transport

    monkeypatch.setattr(mcp_server.server, "run", fake_run)

    mcp_server.main()

    assert seen["transport"] == "stdio"
