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
