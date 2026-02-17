"""JACS envelope signing and verification for HAI transport.

Handles:
  - Detecting JACS-signed events from SSE/WS
  - Verifying server signatures using cached HAI public keys
  - Signing job responses as JACS documents
"""

from __future__ import annotations

import hashlib
import json
import logging
import threading
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Optional

import httpx
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)
from cryptography.hazmat.primitives.serialization import load_pem_public_key

from jacs.hai.crypt import canonicalize_json, sign_string, verify_string

logger = logging.getLogger("jacs.hai.signing")


# ---------------------------------------------------------------------------
# Server public key cache
# ---------------------------------------------------------------------------

_KEY_CACHE_TTL = 3600  # 1 hour


@dataclass
class _CachedKey:
    key_id: str
    algorithm: str
    public_key: Ed25519PublicKey
    public_key_pem: str


@dataclass
class _KeyCache:
    keys: list[_CachedKey]
    fetched_at: float
    issuer: str


_key_cache: Optional[_KeyCache] = None
_key_cache_lock = threading.Lock()


def fetch_server_keys(hai_url: str) -> list[_CachedKey]:
    """Fetch HAI public signing keys from ``/.well-known/hai-keys.json``.

    Results are cached for 1 hour with thread-safe refresh.
    """
    global _key_cache

    with _key_cache_lock:
        now = time.monotonic()
        if _key_cache is not None and (now - _key_cache.fetched_at) < _KEY_CACHE_TTL:
            return _key_cache.keys

    url = f"{hai_url.rstrip('/')}/.well-known/hai-keys.json"
    try:
        resp = httpx.get(url, timeout=10.0)
        resp.raise_for_status()
        data = resp.json()
    except Exception as exc:
        logger.warning("Failed to fetch HAI signing keys from %s: %s", url, exc)
        with _key_cache_lock:
            if _key_cache is not None:
                return _key_cache.keys
        return []

    parsed: list[_CachedKey] = []
    for key_data in data.get("keys", []):
        if not key_data.get("is_active", False):
            continue
        pem_str = key_data.get("public_key", "")
        try:
            loaded = load_pem_public_key(pem_str.encode())
            if isinstance(loaded, Ed25519PublicKey):
                parsed.append(
                    _CachedKey(
                        key_id=key_data.get("key_id", ""),
                        algorithm=key_data.get("algorithm", "ed25519"),
                        public_key=loaded,
                        public_key_pem=pem_str,
                    )
                )
        except Exception:
            logger.debug("Skipping non-Ed25519 key: %s", key_data.get("key_id"))

    with _key_cache_lock:
        _key_cache = _KeyCache(
            keys=parsed, fetched_at=time.monotonic(), issuer=data.get("issuer", "")
        )

    logger.info("Cached %d HAI signing keys from %s", len(parsed), url)
    return parsed


def invalidate_key_cache() -> None:
    """Force the next ``fetch_server_keys`` call to re-fetch."""
    global _key_cache
    with _key_cache_lock:
        _key_cache = None


# ---------------------------------------------------------------------------
# Unwrap signed events
# ---------------------------------------------------------------------------


def is_signed_event(data: dict[str, Any]) -> bool:
    """Return True if *data* looks like a JACS-signed document."""
    if "payload" in data and "signature" in data and "metadata" in data:
        return True
    if "jacs_envelope" in data:
        return True
    return False


def unwrap_signed_event(
    data: dict[str, Any],
    hai_url: Optional[str] = None,
    *,
    verify: bool = True,
) -> tuple[dict[str, Any], bool]:
    """Unwrap a JACS-signed event, optionally verifying the server signature.

    Args:
        data: The parsed JSON from SSE/WS.
        hai_url: HAI server URL (needed to fetch keys for verification).
        verify: Whether to verify the server's signature.

    Returns:
        ``(payload, verified)`` -- the inner event payload and whether
        the signature was cryptographically verified.
    """
    # JacsDocument format
    if "payload" in data and "signature" in data:
        payload = data["payload"]
        verified = False

        if verify and hai_url:
            sig_info = data.get("signature", {})
            sig_value = sig_info.get("signature", "")
            metadata = data.get("metadata", {})

            if sig_value and isinstance(payload, dict):
                canonical = canonicalize_json(payload)
                keys = fetch_server_keys(hai_url)
                for cached_key in keys:
                    if verify_string(cached_key.public_key, canonical, sig_value):
                        verified = True
                        break

                if not verified:
                    payload_hash = metadata.get("hash", "")
                    if payload_hash:
                        for cached_key in keys:
                            if verify_string(
                                cached_key.public_key, payload_hash, sig_value
                            ):
                                verified = True
                                break

                if not verified:
                    logger.warning("Could not verify server signature on event")

        if isinstance(payload, dict):
            return payload, verified
        return data, False

    # Legacy "jacs_envelope" format
    if "jacs_envelope" in data:
        inner = data.get("payload", data)
        if isinstance(inner, dict):
            return inner, False
        return data, False

    return data, False


# ---------------------------------------------------------------------------
# Sign response
# ---------------------------------------------------------------------------


def sign_response(
    job_response_payload: dict[str, Any],
    private_key: Ed25519PrivateKey,
    jacs_id: str,
) -> dict[str, str]:
    """Sign a job response and return a ``SignedJobResponse`` dict.

    The returned dict matches the server's ``SignedJobResponse`` schema::

        {"signed_document": "<json string>", "agent_jacs_id": "..."}

    Args:
        job_response_payload: The ``JobResponseRequest`` dict.
        private_key: Agent's Ed25519 private key.
        jacs_id: Agent's JACS identity ID.

    Returns:
        Dict with ``signed_document`` (JSON string) and ``agent_jacs_id``.
    """
    doc_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc).isoformat()

    canonical_payload = canonicalize_json(job_response_payload)
    payload_hash = hashlib.sha256(canonical_payload.encode("utf-8")).hexdigest()

    # Store data in canonical (sorted-key) form for cross-language compat
    sorted_data: dict[str, Any] = json.loads(canonical_payload)

    jacs_doc: dict[str, Any] = {
        "version": "1.0.0",
        "document_type": "job_response",
        "data": sorted_data,
        "metadata": {
            "issuer": jacs_id,
            "document_id": doc_id,
            "created_at": now,
            "hash": payload_hash,
        },
        "jacsSignature": {
            "agentID": jacs_id,
            "date": now,
        },
    }

    signature = sign_string(private_key, canonical_payload)
    jacs_doc["jacsSignature"]["signature"] = signature

    return {
        "signed_document": json.dumps(jacs_doc, separators=(",", ":")),
        "agent_jacs_id": jacs_id,
    }
