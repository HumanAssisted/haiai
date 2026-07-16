"""Helpers for creating real, signed JACS agents in test temp directories."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class SignedJacsAgentFixture:
    """Paths and identity returned by a freshly generated JACS test agent."""

    config_path: Path
    data_dir: Path
    key_dir: Path
    private_key_path: Path
    public_key_path: Path
    jacs_id: str
    version: str
    info: dict[str, Any]


def create_signed_jacs_agent(
    base_dir: Path,
    *,
    name: str,
    password: str,
    algorithm: str = "ring-Ed25519",
) -> SignedJacsAgentFixture:
    """Generate and validate an agent using its final absolute paths.

    A JACS config is itself signed. Copying one and rewriting its data/key
    paths invalidates that signature, so integration fixtures must be created
    at the paths from which they will later be loaded.
    """
    if os.environ.get("JACS_ALLOW_UNSIGNED_AGENT_CONFIG"):
        raise AssertionError("tests must not enable JACS_ALLOW_UNSIGNED_AGENT_CONFIG")

    from jacs import SimpleAgent

    root = base_dir.resolve()
    root.mkdir(parents=True, exist_ok=True)
    data_dir = root / "data"
    key_dir = root / "keys"
    data_dir.mkdir()
    key_dir.mkdir()
    config_path = root / "jacs.config.json"

    os.environ["JACS_PRIVATE_KEY_PASSWORD"] = password
    _agent, info = SimpleAgent.create_agent(
        name=name,
        password=password,
        algorithm=algorithm,
        data_directory=str(data_dir),
        key_directory=str(key_dir),
        config_path=str(config_path),
        description="",
        domain="",
        default_storage="fs",
    )

    config = json.loads(config_path.read_text(encoding="utf-8"))
    signature = config.get("jacsSignature")
    if not isinstance(signature, dict):
        raise AssertionError("generated JACS test config is not signed")
    if signature.get("signatureContentVersion") != "jacs-signature-v2":
        raise AssertionError(
            "generated JACS test config does not use the current signed-content schema"
        )
    if config.get("jacs_default_storage") != "fs":
        raise AssertionError(
            "generated JACS signer config must keep local key storage even when "
            "the surrounding HAI document route is remote"
        )
    if config.get("jacs_data_directory") != str(data_dir):
        raise AssertionError("generated JACS config does not use its final data path")
    if config.get("jacs_key_directory") != str(key_dir):
        raise AssertionError("generated JACS config does not use its final key path")

    # Prove that hardened JACS accepts the generated config before a caller
    # relies on it. This check is intentionally performed without a migration
    # or unsigned-config compatibility flag.
    SimpleAgent.load(str(config_path))

    return SignedJacsAgentFixture(
        config_path=config_path,
        data_dir=data_dir,
        key_dir=key_dir,
        private_key_path=Path(info["private_key_path"]),
        public_key_path=Path(info["public_key_path"]),
        jacs_id=str(info["agent_id"]),
        version=str(info["version"]),
        info=info,
    )
