"""Tests for HaiClient.rotate_keys() key rotation functionality."""

from __future__ import annotations

import json
import os
import shutil
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    BestAvailableEncryption,
    Encoding,
    NoEncryption,
    PrivateFormat,
    PublicFormat,
)

from jacs.hai.client import HaiClient
from jacs.hai.config import load, reset, get_config, get_private_key
from jacs.hai.errors import HaiAuthError
from jacs.hai.models import AgentConfig, RotationResult


@pytest.fixture(autouse=True)
def _reset_config():
    """Reset module-level config state before and after each test."""
    reset()
    yield
    reset()


@pytest.fixture
def agent_dir(tmp_path):
    """Create a temporary agent directory with config, keys, and password."""
    key_dir = tmp_path / "keys"
    key_dir.mkdir()

    # Generate a test Ed25519 keypair
    private_key = Ed25519PrivateKey.generate()
    password = b"TestRotation!2026"

    priv_pem = private_key.private_bytes(
        Encoding.PEM, PrivateFormat.PKCS8, BestAvailableEncryption(password),
    )
    pub_pem = private_key.public_key().public_bytes(
        Encoding.PEM, PublicFormat.SubjectPublicKeyInfo,
    )

    priv_path = key_dir / "agent_private_key.pem"
    pub_path = key_dir / "agent_public_key.pem"
    priv_path.write_bytes(priv_pem)
    pub_path.write_bytes(pub_pem)

    # Write config
    config = {
        "jacsAgentName": "test-rotation-agent",
        "jacsAgentVersion": "v1-original",
        "jacsKeyDir": str(key_dir),
        "jacsId": "test-jacs-id-12345",
    }
    config_path = tmp_path / "jacs.config.json"
    config_path.write_text(json.dumps(config, indent=2))

    # Set env var for password
    os.environ["JACS_PRIVATE_KEY_PASSWORD"] = "TestRotation!2026"

    yield {
        "tmp_path": tmp_path,
        "key_dir": key_dir,
        "config_path": str(config_path),
        "priv_path": priv_path,
        "pub_path": pub_path,
        "private_key": private_key,
        "password": password,
    }

    # Cleanup env var
    os.environ.pop("JACS_PRIVATE_KEY_PASSWORD", None)


def _load_agent(agent_dir):
    """Load the test agent config."""
    load(agent_dir["config_path"])


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
        archive_priv = key_dir / "agent_private_key.v1-original.pem"
        assert archive_priv.is_file(), "Old private key should be archived"

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
    @patch("jacs.hai.client.httpx.post")
    def test_registers_with_hai(self, mock_post, agent_dir):
        mock_post.return_value = MagicMock(
            status_code=200,
            json=lambda: {"agent_id": "hai-agent-uuid", "registered_at": "2026-03-02"},
        )

        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            hai_url="https://hai.ai",
            register_with_hai=True,
            config_path=agent_dir["config_path"],
        )

        assert result.registered_with_hai is True
        # Verify registration was called
        assert mock_post.called
        call_args = mock_post.call_args
        assert "/api/v1/agents/register" in call_args[0][0]


class TestRotateKeysHaiFailureKeepsLocal:
    @patch("jacs.hai.client.httpx.post")
    def test_hai_failure_preserves_local(self, mock_post, agent_dir):
        mock_post.return_value = MagicMock(
            status_code=500,
            text="Internal Server Error",
        )

        _load_agent(agent_dir)
        client = HaiClient()

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


class TestRotateKeysUpdatesInMemoryState:
    def test_sign_uses_new_key(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()
        old_key = get_private_key()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        new_key = get_private_key()
        # Keys should be different
        old_pub = old_key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        new_pub = new_key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        assert old_pub != new_pub, "In-memory key should be updated after rotation"


class TestRotateKeysRequiresExistingAgent:
    def test_raises_without_loaded_agent(self):
        client = HaiClient()
        with pytest.raises(RuntimeError):
            client.rotate_keys(register_with_hai=False)

    def test_raises_without_jacs_id(self, agent_dir):
        # Modify config to remove jacsId
        config = json.loads(Path(agent_dir["config_path"]).read_text())
        del config["jacsId"]
        Path(agent_dir["config_path"]).write_text(json.dumps(config))

        _load_agent(agent_dir)
        client = HaiClient()
        with pytest.raises(HaiAuthError, match="no jacsId"):
            client.rotate_keys(register_with_hai=False)


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


class TestRotateKeysRollback:
    def test_rollback_on_generation_failure(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()

        # Save original key content for comparison
        original_priv = agent_dir["priv_path"].read_bytes()
        original_pub = agent_dir["pub_path"].read_bytes()

        with patch(
            "cryptography.hazmat.primitives.asymmetric.ed25519.Ed25519PrivateKey.generate",
            side_effect=RuntimeError("Simulated key generation failure"),
        ):
            with pytest.raises(HaiAuthError, match="Key generation failed"):
                client.rotate_keys(
                    register_with_hai=False,
                    config_path=agent_dir["config_path"],
                )

        # Original keys should be restored
        assert agent_dir["priv_path"].read_bytes() == original_priv
        assert agent_dir["pub_path"].read_bytes() == original_pub


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


class TestRotateKeysTwice:
    def test_double_rotation_archives_both_versions(self, agent_dir):
        _load_agent(agent_dir)
        client = HaiClient()

        # First rotation: V1 -> V2
        result1 = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )
        v2 = result1.new_version

        # Reload config for second rotation
        reset()
        load(agent_dir["config_path"])

        # Second rotation: V2 -> V3
        result2 = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        key_dir = agent_dir["key_dir"]

        # Standard key files should be V3 (current)
        assert agent_dir["priv_path"].is_file()
        assert agent_dir["pub_path"].is_file()

        # V1 archive should exist
        archive_v1 = key_dir / "agent_private_key.v1-original.pem"
        assert archive_v1.is_file(), "V1 private key should be archived"

        # V2 archive should exist
        archive_v2 = key_dir / f"agent_private_key.{v2}.pem"
        assert archive_v2.is_file(), "V2 private key should be archived"

        # Versions should all be different
        assert result1.old_version != result1.new_version
        assert result2.old_version != result2.new_version
        assert result1.new_version == result2.old_version


class TestRotateKeysDocIsSelfSigned:
    def test_new_doc_signature_verifiable(self, agent_dir):
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
        import hashlib
        import base64

        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            register_with_hai=False,
            config_path=agent_dir["config_path"],
        )

        # Read the new public key from disk
        new_pub_pem = agent_dir["pub_path"].read_bytes()
        from cryptography.hazmat.primitives.serialization import load_pem_public_key
        pub_key = load_pem_public_key(new_pub_pem)
        assert isinstance(pub_key, Ed25519PublicKey)

        # Parse signed agent JSON
        doc = json.loads(result.signed_agent_json)
        sig_block = doc["jacsSignature"]
        sig_b64 = sig_block["signature"]
        sig_bytes = base64.b64decode(sig_b64)

        # Remove signature from doc and rebuild canonical form
        doc_copy = json.loads(result.signed_agent_json)
        del doc_copy["jacsSignature"]["signature"]
        canonical = json.dumps(doc_copy, sort_keys=True, separators=(",", ":"))

        # Verify signature
        pub_key.verify(sig_bytes, canonical.encode("utf-8"))


class TestRotateKeysFixtureContract:
    def test_rotation_result_fields_match_fixture(self):
        """Verify RotationResult has all fields defined in the shared fixture."""
        fixture_path = Path(__file__).parent.parent.parent / "fixtures" / "rotation_result.json"
        if not fixture_path.is_file():
            pytest.skip("Shared fixture not found")

        fixture = json.loads(fixture_path.read_text())
        fixture_fields = set(fixture.keys())

        # RotationResult dataclass fields
        import dataclasses
        result_fields = {f.name for f in dataclasses.fields(RotationResult)}

        assert fixture_fields == result_fields, (
            f"RotationResult fields {result_fields} do not match fixture fields {fixture_fields}"
        )


class TestRotateKeysSendsCorrectPayload:
    @patch("jacs.hai.client.httpx.post")
    def test_register_payload_contains_agent_json(self, mock_post, agent_dir):
        mock_post.return_value = MagicMock(
            status_code=200,
            json=lambda: {"agent_id": "hai-uuid", "registered_at": "2026-03-02"},
        )

        _load_agent(agent_dir)
        client = HaiClient()

        result = client.rotate_keys(
            hai_url="https://hai.ai",
            register_with_hai=True,
            config_path=agent_dir["config_path"],
        )

        assert mock_post.called
        call_kwargs = mock_post.call_args
        # The payload should be the second positional arg or in json kwarg
        if call_kwargs.kwargs.get("json"):
            payload = call_kwargs.kwargs["json"]
        else:
            payload = json.loads(call_kwargs.kwargs.get("content", "{}"))

        assert "agent_json" in payload
        # agent_json should contain the new version
        agent_doc = json.loads(payload["agent_json"])
        assert agent_doc["jacsVersion"] == result.new_version
        assert agent_doc["jacsId"] == "test-jacs-id-12345"
