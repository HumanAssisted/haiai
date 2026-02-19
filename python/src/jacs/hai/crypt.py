"""Ed25519 signing and verification.

CRYPTO POLICY:
This module is transitional. Cryptographic operations in haisdk must delegate
to JACS functions. Do not add new local cryptographic implementations.

Produces signatures identical to Rust jacs::crypt::ringwrapper -- both use raw
Ed25519 (RFC 8032), NOT Ed25519ph.
"""

from __future__ import annotations

import base64
import json
import uuid
from datetime import datetime, timezone
from typing import Optional

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)


def sign_string(private_key: Ed25519PrivateKey, message: str) -> str:
    """Sign a UTF-8 message and return the base64-encoded signature."""
    signature = private_key.sign(message.encode("utf-8"))
    return base64.b64encode(signature).decode("ascii")


def verify_string(
    public_key: Ed25519PublicKey, message: str, signature_b64: str
) -> bool:
    """Verify a base64-encoded Ed25519 signature against a UTF-8 message."""
    try:
        signature = base64.b64decode(signature_b64)
        public_key.verify(signature, message.encode("utf-8"))
        return True
    except Exception:
        return False


def canonicalize_json(obj: dict) -> str:
    """Produce canonical JSON matching Rust serde_json::to_string() with BTreeMap.

    Sorted keys, compact separators, no trailing newline.
    """
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


def create_agent_document(
    name: str,
    version: str,
    public_key_pem: str,
    private_key: Ed25519PrivateKey,
    jacs_id: Optional[str] = None,
    extra_fields: Optional[dict] = None,
) -> dict:
    """Create a self-signed JACS agent document.

    The signing algorithm matches the Rust verifier: the canonical form
    includes the ``jacsSignature`` object with ``agentID`` and ``date``
    but WITHOUT the ``signature`` sub-field.

    Args:
        name: Agent name (ASCII-only).
        version: Agent version string.
        public_key_pem: PEM-encoded Ed25519 public key.
        private_key: Ed25519 private key for signing.
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
        "jacsPublicKey": public_key_pem,
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
    signature = sign_string(private_key, canonical)

    # Insert signature into the jacsSignature object
    doc["jacsSignature"]["signature"] = signature
    return doc
