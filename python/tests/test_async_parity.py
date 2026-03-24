"""Tests verifying async method parity with sync HaiClient.

Each new async method (free_run, submit_benchmark_response, send_signed_email,
rotate_keys) must exist on AsyncHaiClient with matching parameter signatures.

Also includes behavioral tests that mock FFI to verify correct endpoint calls.
"""
from __future__ import annotations

import asyncio
import inspect
from unittest.mock import AsyncMock, MagicMock, patch

import pytest


class TestAsyncMethodsExist:
    """Verify all expected async methods exist on AsyncHaiClient."""

    def test_free_run_exists(self) -> None:
        from haiai.async_client import AsyncHaiClient

        assert hasattr(AsyncHaiClient, "free_run")
        assert inspect.iscoroutinefunction(AsyncHaiClient.free_run)

    def test_submit_benchmark_response_exists(self) -> None:
        from haiai.async_client import AsyncHaiClient

        assert hasattr(AsyncHaiClient, "submit_benchmark_response")
        assert inspect.iscoroutinefunction(AsyncHaiClient.submit_benchmark_response)

    def test_send_signed_email_exists(self) -> None:
        from haiai.async_client import AsyncHaiClient

        assert hasattr(AsyncHaiClient, "send_signed_email")
        assert inspect.iscoroutinefunction(AsyncHaiClient.send_signed_email)

    def test_rotate_keys_exists(self) -> None:
        from haiai.async_client import AsyncHaiClient

        assert hasattr(AsyncHaiClient, "rotate_keys")
        assert inspect.iscoroutinefunction(AsyncHaiClient.rotate_keys)


class TestAsyncSignaturesMatchSync:
    """Verify async method parameter names match their sync counterparts."""

    @staticmethod
    def _param_names(method: object) -> list[str]:
        """Extract non-self parameter names from a method."""
        sig = inspect.signature(method)  # type: ignore[arg-type]
        return [
            name
            for name, p in sig.parameters.items()
            if name != "self"
        ]

    def test_free_run_signature_matches(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.client import HaiClient

        async_params = self._param_names(AsyncHaiClient.free_run)
        sync_params = self._param_names(HaiClient.free_run)
        assert async_params == sync_params, (
            f"free_run param mismatch: async={async_params}, sync={sync_params}"
        )

    def test_submit_benchmark_response_signature_matches(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.client import HaiClient

        async_params = self._param_names(AsyncHaiClient.submit_benchmark_response)
        sync_params = self._param_names(HaiClient.submit_benchmark_response)
        assert async_params == sync_params, (
            f"submit_benchmark_response param mismatch: async={async_params}, sync={sync_params}"
        )

    def test_send_signed_email_signature_matches(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.client import HaiClient

        async_params = self._param_names(AsyncHaiClient.send_signed_email)
        sync_params = self._param_names(HaiClient.send_signed_email)
        assert async_params == sync_params, (
            f"send_signed_email param mismatch: async={async_params}, sync={sync_params}"
        )

    def test_rotate_keys_signature_matches(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.client import HaiClient

        async_params = self._param_names(AsyncHaiClient.rotate_keys)
        sync_params = self._param_names(HaiClient.rotate_keys)
        assert async_params == sync_params, (
            f"rotate_keys param mismatch: async={async_params}, sync={sync_params}"
        )


class TestAsyncClientConstruction:
    """Verify AsyncHaiClient can be constructed."""

    def test_constructor(self) -> None:
        from haiai.async_client import AsyncHaiClient

        client = AsyncHaiClient(timeout=10.0)
        assert client._timeout == 10.0


# ---------------------------------------------------------------------------
# Behavioral async tests (mock FFI, verify correct endpoint calls)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
class TestAsyncFreeRunBehavior:
    """Verify async free_run calls the correct FFI method."""

    async def test_free_run_calls_ffi(self) -> None:
        from haiai.async_client import AsyncHaiClient

        response_data = {
            "run_id": "run-abc-123",
            "transcript": [
                {"role": "system", "content": "Hello", "timestamp": "2026-01-01T00:00:00Z"}
            ],
            "upsell_message": "Upgrade for scoring!",
        }

        client = AsyncHaiClient(timeout=30.0)
        mock_ffi = client._get_ffi()
        mock_ffi.responses["free_run"] = response_data

        result = await client.free_run("https://hai.ai")

        assert mock_ffi.calls[0][0] == "free_run"
        assert result.success is True
        assert result.run_id == "run-abc-123"

    async def test_free_run_rate_limited_raises(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.errors import RateLimited

        client = AsyncHaiClient(timeout=30.0)
        mock_ffi = client._get_ffi()

        def raise_rate_limited(*args, **kwargs):
            raise RateLimited("Rate limited", status_code=429)

        mock_ffi.responses["free_run"] = raise_rate_limited

        with pytest.raises(RateLimited):
            await client.free_run("https://hai.ai")


@pytest.mark.asyncio
class TestAsyncSubmitBenchmarkResponseBehavior:
    """Verify async submit_benchmark_response calls the correct FFI method."""

    async def test_submit_benchmark_response_calls_ffi(self) -> None:
        from haiai.async_client import AsyncHaiClient

        response_data = {
            "success": True,
            "job_id": "job-456",
            "message": "Response accepted",
        }

        client = AsyncHaiClient(timeout=30.0)
        mock_ffi = client._get_ffi()
        mock_ffi.responses["submit_response"] = response_data

        result = await client.submit_benchmark_response(
            hai_url="https://hai.ai",
            job_id="job-456",
            message="The content is safe",
            metadata={"confidence": 0.95},
        )

        assert mock_ffi.calls[0][0] == "submit_response"
        params = mock_ffi.calls[0][1][0]
        assert params["job_id"] == "job-456"
        assert result.success is True
        assert result.job_id == "job-456"


@pytest.mark.asyncio
class TestAsyncRotateKeysBehavior:
    """Verify async rotate_keys delegates to sync via asyncio.to_thread."""

    async def test_rotate_keys_delegates_to_sync(self) -> None:
        from haiai.async_client import AsyncHaiClient
        from haiai.models import RotationResult

        expected_result = RotationResult(
            jacs_id="test-agent-id:v2",
            old_version="v1",
            new_version="v2",
            new_public_key_hash="abc123",
            registered_with_hai=True,
            signed_agent_json='{"agent": "doc"}',
        )

        client = AsyncHaiClient(timeout=30.0)

        with patch("haiai.async_client.AsyncHaiClient._get_jacs_id", return_value="test-agent-id:v1"):
            # Patch HaiClient.rotate_keys to return our expected result
            with patch("haiai.client.HaiClient.rotate_keys", return_value=expected_result) as mock_rotate:
                result = await client.rotate_keys(
                    hai_url="https://hai.ai",
                    register_with_hai=True,
                    algorithm="pq2025",
                )

        assert result.old_version == "v1"
        assert result.new_version == "v2"
        assert result.registered_with_hai is True
        # Verify it was called with the right args
        mock_rotate.assert_called_once_with(
            hai_url="https://hai.ai",
            register_with_hai=True,
            config_path=None,
            algorithm="pq2025",
        )
