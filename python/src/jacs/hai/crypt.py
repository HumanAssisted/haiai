"""Ed25519 signing and verification.

Produces signatures identical to Rust jacs::crypt::ringwrapper -- both use raw
Ed25519 (RFC 8032), NOT Ed25519ph.
"""

from __future__ import annotations

import base64
import json
import uuid
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
) -> dict:
    """Create a self-signed JACS agent document.

    Args:
        name: Agent name (ASCII-only).
        version: Agent version string.
        public_key_pem: PEM-encoded Ed25519 public key.
        private_key: Ed25519 private key for signing.
        jacs_id: Optional pre-assigned JACS ID. Generated if omitted.

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
    }

    canonical = canonicalize_json(doc)
    signature = sign_string(private_key, canonical)
    doc["jacsSignature"] = signature
    return doc
