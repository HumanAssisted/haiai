"""JACS config loader and module-level agent state.

ALL cryptographic operations delegate to JACS binding-core via JacsAgent.
There is zero local crypto in this module.

Usage::

    from haiai.config import load, get_config, get_agent

    load("./jacs.config.json")
    config = get_config()
    agent = get_agent()
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path
from typing import Any, Optional

from haiai.models import AgentConfig

logger = logging.getLogger("haiai.config")

# ---------------------------------------------------------------------------
# Module-level state
# ---------------------------------------------------------------------------
_config: Optional[AgentConfig] = None
_agent: Any = None  # JacsAgent instance from JACS binding-core

_REQUIRED_FIELDS = ("jacsAgentName", "jacsAgentVersion", "jacsKeyDir")


def _trim_trailing_newlines(value: str) -> str:
    return value.rstrip("\r\n")


def _is_disabled(flag_name: str) -> bool:
    value = os.environ.get(flag_name, "").strip().lower()
    return value in {"1", "true", "yes", "on"}


def _read_password_file_strict(file_path: Path) -> bytes:
    try:
        stat_result = file_path.lstat()
    except FileNotFoundError:
        raise FileNotFoundError(
            f"JACS_PASSWORD_FILE does not exist: {file_path}"
        ) from None
    except OSError as exc:
        raise ValueError(
            f"Failed to read JACS_PASSWORD_FILE ({file_path}): {exc}"
        ) from exc

    if file_path.is_symlink():
        raise ValueError(
            f"JACS_PASSWORD_FILE must not be a symlink: {file_path}"
        )

    if not file_path.is_file():
        raise ValueError(
            f"JACS_PASSWORD_FILE must be a regular file: {file_path}"
        )

    if os.name != "nt":
        mode = stat_result.st_mode & 0o777
        if mode & 0o077:
            raise ValueError(
                "JACS_PASSWORD_FILE has insecure permissions "
                f"({mode:o}): {file_path}. Restrict to owner-only "
                "(for example: chmod 600 /path/to/password.txt)."
            )

    try:
        file_contents = file_path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ValueError(
            f"Failed to read JACS_PASSWORD_FILE ({file_path}): {exc}"
        ) from exc

    file_value = _trim_trailing_newlines(file_contents)
    if not file_value:
        raise ValueError(f"JACS_PASSWORD_FILE is empty: {file_path}")

    return file_value.encode("utf-8")


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
    return _read_password_file_strict(file_path)


def _create_jacs_config(
    name: str,
    version: str,
    key_dir: str,
    jacs_id: Optional[str],
    config_dir: Path,
) -> str:
    """Create a temporary JACS config file that JacsAgent.load() can consume.

    JacsAgent.load() expects a JSON config with specific field names.
    We build a config that points to the existing key files.

    Returns:
        Path to the generated config file (in config_dir).
    """
    key_dir_path = Path(key_dir)

    # Find the private key file (includes JACS-native naming)
    priv_candidates = [
        key_dir_path / "agent_private_key.pem",
        key_dir_path / "jacs.private.pem.enc",
        key_dir_path / f"{name}.private.pem",
        key_dir_path / "private_key.pem",
    ]
    priv_file = None
    for p in priv_candidates:
        if p.is_file():
            priv_file = p.name
            break

    # Find the public key file (includes JACS-native naming)
    pub_candidates = [
        key_dir_path / "agent_public_key.pem",
        key_dir_path / "jacs.public.pem",
        key_dir_path / f"{name}.public.pem",
        key_dir_path / "public_key.pem",
    ]
    pub_file = None
    for p in pub_candidates:
        if p.is_file():
            pub_file = p.name
            break

    jacs_config = {
        # Empty string skips agent document loading in load_by_config
        "jacs_agent_id_and_version": "",
        "jacs_data_directory": str(config_dir / "jacs_data"),
        "jacs_key_directory": str(key_dir_path),
        "jacs_agent_private_key_filename": priv_file or "agent_private_key.pem",
        "jacs_agent_public_key_filename": pub_file or "agent_public_key.pem",
        "jacs_agent_key_algorithm": "pq2025",
        "jacs_default_storage": "fs",
        "name": name,
    }

    # Write the JACS config
    jacs_config_path = config_dir / ".jacs_agent_config.json"
    config_dir.mkdir(parents=True, exist_ok=True)
    with open(jacs_config_path, "w", encoding="utf-8") as f:
        json.dump(jacs_config, f, indent=2)

    return str(jacs_config_path)


def load(config_path: str | None = None) -> None:
    """Load JACS config and initialize a JacsAgent via binding-core.

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
    global _config, _agent

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

    # Validate password is configured (fail early)
    load_private_key_password()

    # Load agent from binding-core using SimpleAgent (handles key loading)
    try:
        from jacs import SimpleAgent as _SimpleAgent
    except ImportError:
        from jacs.jacs import SimpleAgent as _SimpleAgent  # type: ignore[no-redef]

    # Create a JACS-format config for SimpleAgent.load()
    jacs_cfg_path = _create_jacs_config(
        name=_config.name,
        version=_config.version,
        key_dir=str(key_dir),
        jacs_id=_config.jacs_id,
        config_dir=path.parent,
    )

    # Ensure data directory exists
    data_dir = path.parent / "jacs_data"
    data_dir.mkdir(parents=True, exist_ok=True)

    native_agent = _SimpleAgent.load(jacs_cfg_path)

    # Wrap in adapter for JacsAgent API compatibility
    try:
        from jacs.simple import _EphemeralAgentAdapter
        _agent = _EphemeralAgentAdapter(native_agent)
    except ImportError:
        _agent = native_agent

    logger.info("JACS agent '%s' v%s loaded via binding-core", _config.name, _config.version)


def get_config() -> AgentConfig:
    """Return the loaded agent config. Raises if ``load()`` has not been called."""
    if _config is None:
        raise RuntimeError("haiai.config.load() has not been called")
    return _config


def get_agent() -> Any:
    """Return the loaded JacsAgent. Raises if ``load()`` has not been called."""
    if _agent is None:
        raise RuntimeError("haiai.config.load() has not been called")
    return _agent


# Backward compatibility alias
def get_private_key() -> Any:
    """Return the loaded JacsAgent (backward compat for code using get_private_key).

    Returns:
        The loaded JacsAgent instance, which provides sign_string() etc.

    Raises:
        RuntimeError: If ``load()`` has not been called.
    """
    return get_agent()


def is_loaded() -> bool:
    """Return True if the config and agent have been loaded."""
    return _config is not None and _agent is not None


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
    global _config, _agent
    _config = None
    _agent = None


# Keep _private_key as a module-level attribute alias for backward compat
# (some code does `config_mod._private_key = ...` during rotation)
@property
def _private_key_property():
    return _agent


# For direct attribute assignment compatibility in rotate_keys
_private_key: Any = None
