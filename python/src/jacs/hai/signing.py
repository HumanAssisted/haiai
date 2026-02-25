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
from jacs.hai.models import EmailVerificationResult

logger = logging.getLogger("jacs.hai.signing")

_MAX_TIMESTAMP_AGE = 86400  # 24 hours
_MAX_TIMESTAMP_FUTURE = 300  # 5 minutes


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


# ---------------------------------------------------------------------------
# Email signature verification
# ---------------------------------------------------------------------------


def _parse_jacs_signature_header(header: str) -> dict[str, str]:
    """Parse the X-JACS-Signature header into a dict of key=value pairs.

    Format: ``v=1; a=ed25519; id=agent-id; t=1740000000; s=base64sig``
    """
    fields: dict[str, str] = {}
    for part in header.split(";"):
        part = part.strip()
        if "=" not in part:
            continue
        key, _, value = part.partition("=")
        fields[key.strip()] = value.strip()
    return fields


def verify_email_signature(
    headers: dict[str, str],
    subject: str,
    body: str,
    hai_url: str = "https://hai.ai",
) -> EmailVerificationResult:
    """Verify an email's JACS signature (v1 and v2).

    This is a standalone function -- no agent authentication required.

    **v1 payload:** ``{content_hash}:{timestamp}``
    **v2 payload:** ``{content_hash}:{from}:{timestamp}``

    v2 is detected by the explicit ``v=2`` field in the X-JACS-Signature
    header.  v2 carries the content hash inline (``h=sha256:...``) and does
    not require a separate ``X-JACS-Content-Hash`` header.

    Args:
        headers: Email headers dict. Must contain ``X-JACS-Signature`` and
            ``From``.  v1 also requires ``X-JACS-Content-Hash``.
        subject: Email subject line.
        body: Email body text.
        hai_url: HAI server URL for public key lookup.

    Returns:
        An :class:`EmailVerificationResult` with ``valid``, ``jacs_id``,
        ``reputation_tier``, and ``error`` fields.
    """
    # Step 1: Extract required headers
    sig_header = headers.get("X-JACS-Signature", "")
    from_address = headers.get("From", "")

    if not sig_header:
        return EmailVerificationResult(
            valid=False, jacs_id="", reputation_tier="", error="Missing X-JACS-Signature header"
        )
    if not from_address:
        return EmailVerificationResult(
            valid=False, jacs_id="", reputation_tier="", error="Missing From header"
        )

    # Step 2: Parse signature header fields and detect version
    fields = _parse_jacs_signature_header(sig_header)
    jacs_id = fields.get("id", "")
    timestamp_str = fields.get("t", "")
    signature_b64 = fields.get("s", "")
    algorithm = fields.get("a", "ed25519")
    version = fields.get("v", "1")
    header_hash = fields.get("h", "")         # v2: content hash inline
    header_from = fields.get("from", "")       # v2: sender identity

    # Detect v2: use the explicit v= field value, not the presence of h=
    is_v2 = (version == "2")

    if not jacs_id or not timestamp_str or not signature_b64:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier="",
            error="Incomplete X-JACS-Signature header (missing id, t, or s)",
        )

    if algorithm != "ed25519":
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier="",
            error=f"Unsupported algorithm: {algorithm}",
        )

    try:
        timestamp = int(timestamp_str)
    except ValueError:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier="",
            error=f"Invalid timestamp: {timestamp_str}",
        )

    # Step 3: Determine content hash and signing payload based on version
    if is_v2:
        # v2: content hash comes from the h= field in the signature header.
        # The signature itself proves h= is authentic, so we trust it.
        content_hash = header_hash

        # v2: require from= in the signature header
        if not header_from:
            return EmailVerificationResult(
                valid=False, jacs_id=jacs_id, reputation_tier="",
                error="v2 signature missing from= field",
            )

        # v2: check that from= matches the email From header
        if header_from != from_address:
            return EmailVerificationResult(
                valid=False, jacs_id=jacs_id, reputation_tier="",
                error=f"Signature from ({header_from}) does not match email From header ({from_address})",
            )

        # Recompute content hash from subject+body for comparison.
        # A mismatch may be due to attachments (which we cannot recompute here).
        computed_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()
        if computed_hash != content_hash:
            logger.warning(
                "v2 content hash mismatch (may include attachments): "
                "header=%s computed=%s",
                content_hash, computed_hash,
            )

        # v2 signing payload: {content_hash}:{from}:{timestamp}
        sign_input = f"{content_hash}:{header_from}:{timestamp}"
    else:
        # v1: require X-JACS-Content-Hash header
        content_hash_header = headers.get("X-JACS-Content-Hash", "")
        if not content_hash_header:
            return EmailVerificationResult(
                valid=False, jacs_id="", reputation_tier="",
                error="Missing X-JACS-Content-Hash header",
            )

        # v1: recompute content hash and compare
        computed_hash = "sha256:" + hashlib.sha256(
            (subject + "\n" + body).encode("utf-8")
        ).hexdigest()
        if computed_hash != content_hash_header:
            return EmailVerificationResult(
                valid=False, jacs_id=jacs_id, reputation_tier="",
                error="Content hash mismatch",
            )
        content_hash = content_hash_header

        # v1 signing payload: {content_hash}:{timestamp}
        sign_input = f"{content_hash}:{timestamp}"

    # Step 4: Fetch public key from registry
    registry_url = f"{hai_url.rstrip('/')}/api/agents/keys/{from_address}"
    try:
        resp = httpx.get(registry_url, timeout=10.0)
        resp.raise_for_status()
        registry_data = resp.json()
    except Exception as exc:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier="",
            error=f"Failed to fetch public key: {exc}",
        )

    public_key_pem = registry_data.get("public_key", "")
    reputation_tier = registry_data.get("reputation_tier", "")
    registry_jacs_id = registry_data.get("jacs_id") or registry_data.get("jacsId") or ""

    if not public_key_pem:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier=reputation_tier,
            error="No public key found in registry",
        )
    if not registry_jacs_id:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier=reputation_tier,
            error="No jacs_id found in registry",
        )
    if registry_jacs_id != jacs_id:
        return EmailVerificationResult(
            valid=False, jacs_id=registry_jacs_id, reputation_tier=reputation_tier,
            error="Signature id does not match registry jacs_id",
        )

    try:
        loaded_key = load_pem_public_key(public_key_pem.encode())
        if not isinstance(loaded_key, Ed25519PublicKey):
            return EmailVerificationResult(
                valid=False, jacs_id=jacs_id, reputation_tier=reputation_tier,
                error="Registry key is not Ed25519",
            )
    except Exception as exc:
        return EmailVerificationResult(
            valid=False, jacs_id=jacs_id, reputation_tier=reputation_tier,
            error=f"Invalid public key format: {exc}",
        )

    # Step 5: Verify Ed25519 signature
    if not verify_string(loaded_key, sign_input, signature_b64):
        return EmailVerificationResult(
            valid=False, jacs_id=registry_jacs_id, reputation_tier=reputation_tier,
            error="Signature verification failed",
        )

    # Step 6: Check timestamp freshness
    now = int(time.time())
    age = now - timestamp
    if age > _MAX_TIMESTAMP_AGE:
        return EmailVerificationResult(
            valid=False, jacs_id=registry_jacs_id, reputation_tier=reputation_tier,
            error="Signature timestamp is too old (>24h)",
        )
    if age < -_MAX_TIMESTAMP_FUTURE:
        return EmailVerificationResult(
            valid=False, jacs_id=registry_jacs_id, reputation_tier=reputation_tier,
            error="Signature timestamp is too far in the future (>5min)",
        )

    return EmailVerificationResult(
        valid=True, jacs_id=registry_jacs_id, reputation_tier=reputation_tier, error=None,
    )
