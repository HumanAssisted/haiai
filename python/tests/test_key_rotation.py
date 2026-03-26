"""Tests for HaiClient.rotate_keys() key rotation functionality.

Key rotation delegates key generation and signing to JACS binding-core.
These tests verify the rotation workflow (archive, generate, sign, config update).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from haiai.client import HaiClient
from haiai.config import load, reset, get_config
from haiai.errors import HaiAuthError
from haiai.models import AgentConfig, RotationResult


@pytest.fixture(autouse=True)
def _reset_config():
    """Reset module-level config state before and after each test."""
    reset()
    yield
    reset()


@pytest.fixture
def agent_dir(tmp_path, monkeypatch):
    """Create a temporary agent directory with config and placeholder keys.

    Uses JACS SimpleAgent.create_agent() if bindings are available,
    otherwise creates placeholder files for config.load() testing.
    """
    key_dir = tmp_path / "keys"
    key_dir.mkdir()
    data_dir = tmp_path / "jacs_data"
    data_dir.mkdir()

    password = "TestRotation!2026"
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", password)

    # Try to create a real JACS agent
    try:
        from jacs import SimpleAgent as _SimpleAgent

        config_path = tmp_path / "jacs.config.json"
        _agent, info = _SimpleAgent.create_agent(
            name="test-rotation-agent",
            password=password,
            algorithm="ring-Ed25519",
            data_directory=str(data_dir),
            key_directory=str(key_dir),
            config_path=str(config_path),
            description="",
            domain="",
            default_storage="fs",
        )

        # Find generated key files
        priv_path = Path(info.get("private_key_path", str(key_dir / "agent_private_key.pem")))
        pub_path = Path(info.get("public_key_path", str(key_dir / "agent_public_key.pem")))

        # Write a HAI-format config
        config = {
            "jacsAgentName": "test-rotation-agent",
            "jacsAgentVersion": "v1-original",
            "jacsKeyDir": str(key_dir),
            "jacsId": "test-jacs-id-12345",
        }
        config_path.write_text(json.dumps(config, indent=2))

        yield {
            "tmp_path": tmp_path,
            "key_dir": key_dir,
            "config_path": str(config_path),
            "priv_path": priv_path if priv_path.is_file() else key_dir / "agent_private_key.pem",
            "pub_path": pub_path if pub_path.is_file() else key_dir / "agent_public_key.pem",
            "has_real_jacs": True,
        }

    except ImportError:
        # No JACS bindings -- create placeholder files
        priv_path = key_dir / "agent_private_key.pem"
        pub_path = key_dir / "agent_public_key.pem"
        priv_path.write_text("-----BEGIN ENCRYPTED PRIVATE KEY-----\nplaceholder\n-----END ENCRYPTED PRIVATE KEY-----\n")
        pub_path.write_text("-----BEGIN PUBLIC KEY-----\nplaceholder\n-----END PUBLIC KEY-----\n")

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
            "has_real_jacs": False,
        }

    monkeypatch.delenv("JACS_PRIVATE_KEY_PASSWORD", raising=False)


def _load_agent(agent_dir):
    """Load the test agent config."""
    if agent_dir["has_real_jacs"]:
        load(agent_dir["config_path"])
    else:
        pytest.skip("JACS bindings not available for rotation test")


class TestRotateKeysRequiresExistingAgent:
    def test_raises_without_loaded_agent(self):
        client = HaiClient()
        with pytest.raises(RuntimeError):
            client.rotate_keys(register_with_hai=False)

    def test_raises_without_jacs_id(self, agent_dir):
        if not agent_dir["has_real_jacs"]:
            pytest.skip("JACS bindings not available")

        # Modify config to remove jacsId
        config = json.loads(Path(agent_dir["config_path"]).read_text())
        del config["jacsId"]
        Path(agent_dir["config_path"]).write_text(json.dumps(config))

        _load_agent(agent_dir)
        client = HaiClient()
        with pytest.raises(HaiAuthError, match="no jacsId"):
            client.rotate_keys(register_with_hai=False)


class TestRotateKeysGeneratesNewKeypair:
    def test_new_key_files_on_disk(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # New key files should exist at standard paths
        assert agent_dir["priv_path"].is_file()
        assert agent_dir["pub_path"].is_file()

        # Old keys should be archived with version suffix
        key_dir = agent_dir["key_dir"]
        orig_priv = agent_dir["priv_path"]
        archive_priv = orig_priv.with_suffix(f".v1-original.pem")
        assert archive_priv.is_file(), (
            f"Old private key should be archived at {archive_priv}. "
            f"Files in key_dir: {list(key_dir.iterdir())}"
        )

    def test_config_updated(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # Config should have the new version
        config_str = Path(agent_dir["config_path"]).read_text()
        config = json.loads(config_str)
        assert config["jacsAgentVersion"] == result.new_version
        assert config["jacsId"] == "test-jacs-id-12345"


class TestRotateKeysRegistersWithHai:
    def test_registers_with_hai(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()
        # The _auto_mock_ffi fixture ensures _get_ffi() returns a MockFFIAdapter.
        # MockFFIAdapter.register returns {} by default, so registration succeeds.

        result = client.rotate_keys(
            hai_url="https://hai.ai",
            register_with_hai=True,
            config_path=agent_dir["config_path"],
        )

        assert result.registered_with_hai is True
        # Verify FFI register was called (after rotation resets _ffi)
        ffi = client._get_ffi()
        register_calls = [c for c in ffi.calls if c[0] == "register"]
        assert len(register_calls) >= 1


class TestRotateKeysHaiFailureKeepsLocal:
    def test_hai_failure_preserves_local(self, agent_dir, monkeypatch):
        _load_agent(agent_dir)
        client = HaiClient()

        # After rotation, client resets _ffi and calls _get_ffi() which creates
        # a new MockFFIAdapter. We need to make register fail on the new FFI.
        # Patch _get_ffi on the class to inject our error-raising mock.
        from haiai.client import HaiClient as _HC
        from haiai.errors import HaiApiError

        original_get_ffi = _HC._get_ffi

        def patched_get_ffi(self_client):
            ffi = original_get_ffi(self_client)
            # Make register always raise
            def raise_on_register(options):
                raise HaiApiError("Internal Server Error", status_code=500)
            ffi.responses["register"] = raise_on_register
            return ffi

        monkeypatch.setattr(_HC, "_get_ffi", patched_get_ffi)

        result = client.rotate_keys(
            hai_url="https://hai.ai",
            register_with_hai=True,
            config_path=agent_dir["config_path"],
        )

        # Local rotation should succeed
        assert result.new_version != "v1-original"
        assert result.jacs_id == "test-jacs-id-12345"
        # But HAI registration should have failed
        assert result.registered_with_hai is False


class TestRotateKeysResultFields:
    def test_result_has_all_fields(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        assert isinstance(result, RotationResult)
        assert result.jacs_id == "test-jacs-id-12345"
        assert result.old_version == "v1-original"
        assert result.new_version != "v1-original"
        assert len(result.new_version) > 0
        assert len(result.new_public_key_hash) == 64  # SHA-256 hex
        assert result.registered_with_hai is False
        assert len(result.signed_agent_json) > 0

        # Signed agent JSON should be valid and contain expected fields
        doc = json.loads(result.signed_agent_json)
        assert doc["jacsId"] == "test-jacs-id-12345"
        assert doc["jacsVersion"] == result.new_version
        assert doc["jacsPreviousVersion"] == "v1-original"
        assert "jacsSignature" in doc


class TestRotateKeysVersionIsUUID:
    def test_new_version_is_valid_uuid(self, agent_dir):
        import uuid

        _load_agent(agent_dir)
        client = HaiClient()

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
        fixture_path = Path(__file__).parent.parent.parent / "fixtures" / "rotation_result.json"
        if not fixture_path.is_file():
            pytest.skip("Shared fixture not found")

        fixture = json.loads(fixture_path.read_text())
        fixture_fields = set(fixture.keys())

        import dataclasses
        result_fields = {f.name for f in dataclasses.fields(RotationResult)}

        assert fixture_fields == result_fields, (
            f"RotationResult fields {result_fields} do not match fixture fields {fixture_fields}"
        )
