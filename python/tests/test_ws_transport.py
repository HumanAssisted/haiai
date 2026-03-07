from __future__ import annotations

import json
import sys
import types

import pytest

from jacs.hai.client import HaiClient


class _FakeWebSocket:
    def __init__(self, messages: list[object]) -> None:
        self._messages = iter(messages)
        self.closed = False

    def __iter__(self):  # type: ignore[override]
        return self

    def __next__(self) -> object:
        return next(self._messages)

    def close(self) -> None:
        self.closed = True


class _FakeWebSocketContext:
    def __init__(self, websocket: _FakeWebSocket) -> None:
        self.websocket = websocket

    def __enter__(self) -> _FakeWebSocket:
        return self.websocket

    def __exit__(self, *_args: object) -> None:
        self.websocket.close()


def _install_ws_client(
    monkeypatch: pytest.MonkeyPatch,
    connect_impl,
) -> None:
    client_mod = types.ModuleType("websockets.sync.client")
    client_mod.connect = connect_impl  # type: ignore[attr-defined]

    sync_mod = types.ModuleType("websockets.sync")
    sync_mod.client = client_mod  # type: ignore[attr-defined]

    websockets_mod = types.ModuleType("websockets")
    websockets_mod.sync = sync_mod  # type: ignore[attr-defined]

    monkeypatch.setitem(sys.modules, "websockets", websockets_mod)
    monkeypatch.setitem(sys.modules, "websockets.sync", sync_mod)
    monkeypatch.setitem(sys.modules, "websockets.sync.client", client_mod)


def test_connect_ws_uses_wss_scheme_and_yields_events(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, object] = {}

    def fake_connect(
        ws_url: str,
        *,
        additional_headers: dict[str, str],
        close_timeout: int,
    ) -> _FakeWebSocketContext:
        captured["ws_url"] = ws_url
        captured["headers"] = additional_headers
        captured["close_timeout"] = close_timeout
        websocket = _FakeWebSocket([
            json.dumps({"type": "connected", "agent_id": "agent-1"}),
            json.dumps({"type": "benchmark_job", "job_id": "job-1", "scenario_id": "scenario-1"}),
        ])
        return _FakeWebSocketContext(websocket)

    _install_ws_client(monkeypatch, fake_connect)
    monkeypatch.setattr(HaiClient, "_build_auth_headers", lambda self: {"Authorization": "JACS token"})

    client = HaiClient()
    stream = client._connect_ws("https://hai.example")

    first = next(stream)
    second = next(stream)
    client.disconnect()
    with pytest.raises(StopIteration):
        next(stream)

    assert captured["ws_url"] == "wss://hai.example/ws/agent/connect"
    assert captured["headers"] == {"Authorization": "JACS token"}
    assert captured["close_timeout"] == 5
    assert first.event_type == "connected"
    assert second.event_type == "benchmark_job"
    assert second.data["job_id"] == "job-1"


def test_connect_ws_retries_after_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    attempts = {"count": 0}

    def fake_connect(
        ws_url: str,
        *,
        additional_headers: dict[str, str],
        close_timeout: int,
    ) -> _FakeWebSocketContext:
        attempts["count"] += 1
        if attempts["count"] == 1:
            raise RuntimeError("temporary failure")
        websocket = _FakeWebSocket([
            json.dumps({"type": "connected", "agent_id": "agent-1"}),
        ])
        return _FakeWebSocketContext(websocket)

    _install_ws_client(monkeypatch, fake_connect)
    monkeypatch.setattr(HaiClient, "_build_auth_headers", lambda self: {"Authorization": "JACS token"})
    monkeypatch.setattr("jacs.hai.client.backoff", lambda attempt: 0.0)
    monkeypatch.setattr("jacs.hai.client.time.sleep", lambda _seconds: None)

    client = HaiClient()
    stream = client._connect_ws("http://hai.example")

    event = next(stream)
    client.disconnect()
    with pytest.raises(StopIteration):
        next(stream)

    assert attempts["count"] == 2
    assert event.event_type == "connected"
