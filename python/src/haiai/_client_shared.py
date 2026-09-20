"""Shared internal helpers for sync and async HAI clients."""

from __future__ import annotations

import base64
import json
from pathlib import Path
from typing import Any
from urllib.parse import quote

from haiai.errors import HaiAuthError, HaiError
from haiai.models import (
    AgentConfig,
    HaiEvent,
    PublicKeyInfo,
    RotationResult,
    TranscriptMessage,
)


def make_url(base_url: str, path: str, *, validate_scheme: bool = True) -> str:
    """Construct a full URL from base and path."""
    if validate_scheme and (
        not base_url or not base_url.startswith(("http://", "https://"))
    ):
        raise ValueError(
            f"Invalid base URL: {base_url!r} — URL must start with http:// or https://"
        )
    base = base_url.rstrip("/")
    normalized_path = "/" + path.lstrip("/")
    return base + normalized_path


def escape_path_segment(value: str) -> str:
    """Escape a user-controlled URL path segment."""
    return quote(value, safe="")


def get_jacs_id() -> str:
    """Return the loaded JACS ID, raising if not available."""
    from haiai.config import get_config

    cfg = get_config()
    if cfg.jacs_id is None:
        raise HaiAuthError("jacsId is required in config for JACS authentication")
    return cfg.jacs_id


def get_hai_agent_id(hai_agent_id: str | None) -> str:
    """Return the HAI-assigned agent UUID, falling back to the loaded JACS ID."""
    return hai_agent_id or get_jacs_id()


def build_jacs_auth_header() -> str:
    """Reject authentication without the final HTTP request context."""
    raise HaiError(
        "Request authentication requires the final method, URL and exact body bytes",
        code="INVALID_ARGUMENT",
        action="Use client.build_request_auth_header(method, url, body)",
    )


def request_auth_input(method: str, url: str, body: bytes) -> str:
    """Encode exact bytes for the shared Rust request-auth facade, without signing."""
    if not isinstance(body, bytes):
        raise TypeError(
            "body must be bytes containing the exact transmitted request body"
        )
    return json.dumps(
        {
            "method": method,
            "url": url,
            "body_base64": base64.b64encode(body).decode("ascii"),
        }
    )


def build_auth_headers() -> dict[str, str]:
    """Return auth headers using JACS signature authentication."""
    from haiai.config import get_config, is_loaded

    if not (is_loaded() and get_config().jacs_id):
        raise HaiAuthError(
            "No JACS authentication available. "
            "Call haiai.config.load() with a config containing jacsId."
        )
    return {"Authorization": build_jacs_auth_header()}


def build_jacs_auth_header_with_key(jacs_id: str, version: str, agent: Any) -> str:
    """Retired no-context helper; key rotation is owned by the Rust client."""
    return build_jacs_auth_header()


def rotation_config(config_path: str | None) -> tuple[AgentConfig, Path]:
    """Pin rotation to the config authenticated by the loaded Python signer."""
    from haiai import config as config_mod

    cfg = config_mod.get_config()
    if cfg.jacs_id is None:
        raise HaiAuthError(
            "Cannot rotate keys: no jacsId in config. Register an agent first."
        )
    loaded_path = config_mod._get_loaded_config_path()
    if config_path is not None and Path(config_path).resolve() != loaded_path:
        raise HaiAuthError(
            "Cannot rotate a different config path than the authenticated "
            "identity currently loaded by haiai.config.load()."
        )
    return cfg, loaded_path


def validate_rotation_client(
    cfg: AgentConfig, ffi_jacs_id: str, hai_url: str | None, base_url: str
) -> None:
    """Refuse a stale client identity or a different registration destination."""
    if ffi_jacs_id != cfg.jacs_id:
        raise HaiAuthError(
            "Cannot rotate keys: the client identity differs from the loaded config. "
            "Create a new client after loading a different agent."
        )
    if hai_url is not None and hai_url.rstrip("/") != base_url.rstrip("/"):
        raise HaiAuthError(
            "Cannot rotate keys: hai_url must match the client's configured HAI URL. "
            "Set HAI_URL before creating the client."
        )


def complete_key_rotation(
    rotation: Any, previous: AgentConfig, config_path: Path
) -> RotationResult:
    """Map Rust's result and reload Python's signer from the same canonical file."""
    from haiai import config as config_mod

    if not isinstance(rotation, dict):
        raise HaiAuthError("Key rotation failed: JACS returned an invalid result")
    if rotation.get("jacs_id") != previous.jacs_id:
        raise HaiAuthError("Key rotation failed: JACS changed the agent identity")
    if rotation.get("old_version") != previous.version:
        raise HaiAuthError("Key rotation failed: JACS returned the wrong old version")
    if not all(
        isinstance(rotation.get(field), str) and rotation[field]
        for field in ("new_version", "new_public_key_hash", "signed_agent_json")
    ) or not isinstance(rotation.get("registered_with_hai"), bool):
        raise HaiAuthError("Key rotation failed: JACS returned incomplete metadata")

    # Rust already updated its in-memory signer. Refresh the separate Python
    # JACS handle too, including when HAI registration was not confirmed.
    try:
        config_mod.load(str(config_path))
    except Exception as exc:
        config_mod.reset()
        raise HaiAuthError(
            "Keys rotated, but Python could not reload the canonical config. "
            "Reload haiai.config before signing again."
        ) from exc
    current = config_mod.get_config()
    if (
        current.jacs_id != previous.jacs_id
        or current.version != rotation["new_version"]
    ):
        config_mod.reset()
        raise HaiAuthError(
            "Key rotation failed: reloaded config does not match the new identity"
        )

    return RotationResult(
        jacs_id=rotation["jacs_id"],
        old_version=rotation["old_version"],
        new_version=rotation["new_version"],
        new_public_key_hash=rotation["new_public_key_hash"],
        registered_with_hai=rotation["registered_with_hai"],
        signed_agent_json=rotation["signed_agent_json"],
    )


def parse_transcript(raw_messages: list[dict[str, Any]]) -> list[TranscriptMessage]:
    """Parse raw transcript messages from an API response."""
    return [
        TranscriptMessage(
            role=msg.get("role", "system"),
            content=msg.get("content", ""),
            timestamp=msg.get("timestamp", ""),
            annotations=msg.get("annotations", []),
        )
        for msg in raw_messages
    ]


def parse_public_key_info(data: dict[str, Any], **defaults: Any) -> PublicKeyInfo:
    """Parse a PublicKeyInfo from an FFI response dict."""
    return PublicKeyInfo(
        jacs_id=data.get("jacs_id", defaults.get("jacs_id", "")),
        version=data.get("version", defaults.get("version", "")),
        public_key=data.get("public_key", ""),
        public_key_raw_b64=data.get("public_key_raw_b64", ""),
        algorithm=data.get("algorithm", ""),
        public_key_hash=data.get("public_key_hash", ""),
        status=data.get("status", ""),
        dns_verified=data.get("dns_verified", False),
        created_at=data.get("created_at", ""),
    )


def make_ffi_event(event_data: dict[str, Any]) -> HaiEvent:
    """Normalize an FFI transport payload into a HaiEvent."""
    return HaiEvent(
        event_type=event_data.get("event_type", ""),
        data=event_data.get("data", {}),
        id=event_data.get("id"),
        raw=event_data.get("raw", ""),
    )
