"""Shared mock API contract tests for method/path/auth consistency."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

from haiai.client import HaiClient


def _load_contract() -> dict[str, Any]:
    fixture_path = Path(__file__).resolve().parents[2] / "fixtures" / "contract_endpoints.json"
    return json.loads(fixture_path.read_text())


class _FakeResponse:
    def __init__(self, status_code: int, payload: dict[str, Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = ""
        self.headers: dict[str, str] = {}

    def json(self) -> dict[str, Any]:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise RuntimeError(f"http error {self.status_code}")


def test_hello_contract_uses_shared_method_path_and_auth(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    contract = _load_contract()
    captured: dict[str, Any] = {}

    def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["headers"] = kwargs.get("headers", {})
        return _FakeResponse(
            200,
            {
                "timestamp": "2026-01-01T00:00:00Z",
                "client_ip": "127.0.0.1",
                "hai_public_key_fingerprint": "fp",
                "message": "ok",
                "hello_id": "h1",
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)
    HaiClient().hello_world(contract["base_url"])

    assert captured["url"] == contract["base_url"] + contract["hello"]["path"]
    if contract["hello"]["auth_required"]:
        assert str(captured["headers"].get("Authorization", "")).startswith("JACS ")
    else:
        assert "Authorization" not in captured["headers"]


def test_check_username_contract_uses_shared_method_path_and_auth(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    contract = _load_contract()
    captured: dict[str, Any] = {}

    def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["headers"] = kwargs.get("headers", {})
        captured["params"] = kwargs.get("params", {})
        return _FakeResponse(200, {"available": True, "username": "alice"})

    import httpx

    monkeypatch.setattr(httpx, "get", fake_get)
    HaiClient().check_username(contract["base_url"], "alice")

    assert captured["url"] == contract["base_url"] + contract["check_username"]["path"]
    assert captured["params"]["username"] == "alice"
    if contract["check_username"]["auth_required"]:
        assert str(captured["headers"].get("Authorization", "")).startswith("JACS ")
    else:
        assert "Authorization" not in captured["headers"]


def test_submit_response_contract_uses_shared_method_path_and_auth(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    contract = _load_contract()
    captured: dict[str, Any] = {}
    job_id = "job-123"
    expected_path = contract["submit_response"]["path"].replace("{job_id}", job_id)

    def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
        captured["url"] = url
        captured["headers"] = kwargs.get("headers", {})
        return _FakeResponse(
            200,
            {
                "success": True,
                "job_id": job_id,
                "message": "ok",
            },
        )

    import httpx

    monkeypatch.setattr(httpx, "post", fake_post)
    HaiClient().submit_benchmark_response(contract["base_url"], job_id=job_id, message="ok")

    assert captured["url"] == contract["base_url"] + expected_path
    if contract["submit_response"]["auth_required"]:
        assert str(captured["headers"].get("Authorization", "")).startswith("JACS ")
    else:
        assert "Authorization" not in captured["headers"]
