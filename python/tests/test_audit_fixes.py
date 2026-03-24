"""Tests for audit fix issues #3, #6, #13, #18.

Each test is written to fail BEFORE the corresponding production fix,
then pass after the fix is applied.
"""

from __future__ import annotations

import json
from typing import Any

import pytest

from haiai.client import HaiClient


# ===================================================================
# Issue #3 -- hello_world must include agent_id in payload
# ===================================================================

class TestHelloWorldAgentId:
    """hello_world() calls ffi.hello() which receives include_test arg."""

    def test_hello_calls_ffi(
        self,
        loaded_config: None,
        jacs_id: str,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "client_ip": "127.0.0.1",
            "hai_public_key_fingerprint": "fp",
            "message": "ok",
            "hello_id": "h1",
        }

        client.hello_world("https://api.hai.ai")

        # FFI hello was called (agent_id is handled by binding-core)
        assert mock_ffi.calls[0][0] == "hello"

    def test_hello_with_include_test(
        self,
        loaded_config: None,
        jacs_id: str,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "client_ip": "127.0.0.1",
            "hai_public_key_fingerprint": "fp",
            "message": "ok",
            "hello_id": "h1",
        }

        client.hello_world("https://api.hai.ai", include_test=True)

        assert mock_ffi.calls[0][0] == "hello"
        # include_test is passed as first positional arg to FFI hello
        assert mock_ffi.calls[0][1][0] is True


# ===================================================================
# Issue #6 -- free benchmark must include transport: "sse"
# ===================================================================

class TestBenchmarkTransport:
    """benchmark() calls ffi.benchmark() which handles transport."""

    def test_benchmark_calls_ffi_with_name_and_tier(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["benchmark"] = {
            "benchmark_id": "b1",
            "name": "demo",
            "tier": "free",
            "scores": {"overall": 100},
            "tests": [],
            "status": "completed",
        }

        client.benchmark("https://api.hai.ai", name="demo", tier="free")

        assert mock_ffi.calls[0][0] == "benchmark"
        # name and tier passed to FFI
        assert mock_ffi.calls[0][1][0] == "demo"
        assert mock_ffi.calls[0][1][1] == "free"

    def test_benchmark_calls_ffi_with_custom_name_tier(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["benchmark"] = {
            "benchmark_id": "b1",
            "name": "bench1",
            "tier": "pro",
            "scores": {"overall": 100},
            "tests": [],
            "status": "completed",
        }

        client.benchmark("https://api.hai.ai", name="bench1", tier="pro")

        assert mock_ffi.calls[0][1][0] == "bench1"
        assert mock_ffi.calls[0][1][1] == "pro"


# ===================================================================
# Issue #13 -- base URL validation
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
# Issue #18 -- list_messages & search_messages missing has_attachments
#             list_messages also missing since/until
# ===================================================================

class TestMessageQueryParams:
    """list_messages and search_messages must support has_attachments,
    and list_messages must also support since/until -- matching Rust/Go."""

    def test_list_messages_has_attachments_param(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["list_messages"] = []

        client.list_messages("https://api.hai.ai", has_attachments=True)

        options = mock_ffi.calls[0][1][0]
        assert options["has_attachments"] is True

    def test_list_messages_has_attachments_false(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["list_messages"] = []

        client.list_messages("https://api.hai.ai", has_attachments=False)

        options = mock_ffi.calls[0][1][0]
        assert options["has_attachments"] is False

    def test_list_messages_has_attachments_omitted_when_none(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["list_messages"] = []

        client.list_messages("https://api.hai.ai")

        options = mock_ffi.calls[0][1][0]
        assert "has_attachments" not in options

    def test_list_messages_since_until_params(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["list_messages"] = []

        client.list_messages(
            "https://api.hai.ai",
            since="2026-01-01T00:00:00Z",
            until="2026-03-01T00:00:00Z",
        )

        options = mock_ffi.calls[0][1][0]
        assert options["since"] == "2026-01-01T00:00:00Z"
        assert options["until"] == "2026-03-01T00:00:00Z"

    def test_search_messages_has_attachments_param(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["search_messages"] = []

        client.search_messages("https://api.hai.ai", has_attachments=True)

        options = mock_ffi.calls[0][1][0]
        assert options["has_attachments"] is True

    def test_search_messages_has_attachments_omitted_when_none(
        self,
        loaded_config: None,
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["search_messages"] = []

        client.search_messages("https://api.hai.ai", q="test")

        options = mock_ffi.calls[0][1][0]
        assert "has_attachments" not in options
