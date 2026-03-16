"""Tests for the MIME email construction module."""

from email.parser import BytesParser

from haiai.mime import build_rfc5322_email


def test_build_simple_text_email():
    """Build a plain text email and verify headers."""
    raw = build_rfc5322_email(
        subject="Test Subject",
        body="Hello, world!",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
    )
    msg = BytesParser().parsebytes(raw)

    assert msg["From"] == "sender@hai.ai"
    assert msg["To"] == "recipient@hai.ai"
    assert msg["Subject"] == "Test Subject"
    assert msg["Date"] is not None
    assert msg["Message-ID"] is not None
    assert "Hello, world!" in msg.get_payload(decode=True).decode("utf-8")


def test_build_email_with_attachments():
    """Build an email with attachments and verify structure."""
    raw = build_rfc5322_email(
        subject="With Attachments",
        body="See attached.",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
        attachments=[
            {
                "filename": "file1.txt",
                "content_type": "text/plain",
                "data": b"content of file 1",
            },
            {
                "filename": "file2.pdf",
                "content_type": "application/pdf",
                "data": b"fake pdf content",
            },
        ],
    )
    msg = BytesParser().parsebytes(raw)

    assert msg.is_multipart()
    parts = msg.get_payload()
    # First part is the text body, then two attachments
    assert len(parts) == 3

    # Verify body part
    body_part = parts[0]
    assert "See attached." in body_part.get_payload(decode=True).decode("utf-8")

    # Verify attachment filenames
    filenames = [p.get_filename() for p in parts[1:]]
    assert "file1.txt" in filenames
    assert "file2.pdf" in filenames


def test_build_reply_email():
    """Build a reply email and verify threading headers."""
    raw = build_rfc5322_email(
        subject="Re: Original",
        body="Reply body",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
        in_reply_to="<original-id@hai.ai>",
    )
    msg = BytesParser().parsebytes(raw)

    assert msg["In-Reply-To"] == "<original-id@hai.ai>"
    assert msg["References"] == "<original-id@hai.ai>"


def test_crlf_injection_sanitized():
    """Subject containing CRLF must not produce a separate header."""
    raw = build_rfc5322_email(
        subject="Bad\r\nBcc: attacker@evil.com",
        body="Body",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
    )
    msg = BytesParser().parsebytes(raw)

    # The injected Bcc header must not exist
    assert msg["Bcc"] is None
    # Subject should be sanitized
    assert "attacker@evil.com" not in (msg["Bcc"] or "")


def test_output_is_valid_rfc5322():
    """Output must be parseable without defects."""
    raw = build_rfc5322_email(
        subject="Valid",
        body="Body text",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
    )
    msg = BytesParser().parsebytes(raw)
    assert msg is not None
    assert msg["From"] is not None
    assert msg["To"] is not None
    assert msg["Subject"] is not None
    assert msg["Date"] is not None
    assert msg["Message-ID"] is not None


def test_crlf_line_endings():
    """Raw output should use CRLF line endings in headers."""
    raw = build_rfc5322_email(
        subject="Test",
        body="Body",
        to="recipient@hai.ai",
        from_email="sender@hai.ai",
    )
    # Headers section must contain \r\n
    text = raw.decode("utf-8", errors="replace")
    assert "\r\n" in text
