"""Shared internal helpers for sync and async HAI clients."""

from __future__ import annotations

import time
from typing import Any
from urllib.parse import quote

from haiai.errors import HaiAuthError, HaiError
from haiai.models import HaiEvent, PublicKeyInfo, TranscriptMessage


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
    """Build a JACS authorization header using the loaded agent."""
    from haiai.config import get_agent, get_config

    cfg = get_config()
    agent = get_agent()

    if cfg.jacs_id is None:
        raise HaiAuthError("jacsId is required for JACS authentication")

    if hasattr(agent, "build_auth_header"):
        return agent.build_auth_header()

    if not hasattr(agent, "sign_string"):
        raise HaiError(
            "build_auth_header requires a JACS agent with sign_string support",
            code="JACS_NOT_LOADED",
            action="Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
        )

    timestamp = int(time.time())
    message = f"{cfg.jacs_id}:{timestamp}"
    signature = agent.sign_string(message)
    return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"


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
    """Build a 4-part JACS auth header signed by an explicit agent."""
    timestamp = int(time.time())
    message = f"{jacs_id}:{version}:{timestamp}"
    signature = agent.sign_string(message)
    return f"JACS {jacs_id}:{version}:{timestamp}:{signature}"


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


def parse_public_key_info(
    data: dict[str, Any], **defaults: Any
) -> PublicKeyInfo:
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
