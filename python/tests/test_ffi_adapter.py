"""Tests for the FFI adapter error mapping and Python-side logic.

These tests exercise the error mapping functions in _ffi_adapter.py without
requiring a real haiipy native binding. They verify that FFI error strings
(in the format "{ErrorKind}: {message}") are correctly mapped to the
appropriate Python exception classes.
"""

from __future__ import annotations

import pytest

from haiai._ffi_adapter import map_ffi_error
from haiai.errors import (
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    EmailNotActive,
    RateLimited,
    RecipientNotFound,
)


class TestMapFFIError:
    """Test map_ffi_error() error mapping."""

    def test_auth_failed(self):
        err = RuntimeError("AuthFailed: JACS signature rejected")
        result = map_ffi_error(err)
        assert isinstance(result, HaiAuthError)
        assert "JACS signature rejected" in str(result)

    def test_rate_limited(self):
        err = RuntimeError("RateLimited: too many requests")
        result = map_ffi_error(err)
        assert isinstance(result, RateLimited)

    def test_not_found(self):
        err = RuntimeError("NotFound: resource missing")
        result = map_ffi_error(err)
        assert isinstance(result, HaiApiError)

    def test_not_found_email_not_active(self):
        err = RuntimeError("NotFound: email not active for agent")
        result = map_ffi_error(err)
        assert isinstance(result, EmailNotActive)

    def test_not_found_recipient(self):
        err = RuntimeError("NotFound: recipient not found")
        result = map_ffi_error(err)
        assert isinstance(result, RecipientNotFound)

    def test_network_failed(self):
        err = RuntimeError("NetworkFailed: connection refused")
        result = map_ffi_error(err)
        assert isinstance(result, HaiConnectionError)

    def test_api_error_with_status(self):
        err = RuntimeError("ApiError: status 500 internal server error")
        result = map_ffi_error(err)
        assert isinstance(result, HaiApiError)

    def test_api_error_email_not_active(self):
        err = RuntimeError("ApiError: status 403 email not active")
        result = map_ffi_error(err)
        assert isinstance(result, EmailNotActive)

    def test_api_error_recipient(self):
        err = RuntimeError("ApiError: status 400 recipient not found")
        result = map_ffi_error(err)
        assert isinstance(result, RecipientNotFound)

    def test_config_failed(self):
        err = RuntimeError("ConfigFailed: invalid base_url")
        result = map_ffi_error(err)
        assert isinstance(result, HaiError)
        assert "invalid base_url" in str(result)

    def test_serialization_failed(self):
        err = RuntimeError("SerializationFailed: invalid JSON")
        result = map_ffi_error(err)
        assert isinstance(result, HaiError)

    def test_invalid_argument(self):
        err = RuntimeError("InvalidArgument: message_id is required")
        result = map_ffi_error(err)
        assert isinstance(result, HaiError)

    def test_provider_error(self):
        err = RuntimeError("ProviderError: JACS agent not initialized")
        result = map_ffi_error(err)
        assert isinstance(result, HaiAuthError)

    def test_generic_fallback(self):
        err = RuntimeError("some unknown error")
        result = map_ffi_error(err)
        assert isinstance(result, HaiError)
        assert "some unknown error" in str(result)

    def test_empty_message(self):
        err = RuntimeError("")
        result = map_ffi_error(err)
        assert isinstance(result, HaiError)


class TestFFIAdapterImport:
    """Test that the FFI adapter module is importable."""

    def test_import_ffi_adapter(self):
        from haiai._ffi_adapter import FFIAdapter, AsyncFFIAdapter
        assert FFIAdapter is not None
        assert AsyncFFIAdapter is not None

    def test_ffi_adapter_requires_haiipy(self):
        """FFIAdapter constructor raises HaiError if haiipy is not installed."""
        # In test environment without maturin build, haiipy may not be available.
        # This test verifies the error handling path.
        try:
            from haiai._ffi_adapter import FFIAdapter
            adapter = FFIAdapter('{"base_url":"https://test.hai.ai","jacs_id":"test"}')
            # If haiipy IS installed, construction should succeed
            assert adapter is not None
        except HaiError as e:
            # Expected when haiipy is not installed
            assert "haiipy" in str(e).lower()
