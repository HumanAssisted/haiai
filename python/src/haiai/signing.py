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
import ipaddress
import json
import logging
import threading
import time
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Optional, TypedDict
from urllib.parse import urlsplit

logger = logging.getLogger("haiai.signing")

SIGNED_JOB_RESPONSE_CONTRACT = "hai.job-response"
SIGNED_JOB_RESPONSE_VERSION = 2


class SignedJobResponsePayloadV2(TypedDict):
    """Payload placed inside a signed HAI job-response document's data field."""

    contract: str
    version: int
    job_id: str
    response: Any


# ---------------------------------------------------------------------------
# Canonical JSON
# ---------------------------------------------------------------------------


def canonicalize_json(obj: dict) -> str:
    """Produce canonical JSON per RFC 8785 (JCS) via JACS binding-core.

    Delegates to JACS binding-core ``canonicalize_json``. There is no local
    fallback: sorted-key ``json.dumps`` is NOT byte-equivalent to RFC 8785
    (numeric formatting, Unicode escape rules, and float canonicalization all
    differ), so signatures produced over a fallback string would not verify
    against JACS-canonicalized input on the verifier side.

    Resolution order:
      1. The currently loaded agent's ``canonicalize_json`` method (when
         available, e.g. ``JacsAgent``).
      2. A stateless ``jacs.JacsAgent()`` instance — RFC 8785 canonicalization
         is keyless, so any JACS install can produce the canonical bytes even
         when the loaded agent (e.g. ``SimpleAgent`` / ``EphemeralAgentAdapter``)
         doesn't expose ``canonicalize_json`` directly.

    Raises :class:`~haiai.errors.HaiError` if no JACS agent is loaded, or if
    JACS itself is not importable.
    """
    from haiai.config import is_loaded, get_agent
    from haiai.errors import HaiError

    if not is_loaded():
        raise HaiError(
            "canonicalize_json requires a loaded JACS agent",
            code="JACS_NOT_LOADED",
            action="Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
        )

    json_str = json.dumps(obj, sort_keys=True, separators=(",", ":"))

    agent = get_agent()
    if hasattr(agent, "canonicalize_json"):
        return agent.canonicalize_json(json_str)

    # Fallback: stateless JACS canonicalization. RFC 8785 needs no keys, and
    # JacsAgent() exposes canonicalize_json without requiring load(). This is
    # a delegation, not a re-implementation — the bytes are still produced by
    # jacs::protocol::canonicalize_json.
    try:
        from jacs import JacsAgent as _JacsAgent
    except ImportError as exc:
        raise HaiError(
            "canonicalize_json requires the jacs Python binding",
            code="JACS_NOT_LOADED",
            action="Install JACS: pip install jacs",
        ) from exc

    try:
        return _JacsAgent().canonicalize_json(json_str)
    except Exception as exc:
        raise HaiError(
            f"JACS canonicalize_json failed: {exc}",
            code="JACS_OP_FAILED",
            action="Verify the JACS Python binding exposes JacsAgent().canonicalize_json",
        ) from exc


# ---------------------------------------------------------------------------
# Signature verification (delegates to JACS binding-core)
# ---------------------------------------------------------------------------


def _extract_raw_key_from_pem(pem_str: str) -> bytes:
    """Decode a public-key PEM to raw bytes.

    Handles both JACS-style PEMs (body = raw bytes base64-encoded) and
    standard X.509 SubjectPublicKeyInfo PEMs (body = ASN.1 DER). The two
    are distinguished by the leading byte: ``0x30`` (SEQUENCE) marks DER.

    For SubjectPublicKeyInfo we walk the minimal structure
    ``SEQUENCE { AlgorithmIdentifier, BIT STRING(rawkey) }`` and return
    the raw key. This is encoding parsing, not crypto.

    Raises :class:`~haiai.errors.HaiError` on malformed input.
    """

    from haiai.errors import HaiError

    body_lines = [
        ln for ln in pem_str.splitlines() if ln and not ln.startswith("-----")
    ]
    if not body_lines:
        raise HaiError(
            "PEM body is empty",
            code="INVALID_PUBLIC_KEY",
            action="Pass a PEM-encoded public key",
        )
    try:
        body = base64.b64decode("".join(body_lines))
    except Exception as exc:
        raise HaiError(
            f"PEM base64 decode failed: {exc}",
            code="INVALID_PUBLIC_KEY",
            action="Pass a valid PEM-encoded public key",
        ) from exc

    # JACS-style: raw bytes, no DER wrapper.
    if not body or body[0] != 0x30:
        return body

    # Standard SubjectPublicKeyInfo: SEQUENCE { AlgorithmId, BIT STRING(key) }
    def _read_len(buf: bytes, off: int) -> tuple[int, int]:
        first = buf[off]
        if first < 0x80:
            return first, off + 1
        n = first & 0x7F
        return int.from_bytes(buf[off + 1 : off + 1 + n], "big"), off + 1 + n

    try:
        _, off = _read_len(body, 1)  # outer SEQUENCE length, body starts at off
        if body[off] != 0x30:
            raise ValueError("expected AlgorithmIdentifier SEQUENCE")
        alg_len, alg_body_off = _read_len(body, off + 1)
        off = alg_body_off + alg_len
        if body[off] != 0x03:
            raise ValueError("expected BIT STRING")
        bs_len, bs_body_off = _read_len(body, off + 1)
        # First content byte of BIT STRING = unused-bits count (0 for whole bytes).
        return body[bs_body_off + 1 : bs_body_off + bs_len]
    except (IndexError, ValueError) as exc:
        raise HaiError(
            f"Failed to parse SubjectPublicKeyInfo DER: {exc}",
            code="INVALID_PUBLIC_KEY",
            action="Pass a valid PEM-encoded public key (JACS or X.509)",
        ) from exc


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
        True if the signature is valid.

    Raises:
        HaiError: If JACS is not available or verification encounters an error.
    """
    from haiai.errors import HaiError

    # Use the JacsAgent instance method (the legacy module-level
    # `jacs.jacs.verify_string` shim is a deprecated stub in jacs 0.10.1
    # that only accepts one positional argument and always errors).
    try:
        from jacs import JacsAgent
    except ImportError as exc:
        raise HaiError(
            "verify_string requires JACS binding-core",
            code="JACS_NOT_LOADED",
            action="Install JACS: pip install jacs",
        ) from exc

    try:
        key_bytes = _extract_raw_key_from_pem(public_key_pem)
    except HaiError:
        raise

    try:
        return JacsAgent().verify_string(data, signature_b64, key_bytes, algorithm)
    except Exception as exc:
        # JACS raises RuntimeError("Signature verification failed: ...") for
        # tampered messages or wrong keys — that's the standalone API's False
        # case, not an exception. Anything else (unknown algorithm, bad key
        # length, missing binding) is a real configuration error.
        msg = str(exc)
        if "Signature verification failed" in msg or "Verification failed" in msg:
            return False
        raise HaiError(
            f"verify_string failed: {exc}",
            code="VERIFICATION_ERROR",
            action="Verify the public key and algorithm match the signer",
        ) from exc


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
    signer_id: str
    key_id: str
    algorithm: str
    public_key_pem: str


@dataclass
class _KeyCache:
    keys: list[_CachedKey]
    fetched_at: float
    issuer: str
    origin: str


_key_cache: Optional[_KeyCache] = None
_key_cache_lock = threading.Lock()


def _trusted_server_key_origin(hai_url: str) -> str:
    from haiai.errors import HaiError

    try:
        parsed = urlsplit(hai_url)
        hostname = parsed.hostname
        port = parsed.port
    except ValueError as exc:
        raise HaiError(
            f"Invalid HAI server origin: {exc}",
            code="SERVER_KEY_ORIGIN_INVALID",
            action="Configure an absolute HTTPS HAI server URL",
        ) from exc

    loopback = hostname == "localhost"
    if hostname is not None and not loopback:
        try:
            loopback = ipaddress.ip_address(hostname).is_loopback
        except ValueError:
            loopback = False
    secure = parsed.scheme.lower() == "https"
    loopback_http = parsed.scheme.lower() == "http" and loopback
    if (
        not hostname
        or (not secure and not loopback_http)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.fragment
    ):
        raise HaiError(
            "HAI server signing keys require HTTPS (HTTP is allowed only on loopback)",
            code="SERVER_KEY_ORIGIN_INVALID",
            action="Configure an HTTPS HAI server URL without userinfo or fragments",
        )

    authority = f"[{hostname.lower()}]" if ":" in hostname else hostname.lower()
    default_port = 443 if secure else 80
    if port is not None and port != default_port:
        authority = f"{authority}:{port}"
    return f"{parsed.scheme.lower()}://{authority}"


def fetch_server_keys(hai_url: str, ffi=None) -> list[_CachedKey]:
    """Fetch HAI public signing keys from ``/.well-known/hai-keys.json``.

    Results are cached for 1 hour with thread-safe refresh.
    """
    global _key_cache

    origin = _trusted_server_key_origin(hai_url)
    with _key_cache_lock:
        now = time.monotonic()
        if (
            _key_cache is not None
            and _key_cache.origin == origin
            and (now - _key_cache.fetched_at) < _KEY_CACHE_TTL
        ):
            return _key_cache.keys

    try:
        if ffi is None:
            raise RuntimeError(
                "FFI client required for fetch_server_keys (no native HTTP fallback)"
            )
        data = ffi.fetch_server_keys()
    except Exception as exc:
        from haiai.errors import HaiError

        raise HaiError(
            f"Failed to fetch HAI signing keys: {exc}",
            code="SERVER_KEY_UNAVAILABLE",
            action="Reject the event and retry after the server key endpoint recovers",
        ) from exc

    parsed: list[_CachedKey] = []
    by_signer: dict[str, str] = {}
    for key_data in data.get("keys", []):
        if key_data.get("is_active") is not True:
            continue
        pem_str = key_data.get("public_key", "")
        jacs_id = key_data.get("jacs_id", "")
        key_id = key_data.get("key_id", "")
        signer_id = key_data.get("signer_id", "")
        if (
            not signer_id
            and jacs_id
            and isinstance(key_id, str)
            and key_id.startswith(f"{jacs_id}:")
        ):
            signer_id = key_id
        if not isinstance(signer_id, str) or not signer_id.strip():
            from haiai.errors import HaiError

            raise HaiError(
                "HAI server published an active key without an exact signer_id",
                code="SERVER_KEY_INVALID",
                action="The server must publish signer_id for every active signing key",
            )
        if not isinstance(pem_str, str) or not pem_str.strip():
            from haiai.errors import HaiError

            raise HaiError(
                f"HAI server published no PEM for signer {signer_id}",
                code="SERVER_KEY_INVALID",
                action="The server must publish a non-empty PEM public key",
            )
        signer_id = signer_id.strip()
        prior = by_signer.get(signer_id)
        if prior is not None and prior != pem_str:
            from haiai.errors import HaiError

            raise HaiError(
                f"HAI server published conflicting active keys for signer {signer_id}",
                code="SERVER_KEY_CONFLICT",
                action="Reject events until the server key registry is consistent",
            )
        if prior is not None:
            continue
        by_signer[signer_id] = pem_str
        parsed.append(
            _CachedKey(
                signer_id=signer_id,
                key_id=key_id if isinstance(key_id, str) else "",
                algorithm=str(key_data.get("algorithm", "")),
                public_key_pem=pem_str,
            )
        )

    if not parsed:
        from haiai.errors import HaiError

        raise HaiError(
            "HAI server key response contains no usable active signing key",
            code="SERVER_KEY_UNAVAILABLE",
            action="The server must publish its signing key before streaming events",
        )

    with _key_cache_lock:
        _key_cache = _KeyCache(
            keys=parsed,
            fetched_at=time.monotonic(),
            issuer=data.get("issuer", ""),
            origin=origin,
        )

    logger.info("Cached %d HAI signing keys", len(parsed))
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
    if (
        isinstance(data.get("version"), str)
        and isinstance(data.get("document_type"), str)
        and "data" in data
        and isinstance(data.get("metadata"), dict)
        and isinstance(data.get("jacsSignature"), dict)
    ):
        return True
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
    ffi=None,
) -> tuple[dict[str, Any], bool]:
    """Unwrap a JACS-signed event with fail-closed verification.

    With ``verify=True`` this accepts only a fully bound v2 envelope and
    delegates signature, freshness, and replay enforcement to JACS
    binding-core ``unwrap_signed_event``. No local or payload-only verification
    fallback is used. ``verify=False`` is an explicit unsafe migration mode;
    its returned boolean is always ``False``.

    Args:
        data: The parsed JSON from SSE/WS.
        hai_url: HAI server URL (needed to fetch keys for verification).
        verify: Whether to verify the server's signature.
        ffi: HAIAI FFI adapter used to fetch the server's authenticated key
            registry. Required when verification is enabled.

    Returns:
        ``(payload, verified)`` -- the inner event payload and whether
        the signature was cryptographically verified.
    """
    bound_v2 = (
        isinstance(data.get("version"), str)
        and isinstance(data.get("document_type"), str)
        and "data" in data
        and isinstance(data.get("metadata"), dict)
        and isinstance(data.get("jacsSignature"), dict)
    )

    if verify:
        from haiai.config import is_loaded, get_agent
        from haiai.errors import HaiError

        if not hai_url:
            raise HaiError(
                "Strict server event verification requires the HAI server URL",
                code="SERVER_KEY_UNAVAILABLE",
                action="Pass the exact HAI origin used by the transport",
            )
        if not bound_v2:
            legacy = "payload" in data or "signature" in data or "jacs_envelope" in data
            raise HaiError(
                "Legacy payload-only signed events are not accepted"
                if legacy
                else "Event is not a fully bound v2 JACS signed event",
                code="VERIFICATION_FAILED",
                action="Require the server to emit fully bound v2 event envelopes",
            )
        if not is_loaded():
            raise HaiError(
                "Strict event verification requires a loaded JACS agent",
                code="JACS_NOT_LOADED",
                action="Run 'haiai init' or set JACS_CONFIG_PATH",
            )
        agent = get_agent()
        if not hasattr(agent, "unwrap_signed_event"):
            raise HaiError(
                "Loaded JACS binding lacks strict unwrap_signed_event support",
                code="JACS_TOO_OLD",
                action="Upgrade jacs to 0.11.4 or newer",
            )

        keys = fetch_server_keys(hai_url, ffi)
        server_keys: dict[str, str] = {}
        for key in keys:
            prior = server_keys.get(key.signer_id)
            if prior is not None and prior != key.public_key_pem:
                raise HaiError(
                    f"Conflicting public keys for server signer {key.signer_id}",
                    code="SERVER_KEY_CONFLICT",
                    action="Reject events until the key registry is consistent",
                )
            server_keys[key.signer_id] = key.public_key_pem

        try:
            result_json = agent.unwrap_signed_event(
                json.dumps(data), json.dumps(server_keys)
            )
        except Exception as exc:
            raise HaiError(
                f"Signed event verification failed: {exc}",
                code="VERIFICATION_FAILED",
                action="Reject the event and refresh the server key registry",
            ) from exc
        try:
            result = json.loads(result_json)
        except (TypeError, json.JSONDecodeError) as exc:
            raise HaiError(
                f"Strict JACS verifier returned malformed JSON: {exc}",
                code="JACS_CONTRACT_INVALID",
                action="Upgrade jacs and report the malformed native result",
            ) from exc
        if not isinstance(result, dict):
            raise HaiError(
                "Strict JACS verifier result must be an object",
                code="JACS_CONTRACT_INVALID",
                action="Upgrade jacs and reject this event",
            )
        if result.get("verified") is not True or "data" not in result:
            raise HaiError(
                "Strict JACS verifier did not return verified data",
                code="JACS_CONTRACT_INVALID",
                action="Reject the event and upgrade jacs",
            )
        payload = result["data"]
        if not isinstance(payload, dict):
            raise HaiError(
                "Verified HAI event payload must be a JSON object",
                code="JACS_CONTRACT_INVALID",
                action="Reject the malformed event payload",
            )
        return payload, True

    if bound_v2:
        payload = data["data"]
        return (payload, False) if isinstance(payload, dict) else (data, False)

    if "payload" in data and "signature" in data:
        payload = data["payload"]
        return (payload, False) if isinstance(payload, dict) else (data, False)

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


def _sign_payload(
    payload: dict[str, Any],
    agent: Any,
    jacs_id: str,
) -> dict[str, str]:
    """Sign arbitrary data without applying the HAI job-response contract.

    The returned dict matches the server's ``SignedJobResponse`` schema::

        {"signed_document": "<json string>", "agent_jacs_id": "..."}

    Delegates full v2 envelope construction to JACS binding-core. HAIAI never
    constructs the legacy local v1 envelope because it signs only ``data``
    and cannot pass HAI's strict complete-envelope verifier.

    Args:
        payload: Data to place inside the signed JACS document.
        agent: A loaded JacsAgent instance (from JACS binding-core).
        jacs_id: Agent's JACS identity ID.

    Returns:
        Dict with ``signed_document`` (JSON string) and ``agent_jacs_id``.

    Raises:
        HaiError: If the agent does not support signing.
    """
    from haiai.errors import HaiError

    if not hasattr(agent, "sign_string"):
        raise HaiError(
            "sign_response requires a JACS agent with sign_string support",
            code="JACS_NOT_LOADED",
            action="Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
        )

    native_signer = getattr(agent, "sign_response", None)
    if not callable(native_signer):
        # Current JACS ephemeral agents are wrapped by a Python adapter whose
        # underlying SimpleAgent owns the native v2 protocol helper.
        native_signer = getattr(getattr(agent, "_native", None), "sign_response", None)
    if not callable(native_signer):
        raise HaiError(
            "Strict response signing requires JACS sign_response; legacy local "
            "v1 envelopes are not accepted",
            code="JACS_TOO_OLD",
            action="Upgrade jacs to 0.11.4 or newer",
        )

    raw_json = json.dumps(payload, separators=(",", ":"))
    result_json = native_signer(raw_json)
    _assert_v2_signed_response_document(result_json)
    return {"signed_document": result_json, "agent_jacs_id": jacs_id}


def _assert_v2_signed_response_document(signed_document: Any) -> None:
    """Reject old or malformed native response-envelope contracts."""
    from haiai.errors import HaiError

    try:
        document = json.loads(signed_document)
    except (TypeError, json.JSONDecodeError) as exc:
        raise HaiError(
            f"JACS sign_response returned malformed JSON: {exc}",
            code="JACS_CONTRACT_INVALID",
            action="Upgrade jacs to 0.11.4 or newer",
        ) from exc

    metadata = document.get("metadata") if isinstance(document, dict) else None
    signature = document.get("jacsSignature") if isinstance(document, dict) else None

    def nonempty_string(value: Any) -> bool:
        return isinstance(value, str) and bool(value)

    valid = (
        isinstance(document, dict)
        and document.get("version") == "2.0.0"
        and document.get("document_type") == "job_response"
        and "data" in document
        and isinstance(metadata, dict)
        and all(
            nonempty_string(metadata.get(field))
            for field in ("issuer", "document_id", "created_at", "hash")
        )
        and isinstance(signature, dict)
        and all(
            nonempty_string(signature.get(field))
            for field in (
                "agentID",
                "date",
                "signingAlgorithm",
                "publicKeyHash",
                "signature",
            )
        )
        and signature.get("signatureContentVersion") == "jacs-response-v2"
    )
    if not valid:
        raise HaiError(
            "JACS sign_response did not return a fully bound v2 response envelope",
            code="JACS_CONTRACT_INVALID",
            action="Upgrade jacs to 0.11.4 or newer",
        )


def sign_response(
    job_id: str,
    job_response_payload: dict[str, Any],
    agent: Any,
    jacs_id: str,
) -> dict[str, str]:
    """Sign a version-2 HAI job response with its job ID in the signed bytes.

    The server requires the signed ``job_id`` to equal the HTTP path or
    WebSocket event job ID. This prevents a valid response from being replayed
    against a different job owned by the same agent.
    """
    from haiai.errors import HaiError

    if not isinstance(job_id, str) or not job_id.strip():
        raise HaiError(
            "sign_response requires a non-empty job ID",
            code="INVALID_ARGUMENT",
            action="Pass the job ID from the HAI job event",
        )
    if (
        not isinstance(job_response_payload, dict)
        or "response" not in job_response_payload
    ):
        raise HaiError(
            "sign_response requires a job response dict with a response field",
            code="INVALID_ARGUMENT",
            action=(
                "Pass {'response': {'message': ..., 'metadata': ..., "
                "'processing_time_ms': ...}}"
            ),
        )

    signed_payload: SignedJobResponsePayloadV2 = {
        "contract": SIGNED_JOB_RESPONSE_CONTRACT,
        "version": SIGNED_JOB_RESPONSE_VERSION,
        "job_id": job_id,
        "response": job_response_payload["response"],
    }
    return _sign_payload(signed_payload, agent, jacs_id)
