"""Shared test fixtures for the HAI SDK test suite."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Generator

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    BestAvailableEncryption,
    Encoding,
    PrivateFormat,
    PublicFormat,
)


TEST_PRIVATE_KEY_PASSWORD = "test-private-key-password"


@pytest.fixture()
def ed25519_keypair() -> tuple[Ed25519PrivateKey, str]:
    """Generate a fresh Ed25519 keypair.

    Returns:
        (private_key, public_key_pem)
    """
    private_key = Ed25519PrivateKey.generate()
    public_pem = (
        private_key.public_key()
        .public_bytes(Encoding.PEM, PublicFormat.SubjectPublicKeyInfo)
        .decode()
    )
    return private_key, public_pem


@pytest.fixture(autouse=True)
def password_env(monkeypatch: pytest.MonkeyPatch) -> Generator[None, None, None]:
    """Default developer path: env-based password source."""
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", TEST_PRIVATE_KEY_PASSWORD)
    monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)
    monkeypatch.delenv("JACS_DISABLE_PASSWORD_ENV", raising=False)
    monkeypatch.delenv("JACS_DISABLE_PASSWORD_FILE", raising=False)
    yield


@pytest.fixture()
def key_dir(tmp_path: Path, ed25519_keypair: tuple) -> Path:
    """Write a keypair to a temporary directory and return the path."""
    private_key, public_pem = ed25519_keypair
    kd = tmp_path / "keys"
    kd.mkdir()
    (kd / "agent_private_key.pem").write_bytes(
        private_key.private_bytes(
            Encoding.PEM,
            PrivateFormat.PKCS8,
            BestAvailableEncryption(TEST_PRIVATE_KEY_PASSWORD.encode("utf-8")),
        )
    )
    (kd / "agent_public_key.pem").write_text(public_pem)
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
def loaded_config(jacs_config_path: Path) -> Generator[None, None, None]:
    """Load the test config into the SDK module state, then clean up."""
    from jacs.hai.config import load, reset

    load(str(jacs_config_path))
    yield
    reset()


@pytest.fixture()
def jacs_id() -> str:
    """Return the test JACS ID matching the config fixture."""
    return "test-jacs-id-1234"
