"""JACS config loader and module-level agent state.

Usage::

    from jacs.hai.config import load, get_config, get_private_key

    load("./jacs.config.json")
    config = get_config()
    key = get_private_key()
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path
from typing import Optional

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import load_pem_private_key

from jacs.hai.models import AgentConfig

logger = logging.getLogger("jacs.hai.config")

# ---------------------------------------------------------------------------
# Module-level state
# ---------------------------------------------------------------------------
_config: Optional[AgentConfig] = None
_private_key: Optional[Ed25519PrivateKey] = None

_REQUIRED_FIELDS = ("jacsAgentName", "jacsAgentVersion", "jacsKeyDir")


def _trim_trailing_newlines(value: str) -> str:
    return value.rstrip("\r\n")


def _is_disabled(flag_name: str) -> bool:
    value = os.environ.get(flag_name, "").strip().lower()
    return value in {"1", "true", "yes", "on"}


def load_private_key_password() -> bytes:
    """Resolve private-key password from configured secret sources.

    Available sources:
      - ``JACS_PRIVATE_KEY_PASSWORD`` (developer default)
      - ``JACS_PASSWORD_FILE``

    Exactly one source must be configured after source filters are applied.

    Optional source disable flags:
      - ``JACS_DISABLE_PASSWORD_ENV=1``
      - ``JACS_DISABLE_PASSWORD_FILE=1``

    Raises:
        FileNotFoundError: If ``JACS_PASSWORD_FILE`` is selected but missing.
        ValueError: If zero or multiple password sources are configured.
    """
    env_enabled = not _is_disabled("JACS_DISABLE_PASSWORD_ENV")
    file_enabled = not _is_disabled("JACS_DISABLE_PASSWORD_FILE")

    env_password = os.environ.get("JACS_PRIVATE_KEY_PASSWORD")
    password_file = os.environ.get("JACS_PASSWORD_FILE")

    configured_sources: list[str] = []
    if env_enabled and env_password:
        configured_sources.append("JACS_PRIVATE_KEY_PASSWORD")
    if file_enabled and password_file:
        configured_sources.append("JACS_PASSWORD_FILE")

    if len(configured_sources) > 1:
        raise ValueError(
            "Multiple password sources configured: "
            f"{', '.join(configured_sources)}. Configure exactly one."
        )

    if not configured_sources:
        raise ValueError(
            "Private key password required. Configure exactly one of "
            "JACS_PRIVATE_KEY_PASSWORD or JACS_PASSWORD_FILE."
        )

    selected = configured_sources[0]
    if selected == "JACS_PRIVATE_KEY_PASSWORD":
        assert env_password is not None
        return env_password.encode("utf-8")

    assert password_file is not None
    file_path = Path(password_file)
    if not file_path.is_file():
        raise FileNotFoundError(
            f"JACS_PASSWORD_FILE does not exist: {file_path}"
        )

    file_value = _trim_trailing_newlines(
        file_path.read_text(encoding="utf-8")
    )
    if not file_value:
        raise ValueError(f"JACS_PASSWORD_FILE is empty: {file_path}")

    return file_value.encode("utf-8")


def load(config_path: str | None = None) -> None:
    """Load JACS config and the Ed25519 private key from disk.

    Discovery order:
      1. Explicit ``config_path`` argument
      2. ``JACS_CONFIG_PATH`` environment variable
      3. ``./jacs.config.json`` in the current directory

    Password source discovery:
      - ``JACS_PRIVATE_KEY_PASSWORD``
      - ``JACS_PASSWORD_FILE``

    Exactly one password source must be configured.
    Keys must be encrypted at rest.
    """
    global _config, _private_key

    if config_path is None:
        config_path = os.environ.get("JACS_CONFIG_PATH", "./jacs.config.json")
    path = Path(config_path)
    if not path.is_file():
        raise FileNotFoundError(f"JACS config not found: {path}")

    with open(path, encoding="utf-8") as f:
        raw = json.load(f)

    missing = [k for k in _REQUIRED_FIELDS if k not in raw]
    if missing:
        raise ValueError(
            f"JACS config missing required fields: {', '.join(missing)}"
        )

    key_dir = Path(raw["jacsKeyDir"])
    if not key_dir.is_absolute():
        key_dir = path.parent / key_dir

    _config = AgentConfig(
        name=raw["jacsAgentName"],
        version=raw["jacsAgentVersion"],
        key_dir=str(key_dir),
        jacs_id=raw.get("jacsId"),
    )

    # Find the first .pem private key file in key_dir
    pem_files = sorted(key_dir.glob("*private*.pem"))
    if not pem_files:
        pem_files = sorted(key_dir.glob("*.pem"))
    if not pem_files:
        raise FileNotFoundError(f"No .pem key file found in {key_dir}")

    pem_path = pem_files[0]
    logger.info("Loading private key from %s", pem_path)

    pem_data = pem_path.read_bytes()
    # Strip comment lines (e.g. "# WARNING: TEST-ONLY KEY ...")
    pem_lines = [
        line for line in pem_data.split(b"\n") if not line.startswith(b"#")
    ]
    pem_data = b"\n".join(pem_lines)

    password = load_private_key_password()

    try:
        loaded_key = load_pem_private_key(pem_data, password=password)
    except (TypeError, ValueError) as exc:
        raise ValueError(
            f"Failed to load encrypted private key from {pem_path}: {exc}"
        ) from exc

    if not isinstance(loaded_key, Ed25519PrivateKey):
        raise TypeError(
            f"Expected Ed25519 private key, got {type(loaded_key).__name__}"
        )
    _private_key = loaded_key
    logger.info("JACS agent '%s' v%s loaded", _config.name, _config.version)


def get_config() -> AgentConfig:
    """Return the loaded agent config. Raises if ``load()`` has not been called."""
    if _config is None:
        raise RuntimeError("jacs.hai.config.load() has not been called")
    return _config


def get_private_key() -> Ed25519PrivateKey:
    """Return the loaded private key. Raises if ``load()`` has not been called."""
    if _private_key is None:
        raise RuntimeError("jacs.hai.config.load() has not been called")
    return _private_key


def is_loaded() -> bool:
    """Return True if the config and key have been loaded."""
    return _config is not None and _private_key is not None


def save(config_path: str = "./jacs.config.json") -> None:
    """Write the current in-memory config back to disk."""
    if _config is None:
        raise RuntimeError("Nothing to save -- call load() or register first")

    data: dict = {
        "jacsAgentName": _config.name,
        "jacsAgentVersion": _config.version,
        "jacsKeyDir": _config.key_dir,
    }
    if _config.jacs_id:
        data["jacsId"] = _config.jacs_id

    path = Path(config_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)
        f.write("\n")

    logger.info("Saved JACS config to %s", path)


def reset() -> None:
    """Reset module state (useful for testing)."""
    global _config, _private_key
    _config = None
    _private_key = None
