"""Tests verifying async method parity with sync HaiClient.

Each new async method (free_run, submit_benchmark_response, send_signed_email,
rotate_keys) must exist on AsyncHaiClient with matching parameter signatures.

Also includes behavioral tests that mock FFI to verify correct endpoint calls.
"""

from __future__ import annotations

import inspect
import json
from dataclasses import replace
from unittest.mock import AsyncMock, Mock, patch

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
        return [name for name, p in sig.parameters.items() if name != "self"]

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
        assert (
            inspect.signature(HaiClient.rotate_keys).parameters["algorithm"].default
            == "pq2025"
        )
        assert (
            inspect.signature(AsyncHaiClient.rotate_keys)
            .parameters["algorithm"]
            .default
            == "pq2025"
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
                {
                    "role": "system",
                    "content": "Hello",
                    "timestamp": "2026-01-01T00:00:00Z",
                }
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
    """Rotation must stay on the existing async FFI client and its pinned context."""

    @pytest.mark.parametrize("registered", [True, False])
    async def test_rotate_keys_delegates_to_existing_async_ffi(
        self, loaded_config, tmp_path, monkeypatch, registered
    ) -> None:
        from haiai import config as config_mod
        from haiai.async_client import AsyncHaiClient
        from haiai.models import RotationResult

        cfg = config_mod.get_config()
        config_path = tmp_path / "canonical.config.json"
        monkeypatch.setattr(config_mod, "_loaded_config_path", config_path)
        native_agent = Mock()
        native_agent.rotate_keys.side_effect = AssertionError(
            "rotation belongs to the HAI FFI client"
        )
        monkeypatch.setattr(
            config_mod, "_get_native_agent", Mock(return_value=native_agent)
        )
        data = {
            "jacs_id": cfg.jacs_id,
            "old_version": cfg.version,
            "new_version": "v2-rotated",
            "new_public_key_hash": "a" * 64,
            "registered_with_hai": registered,
            "signed_agent_json": json.dumps(
                {"jacsId": cfg.jacs_id, "jacsVersion": "v2-rotated"}
            ),
        }
        ffi = Mock()
        ffi.jacs_id = AsyncMock(return_value=cfg.jacs_id)
        ffi.base_url = AsyncMock(return_value="https://staging.hai.example")
        ffi.rotate_keys = AsyncMock(return_value=data)

        def reload_config(path):
            assert path == str(config_path)
            ffi.rotate_keys.assert_awaited_once()
            config_mod._config = replace(cfg, version=data["new_version"])

        reload = Mock(side_effect=reload_config)
        monkeypatch.setattr(config_mod, "load", reload)
        client = AsyncHaiClient(request_auth_audience="staging.hai")
        client._ffi = ffi

        with patch(
            "haiai.client.HaiClient.rotate_keys",
            side_effect=AssertionError("use async FFI"),
        ) as sync_rotate:
            result = await client.rotate_keys(
                hai_url="https://staging.hai.example/",
                config_path=str(config_path),
                algorithm="ring-Ed25519",
            )

        ffi.rotate_keys.assert_awaited_once_with(
            {"register_with_hai": True, "algorithm": "ring-Ed25519"}
        )
        ffi.register.assert_not_called()
        sync_rotate.assert_not_called()
        native_agent.rotate_keys.assert_not_called()
        reload.assert_called_once_with(str(config_path))
        assert client._ffi is ffi
        assert client._request_auth_audience == "staging.hai"
        assert result == RotationResult(**data)
        assert config_mod.get_config().version == result.new_version

    @pytest.mark.parametrize("mismatch", ["config_path", "jacs_id", "hai_url"])
    async def test_rotation_rejects_mismatched_context_before_ffi(
        self, loaded_config, tmp_path, monkeypatch, mismatch
    ) -> None:
        from haiai import config as config_mod
        from haiai.async_client import AsyncHaiClient
        from haiai.errors import HaiAuthError

        config_path = tmp_path / "canonical.config.json"
        monkeypatch.setattr(config_mod, "_loaded_config_path", config_path)
        ffi = Mock()
        ffi.jacs_id = AsyncMock(return_value=config_mod.get_config().jacs_id)
        ffi.base_url = AsyncMock(return_value="https://staging.hai.example")
        ffi.rotate_keys = AsyncMock()
        client = AsyncHaiClient()
        client._ffi = ffi
        kwargs = {}
        if mismatch == "config_path":
            kwargs["config_path"] = str(config_path.with_name("other.config.json"))
        elif mismatch == "jacs_id":
            ffi.jacs_id.return_value = "another-agent"
        else:
            kwargs["hai_url"] = "https://another.hai.example"

        with pytest.raises(HaiAuthError):
            await client.rotate_keys(**kwargs)

        ffi.rotate_keys.assert_not_awaited()


@pytest.mark.asyncio
class TestAsyncHelloAndHealthParity:
    """Verify the async client matches the sync client's FFI health/hello behavior."""

    async def test_testconnection_calls_ffi_hello(
        self, async_ffi_client: tuple[object, object]
    ) -> None:
        client, mock_ffi = async_ffi_client
        mock_ffi.responses["hello"] = {"message": "ok"}

        result = await client.testconnection("https://hai.ai")

        assert result is True
        assert mock_ffi.calls[0][0] == "hello"
        assert mock_ffi.calls[0][1] == (False,)

    async def test_hello_world_calls_ffi_and_verifies_signature(
        self, async_ffi_client: tuple[object, object]
    ) -> None:
        client, mock_ffi = async_ffi_client
        mock_ffi.responses["hello"] = {
            "timestamp": "2026-01-01T00:00:00Z",
            "client_ip": "127.0.0.1",
            "hai_public_key_fingerprint": "fp",
            "message": "ok",
            "hello_id": "hello-1",
            "hai_signed_ack": "sig",
        }

        with patch.object(client, "verify_hai_message", return_value=True) as verify:
            result = await client.hello_world("https://hai.ai", include_test=True)

        assert mock_ffi.calls[0][0] == "hello"
        assert mock_ffi.calls[0][1] == (True,)
        verify.assert_called_once()
        assert result.message == "ok"
        assert result.hai_signature_valid is True


@pytest.mark.asyncio
class TestAsyncTransportParity:
    """Verify async streaming matches the sync client's retry and cleanup behavior."""

    @pytest.mark.parametrize(
        ("transport", "connect_method", "next_method", "close_method"),
        [
            ("sse", "connect_sse", "sse_next_event", "sse_close"),
            ("ws", "connect_ws", "ws_next_event", "ws_close"),
        ],
    )
    async def test_connect_updates_last_event_id_and_closes_handle(
        self,
        async_ffi_client: tuple[object, object],
        transport: str,
        connect_method: str,
        next_method: str,
        close_method: str,
    ) -> None:
        client, mock_ffi = async_ffi_client
        mock_ffi.responses[connect_method] = 42
        events = iter(
            [
                {
                    "event_type": "connected",
                    "data": {"agent_id": "agent-1"},
                    "id": "evt-42",
                    "raw": '{"agent_id":"agent-1"}',
                },
                None,
            ]
        )
        mock_ffi.responses[next_method] = lambda handle: next(events)

        stream = client.connect("https://hai.ai", transport=transport)
        seen = [await anext(stream)]
        await stream.aclose()

        assert len(seen) == 1
        assert seen[0].event_type == "connected"
        assert seen[0].id == "evt-42"
        assert client._last_event_id == "evt-42"
        assert mock_ffi.calls[0][0] == connect_method
        assert mock_ffi.calls[-1][0] == close_method
        assert mock_ffi.calls[-1][1] == (42,)

    @pytest.mark.parametrize(
        ("transport", "connect_method", "next_method", "close_method"),
        [
            ("sse", "connect_sse", "sse_next_event", "sse_close"),
            ("ws", "connect_ws", "ws_next_event", "ws_close"),
        ],
    )
    async def test_connect_retries_until_a_transport_connects(
        self,
        async_ffi_client: tuple[object, object],
        monkeypatch: pytest.MonkeyPatch,
        transport: str,
        connect_method: str,
        next_method: str,
        close_method: str,
    ) -> None:
        from haiai._retry import RETRY_MAX_ATTEMPTS  # noqa: F401

        client, mock_ffi = async_ffi_client
        attempts = {"count": 0}
        sleep_delays: list[float] = []

        def flaky_connect() -> int:
            attempts["count"] += 1
            if attempts["count"] < 3:
                raise RuntimeError("temporary failure")
            return 99

        events = iter(
            [
                {
                    "event_type": "connected",
                    "data": {"ok": True},
                    "id": "evt-ok",
                    "raw": "raw",
                },
                None,
            ]
        )

        async def fake_sleep(delay: float) -> None:
            sleep_delays.append(delay)

        monkeypatch.setattr("haiai.async_client.RETRY_MAX_ATTEMPTS", 5)
        monkeypatch.setattr("haiai.async_client.backoff", lambda attempt: 0.5 + attempt)
        monkeypatch.setattr("haiai.async_client.asyncio.sleep", fake_sleep)

        mock_ffi.responses[connect_method] = flaky_connect
        mock_ffi.responses[next_method] = lambda handle: next(events)

        stream = client.connect("https://hai.ai", transport=transport)
        seen = [await anext(stream)]
        await stream.aclose()

        assert len(seen) == 1
        assert seen[0].id == "evt-ok"
        assert attempts["count"] == 3
        assert sleep_delays == [0.5, 1.5]
        assert mock_ffi.calls[-1][0] == close_method
        assert mock_ffi.calls[-1][1] == (99,)

    @pytest.mark.parametrize(
        ("transport", "connect_method"),
        [
            ("sse", "connect_sse"),
            ("ws", "connect_ws"),
        ],
    )
    async def test_connect_exhaustion_raises_connection_error(
        self,
        async_ffi_client: tuple[object, object],
        monkeypatch: pytest.MonkeyPatch,
        transport: str,
        connect_method: str,
    ) -> None:
        from haiai.errors import HaiConnectionError

        client, mock_ffi = async_ffi_client
        attempts = {"count": 0}
        sleep_delays: list[float] = []

        def always_fail() -> int:
            attempts["count"] += 1
            raise RuntimeError("still down")

        async def fake_sleep(delay: float) -> None:
            sleep_delays.append(delay)

        monkeypatch.setattr("haiai.async_client.RETRY_MAX_ATTEMPTS", 1)
        monkeypatch.setattr(
            "haiai.async_client.backoff", lambda attempt: float(attempt)
        )
        monkeypatch.setattr("haiai.async_client.asyncio.sleep", fake_sleep)

        mock_ffi.responses[connect_method] = always_fail

        with pytest.raises(HaiConnectionError):
            async for _ in client.connect("https://hai.ai", transport=transport):
                pass

        assert attempts["count"] == 2
        assert sleep_delays == [0.0]

    @pytest.mark.parametrize(
        ("transport", "connect_method", "next_method", "close_method"),
        [
            ("sse", "connect_sse", "sse_next_event", "sse_close"),
            ("ws", "connect_ws", "ws_next_event", "ws_close"),
        ],
    )
    async def test_connect_reraises_auth_errors_and_still_closes_handle(
        self,
        async_ffi_client: tuple[object, object],
        transport: str,
        connect_method: str,
        next_method: str,
        close_method: str,
    ) -> None:
        from haiai.errors import HaiAuthError

        client, mock_ffi = async_ffi_client
        mock_ffi.responses[connect_method] = 11
        mock_ffi.responses[next_method] = lambda handle: (_ for _ in ()).throw(
            HaiAuthError("auth failed", status_code=401)
        )

        with pytest.raises(HaiAuthError):
            async for _ in client.connect("https://hai.ai", transport=transport):
                pass

        assert mock_ffi.calls[-1][0] == close_method
        assert mock_ffi.calls[-1][1] == (11,)
