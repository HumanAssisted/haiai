"""JACS envelope signing and verification for HAI transport.

ALL cryptographic operations delegate to JACS binding-core.
There is zero local crypto in this module.

Handles:
  - Canonical JSON for cross-language signature compatibility
  - Detecting JACS-signed events from SSE/WS
  - Verifying server signatures using cached HAI public keys
  - Signing job responses as JACS documents
  - Creating self-signed JACS agent documents
"""

from __future__ import annotations

import base64
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

logger = logging.getLogger("haiai.signing")


# ---------------------------------------------------------------------------
# Canonical JSON
# ---------------------------------------------------------------------------


def canonicalize_json(obj: dict) -> str:
    """Produce canonical JSON per RFC 8785 (JCS).

    Delegates to JACS binding-core when available, falling back to
    Python's ``json.dumps`` with sorted keys for environments where
    the JACS native module is not installed.
    """
    try:
        from haiai.config import is_loaded, get_agent

        if is_loaded():
            agent = get_agent()
            if hasattr(agent, "canonicalize_json"):
                # JACS expects a JSON string, not a dict
                json_str = json.dumps(obj, sort_keys=True, separators=(",", ":"))
                return agent.canonicalize_json(json_str)
    except Exception:
        pass

    # Fallback: local sorted-key JSON (matches for simple cases)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


# ---------------------------------------------------------------------------
# Signature verification (delegates to JACS binding-core)
# ---------------------------------------------------------------------------


def _extract_raw_key_from_pem(pem_str: str) -> bytes:
    """Extract raw key bytes from a PEM-encoded public key.

    Delegates ASN.1 parsing to JACS binding-core when available so that
    all key formats (Ed25519, pq2025, future algorithms) are supported.
    Falls back to manual DER stripping only when JACS is unavailable.
    """
    try:
        from jacs.jacs import extract_public_key_bytes as _jacs_extract
        return _jacs_extract(pem_str)
    except (ImportError, AttributeError):
        pass

    # Fallback: manual PEM → DER → raw key extraction
    lines = pem_str.strip().splitlines()
    if lines[0].startswith("-----BEGIN"):
        b64_data = "".join(
            line for line in lines
            if not line.startswith("-----") and not line.startswith("#")
        )
        der_bytes = base64.b64decode(b64_data)

        # SubjectPublicKeyInfo: strip the ASN.1 header to get raw key.
        # Ed25519 = 44 bytes total (12 header + 32 key).
        # Other algorithms have different sizes — return full DER
        # and let the caller/JACS handle it.
        if len(der_bytes) > 32 and len(der_bytes) <= 64:
            # Heuristic: find the BIT STRING (03) or OCTET STRING (04)
            # containing the raw key at the end of the structure.
            # For Ed25519: 12-byte prefix. For others, return full DER.
            if len(der_bytes) == 44:
                return der_bytes[12:]
        return der_bytes
    else:
        return base64.b64decode(pem_str)


def verify_string(
    data: str,
    signature_b64: str,
    public_key_pem: str,
    algorithm: str = "pq2025",
) -> bool:
    """Verify a base64-encoded signature using JACS binding-core.

    This is a stateless operation that uses the module-level JACS
    verify_string function (no agent needed).

    Args:
        data: The UTF-8 message that was signed.
        signature_b64: Base64-encoded signature to verify.
        public_key_pem: PEM-encoded public key.
        algorithm: Signing algorithm (default "pq2025").

    Returns:
        True if the signature is valid, False otherwise.
    """
    try:
        from jacs.jacs import verify_string as _jacs_verify_string
    except ImportError:
        import jacs as _jacs_module
        _jacs_verify_string = _jacs_module.verify_string

    try:
        # JACS verify_string expects raw key bytes, not PEM.
        key_bytes = _extract_raw_key_from_pem(public_key_pem)
        return _jacs_verify_string(data, signature_b64, key_bytes, algorithm)
    except Exception:
        return False


# ---------------------------------------------------------------------------
# Agent document creation (delegates signing to JACS binding-core)
# ---------------------------------------------------------------------------


def create_agent_document(
    agent: Any,
    name: str,
    version: str,
    jacs_id: Optional[str] = None,
    extra_fields: Optional[dict] = None,
) -> dict:
    """Create a self-signed JACS agent document via binding-core.

    The agent must be loaded with a private key capable of signing.

    Args:
        agent: A loaded JacsAgent instance.
        name: Agent name (ASCII-only).
        version: Agent version string.
        jacs_id: Optional pre-assigned JACS ID. Generated if omitted.
        extra_fields: Optional dict of additional fields to include in the
            document before signing (e.g. ``description``, ``domain``).

    Returns:
        Agent document dict with ``jacsSignature`` field populated.
    """
    if jacs_id is None:
        jacs_id = str(uuid.uuid4())

    doc: dict = {
        "jacsAgentName": name,
        "jacsAgentVersion": version,
        "jacsId": jacs_id,
        "jacsVersion": version,
    }

    # Include extra fields before signing so the signature covers them
    if extra_fields:
        doc.update(extra_fields)

    # Build jacsSignature WITHOUT .signature first (matches Rust canonical form)
    doc["jacsSignature"] = {
        "agentID": jacs_id,
        "date": datetime.now(timezone.utc).isoformat(),
    }

    # Sign the canonical form (includes jacsSignature with agentID+date, no signature)
    canonical = canonicalize_json(doc)
    signature = agent.sign_string(canonical)

    # Insert signature into the jacsSignature object
    doc["jacsSignature"]["signature"] = signature
    return doc


# ---------------------------------------------------------------------------
# Server public key cache
# ---------------------------------------------------------------------------

_KEY_CACHE_TTL = 3600  # 1 hour


@dataclass
class _CachedKey:
    key_id: str
    algorithm: str
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
        if pem_str:
            parsed.append(
                _CachedKey(
                    key_id=key_data.get("key_id", ""),
                    algorithm=key_data.get("algorithm", ""),
                    public_key_pem=pem_str,
                )
            )

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

    Delegates to JACS binding-core ``unwrap_signed_event`` when the agent
    is loaded and supports it.  Falls back to local unwrap + verify_string
    for environments without the native JACS module.

    Args:
        data: The parsed JSON from SSE/WS.
        hai_url: HAI server URL (needed to fetch keys for verification).
        verify: Whether to verify the server's signature.

    Returns:
        ``(payload, verified)`` -- the inner event payload and whether
        the signature was cryptographically verified.
    """
    # Try JACS binding-core delegation first
    if verify and hai_url:
        try:
            from haiai.config import is_loaded, get_agent

            if is_loaded():
                agent = get_agent()
                if hasattr(agent, "unwrap_signed_event"):
                    keys = fetch_server_keys(hai_url)
                    server_keys_dict: dict[str, Any] = {
                        "keys": [
                            {
                                "key_id": k.key_id,
                                "algorithm": k.algorithm,
                                "public_key": k.public_key_pem,
                            }
                            for k in keys
                        ]
                    }
                    event_json = json.dumps(data)
                    server_keys_json = json.dumps(server_keys_dict)
                    result_json = agent.unwrap_signed_event(
                        event_json, server_keys_json
                    )
                    result = json.loads(result_json)
                    payload = result.get("data", data)
                    verified = result.get("verified", False)
                    if isinstance(payload, dict):
                        return payload, verified
                    return data, False
        except Exception:
            # Fall through to local implementation
            pass

    # Fallback: local unwrap with JACS verify_string for signature checks
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
                    if verify_string(canonical, sig_value, cached_key.public_key_pem):
                        verified = True
                        break

                if not verified:
                    payload_hash = metadata.get("hash", "")
                    if payload_hash:
                        for cached_key in keys:
                            if verify_string(
                                payload_hash, sig_value, cached_key.public_key_pem
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
    agent: Any,
    jacs_id: str,
) -> dict[str, str]:
    """Sign a job response using the loaded JACS agent.

    The returned dict matches the server's ``SignedJobResponse`` schema::

        {"signed_document": "<json string>", "agent_jacs_id": "..."}

    Delegates envelope construction to JACS binding-core when the agent
    exposes ``sign_response``. Falls back to local construction for agents
    that only provide ``sign_string`` (e.g. test mocks).

    Args:
        job_response_payload: The ``JobResponseRequest`` dict.
        agent: A loaded JacsAgent instance (from JACS binding-core).
        jacs_id: Agent's JACS identity ID.

    Returns:
        Dict with ``signed_document`` (JSON string) and ``agent_jacs_id``.
    """
    # Prefer JACS binding delegation (JACS canonicalizes internally via RFC 8785)
    if hasattr(agent, "sign_response"):
        raw_json = json.dumps(job_response_payload, separators=(",", ":"))
        result_json = agent.sign_response(raw_json)
        return {"signed_document": result_json, "agent_jacs_id": jacs_id}

    # Fallback for agents without sign_response (test mocks)
    canonical_payload = canonicalize_json(job_response_payload)
    doc_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc).isoformat()
    payload_hash = hashlib.sha256(canonical_payload.encode("utf-8")).hexdigest()
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

    signature = agent.sign_string(canonical_payload)
    jacs_doc["jacsSignature"]["signature"] = signature

    return {
        "signed_document": json.dumps(jacs_doc, separators=(",", ":")),
        "agent_jacs_id": jacs_id,
    }
