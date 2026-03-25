from __future__ import annotations

import json

import pytest

from haiai.client import HaiClient


def test_connect_ws_uses_wss_scheme_and_yields_events(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """WS connect delegates to FFI connect_ws/ws_next_event and yields HaiEvents."""
    events = [
        {"event_type": "connected", "data": {"agent_id": "agent-1"}},
        {"event_type": "benchmark_job", "data": {"job_id": "job-1", "scenario_id": "scenario-1"}},
    ]
    event_iter = iter(events)

    call_log: list[str] = []

    client = HaiClient()
    ffi = client._get_ffi()

    ffi.responses["connect_ws"] = 42  # opaque handle

    def fake_ws_next_event(handle: int):
        call_log.append(f"ws_next_event({handle})")
        return next(event_iter, None)

    ffi.responses["ws_next_event"] = fake_ws_next_event
    ffi.responses["ws_close"] = None

    stream = client._connect_ws("https://hai.example")

    first = next(stream)
    second = next(stream)
    client.disconnect()
    with pytest.raises(StopIteration):
        next(stream)

    assert first.event_type == "connected"
    assert second.event_type == "benchmark_job"
    assert second.data["job_id"] == "job-1"

    # Verify the FFI handle was used
    assert "ws_next_event(42)" in call_log


def test_connect_ws_retries_after_failure(monkeypatch: pytest.MonkeyPatch) -> None:
    """WS connect retries when FFI connect_ws raises an exception."""
    attempts = {"count": 0}

    events = [
        {"event_type": "connected", "data": {"agent_id": "agent-1"}},
    ]
    event_iter = iter(events)

    def fake_connect_ws():
        attempts["count"] += 1
        if attempts["count"] == 1:
            raise RuntimeError("temporary failure")
        return 99  # opaque handle on second attempt

    def fake_ws_next_event(handle: int):
        return next(event_iter, None)

    monkeypatch.setattr("haiai._retry.backoff", lambda attempt: 0.0)
    monkeypatch.setattr("time.sleep", lambda _seconds: None)

    client = HaiClient()
    ffi = client._get_ffi()

    ffi.responses["connect_ws"] = fake_connect_ws
    ffi.responses["ws_next_event"] = fake_ws_next_event
    ffi.responses["ws_close"] = None

    stream = client._connect_ws("http://hai.example")

    event = next(stream)
    client.disconnect()
    with pytest.raises(StopIteration):
        next(stream)

    assert attempts["count"] == 2
    assert event.event_type == "connected"
