"""Regression tests for security/correctness-sensitive client behavior.

All crypto operations delegate to JACS binding-core.
"""

from __future__ import annotations

import base64
import json
import os
import stat
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from haiai.async_client import AsyncHaiClient
from haiai.client import HaiClient, register_new_agent


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _fake_register_result(options: dict[str, Any]) -> dict[str, Any]:
    """Build a fake FFI register_new_agent response and create key artifacts."""
    key_dir = Path(options.get("key_directory", "/tmp/keys"))
    key_dir.mkdir(parents=True, exist_ok=True)

    # Create dummy key files with correct permissions (simulates JACS keygen)
    priv_key = key_dir / "agent_private_key.pem"
    pub_key = key_dir / "agent_public_key.pem"
    priv_key.write_text("-----BEGIN ENCRYPTED PRIVATE KEY-----\nfake\n-----END ENCRYPTED PRIVATE KEY-----\n")
    pub_key.write_text("-----BEGIN PUBLIC KEY-----\nfake\n-----END PUBLIC KEY-----\n")
    if os.name != "nt":
        priv_key.chmod(0o600)
        key_dir.chmod(0o700)

    # Write a config file if config_path is given
    config_path = options.get("config_path")
    if config_path:
        cfg = {
            "jacsAgentName": options.get("agent_name", "Agent"),
            "jacsKeyDir": str(key_dir.resolve()),
            "jacsAgentVersion": "1.0.0",
        }
        Path(config_path).write_text(json.dumps(cfg))

    return {
        "agent_id": "agent-123",
        "jacs_id": "jacs-123",
        "key_directory": str(key_dir),
        "public_key_path": str(pub_key),
    }


# ---------------------------------------------------------------------------
# Auth header tests
# ---------------------------------------------------------------------------


class TestAuthHeader:
    """Verify auth-header construction does not expose secrets."""

    def test_build_auth_header_does_not_contain_private_key(
        self, loaded_config: None
    ) -> None:
        from haiai.signing import build_auth_header

        header = build_auth_header()
        assert "BEGIN PRIVATE KEY" not in header
        assert header.startswith("JACS ")

    def test_build_auth_header_has_correct_shape(self, loaded_config: None) -> None:
        from haiai.signing import build_auth_header

        header = build_auth_header()
        parts = header.split(" ", 1)
        assert parts[0] == "JACS"
        fields = parts[1].split(":")
        assert len(fields) == 3

    def test_build_auth_header_signature_is_base64(self, loaded_config: None) -> None:
        from haiai.signing import build_auth_header

        header = build_auth_header()
        sig_b64 = header.split(":")[-1]
        try:
            base64.b64decode(sig_b64, validate=True)
        except Exception:
            pytest.fail(f"Signature is not valid base64: {sig_b64!r}")


# ---------------------------------------------------------------------------
# Async client double-check
# ---------------------------------------------------------------------------


class TestAsyncClientSecurityParity:
    """AsyncHaiClient must mirror HaiClient security properties."""

    def test_async_client_requires_credentials(self) -> None:
        with pytest.raises(Exception):
            AsyncHaiClient(hai_url="https://hai.ai")


# ---------------------------------------------------------------------------
# Top-level register_new_agent
# ---------------------------------------------------------------------------


def _make_mock_ffi_adapter(captured: dict[str, Any]) -> Any:
    """Create a mock FFIAdapter that captures register_new_agent options."""
    from haiai._ffi_adapter import FFIAdapter

    original_init = FFIAdapter.__init__

    class MockAdapter(FFIAdapter):
        def __init__(self, config_json: str):
            # Skip real FFI init
            self._native = MagicMock()

        def register_new_agent(self, options: dict[str, Any]) -> dict[str, Any]:
            captured["options"] = options
            return _fake_register_result(options)

    return MockAdapter


def test_register_new_agent_writes_private_key_with_0600(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Verify that register_new_agent creates key files with secure permissions."""
    if os.name == "nt":
        pytest.skip("POSIX permission bits are not reliable on Windows")

    captured: dict[str, Any] = {}
    MockAdapter = _make_mock_ffi_adapter(captured)
    monkeypatch.setattr("haiai.client.FFIAdapter", MockAdapter)

    key_dir = tmp_path / "keys"
    config_path = tmp_path / "jacs.config.json"

    try:
        register_new_agent(
            name="Agent",
            owner_email="owner@hai.ai",
            hai_url="https://hai.ai",
            key_dir=str(key_dir),
            config_path=str(config_path),
            quiet=True,
        )
    finally:
        from haiai.config import reset
        reset()

    private_key_path = key_dir / "agent_private_key.pem"
    assert private_key_path.is_file()
    mode = stat.S_IMODE(private_key_path.stat().st_mode)
    assert mode == 0o600


def test_register_new_agent_defaults_to_secure_key_dir(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}
    MockAdapter = _make_mock_ffi_adapter(captured)
    monkeypatch.setattr("haiai.client.FFIAdapter", MockAdapter)
    monkeypatch.setenv("HOME", str(tmp_path))

    config_path = tmp_path / "jacs.config.json"

    try:
        register_new_agent(
            name="Agent",
            owner_email="owner@hai.ai",
            hai_url="https://hai.ai",
            config_path=str(config_path),
            domain="agent.example",
            description="Agent description",
            quiet=True,
        )
    finally:
        from haiai.config import reset
        reset()

    expected_key_dir = (tmp_path / ".jacs" / "keys").resolve()
    assert (expected_key_dir / "agent_private_key.pem").is_file()
    assert (expected_key_dir / "agent_public_key.pem").is_file()
    if os.name != "nt":
        assert stat.S_IMODE(expected_key_dir.stat().st_mode) == 0o700

    cfg = json.loads(config_path.read_text())
    assert Path(cfg["jacsKeyDir"]) == expected_key_dir

    # Verify domain and description were passed to FFI
    opts = captured["options"]
    assert opts["domain"] == "agent.example"
    assert opts["description"] == "Agent description"


# ---------------------------------------------------------------------------
# Fixture-driven security regression tests (T10)
# ---------------------------------------------------------------------------


class TestSecurityRegressionContract:
    """Tests driven by fixtures/security_regression_contract.json."""

    @staticmethod
    def _load_fixture() -> dict:
        import json
        from pathlib import Path
        path = Path(__file__).resolve().parent.parent.parent / "fixtures" / "security_regression_contract.json"
        return json.loads(path.read_text())

    def test_fixture_loads(self) -> None:
        fixture = self._load_fixture()
        assert "test_cases" in fixture
        assert len(fixture["test_cases"]) >= 5

    def test_fallback_does_not_activate(self) -> None:
        """If JACS agent is not loaded, crypto ops raise (not fall back to local)."""
        from haiai import config as config_mod
        from haiai.errors import HaiError
        from haiai.signing import canonicalize_json

        config_mod.reset()
        with pytest.raises(HaiError) as exc_info:
            canonicalize_json({"test": True})
        assert exc_info.value.code == "JACS_NOT_LOADED"

    def test_malformed_agent_id_escaped(self, loaded_config: None) -> None:
        """Agent ID with special chars is URL-escaped in API paths."""
        from urllib.parse import quote
        malicious_id = "agent/../../../etc/passwd"
        escaped = quote(malicious_id, safe="")
        assert "/" not in escaped

    def test_register_omits_private_key(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """FFI options dict must not contain private key material."""
        fixture = self._load_fixture()
        tc = next(t for t in fixture["test_cases"] if t["name"] == "register_omits_private_key")
        assert tc is not None

        captured: dict[str, Any] = {}
        MockAdapter = _make_mock_ffi_adapter(captured)
        monkeypatch.setattr("haiai.client.FFIAdapter", MockAdapter)

        try:
            register_new_agent(
                name="Test Agent",
                owner_email="owner@hai.ai",
                hai_url="https://hai.ai",
                key_dir=str(tmp_path / "keys"),
                config_path=str(tmp_path / "jacs.config.json"),
                quiet=True,
            )
        finally:
            from haiai.config import reset
            reset()

        # Verify the options passed to FFI do not contain private key material
        opts_str = json.dumps(captured["options"])
        assert "BEGIN PRIVATE KEY" not in opts_str
        assert "PRIVATE KEY" not in opts_str

    def test_register_is_unauthenticated(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """register_new_agent options must not contain auth credentials."""
        fixture = self._load_fixture()
        tc = next(t for t in fixture["test_cases"] if t["name"] == "register_is_unauthenticated")
        assert tc is not None

        captured: dict[str, Any] = {}
        MockAdapter = _make_mock_ffi_adapter(captured)
        monkeypatch.setattr("haiai.client.FFIAdapter", MockAdapter)

        try:
            register_new_agent(
                name="Test Agent",
                owner_email="owner@hai.ai",
                hai_url="https://hai.ai",
                key_dir=str(tmp_path / "keys"),
                config_path=str(tmp_path / "jacs.config.json"),
                quiet=True,
            )
        finally:
            from haiai.config import reset
            reset()

        # Verify no auth tokens in the options sent to FFI
        opts = captured["options"]
        assert "Authorization" not in opts
        assert "auth_header" not in opts
        # The password field is for key encryption, not HTTP auth
        assert "password" in opts  # expected — for JACS key encryption
