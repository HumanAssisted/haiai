"""Tests for audit fix issues #3, #6, #13, #18.

Each test is written to fail BEFORE the corresponding production fix,
then pass after the fix is applied.
"""

from __future__ import annotations

import json
from typing import Any

import httpx
import pytest

from haiai.client import HaiClient


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

class _FakeResponse:
    """Minimal httpx.Response stand-in for monkeypatching."""

    def __init__(self, status_code: int, payload: dict[str, Any]) -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = json.dumps(payload)
        self.headers: dict[str, str] = {"content-type": "application/json"}

    def json(self) -> dict[str, Any]:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise RuntimeError(f"HTTP {self.status_code}")


_HELLO_OK = {
    "timestamp": "2026-01-01T00:00:00Z",
    "client_ip": "127.0.0.1",
    "hai_public_key_fingerprint": "fp",
    "message": "ok",
    "hello_id": "h1",
}

_BENCHMARK_OK = {
    "benchmark_id": "b1",
    "name": "demo",
    "tier": "free",
    "scores": {"overall": 100},
    "tests": [],
    "status": "completed",
}

_EMAIL_LIST_OK = {"messages": []}


# ===================================================================
# Issue #3 — hello_world must include agent_id in payload
# ===================================================================

class TestHelloWorldAgentId:
    """hello_world() POST body must contain {"agent_id": "<jacsId>"}."""

    def test_hello_payload_contains_agent_id(
        self,
        loaded_config: None,
        jacs_id: str,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, _HELLO_OK)

        monkeypatch.setattr(httpx, "post", fake_post)

        client = HaiClient()
        client.hello_world("https://api.hai.ai")

        assert "agent_id" in captured["json"], (
            "hello_world payload must include 'agent_id'"
        )
        assert captured["json"]["agent_id"] == jacs_id

    def test_hello_payload_agent_id_coexists_with_include_test(
        self,
        loaded_config: None,
        jacs_id: str,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, _HELLO_OK)

        monkeypatch.setattr(httpx, "post", fake_post)

        client = HaiClient()
        client.hello_world("https://api.hai.ai", include_test=True)

        assert captured["json"].get("agent_id") == jacs_id
        assert captured["json"].get("include_test") is True


# ===================================================================
# Issue #6 — free benchmark must include transport: "sse"
# ===================================================================

class TestBenchmarkTransport:
    """run_benchmark() payload must include "transport": "sse"."""

    def test_benchmark_payload_contains_transport(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, _BENCHMARK_OK)

        monkeypatch.setattr(httpx, "post", fake_post)

        client = HaiClient()
        client.benchmark("https://api.hai.ai", name="demo", tier="free")

        assert "transport" in captured["json"], (
            "run_benchmark payload must include 'transport'"
        )
        assert captured["json"]["transport"] == "sse"

    def test_benchmark_payload_has_name_tier_and_transport(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_post(url: str, **kwargs: Any) -> _FakeResponse:
            captured["json"] = kwargs.get("json", {})
            return _FakeResponse(200, _BENCHMARK_OK)

        monkeypatch.setattr(httpx, "post", fake_post)

        client = HaiClient()
        client.benchmark("https://api.hai.ai", name="bench1", tier="pro")

        payload = captured["json"]
        assert payload["name"] == "bench1"
        assert payload["tier"] == "pro"
        assert payload["transport"] == "sse"


# ===================================================================
# Issue #13 — base URL validation
# ===================================================================

class TestBaseUrlValidation:
    """_make_url (or client construction) must reject invalid base URLs."""

    @pytest.mark.parametrize("bad_url", [
        "",
        "ftp://example.com",
        "file:///etc/passwd",
        "not-a-url",
        "://missing-scheme",
        "javascript:alert(1)",
    ])
    def test_invalid_base_url_raises_valueerror(self, bad_url: str) -> None:
        with pytest.raises(ValueError, match="(?i)url|scheme|http"):
            HaiClient._make_url(bad_url, "/api/v1/test")

    @pytest.mark.parametrize("good_url,expected", [
        ("https://api.hai.ai", "https://api.hai.ai/api/v1/test"),
        ("http://localhost:8080", "http://localhost:8080/api/v1/test"),
        ("https://api.hai.ai/", "https://api.hai.ai/api/v1/test"),
    ])
    def test_valid_base_url_accepted(self, good_url: str, expected: str) -> None:
        result = HaiClient._make_url(good_url, "/api/v1/test")
        assert result == expected


# ===================================================================
# Issue #18 — list_messages & search_messages missing has_attachments
#             list_messages also missing since/until
# ===================================================================

class TestMessageQueryParams:
    """list_messages and search_messages must support has_attachments,
    and list_messages must also support since/until — matching Rust/Go."""

    def test_list_messages_has_attachments_param(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.list_messages("https://api.hai.ai", has_attachments=True)

        assert captured["params"].get("has_attachments") == "true"

    def test_list_messages_has_attachments_false(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.list_messages("https://api.hai.ai", has_attachments=False)

        assert captured["params"].get("has_attachments") == "false"

    def test_list_messages_has_attachments_omitted_when_none(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.list_messages("https://api.hai.ai")

        assert "has_attachments" not in captured["params"]

    def test_list_messages_since_until_params(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.list_messages(
            "https://api.hai.ai",
            since="2026-01-01T00:00:00Z",
            until="2026-03-01T00:00:00Z",
        )

        assert captured["params"]["since"] == "2026-01-01T00:00:00Z"
        assert captured["params"]["until"] == "2026-03-01T00:00:00Z"

    def test_search_messages_has_attachments_param(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.search_messages("https://api.hai.ai", has_attachments=True)

        assert captured["params"].get("has_attachments") == "true"

    def test_search_messages_has_attachments_omitted_when_none(
        self,
        loaded_config: None,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        captured: dict[str, Any] = {}

        def fake_get(url: str, **kwargs: Any) -> _FakeResponse:
            captured["params"] = kwargs.get("params", {})
            return _FakeResponse(200, _EMAIL_LIST_OK)

        monkeypatch.setattr(httpx, "get", fake_get)

        client = HaiClient()
        client.search_messages("https://api.hai.ai", q="test")

        assert "has_attachments" not in captured["params"]
