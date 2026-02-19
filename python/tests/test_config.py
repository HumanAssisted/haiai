"""Tests for jacs.hai.config module."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from jacs.hai.config import (
    get_config,
    get_private_key,
    is_loaded,
    load,
    load_private_key_password,
    reset,
    save,
)


class TestLoad:
    def test_load_valid_config(self, jacs_config_path: Path) -> None:
        reset()
        load(str(jacs_config_path))
        assert is_loaded()
        cfg = get_config()
        assert cfg.name == "TestAgent"
        assert cfg.version == "1.0.0"
        assert cfg.jacs_id == "test-jacs-id-1234"
        reset()

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

    def test_load_no_pem_file(self, tmp_path: Path) -> None:
        reset()
        kd = tmp_path / "empty_keys"
        kd.mkdir()
        config = {
            "jacsAgentName": "test",
            "jacsAgentVersion": "1.0",
            "jacsKeyDir": str(kd),
        }
        p = tmp_path / "nopem.json"
        p.write_text(json.dumps(config))
        with pytest.raises(FileNotFoundError, match="No .pem"):
            load(str(p))


class TestGetters:
    def test_get_config_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="has not been called"):
            get_config()

    def test_get_private_key_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="has not been called"):
            get_private_key()

    def test_is_loaded_false(self) -> None:
        reset()
        assert not is_loaded()

    def test_get_private_key_type(self, jacs_config_path: Path) -> None:
        reset()
        load(str(jacs_config_path))
        key = get_private_key()
        assert isinstance(key, Ed25519PrivateKey)
        reset()


class TestSave:
    def test_save_round_trip(self, jacs_config_path: Path, tmp_path: Path) -> None:
        reset()
        load(str(jacs_config_path))
        out = tmp_path / "saved.json"
        save(str(out))
        data = json.loads(out.read_text())
        assert data["jacsAgentName"] == "TestAgent"
        assert data["jacsId"] == "test-jacs-id-1234"
        reset()

    def test_save_before_load(self) -> None:
        reset()
        with pytest.raises(RuntimeError, match="Nothing to save"):
            save("/tmp/nope.json")


class TestReset:
    def test_reset_clears_state(self, jacs_config_path: Path) -> None:
        reset()
        load(str(jacs_config_path))
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

    def test_password_required_raises_without_any_source(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(ValueError, match="Private key password required"):
            load_private_key_password()

    def test_load_requires_password_source(
        self,
        jacs_config_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        reset()
        monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
        monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)

        with pytest.raises(ValueError, match="Private key password required"):
            load(str(jacs_config_path))

    def test_load_fails_on_multiple_sources(
        self,
        jacs_config_path: Path,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        reset()
        password_file = tmp_path / "password.txt"
        password_file.write_text("test-private-key-password\n", encoding="utf-8")
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "test-private-key-password")
        monkeypatch.setenv("JACS_PASSWORD_FILE", str(password_file))

        with pytest.raises(ValueError, match="Multiple password sources configured"):
            load(str(jacs_config_path))
