"""FFI integration tests -- prove that the Python SDK delegates to FFI
and that no old HTTP code remains for core API calls.

These tests verify:
1. The FFI adapter exposes all methods from the parity fixture
2. The sync and async adapters have matching method sets
3. HaiClient delegates API calls to FFI, not httpx
4. Error mapping covers all error kinds from the parity fixture
5. MockFFIAdapter (test infrastructure) matches the real FFI adapter surface
"""

from __future__ import annotations

import inspect
import json
from pathlib import Path
from typing import Any

import pytest

from haiai._ffi_adapter import FFIAdapter, AsyncFFIAdapter, map_ffi_error
from haiai.errors import (
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RateLimited,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

FIXTURES_DIR = Path(__file__).resolve().parents[2] / "fixtures"


def _load_parity_fixture() -> dict[str, Any]:
    path = FIXTURES_DIR / "ffi_method_parity.json"
    return json.loads(path.read_text())


def _snake_case(name: str) -> str:
    """Convert camelCase/PascalCase to snake_case."""
    import re
    s1 = re.sub("(.)([A-Z][a-z]+)", r"\1_\2", name)
    return re.sub("([a-z0-9])([A-Z])", r"\1_\2", s1).lower()


def _get_all_fixture_method_names() -> list[str]:
    """Return all method names from the parity fixture in snake_case."""
    fixture = _load_parity_fixture()
    names = []
    for group in fixture["methods"].values():
        for method in group:
            names.append(_snake_case(method["name"]))
    return sorted(names)


def _get_adapter_methods(cls: type) -> set[str]:
    """Return public method names from an adapter class (excluding dunders)."""
    return {
        name
        for name, _ in inspect.getmembers(cls, predicate=inspect.isfunction)
        if not name.startswith("_")
    }


# ---------------------------------------------------------------------------
# Test: FFI method parity -- fixture vs FFIAdapter
# ---------------------------------------------------------------------------


class TestFFIMethodParity:
    """Verify the FFI adapter surface matches the shared parity fixture."""

    def test_sync_adapter_has_all_fixture_methods(self) -> None:
        """FFIAdapter must expose every method listed in ffi_method_parity.json."""
        fixture_methods = set(_get_all_fixture_method_names())
        adapter_methods = _get_adapter_methods(FFIAdapter)

        missing = fixture_methods - adapter_methods
        assert not missing, (
            f"FFIAdapter is missing methods from parity fixture: {sorted(missing)}"
        )

    def test_async_adapter_has_all_fixture_methods(self) -> None:
        """AsyncFFIAdapter must expose every method listed in ffi_method_parity.json."""
        fixture_methods = set(_get_all_fixture_method_names())
        adapter_methods = _get_adapter_methods(AsyncFFIAdapter)

        missing = fixture_methods - adapter_methods
        assert not missing, (
            f"AsyncFFIAdapter is missing methods from parity fixture: {sorted(missing)}"
        )

    def test_sync_and_async_adapters_have_same_methods(self) -> None:
        """Sync and async adapters must have the same public method names."""
        sync_methods = _get_adapter_methods(FFIAdapter)
        async_methods = _get_adapter_methods(AsyncFFIAdapter)

        sync_only = sync_methods - async_methods
        async_only = async_methods - sync_methods

        assert not sync_only, f"Methods only in FFIAdapter: {sorted(sync_only)}"
        assert not async_only, f"Methods only in AsyncFFIAdapter: {sorted(async_only)}"

    def test_fixture_total_method_count(self) -> None:
        """Sanity check: fixture declares expected total method count."""
        fixture = _load_parity_fixture()
        expected = fixture["total_method_count"]
        actual = len(_get_all_fixture_method_names())
        assert actual == expected, (
            f"Fixture declares {expected} methods but has {actual}"
        )

    def test_mock_ffi_adapter_has_all_fixture_methods(self) -> None:
        """MockFFIAdapter (from conftest) must match the parity fixture too."""
        # Import from the conftest in the tests directory
        import sys
        import importlib.util

        conftest_path = Path(__file__).parent / "conftest.py"
        spec = importlib.util.spec_from_file_location("conftest", conftest_path)
        conftest_mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(conftest_mod)
        MockFFIAdapter = conftest_mod.MockFFIAdapter

        fixture_methods = set(_get_all_fixture_method_names())
        mock_methods = {
            name
            for name in dir(MockFFIAdapter)
            if not name.startswith("_") and callable(getattr(MockFFIAdapter, name))
        }

        # The mock may have utility methods like _record; only check fixture methods
        missing = fixture_methods - mock_methods
        # Allow jacs_id vs jacs_id_sync discrepancy if present
        missing.discard("jacs_id")  # mock has jacs_id_sync
        assert not missing, (
            f"MockFFIAdapter is missing methods from parity fixture: {sorted(missing)}"
        )


# ---------------------------------------------------------------------------
# Test: All async methods are actually coroutines
# ---------------------------------------------------------------------------


class TestAsyncAdapterCoroutines:
    """Verify that AsyncFFIAdapter methods are actually async."""

    def test_all_public_methods_are_coroutines(self) -> None:
        fixture_methods = set(_get_all_fixture_method_names())
        for name in fixture_methods:
            method = getattr(AsyncFFIAdapter, name, None)
            if method is None:
                continue
            assert inspect.iscoroutinefunction(method), (
                f"AsyncFFIAdapter.{name} should be a coroutine function"
            )

    def test_sync_methods_are_not_coroutines(self) -> None:
        fixture_methods = set(_get_all_fixture_method_names())
        for name in fixture_methods:
            method = getattr(FFIAdapter, name, None)
            if method is None:
                continue
            assert not inspect.iscoroutinefunction(method), (
                f"FFIAdapter.{name} should NOT be a coroutine function"
            )


# ---------------------------------------------------------------------------
# Test: HaiClient delegates to FFI, not httpx
# ---------------------------------------------------------------------------


class TestClientDelegatesToFFI:
    """Verify HaiClient uses FFI adapter for core API calls."""

    def test_hello_world_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "client_ip": "127.0.0.1",
            "hai_public_key_fingerprint": "fp",
            "message": "ok",
            "hello_id": "h1",
        }

        result = client.hello_world("https://api.hai.ai")

        assert mock_ffi.calls[0][0] == "hello"
        assert result.message == "ok"

    def test_register_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["register"] = {
            "agent_id": "new-agent-id",
            "email": "agent@hai.ai",
            "status": "registered",
            "message": "ok",
        }

        result = client.register("https://api.hai.ai")

        assert mock_ffi.calls[0][0] == "register"

    def test_send_email_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        # send_email requires agent_email to be set (normally set during registration)
        client._agent_email = "test@hai.ai"
        mock_ffi = client._get_ffi()
        mock_ffi.responses["send_email"] = {
            "message_id": "msg-1",
            "status": "sent",
        }

        result = client.send_email(
            "https://api.hai.ai",
            to="recipient@hai.ai",
            subject="Test",
            body="Hello",
        )

        assert mock_ffi.calls[0][0] == "send_email"
        assert result.message_id == "msg-1"

    def test_list_messages_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["list_messages"] = []

        result = client.list_messages("https://api.hai.ai")

        assert mock_ffi.calls[0][0] == "list_messages"
        assert result == []

    def test_verify_document_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["verify_document"] = {
            "valid": True,
            "verified_at": "2026-01-01T00:00:00Z",
        }

        result = client.verify_document("https://api.hai.ai", '{"jacsId":"a"}')

        assert mock_ffi.calls[0][0] == "verify_document"

    def test_fetch_remote_key_delegates_to_ffi(
        self, loaded_config: None
    ) -> None:
        from haiai.client import HaiClient

        client = HaiClient()
        mock_ffi = client._get_ffi()
        mock_ffi.responses["fetch_remote_key"] = {
            "jacs_id": "agent-1",
            "version": "v1",
            "public_key": "-----BEGIN PUBLIC KEY-----\ntest\n-----END PUBLIC KEY-----",
            "algorithm": "ed25519",
            "public_key_hash": "sha256:" + "a" * 64,
            "public_key_raw_b64": "dGVzdA==",
            "status": "active",
            "dns_verified": True,
            "created_at": "2026-01-01T00:00:00Z",
        }

        result = client.fetch_remote_key("https://api.hai.ai", "agent-1")

        assert mock_ffi.calls[0][0] == "fetch_remote_key"


# ---------------------------------------------------------------------------
# Test: Error mapping covers all error kinds from fixture
# ---------------------------------------------------------------------------


class TestFFIErrorMappingParity:
    """Verify map_ffi_error handles every error kind from the parity fixture."""

    def test_all_error_kinds_are_mapped(self) -> None:
        """Every error kind in ffi_method_parity.json must be handled."""
        fixture = _load_parity_fixture()
        error_kinds = fixture["error_kinds"]

        for kind in error_kinds:
            err = RuntimeError(f"{kind}: test message")
            result = map_ffi_error(err)
            assert isinstance(result, HaiError), (
                f"map_ffi_error did not return HaiError for kind '{kind}': "
                f"got {type(result).__name__}"
            )
            # Verify it's not just a generic fallback for known kinds
            if kind == "AuthFailed":
                assert isinstance(result, HaiAuthError)
            elif kind == "RateLimited":
                assert isinstance(result, RateLimited)
            elif kind == "NetworkFailed":
                assert isinstance(result, HaiConnectionError)
            elif kind == "ProviderError":
                assert isinstance(result, HaiAuthError)
            elif kind == "NotFound":
                assert isinstance(result, HaiApiError)
            elif kind == "ApiError":
                assert isinstance(result, HaiApiError)

    def test_error_format_matches_fixture(self) -> None:
        """Error format must be '{ErrorKind}: {message}'."""
        fixture = _load_parity_fixture()
        assert fixture["error_format"] == "{ErrorKind}: {message}"

        # Verify the parser extracts the message portion correctly
        err = RuntimeError("AuthFailed: token expired")
        result = map_ffi_error(err)
        assert "token expired" in str(result)


# ---------------------------------------------------------------------------
# Test: No httpx imports in core API methods
# ---------------------------------------------------------------------------


class TestNoOldHTTPCode:
    """Verify that core API methods in client.py use FFI, not httpx."""

    def test_client_imports_ffi_adapter(self) -> None:
        """client.py must import FFIAdapter."""
        import haiai.client as client_mod
        source = inspect.getsource(client_mod)
        assert "from haiai._ffi_adapter import FFIAdapter" in source

    def test_hello_world_does_not_use_httpx(self) -> None:
        """hello_world method should not contain httpx references."""
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.hello_world)
        assert "httpx" not in source, "hello_world should delegate to FFI, not httpx"

    def test_register_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.register)
        assert "httpx" not in source, "register should delegate to FFI, not httpx"

    def test_send_email_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.send_email)
        assert "httpx" not in source, "send_email should delegate to FFI, not httpx"

    def test_list_messages_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.list_messages)
        assert "httpx" not in source, "list_messages should delegate to FFI, not httpx"

    def test_verify_document_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.verify_document)
        assert "httpx" not in source, "verify_document should delegate to FFI, not httpx"

    def test_benchmark_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.benchmark)
        assert "httpx" not in source, "benchmark should delegate to FFI, not httpx"

    def test_fetch_remote_key_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.fetch_remote_key)
        assert "httpx" not in source, "fetch_remote_key should delegate to FFI, not httpx"

    def test_get_email_status_does_not_use_httpx(self) -> None:
        from haiai.client import HaiClient
        source = inspect.getsource(HaiClient.get_email_status)
        assert "httpx" not in source, "get_email_status should delegate to FFI, not httpx"

    def test_phase2_methods_are_documented(self) -> None:
        """Methods that still use httpx must document why (Phase 2, native, etc.)."""
        from haiai.client import HaiClient

        # These methods are known to still use httpx for legitimate reasons
        phase2_methods = [
            "testconnection",
            "sign_email",
            "verify_email",
            "create_attestation",
            "list_attestations",
            "get_attestation",
            "verify_attestation",
        ]

        # Acceptable justification markers in source comments
        justification_markers = [
            "PHASE2", "phase2", "TODO", "native", "keep native",
            "stays native", "still uses", "not yet",
        ]

        for method_name in phase2_methods:
            method = getattr(HaiClient, method_name, None)
            if method is None:
                continue
            source = inspect.getsource(method)
            if "httpx" in source:
                source_lower = source.lower()
                has_justification = any(
                    marker.lower() in source_lower for marker in justification_markers
                )
                assert has_justification, (
                    f"{method_name} uses httpx but lacks a migration/justification comment"
                )
