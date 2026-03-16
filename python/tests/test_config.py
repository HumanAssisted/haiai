"""Tests for haiai.config module.

config.load() now initializes a JACS binding-core JacsAgent.
Tests that require real crypto operations are skipped when bindings
are unavailable.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from haiai.config import (
    get_config,
    get_agent,
    get_private_key,
    is_loaded,
    load,
    load_private_key_password,
    reset,
    save,
)


class TestLoad:
    def test_load_missing_file(self) -> None:
        reset()
        with pytest.raises(FileNotFoundError, match="JACS config not found"):
            load("/nonexistent/path/config.json")

    def test_load_missing_fields(self, tmp_path: Path) -> None:
        reset()
        config = {"jacsAgentName": "test"}
        p = tmp_path / "bad.json"
        p.write_text(json.dumps(config))
        with pytest.raises(ValueError, match="missing required fields"):
            load(str(p))

    def test_load_valid_config_with_jacs(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Test load() with real JACS bindings."""
        reset()
        try:
            from jacs import SimpleAgent
        except ImportError:
            pytest.skip("JACS bindings not available")

        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "TestConfig!2026")

        # Create a real agent with keys
        key_dir = tmp_path / "keys"
        key_dir.mkdir()
        data_dir = tmp_path / "jacs_data"
        data_dir.mkdir()

        _agent, info = SimpleAgent.create_agent(
            name="TestAgent",
            password="TestConfig!2026",
            algorithm="ring-Ed25519",
            data_directory=str(data_dir),
            key_directory=str(key_dir),
            config_path=str(tmp_path / "jacs_internal.config.json"),
            description="",
            domain="",
            default_storage="fs",
        )

        # Write HAI-format config
        config = {
            "jacsAgentName": "TestAgent",
            "jacsAgentVersion": "1.0.0",
            "jacsKeyDir": str(key_dir),
            "jacsId": "test-jacs-id-1234",
        }
        config_path = tmp_path / "jacs.config.json"
        config_path.write_text(json.dumps(config, indent=2))

        load(str(config_path))
        assert is_loaded()
        cfg = get_config()
        assert cfg.name == "TestAgent"
        assert cfg.version == "1.0.0"
        assert cfg.jacs_id == "test-jacs-id-1234"

        agent = get_agent()
        assert agent is not None

        reset()


class TestGetters:
    def test_get_config_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="has not been called"):
            get_config()

    def test_get_agent_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="has not been called"):
            get_agent()

    def test_get_private_key_before_load(self) -> None:
        """get_private_key is a backward compat alias for get_agent."""
        reset()
        with pytest.raises(RuntimeError, match="has not been called"):
            get_private_key()

    def test_is_loaded_false(self) -> None:
        reset()
        assert not is_loaded()


class TestSave:
    def test_save_round_trip(self, loaded_config: None, tmp_path: Path) -> None:
        out = tmp_path / "saved.json"
        save(str(out))
        data = json.loads(out.read_text())
        assert data["jacsAgentName"] == "TestAgent"
        assert data["jacsId"] == "test-jacs-id-1234"

    def test_save_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="Nothing to save"):
            save("/tmp/nope.json")


class TestReset:
    def test_reset_clears_state(self, loaded_config: None) -> None:
        assert is_loaded()
        reset()
        assert not is_loaded()


class TestPasswordResolution:
    def test_env_password_default_source(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "env-password")
        monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
        assert load_private_key_password() == b"env-password"

    def test_password_file_source_when_env_disabled(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        password_file = tmp_path / "password.txt"
        password_file.write_text("file-password\n", encoding="utf-8")
        if os.name != "nt":
            password_file.chmod(0o600)

        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "env-password")
        monkeypatch.setenv("JACS_PASSWORD_FILE", str(password_file))
        monkeypatch.setenv("JACS_DISABLE_PASSWORD_ENV", "1")

        assert load_private_key_password() == b"file-password"

    def test_multiple_sources_raise(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        password_file = tmp_path / "password.txt"
        password_file.write_text("file-password\n", encoding="utf-8")

        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "env-password")
        monkeypatch.setenv("JACS_PASSWORD_FILE", str(password_file))

        with pytest.raises(ValueError, match="Multiple password sources configured"):
            load_private_key_password()

    def test_password_file_missing_raises(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        monkeypatch.setenv("JACS_PASSWORD_FILE", "/nonexistent/password.txt")
        monkeypatch.setenv("JACS_DISABLE_PASSWORD_ENV", "1")
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(FileNotFoundError, match="JACS_PASSWORD_FILE"):
            load_private_key_password()

    def test_password_file_insecure_permissions_raises(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        if os.name == "nt":
            pytest.skip("permission-mode checks are unix-specific")

        password_file = tmp_path / "password-insecure.txt"
        password_file.write_text("file-password\n", encoding="utf-8")
        password_file.chmod(0o644)

        monkeypatch.setenv("JACS_PASSWORD_FILE", str(password_file))
        monkeypatch.setenv("JACS_DISABLE_PASSWORD_ENV", "1")
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(ValueError, match="insecure permissions"):
            load_private_key_password()

    def test_password_file_symlink_raises(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        if os.name == "nt":
            pytest.skip("symlink checks are unix-specific")

        target = tmp_path / "password-target.txt"
        target.write_text("file-password\n", encoding="utf-8")
        target.chmod(0o600)
        link = tmp_path / "password-link.txt"
        link.symlink_to(target)

        monkeypatch.setenv("JACS_PASSWORD_FILE", str(link))
        monkeypatch.setenv("JACS_DISABLE_PASSWORD_ENV", "1")
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(ValueError, match="must not be a symlink"):
            load_private_key_password()

    def test_password_required_raises_without_any_source(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(ValueError, match="Private key password required"):
            load_private_key_password()
