"""RFC 5322 MIME email construction.

Builds standards-compliant email messages from structured fields that can be
parsed by ``email.parser.BytesParser`` and signed by ``sign_email()``.
Uses only stdlib modules (no external dependencies).
"""

from __future__ import annotations

import email.encoders
import email.generator
import email.policy
import email.utils
import io
from email.mime.base import MIMEBase
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from typing import Any, Optional


def _sanitize_header(value: str) -> str:
    """Strip ``\\r`` and ``\\n`` from a header value to prevent CRLF injection."""
    return value.replace("\r", "").replace("\n", "")


def build_rfc5322_email(
    subject: str,
    body: str,
    to: str,
    from_email: str,
    in_reply_to: Optional[str] = None,
    attachments: Optional[list[dict[str, Any]]] = None,
) -> bytes:
    """Build an RFC 5322 email from structured fields.

    Args:
        subject: Email subject line.
        body: Plain text email body.
        to: Recipient email address.
        from_email: Sender email address.
        in_reply_to: Optional Message-ID for threading.
        attachments: Optional list of attachment dicts, each with keys
            ``filename`` (str), ``content_type`` (str), and ``data`` (bytes).

    Returns:
        Raw RFC 5322 email bytes with CRLF line endings.
    """
    safe_subject = _sanitize_header(subject)
    safe_to = _sanitize_header(to)
    safe_from = _sanitize_header(from_email)

    if attachments:
        msg = MIMEMultipart("mixed")
        msg.attach(MIMEText(body, "plain", "utf-8"))

        for att in attachments:
            filename = _sanitize_header(att["filename"])
            content_type = att.get("content_type", "application/octet-stream")
            maintype, _, subtype = content_type.partition("/")
            if not subtype:
                maintype, subtype = "application", "octet-stream"
            part = MIMEBase(maintype, subtype)
            part.set_payload(att["data"])
            email.encoders.encode_base64(part)
            part.add_header(
                "Content-Disposition", "attachment", filename=filename
            )
            msg.attach(part)
    else:
        msg = MIMEText(body, "plain", "utf-8")  # type: ignore[assignment]

    msg["From"] = safe_from
    msg["To"] = safe_to
    msg["Subject"] = safe_subject
    msg["Date"] = email.utils.formatdate(localtime=False, usegmt=True)
    msg["Message-ID"] = email.utils.make_msgid(domain="hai.ai")
    msg["MIME-Version"] = "1.0"

    if in_reply_to:
        safe_reply = _sanitize_header(in_reply_to)
        msg["In-Reply-To"] = safe_reply
        msg["References"] = safe_reply

    # Produce bytes with CRLF line endings using SMTP policy
    buf = io.BytesIO()
    gen = email.generator.BytesGenerator(buf, mangle_from_=False, policy=email.policy.SMTP)
    gen.flatten(msg)
    return buf.getvalue()
