"""Tests for HaiClient.rotate_keys() key rotation functionality.

Key rotation delegates the complete transaction to the HAI Rust FFI client.
These tests cover facade delegation and the native archive/config workflow.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import replace
from pathlib import Path
from unittest.mock import Mock

import pytest

from haiai.client import HaiClient
from haiai.config import load, reset
from haiai.errors import HaiAuthError
from haiai.models import RotationResult
from tests.jacs_test_utils import create_signed_jacs_agent


@pytest.fixture(autouse=True)
def _reset_config():
    """Reset module-level config state before and after each test."""
    reset()
    yield
    reset()


@pytest.fixture
def agent_dir(tmp_path, monkeypatch):
    """Create a temporary signed agent or placeholder fallback.

    Uses JACS SimpleAgent.create_agent() if bindings are available,
    otherwise creates placeholder files for config.load() testing.
    """
    password = "TestRotation!2026"
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", password)

    # Try to create a real JACS agent
    try:
        fixture = create_signed_jacs_agent(
            tmp_path,
            name="test-rotation-agent",
            password=password,
        )

        yield {
            "tmp_path": tmp_path,
            "key_dir": fixture.key_dir,
            "config_path": str(fixture.config_path),
            "priv_path": fixture.private_key_path,
            "pub_path": fixture.public_key_path,
            "jacs_id": fixture.jacs_id,
            "version": fixture.version,
            "has_real_jacs": True,
        }

    except ImportError:
        # No JACS bindings -- create placeholder files
        key_dir = tmp_path / "keys"
        key_dir.mkdir()
        priv_path = key_dir / "agent_private_key.pem"
        pub_path = key_dir / "agent_public_key.pem"
        priv_path.write_text(
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nplaceholder\n-----END ENCRYPTED PRIVATE KEY-----\n"
        )
        pub_path.write_text(
            "-----BEGIN PUBLIC KEY-----\nplaceholder\n-----END PUBLIC KEY-----\n"
        )

        config = {
            "jacsAgentName": "test-rotation-agent",
            "jacsAgentVersion": "v1-original",
            "jacsKeyDir": str(key_dir),
            "jacsId": "test-jacs-id-12345",
        }
        config_path = tmp_path / "jacs.config.json"
        config_path.write_text(json.dumps(config, indent=2))

        yield {
            "tmp_path": tmp_path,
            "key_dir": key_dir,
            "config_path": str(config_path),
            "priv_path": priv_path,
            "pub_path": pub_path,
            "jacs_id": "test-jacs-id-12345",
            "version": "v1-original",
            "has_real_jacs": False,
        }

    monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)


def _load_agent(agent_dir):
    """Load the test agent config."""
    if agent_dir["has_real_jacs"]:
        load(agent_dir["config_path"])
    else:
        pytest.skip("JACS bindings not available for rotation test")


def _real_rotation_client(agent_dir):
    """Exercise the real Rust transaction despite the suite's FFI auto-mock."""
    pytest.importorskip("haiipy")
    _load_agent(agent_dir)
    from haiai._ffi_adapter import FFIAdapter
    from haiai.client import _build_ffi_config

    client = HaiClient()
    client._ffi = FFIAdapter(_build_ffi_config())
    return client


@pytest.fixture
def rotation_context(loaded_config, tmp_path, monkeypatch):
    """Mock the FFI transaction and authenticated reload, without local crypto."""
    from haiai import config as config_mod

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
        "registered_with_hai": True,
        "signed_agent_json": json.dumps(
            {"jacsId": cfg.jacs_id, "jacsVersion": "v2-rotated"}
        ),
    }
    ffi = Mock()
    ffi.jacs_id.return_value = cfg.jacs_id
    ffi.base_url.return_value = "https://staging.hai.example"
    ffi.rotate_keys.return_value = data

    def reload_config(path):
        assert path == str(config_path)
        ffi.rotate_keys.assert_called_once()
        config_mod._config = replace(cfg, version=data["new_version"])

    reload = Mock(side_effect=reload_config)
    monkeypatch.setattr(config_mod, "load", reload)
    return cfg, config_path, ffi, data, reload, native_agent


class TestRotateKeysRequiresExistingAgent:
    def test_raises_without_loaded_agent(self):
        client = HaiClient()
        with pytest.raises(RuntimeError):
            client.rotate_keys(register_with_hai=False)

    def test_raises_without_jacs_id(self, agent_dir, monkeypatch):
        if not agent_dir["has_real_jacs"]:
            pytest.skip("JACS bindings not available")

        _load_agent(agent_dir)
        from haiai import config as config_mod

        # Exercise the SDK guard without corrupting the signed on-disk JACS
        # identity. A valid existing-agent config always has a JACS ID.
        monkeypatch.setattr(
            config_mod,
            "_config",
            replace(config_mod.get_config(), jacs_id=None),
        )
        client = HaiClient()
        with pytest.raises(HaiAuthError, match="no jacsId"):
            client.rotate_keys(register_with_hai=False)


class TestRotateKeysGeneratesNewKeypair:
    def test_new_key_files_on_disk(self, agent_dir):
        client = _real_rotation_client(agent_dir)

        client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # New key files should exist at standard paths
        assert agent_dir["priv_path"].is_file()
        assert agent_dir["pub_path"].is_file()

        # Old keys should be archived with version suffix
        key_dir = agent_dir["key_dir"]
        archive_priv = key_dir / f"jacs.private.{agent_dir['version']}.pem.enc"
        assert archive_priv.is_file(), (
            f"Old private key should be archived at {archive_priv}. "
            f"Files in key_dir: {list(key_dir.iterdir())}"
        )

    def test_config_updated(self, agent_dir):
        client = _real_rotation_client(agent_dir)
        from haiai import config as config_mod

        old_native = config_mod._get_native_agent()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # JACS owns and re-signs its canonical config after rotation.
        config_str = Path(agent_dir["config_path"]).read_text()
        config = json.loads(config_str)
        assert config["jacs_agent_id_and_version"] == (
            f"{agent_dir['jacs_id']}:{result.new_version}"
        )
        assert isinstance(config.get("jacsSignature"), dict)
        assert "jacsAgentVersion" not in config
        assert config_mod._get_native_agent() is not old_native
        assert config_mod.get_config().version == result.new_version

    def test_rotated_config_reloads_in_fresh_process(self, agent_dir):
        result = _real_rotation_client(agent_dir).rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        env = os.environ.copy()
        env.pop("JACS_ALLOW_UNSIGNED_AGENT_CONFIG", None)
        source_root = Path(__file__).parent.parent / "src"
        existing_pythonpath = env.get("PYTHONPATH")
        env["PYTHONPATH"] = (
            f"{source_root}{os.pathsep}{existing_pythonpath}"
            if existing_pythonpath
            else str(source_root)
        )
        script = """
import sys
from haiai.config import get_config, load
load(sys.argv[1])
cfg = get_config()
expected_id, expected_version = sys.argv[2], sys.argv[3]
assert cfg.jacs_id == expected_id, (cfg.jacs_id, expected_id)
assert cfg.version == expected_version, (cfg.version, expected_version)
"""
        completed = subprocess.run(
            [
                sys.executable,
                "-c",
                script,
                agent_dir["config_path"],
                agent_dir["jacs_id"],
                result.new_version,
            ],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

        assert completed.returncode == 0, completed.stderr

    def test_rejects_rotation_for_a_different_config_path(self, agent_dir):
        _load_agent(agent_dir)
        original_config = Path(agent_dir["config_path"]).read_bytes()

        with pytest.raises(HaiAuthError, match="different config path"):
            HaiClient().rotate_keys(
                register_with_hai=False,
                config_path=str(agent_dir["tmp_path"] / "other.config.json"),
            )

        assert Path(agent_dir["config_path"]).read_bytes() == original_config


class TestRotateKeysRegistersWithHai:
    def test_registers_through_existing_ffi_with_pinned_context(self, rotation_context):
        cfg, config_path, ffi, data, reload, native_agent = rotation_context
        client = HaiClient(request_auth_audience="staging.hai")
        client._ffi = ffi
        result = client.rotate_keys(
            hai_url="https://staging.hai.example/",
            register_with_hai=True,
            config_path=str(config_path),
        )

        ffi.rotate_keys.assert_called_once_with(
            {"register_with_hai": True, "algorithm": "pq2025"}
        )
        ffi.register.assert_not_called()
        native_agent.rotate_keys.assert_not_called()
        reload.assert_called_once_with(str(config_path))
        assert client._ffi is ffi
        assert client._request_auth_audience == "staging.hai"
        assert result == RotationResult(**data)
        from haiai.config import get_config

        assert get_config().jacs_id == cfg.jacs_id
        assert get_config().version == result.new_version

    def test_forwards_offline_rotation_and_explicit_algorithm(self, rotation_context):
        _cfg, config_path, ffi, data, reload, native_agent = rotation_context
        data["registered_with_hai"] = False
        client = HaiClient()
        client._ffi = ffi

        result = client.rotate_keys(register_with_hai=False, algorithm="ring-Ed25519")

        ffi.rotate_keys.assert_called_once_with(
            {"register_with_hai": False, "algorithm": "ring-Ed25519"}
        )
        ffi.register.assert_not_called()
        native_agent.rotate_keys.assert_not_called()
        reload.assert_called_once_with(str(config_path))
        assert result.registered_with_hai is False


class TestRotateKeysHaiFailureKeepsLocal:
    def test_unconfirmed_registration_preserves_ffi_result_and_reloads(
        self, rotation_context
    ):
        cfg, config_path, ffi, data, reload, native_agent = rotation_context
        data["registered_with_hai"] = False
        client = HaiClient()
        client._ffi = ffi

        result = client.rotate_keys(
            hai_url="https://staging.hai.example",
            register_with_hai=True,
        )

        ffi.rotate_keys.assert_called_once_with(
            {"register_with_hai": True, "algorithm": "pq2025"}
        )
        ffi.register.assert_not_called()
        native_agent.rotate_keys.assert_not_called()
        reload.assert_called_once_with(str(config_path))
        assert result.new_version != cfg.version
        assert result.jacs_id == cfg.jacs_id
        assert result.registered_with_hai is False

    def test_ffi_failure_does_not_reload_or_try_unsigned_registration(
        self, rotation_context
    ):
        _cfg, _path, ffi, _data, reload, native_agent = rotation_context
        ffi.rotate_keys.side_effect = HaiAuthError("rotation failed")
        client = HaiClient()
        client._ffi = ffi

        with pytest.raises(HaiAuthError, match="rotation failed"):
            client.rotate_keys()

        reload.assert_not_called()
        ffi.register.assert_not_called()
        native_agent.rotate_keys.assert_not_called()


class TestRotateKeysPinnedIdentity:
    @pytest.mark.parametrize("mismatch", ["config_path", "jacs_id", "hai_url"])
    def test_rejects_mismatched_context_before_rotation(
        self, rotation_context, mismatch
    ):
        _cfg, config_path, ffi, _data, reload, native_agent = rotation_context
        client = HaiClient()
        client._ffi = ffi
        kwargs = {}
        if mismatch == "config_path":
            kwargs["config_path"] = str(config_path.with_name("other.config.json"))
        elif mismatch == "jacs_id":
            ffi.jacs_id.return_value = "another-agent"
        else:
            kwargs["hai_url"] = "https://another.hai.example"

        with pytest.raises(HaiAuthError):
            client.rotate_keys(**kwargs)

        ffi.rotate_keys.assert_not_called()
        reload.assert_not_called()
        native_agent.rotate_keys.assert_not_called()

    @pytest.mark.parametrize(
        ("field", "invalid"),
        [
            ("jacs_id", "other-agent"),
            ("old_version", "wrong-version"),
            ("registered_with_hai", "false"),
        ],
    )
    def test_rejects_invalid_rotation_metadata(self, rotation_context, field, invalid):
        _cfg, _path, ffi, data, reload, _native_agent = rotation_context
        data[field] = invalid
        client = HaiClient()
        client._ffi = ffi

        with pytest.raises(HaiAuthError):
            client.rotate_keys()

        reload.assert_not_called()

    def test_rejects_reload_of_wrong_identity(self, rotation_context, monkeypatch):
        from haiai import config as config_mod

        cfg, _path, ffi, _data, reload, _native_agent = rotation_context
        reload.side_effect = lambda path: monkeypatch.setattr(
            config_mod, "_config", replace(cfg, jacs_id="another-agent")
        )
        client = HaiClient()
        client._ffi = ffi

        with pytest.raises(HaiAuthError):
            client.rotate_keys()
        assert config_mod.is_loaded() is False

    def test_reload_failure_clears_the_stale_python_identity(self, rotation_context):
        from haiai import config as config_mod

        _cfg, _path, ffi, _data, reload, _native_agent = rotation_context
        reload.side_effect = RuntimeError("authenticated config reload failed")
        client = HaiClient()
        client._ffi = ffi

        with pytest.raises(HaiAuthError):
            client.rotate_keys()

        assert config_mod.is_loaded() is False


class TestRotateKeysResultFields:
    def test_result_has_all_fields(self, agent_dir):
        client = _real_rotation_client(agent_dir)

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        assert isinstance(result, RotationResult)
        assert result.jacs_id == agent_dir["jacs_id"]
        assert result.old_version == agent_dir["version"]
        assert result.new_version != agent_dir["version"]
        assert len(result.new_version) > 0
        assert len(result.new_public_key_hash) == 64  # SHA-256 hex
        assert result.registered_with_hai is False
        assert len(result.signed_agent_json) > 0

        # Signed agent JSON should be valid and contain expected fields
        doc = json.loads(result.signed_agent_json)
        assert doc["jacsId"] == agent_dir["jacs_id"]
        assert doc["jacsVersion"] == result.new_version
        assert doc["jacsPreviousVersion"] == agent_dir["version"]
        assert "jacsSignature" in doc


class TestRotateKeysVersionIsUUID:
    def test_new_version_is_valid_uuid(self, agent_dir):
        import uuid

        client = _real_rotation_client(agent_dir)

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # new_version should be a valid UUID
        parsed = uuid.UUID(result.new_version)
        assert parsed.version == 4


class TestRotateKeysFixtureContract:
    def test_rotation_result_fields_match_fixture(self):
        """Verify RotationResult has all fields defined in the shared fixture."""
        fixture_path = (
            Path(__file__).parent.parent.parent / "fixtures" / "rotation_result.json"
        )
        if not fixture_path.is_file():
            pytest.skip("Shared fixture not found")

        fixture = json.loads(fixture_path.read_text())
        fixture_fields = set(fixture.keys())

        import dataclasses

        result_fields = {f.name for f in dataclasses.fields(RotationResult)}

        assert fixture_fields == result_fields, (
            f"RotationResult fields {result_fields} do not match fixture fields {fixture_fields}"
        )
