"""Content hash computation for cross-SDK email conformance.

All SDKs must produce identical content hashes for the same inputs.
The algorithm mirrors JACS's ``compute_attachment_hash`` convention:

1. Per-attachment hash: ``sha256(filename_utf8 + ":" + content_type_lower + ":" + raw_bytes)``
2. Sort attachment hashes lexicographically
3. Overall hash:
   - No attachments: ``sha256(subject + "\\n" + body)``
   - With attachments: ``sha256(subject + "\\n" + body + "\\n" + sorted_hashes.join("\\n"))``

Returns ``"sha256:<hex>"`` format.
"""
from __future__ import annotations

import hashlib
from typing import Any, Sequence


def compute_content_hash(
    subject: str,
    body: str,
    attachments: Sequence[dict[str, Any]] | None = None,
) -> str:
    """Compute a deterministic content hash for email content.

    Args:
        subject: Email subject line.
        body: Email body text.
        attachments: List of dicts with keys ``filename``, ``content_type``,
            and either ``data`` (bytes) or ``data_utf8`` (str).

    Returns:
        ``"sha256:<hex>"`` hash string.
    """
    if attachments is None:
        attachments = []

    # Compute per-attachment hashes
    att_hashes: list[str] = []
    for att in attachments:
        filename: str = att["filename"]
        content_type: str = att["content_type"].lower()
        data: bytes
        if "data" in att and isinstance(att["data"], bytes):
            data = att["data"]
        elif "data_utf8" in att:
            data = att["data_utf8"].encode("utf-8")
        else:
            data = b""

        h = hashlib.sha256()
        h.update(filename.encode("utf-8"))
        h.update(b":")
        h.update(content_type.encode("utf-8"))
        h.update(b":")
        h.update(data)
        att_hashes.append(f"sha256:{h.hexdigest()}")

    att_hashes.sort()

    # Compute overall content hash
    h = hashlib.sha256()
    h.update(subject.encode("utf-8"))
    h.update(b"\n")
    h.update(body.encode("utf-8"))
    for ah in att_hashes:
        h.update(b"\n")
        h.update(ah.encode("utf-8"))
    return f"sha256:{h.hexdigest()}"
