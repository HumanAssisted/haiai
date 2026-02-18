"""Tests for generate_verify_link() in jacs.hai.client."""

from __future__ import annotations

import base64

import pytest

from jacs.hai.client import (
    MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
    generate_verify_link,
)


class TestGenerateVerifyLinkDefaults:
    """Test inline mode with default base_url."""

    def test_basic_url_generation(self) -> None:
        doc = '{"jacsId": "abc123", "data": "hello"}'
        url = generate_verify_link(doc)
        assert url.startswith("https://hai.ai/jacs/verify?s=")

    def test_url_safe_base64_no_plus(self) -> None:
        # Craft a document that would produce '+' in standard base64
        # The character '>' encodes to 'Pg==' in std b64, but in URL-safe it's 'Pg'
        doc = '{"key": ">>>>"}'
        url = generate_verify_link(doc)
        query_part = url.split("?s=")[1]
        assert "+" not in query_part
        assert "/" not in query_part
        assert "=" not in query_part

    def test_url_safe_base64_no_slash(self) -> None:
        doc = '{"key": "test?value"}'
        url = generate_verify_link(doc)
        query_part = url.split("?s=")[1]
        assert "/" not in query_part

    def test_url_safe_base64_no_padding(self) -> None:
        # Use a document whose base64 would normally have padding
        doc = '{"a": 1}'  # 8 bytes -> base64 would be 12 chars with padding
        url = generate_verify_link(doc)
        query_part = url.split("?s=")[1]
        assert not query_part.endswith("=")

    def test_roundtrip_decode(self) -> None:
        doc = '{"jacsId": "test-123", "signed": true}'
        url = generate_verify_link(doc)
        encoded = url.split("?s=")[1]
        # Add padding back for decode
        padding = 4 - len(encoded) % 4
        if padding != 4:
            encoded += "=" * padding
        decoded = base64.urlsafe_b64decode(encoded).decode("utf-8")
        assert decoded == doc


class TestGenerateVerifyLinkCustomBaseUrl:
    """Test custom base_url parameter."""

    def test_custom_base_url(self) -> None:
        doc = '{"id": "1"}'
        url = generate_verify_link(doc, base_url="https://example.com")
        assert url.startswith("https://example.com/jacs/verify?s=")

    def test_trailing_slash_stripped(self) -> None:
        doc = '{"id": "1"}'
        url = generate_verify_link(doc, base_url="https://example.com/")
        assert url.startswith("https://example.com/jacs/verify?s=")
        assert "//jacs" not in url


class TestGenerateVerifyLinkSizeLimits:
    """Test document size enforcement."""

    def test_document_too_large_raises(self) -> None:
        # Create a document that exceeds MAX_VERIFY_DOCUMENT_BYTES
        large_doc = '{"data": "' + "x" * 2000 + '"}'
        assert len(large_doc.encode("utf-8")) > MAX_VERIFY_DOCUMENT_BYTES
        with pytest.raises(ValueError, match="max length"):
            generate_verify_link(large_doc)

    def test_exactly_at_limit_succeeds(self) -> None:
        # The URL format is: base_url + /jacs/verify?s= + encoded
        # "https://hai.ai" + "/jacs/verify?s=" = 29 chars prefix
        # remaining for encoded = 2048 - 29 = 2019 chars
        # base64url(1514 bytes) without padding = 2019 chars -> URL = 2048 (exact)
        # base64url(1515 bytes) without padding = 2020 chars -> URL = 2049 (over!)
        # So 1514 bytes is the actual maximum for the default base_url.
        max_raw = 1514
        filler_len = max_raw - len('{"d":""}')
        doc = '{"d":"' + "a" * filler_len + '"}'
        assert len(doc.encode("utf-8")) == max_raw
        # This should succeed (not raise)
        url = generate_verify_link(doc)
        assert len(url) <= MAX_VERIFY_URL_LEN

    def test_one_over_limit_may_fail(self) -> None:
        # Just over the byte limit -- the URL will exceed 2048
        filler_len = MAX_VERIFY_DOCUMENT_BYTES - len('{"d":""}') + 100
        doc = '{"d":"' + "a" * filler_len + '"}'
        assert len(doc.encode("utf-8")) > MAX_VERIFY_DOCUMENT_BYTES
        with pytest.raises(ValueError):
            generate_verify_link(doc)


class TestGenerateVerifyLinkEmptyDocument:
    """Test edge case with empty or minimal documents."""

    def test_empty_string(self) -> None:
        url = generate_verify_link("")
        assert url.startswith("https://hai.ai/jacs/verify?s=")
        # Empty string should encode fine (just short base64)
        assert len(url) <= MAX_VERIFY_URL_LEN

    def test_empty_json_object(self) -> None:
        url = generate_verify_link("{}")
        assert "?s=" in url
        encoded = url.split("?s=")[1]
        padding = 4 - len(encoded) % 4
        if padding != 4:
            encoded += "=" * padding
        decoded = base64.urlsafe_b64decode(encoded).decode("utf-8")
        assert decoded == "{}"


class TestGenerateVerifyLinkHostedMode:
    """Test hosted mode (uses document ID instead of inline encoding)."""

    def test_hosted_mode_with_jacsDocumentId(self) -> None:
        doc = '{"jacsDocumentId": "doc-abc-123", "data": "x"}'
        url = generate_verify_link(doc, hosted=True)
        assert url == "https://hai.ai/verify/doc-abc-123"

    def test_hosted_mode_with_id_field(self) -> None:
        doc = '{"id": "doc-xyz", "data": "x"}'
        url = generate_verify_link(doc, hosted=True)
        assert url == "https://hai.ai/verify/doc-xyz"

    def test_hosted_mode_no_id_raises(self) -> None:
        doc = '{"data": "no id here"}'
        with pytest.raises(ValueError, match="no document ID"):
            generate_verify_link(doc, hosted=True)

    def test_hosted_mode_custom_base_url(self) -> None:
        doc = '{"jacsDocumentId": "doc-1"}'
        url = generate_verify_link(doc, base_url="https://example.com", hosted=True)
        assert url == "https://example.com/verify/doc-1"

    def test_hosted_mode_invalid_json(self) -> None:
        doc = "not valid json"
        with pytest.raises(ValueError, match="no document ID"):
            generate_verify_link(doc, hosted=True)
