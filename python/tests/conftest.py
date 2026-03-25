"""Shared test fixtures for the HAI SDK test suite.

All cryptographic operations delegate to JACS binding-core.
Test fixtures create ephemeral JACS agents for signing/verification.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any, Generator
from unittest.mock import MagicMock

import pytest


TEST_PRIVATE_KEY_PASSWORD = "test-private-key-password"


class _MockJacsAgent:
    """Mock JacsAgent for tests that don't need real crypto.

    Provides the same API as a real JacsAgent (sign_string, load, etc.).
    When real JACS bindings are available, tests should use actual agents.
    """

    def __init__(self) -> None:
        self._signatures: dict[str, str] = {}
        self._sig_counter = 0

    def sign_string(self, data: str) -> str:
        """Return a deterministic mock signature."""
        import base64
        import hashlib

        sig_bytes = hashlib.sha256(f"mock-sig:{data}".encode()).digest()
        sig_b64 = base64.b64encode(sig_bytes).decode("ascii")
        self._signatures[data] = sig_b64
        return sig_b64

    def canonicalize_json(self, json_str: str) -> str:
        """Return canonical JSON (sorted keys, compact separators)."""
        import json as _json

        obj = _json.loads(json_str)
        return _json.dumps(obj, sort_keys=True, separators=(",", ":"))

    def load(self, config_path: str) -> None:
        pass

    def verify_document(self, doc: str) -> bool:
        return True

    def get_agent_json(self) -> str:
        return '{"jacsId":"mock-agent","jacsName":"MockAgent"}'


def _try_get_real_agent() -> Any | None:
    """Try to create a real ephemeral JACS agent. Returns None if bindings unavailable."""
    try:
        from jacs import SimpleAgent
        agent, info = SimpleAgent.ephemeral("ring-Ed25519")
        # Wrap in EphemeralAgentAdapter for JacsAgent-compatible API
        from jacs.simple import _EphemeralAgentAdapter
        return _EphemeralAgentAdapter(agent)
    except Exception:
        return None


@pytest.fixture()
def jacs_agent() -> Any:
    """Provide a JACS agent for signing/verification.

    Uses real JACS bindings if available, falls back to mock.
    """
    real = _try_get_real_agent()
    if real is not None:
        return real
    return _MockJacsAgent()


@pytest.fixture()
def ed25519_keypair(jacs_agent: Any) -> tuple[Any, str]:
    """Backward-compatible keypair fixture.

    Returns (agent, public_key_pem). The agent provides sign_string()
    via JACS binding-core delegation.
    """
    # Return agent as the "key" -- callers use agent.sign_string(msg)
    pub_pem = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+mock+key+for+testing+purposes==\n-----END PUBLIC KEY-----\n"
    return jacs_agent, pub_pem


@pytest.fixture(autouse=True)
def password_env(monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """Default developer path: env-based password source."""
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", TEST_PRIVATE_KEY_PASSWORD)
    monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
    monkeypatch.delenv("JACS_DISABLE_PASSWORD_ENV", raising=False)
    monkeypatch.delenv("JACS_DISABLE_PASSWORD_FILE", raising=False)
    yield


@pytest.fixture()
def key_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Create a key directory with mock key files for config.load() tests.

    Since we delegate to JACS for crypto, we create minimal key files
    that the config loader expects to find.
    """
    kd = tmp_path / "keys"
    kd.mkdir()

    # Create placeholder key files (JACS agent will handle actual crypto)
    # These are needed for config.load() which checks file existence
    (kd / "agent_private_key.pem").write_text(
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nplaceholder\n-----END ENCRYPTED PRIVATE KEY-----\n"
    )
    (kd / "agent_public_key.pem").write_text(
        "-----BEGIN PUBLIC KEY-----\nplaceholder\n-----END PUBLIC KEY-----\n"
    )
    return kd


@pytest.fixture()
def jacs_config_path(tmp_path: Path, key_dir: Path) -> Path:
    """Create a minimal jacs.config.json and return its path."""
    config = {
        "jacsAgentName": "TestAgent",
        "jacsAgentVersion": "1.0.0",
        "jacsKeyDir": str(key_dir),
        "jacsId": "test-jacs-id-1234",
    }
    config_path = tmp_path / "jacs.config.json"
    config_path.write_text(json.dumps(config, indent=2))
    return config_path


@pytest.fixture()
def loaded_config(
    jacs_config_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Generator[None, None, None]:
    """Load the test config into the SDK module state, then clean up.

    Patches config.load() to use a mock agent since we may not have
    real JACS bindings in the test environment.
    """
    from haiai import config as config_mod

    # Load config metadata (name, version, key_dir, jacs_id)
    import json
    raw = json.loads(jacs_config_path.read_text())
    config_mod._config = config_mod.AgentConfig(
        name=raw["jacsAgentName"],
        version=raw["jacsAgentVersion"],
        key_dir=raw["jacsKeyDir"],
        jacs_id=raw.get("jacsId"),
    )

    # Try real JACS agent, fall back to mock
    real = _try_get_real_agent()
    config_mod._agent = real if real is not None else _MockJacsAgent()

    yield
    config_mod.reset()


@pytest.fixture()
def jacs_id() -> str:
    """Return the test JACS ID matching the config fixture."""
    return "test-jacs-id-1234"


class MockFFIAdapter:
    """Mock FFI adapter for tests that previously mocked httpx.

    Provides a programmable mock that records calls and returns
    pre-configured responses. Used by tests that need to verify
    what gets sent to the API without actually calling the FFI.
    """

    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple, dict]] = []
        self.responses: dict[str, Any] = {}

    def _record(self, method: str, *args: Any, **kwargs: Any) -> Any:
        self.calls.append((method, args, kwargs))
        resp = self.responses.get(method)
        if callable(resp):
            return resp(*args, **kwargs)
        if resp is not None:
            return resp
        return {}

    # Registration & Identity
    def hello(self, include_test: bool = False) -> dict:
        return self._record("hello", include_test)

    def check_username(self, username: str) -> dict:
        return self._record("check_username", username)

    def register(self, options: dict) -> dict:
        return self._record("register", options)

    def rotate_keys(self, options: dict) -> dict:
        return self._record("rotate_keys", options)

    def update_agent(self, agent_data: str) -> dict:
        return self._record("update_agent", agent_data)

    def submit_response(self, params: dict) -> dict:
        return self._record("submit_response", params)

    def verify_status(self, agent_id: str | None = None) -> dict:
        return self._record("verify_status", agent_id)

    # Username
    def claim_username(self, agent_id: str, username: str) -> dict:
        return self._record("claim_username", agent_id, username)

    def update_username(self, agent_id: str, username: str) -> dict:
        return self._record("update_username", agent_id, username)

    def delete_username(self, agent_id: str) -> dict:
        return self._record("delete_username", agent_id)

    # Email
    def send_email(self, options: dict) -> dict:
        return self._record("send_email", options)

    def send_signed_email(self, options: dict) -> dict:
        return self._record("send_signed_email", options)

    def list_messages(self, options: dict) -> list:
        return self._record("list_messages", options)

    def update_labels(self, params: dict) -> dict:
        return self._record("update_labels", params)

    def get_email_status(self) -> dict:
        return self._record("get_email_status")

    def get_message(self, message_id: str) -> dict:
        return self._record("get_message", message_id)

    def get_unread_count(self) -> int:
        result = self._record("get_unread_count")
        if isinstance(result, int):
            return result
        return result.get("count", 0) if isinstance(result, dict) else 0

    def mark_read(self, message_id: str) -> None:
        self._record("mark_read", message_id)

    def mark_unread(self, message_id: str) -> None:
        self._record("mark_unread", message_id)

    def delete_message(self, message_id: str) -> None:
        self._record("delete_message", message_id)

    def archive(self, message_id: str) -> None:
        self._record("archive", message_id)

    def unarchive(self, message_id: str) -> None:
        self._record("unarchive", message_id)

    def reply_with_options(self, params: dict) -> dict:
        return self._record("reply_with_options", params)

    def forward(self, params: dict) -> dict:
        return self._record("forward", params)

    # Search & Contacts
    def search_messages(self, options: dict) -> list:
        return self._record("search_messages", options)

    def contacts(self) -> list:
        return self._record("contacts")

    # Key Operations
    def fetch_remote_key(self, jacs_id: str, version: str = "latest") -> dict:
        return self._record("fetch_remote_key", jacs_id, version)

    def fetch_key_by_hash(self, hash_val: str) -> dict:
        return self._record("fetch_key_by_hash", hash_val)

    def fetch_key_by_email(self, email: str) -> dict:
        return self._record("fetch_key_by_email", email)

    def fetch_key_by_domain(self, domain: str) -> dict:
        return self._record("fetch_key_by_domain", domain)

    def fetch_all_keys(self, jacs_id: str) -> dict:
        return self._record("fetch_all_keys", jacs_id)

    # Verification
    def verify_document(self, document: str) -> dict:
        return self._record("verify_document", document)

    def get_verification(self, agent_id: str) -> dict:
        return self._record("get_verification", agent_id)

    def verify_agent_document(self, request_json: str) -> dict:
        return self._record("verify_agent_document", request_json)

    # Benchmarks
    def benchmark(self, name: str | None = None, tier: str | None = None) -> dict:
        return self._record("benchmark", name, tier)

    def free_run(self, transport: str | None = None) -> dict:
        return self._record("free_run", transport)

    def pro_run(self, options: dict) -> dict:
        return self._record("pro_run", options)

    def enterprise_run(self) -> None:
        self._record("enterprise_run")

    # JACS Delegation
    def build_auth_header(self) -> str:
        return self._record("build_auth_header")

    def sign_message(self, message: str) -> str:
        return self._record("sign_message", message)

    def canonical_json(self, value_json: str) -> str:
        return self._record("canonical_json", value_json)

    def verify_a2a_artifact(self, wrapped_json: str) -> dict:
        return self._record("verify_a2a_artifact", wrapped_json)

    def export_agent_json(self) -> dict:
        return self._record("export_agent_json")

    def jacs_id_sync(self) -> str:
        return self._record("jacs_id")

    # Registration (additional)
    def register_new_agent(self, options: dict) -> dict:
        return self._record("register_new_agent", options)

    # Email Sign/Verify (raw)
    def sign_email_raw(self, raw_email_b64: str) -> str:
        return self._record("sign_email_raw", raw_email_b64)

    def verify_email_raw(self, raw_email_b64: str) -> dict:
        return self._record("verify_email_raw", raw_email_b64)

    # Email Templates
    def create_email_template(self, options: dict) -> dict:
        return self._record("create_email_template", options)

    def list_email_templates(self, options: dict) -> dict:
        return self._record("list_email_templates", options)

    def get_email_template(self, template_id: str) -> dict:
        return self._record("get_email_template", template_id)

    def update_email_template(self, template_id: str, options: dict) -> dict:
        return self._record("update_email_template", template_id, options)

    def delete_email_template(self, template_id: str) -> None:
        self._record("delete_email_template", template_id)

    # Server Keys
    def fetch_server_keys(self) -> dict:
        return self._record("fetch_server_keys")

    # Attestations
    def create_attestation(self, params: dict) -> dict:
        return self._record("create_attestation", params)

    def list_attestations(self, params: dict) -> dict:
        return self._record("list_attestations", params)

    def get_attestation(self, agent_id: str, doc_id: str) -> dict:
        return self._record("get_attestation", agent_id, doc_id)

    def verify_attestation(self, document: str) -> dict:
        return self._record("verify_attestation", document)

    # Streaming
    def connect_sse(self) -> int:
        result = self._record("connect_sse")
        return result if isinstance(result, int) else 0

    def sse_next_event(self, handle: int) -> Any:
        return self._record("sse_next_event", handle)

    def sse_close(self, handle: int) -> None:
        self._record("sse_close", handle)

    def connect_ws(self) -> int:
        result = self._record("connect_ws")
        return result if isinstance(result, int) else 0

    def ws_next_event(self, handle: int) -> Any:
        return self._record("ws_next_event", handle)

    def ws_close(self, handle: int) -> None:
        self._record("ws_close", handle)

    # Client State (accessors)
    def base_url(self) -> str:
        result = self._record("base_url")
        return result if isinstance(result, str) else ""

    def hai_agent_id(self) -> str:
        result = self._record("hai_agent_id")
        return result if isinstance(result, str) else ""

    def agent_email(self) -> Any:
        return self._record("agent_email")

    def set_hai_agent_id(self, agent_id: str) -> None:
        self._record("set_hai_agent_id", agent_id)

    def set_agent_email(self, email: str) -> None:
        self._record("set_agent_email", email)


class MockAsyncFFIAdapter(MockFFIAdapter):
    """Async version of MockFFIAdapter for AsyncHaiClient tests."""

    async def _arecord(self, method: str, *args: Any, **kwargs: Any) -> Any:
        return self._record(method, *args, **kwargs)

    # Override all methods to be async
    async def hello(self, include_test: bool = False) -> dict:  # type: ignore[override]
        return self._record("hello", include_test)

    async def check_username(self, username: str) -> dict:  # type: ignore[override]
        return self._record("check_username", username)

    async def register(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("register", options)

    async def verify_status(self, agent_id: str | None = None) -> dict:  # type: ignore[override]
        return self._record("verify_status", agent_id)

    async def claim_username(self, agent_id: str, username: str) -> dict:  # type: ignore[override]
        return self._record("claim_username", agent_id, username)

    async def update_username(self, agent_id: str, username: str) -> dict:  # type: ignore[override]
        return self._record("update_username", agent_id, username)

    async def delete_username(self, agent_id: str) -> dict:  # type: ignore[override]
        return self._record("delete_username", agent_id)

    async def send_email(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("send_email", options)

    async def send_signed_email(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("send_signed_email", options)

    async def list_messages(self, options: dict) -> list:  # type: ignore[override]
        return self._record("list_messages", options)

    async def update_labels(self, params: dict) -> dict:  # type: ignore[override]
        return self._record("update_labels", params)

    async def get_email_status(self) -> dict:  # type: ignore[override]
        return self._record("get_email_status")

    async def get_message(self, message_id: str) -> dict:  # type: ignore[override]
        return self._record("get_message", message_id)

    async def get_unread_count(self) -> int:  # type: ignore[override]
        result = self._record("get_unread_count")
        if isinstance(result, int):
            return result
        return result.get("count", 0) if isinstance(result, dict) else 0

    async def mark_read(self, message_id: str) -> None:  # type: ignore[override]
        self._record("mark_read", message_id)

    async def mark_unread(self, message_id: str) -> None:  # type: ignore[override]
        self._record("mark_unread", message_id)

    async def delete_message(self, message_id: str) -> None:  # type: ignore[override]
        self._record("delete_message", message_id)

    async def archive(self, message_id: str) -> None:  # type: ignore[override]
        self._record("archive", message_id)

    async def unarchive(self, message_id: str) -> None:  # type: ignore[override]
        self._record("unarchive", message_id)

    async def forward(self, params: dict) -> dict:  # type: ignore[override]
        return self._record("forward", params)

    async def search_messages(self, options: dict) -> list:  # type: ignore[override]
        return self._record("search_messages", options)

    async def contacts(self) -> list:  # type: ignore[override]
        return self._record("contacts")

    async def fetch_remote_key(self, jacs_id: str, version: str = "latest") -> dict:  # type: ignore[override]
        return self._record("fetch_remote_key", jacs_id, version)

    async def fetch_key_by_hash(self, hash_val: str) -> dict:  # type: ignore[override]
        return self._record("fetch_key_by_hash", hash_val)

    async def fetch_key_by_email(self, email: str) -> dict:  # type: ignore[override]
        return self._record("fetch_key_by_email", email)

    async def fetch_key_by_domain(self, domain: str) -> dict:  # type: ignore[override]
        return self._record("fetch_key_by_domain", domain)

    async def fetch_all_keys(self, jacs_id: str) -> dict:  # type: ignore[override]
        return self._record("fetch_all_keys", jacs_id)

    async def verify_document(self, document: str) -> dict:  # type: ignore[override]
        return self._record("verify_document", document)

    async def get_verification(self, agent_id: str) -> dict:  # type: ignore[override]
        return self._record("get_verification", agent_id)

    async def verify_agent_document(self, request_json: str) -> dict:  # type: ignore[override]
        return self._record("verify_agent_document", request_json)

    async def benchmark(self, name: str | None = None, tier: str | None = None) -> dict:  # type: ignore[override]
        return self._record("benchmark", name, tier)

    async def free_run(self, transport: str | None = None) -> dict:  # type: ignore[override]
        return self._record("free_run", transport)

    async def pro_run(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("pro_run", options)

    async def submit_response(self, params: dict) -> dict:  # type: ignore[override]
        return self._record("submit_response", params)

    async def register_new_agent(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("register_new_agent", options)

    async def sign_email_raw(self, raw_email_b64: str) -> str:  # type: ignore[override]
        return self._record("sign_email_raw", raw_email_b64)

    async def verify_email_raw(self, raw_email_b64: str) -> dict:  # type: ignore[override]
        return self._record("verify_email_raw", raw_email_b64)

    async def create_email_template(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("create_email_template", options)

    async def list_email_templates(self, options: dict) -> dict:  # type: ignore[override]
        return self._record("list_email_templates", options)

    async def get_email_template(self, template_id: str) -> dict:  # type: ignore[override]
        return self._record("get_email_template", template_id)

    async def update_email_template(self, template_id: str, options: dict) -> dict:  # type: ignore[override]
        return self._record("update_email_template", template_id, options)

    async def delete_email_template(self, template_id: str) -> None:  # type: ignore[override]
        self._record("delete_email_template", template_id)

    async def fetch_server_keys(self) -> dict:  # type: ignore[override]
        return self._record("fetch_server_keys")

    async def create_attestation(self, params: dict) -> dict:  # type: ignore[override]
        return self._record("create_attestation", params)

    async def list_attestations(self, params: dict) -> dict:  # type: ignore[override]
        return self._record("list_attestations", params)

    async def get_attestation(self, agent_id: str, doc_id: str) -> dict:  # type: ignore[override]
        return self._record("get_attestation", agent_id, doc_id)

    async def verify_attestation(self, document: str) -> dict:  # type: ignore[override]
        return self._record("verify_attestation", document)

    async def connect_sse(self) -> int:  # type: ignore[override]
        result = self._record("connect_sse")
        return result if isinstance(result, int) else 0

    async def sse_next_event(self, handle: int) -> Any:  # type: ignore[override]
        return self._record("sse_next_event", handle)

    async def sse_close(self, handle: int) -> None:  # type: ignore[override]
        self._record("sse_close", handle)

    async def connect_ws(self) -> int:  # type: ignore[override]
        result = self._record("connect_ws")
        return result if isinstance(result, int) else 0

    async def ws_next_event(self, handle: int) -> Any:  # type: ignore[override]
        return self._record("ws_next_event", handle)

    async def ws_close(self, handle: int) -> None:  # type: ignore[override]
        self._record("ws_close", handle)

    async def base_url(self) -> str:  # type: ignore[override]
        result = self._record("base_url")
        return result if isinstance(result, str) else ""

    async def hai_agent_id(self) -> str:  # type: ignore[override]
        result = self._record("hai_agent_id")
        return result if isinstance(result, str) else ""

    async def agent_email(self) -> Any:  # type: ignore[override]
        return self._record("agent_email")


@pytest.fixture()
def mock_ffi() -> MockFFIAdapter:
    """Provide a programmable mock FFI adapter for sync client tests."""
    return MockFFIAdapter()


@pytest.fixture()
def mock_async_ffi() -> MockAsyncFFIAdapter:
    """Provide a programmable mock FFI adapter for async client tests."""
    return MockAsyncFFIAdapter()


@pytest.fixture()
def ffi_client(loaded_config: None, mock_ffi: MockFFIAdapter) -> tuple[Any, MockFFIAdapter]:
    """Provide a HaiClient with a mock FFI adapter pre-injected.

    Returns (client, mock_ffi) so tests can set up responses and verify calls.
    """
    from haiai.client import HaiClient
    client = HaiClient()
    client._ffi = mock_ffi  # type: ignore[assignment]
    return client, mock_ffi


@pytest.fixture()
def async_ffi_client(loaded_config: None, mock_async_ffi: MockAsyncFFIAdapter) -> tuple[Any, MockAsyncFFIAdapter]:
    """Provide an AsyncHaiClient with a mock FFI adapter pre-injected.

    Returns (client, mock_ffi) so tests can set up responses and verify calls.
    """
    from haiai.async_client import AsyncHaiClient
    client = AsyncHaiClient()
    client._ffi = mock_async_ffi  # type: ignore[assignment]
    return client, mock_async_ffi


@pytest.fixture(autouse=True)
def _auto_mock_ffi(monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """Auto-mock the FFI adapter for all tests.

    This ensures tests that create HaiClient/AsyncHaiClient instances
    don't fail when haiipy is not installed. The mock FFI adapter
    returns empty dicts by default -- tests that need specific responses
    should set them via `client._ffi.responses["method_name"] = {...}`.

    Tests that previously mocked httpx.post/get will need updating to
    mock at the FFI level instead.
    """
    from haiai.client import HaiClient
    from haiai.async_client import AsyncHaiClient

    original_sync = HaiClient._get_ffi
    original_async = AsyncHaiClient._get_ffi

    def _patched_sync_get_ffi(self: Any) -> Any:
        if self._ffi is None:
            self._ffi = MockFFIAdapter()
        return self._ffi

    def _patched_async_get_ffi(self: Any) -> Any:
        if self._ffi is None:
            self._ffi = MockAsyncFFIAdapter()
        return self._ffi

    monkeypatch.setattr(HaiClient, "_get_ffi", _patched_sync_get_ffi)
    monkeypatch.setattr(AsyncHaiClient, "_get_ffi", _patched_async_get_ffi)

    yield
