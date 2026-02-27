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

    private_key_path_raw = raw.get("jacsPrivateKeyPath") or raw.get(
        "jacs_private_key_path"
    )
    explicit_private_key_path: Optional[Path] = None
    if private_key_path_raw:
        explicit_private_key_path = Path(str(private_key_path_raw))
        if not explicit_private_key_path.is_absolute():
            explicit_private_key_path = path.parent / explicit_private_key_path

    _config = AgentConfig(
        name=raw["jacsAgentName"],
        version=raw["jacsAgentVersion"],
        key_dir=str(key_dir),
        jacs_id=raw.get("jacsId"),
    )

    candidate_paths: list[Path] = []
    if explicit_private_key_path is not None:
        candidate_paths.append(explicit_private_key_path)

    candidate_paths.extend(
        [
            key_dir / "agent_private_key.pem",
            key_dir / f"{raw['jacsAgentName']}.private.pem",
            key_dir / "private_key.pem",
        ]
    )

    if explicit_private_key_path is not None and not explicit_private_key_path.is_file():
        raise FileNotFoundError(
            f"Configured jacsPrivateKeyPath does not exist: {explicit_private_key_path}"
        )

    pem_path: Optional[Path] = None
    for candidate in candidate_paths:
        if candidate.is_file():
            pem_path = candidate
            break

    if pem_path is None:
        raise FileNotFoundError(
            "No .pem private key file found. Searched: "
            + ", ".join(str(p) for p in candidate_paths)
        )

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
