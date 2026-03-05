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
    from jacs.hai import config as config_mod

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
