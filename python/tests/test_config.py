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
from tests.jacs_test_utils import create_signed_jacs_agent


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
        with pytest.raises(ValueError, match="neither canonical nor legacy fields"):
            load(str(p))

    def test_load_valid_config_with_jacs(self, tmp_path: Path) -> None:
        """Test load() with real JACS bindings."""
        reset()
        try:
            import jacs  # noqa: F401
        except ImportError:
            pytest.skip("JACS bindings not available")

        fixture = create_signed_jacs_agent(
            tmp_path,
            name="TestAgent",
            password="TestConfig!2026",
        )

        original_config = fixture.config_path.read_bytes()
        config = json.loads(fixture.config_path.read_text(encoding="utf-8"))
        assert "jacsSignature" in config

        load(str(fixture.config_path))
        assert is_loaded()
        cfg = get_config()
        assert cfg.name == "TestAgent"
        assert cfg.version == fixture.version
        assert cfg.jacs_id == fixture.jacs_id
        assert cfg.key_dir == str(fixture.key_dir)

        agent = get_agent()
        assert agent is not None
        assert fixture.config_path.read_bytes() == original_config
        assert Path(os.environ["JACS_CONFIG_PATH"]) == fixture.config_path
        assert not (tmp_path / ".haiai_resolved_jacs.config.json").exists()

        reset()

    def test_load_preserves_signed_relative_paths(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Canonical configs are authenticated bytes, not rewriteable templates."""
        reset()
        try:
            from jacs import SimpleAgent
        except ImportError:
            pytest.skip("JACS bindings not available")

        monkeypatch.chdir(tmp_path)
        password = "RelativeConfig!2026"
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", password)
        (tmp_path / "data").mkdir()
        (tmp_path / "keys").mkdir()
        config_path = tmp_path / "jacs.config.json"
        SimpleAgent.create_agent(
            name="RelativeAgent",
            password=password,
            algorithm="ring-Ed25519",
            data_directory="data",
            key_directory="keys",
            config_path="jacs.config.json",
            description="",
            domain="",
            default_storage="fs",
        )

        original_config = config_path.read_bytes()
        signed = json.loads(original_config)
        assert signed["jacs_data_directory"] == "data"
        assert signed["jacs_key_directory"] == "keys"

        load(str(config_path))

        assert config_path.read_bytes() == original_config
        assert Path(os.environ["JACS_CONFIG_PATH"]) == config_path
        assert get_config().key_dir == str(tmp_path / "keys")
        assert not (tmp_path / ".haiai_resolved_jacs.config.json").exists()

        with pytest.raises(ValueError, match="Cannot relocate"):
            save(str(tmp_path / "elsewhere" / "jacs.config.json"))
        assert config_path.read_bytes() == original_config


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

    def test_save_copies_canonical_config_without_rewriting(
        self,
        tmp_path: Path,
    ) -> None:
        reset()
        try:
            import jacs  # noqa: F401
        except ImportError:
            pytest.skip("JACS bindings not available")

        fixture = create_signed_jacs_agent(
            tmp_path,
            name="CanonicalSaveAgent",
            password="CanonicalSave!2026",
        )
        load(str(fixture.config_path))
        expected = fixture.config_path.read_bytes()
        copy_path = tmp_path / "saved.config.json"

        save()
        save(str(copy_path))

        assert fixture.config_path.read_bytes() == expected
        assert copy_path.read_bytes() == expected
        assert (
            json.loads(expected)["jacsSignature"]
            == json.loads(copy_path.read_bytes())["jacsSignature"]
        )


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
