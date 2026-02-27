"""Tests for jacs.hai.config module."""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    BestAvailableEncryption,
    Encoding,
    PrivateFormat,
    PublicFormat,
)

from jacs.hai.config import (
    get_config,
    get_private_key,
    is_loaded,
    load,
    load_private_key_password,
    reset,
    save,
)


def _write_encrypted_private_key(path: Path, key: Ed25519PrivateKey, password: bytes) -> None:
    path.write_bytes(
        key.private_bytes(
            Encoding.PEM,
            PrivateFormat.PKCS8,
            BestAvailableEncryption(password),
        )
    )


def _public_pem(key: Ed25519PrivateKey) -> str:
    return key.public_key().public_bytes(
        Encoding.PEM,
        PublicFormat.SubjectPublicKeyInfo,
    ).decode("utf-8")


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

    def test_load_prefers_agent_name_private_key_before_private_key_fallback(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        reset()
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "test-private-key-password")
        kd = tmp_path / "keys"
        kd.mkdir()

        preferred_key = Ed25519PrivateKey.generate()
        fallback_key = Ed25519PrivateKey.generate()
        _write_encrypted_private_key(
            kd / "order-agent.private.pem",
            preferred_key,
            b"test-private-key-password",
        )
        _write_encrypted_private_key(
            kd / "private_key.pem",
            fallback_key,
            b"test-private-key-password",
        )

        config = {
            "jacsAgentName": "order-agent",
            "jacsAgentVersion": "1.0.0",
            "jacsKeyDir": str(kd),
            "jacsId": "order-agent-id",
        }
        p = tmp_path / "ordered.json"
        p.write_text(json.dumps(config))

        load(str(p))
        loaded_key = get_private_key()
        assert _public_pem(loaded_key) == _public_pem(preferred_key)
        reset()

    def test_load_honors_explicit_private_key_path(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        reset()
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "test-private-key-password")
        kd = tmp_path / "keys"
        kd.mkdir()

        explicit_key = Ed25519PrivateKey.generate()
        other_key = Ed25519PrivateKey.generate()

        explicit_path = kd / "explicit.pem"
        _write_encrypted_private_key(
            explicit_path,
            explicit_key,
            b"test-private-key-password",
        )
        _write_encrypted_private_key(
            kd / "agent_private_key.pem",
            other_key,
            b"test-private-key-password",
        )

        config = {
            "jacsAgentName": "explicit-agent",
            "jacsAgentVersion": "1.0.0",
            "jacsKeyDir": str(kd),
            "jacsPrivateKeyPath": str(explicit_path),
            "jacsId": "explicit-agent-id",
        }
        p = tmp_path / "explicit.json"
        p.write_text(json.dumps(config))

        load(str(p))
        loaded_key = get_private_key()
        assert _public_pem(loaded_key) == _public_pem(explicit_key)
        reset()

    def test_load_requires_explicit_private_key_path_to_exist(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        reset()
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "test-private-key-password")
        kd = tmp_path / "keys"
        kd.mkdir()
        _write_encrypted_private_key(
            kd / "agent_private_key.pem",
            Ed25519PrivateKey.generate(),
            b"test-private-key-password",
        )

        missing_path = kd / "missing-explicit.pem"
        config = {
            "jacsAgentName": "explicit-agent",
            "jacsAgentVersion": "1.0.0",
            "jacsKeyDir": str(kd),
            "jacsPrivateKeyPath": str(missing_path),
            "jacsId": "explicit-agent-id",
        }
        p = tmp_path / "explicit-missing.json"
        p.write_text(json.dumps(config))

        with pytest.raises(FileNotFoundError, match="jacsPrivateKeyPath"):
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
