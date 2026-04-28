"""Unit tests for the Python SDK ``sign_image`` option translation
(Issue 009 — cross-language parity for ``no_backup`` and
``unsafe_bak_mode``).

These tests use ``MockFFIAdapter`` from ``conftest.py`` to capture the
wire-level ``opts`` dict that ``HaiClient.sign_image`` and
``AsyncHaiClient.sign_image`` pass to the FFI. They verify that:

1. ``no_backup=True`` propagates as ``backup=False``.
2. The default (``no_backup=False``) propagates as ``backup=True``.
3. ``unsafe_bak_mode`` is forwarded only when explicitly set.

Mirrors ``go/sign_image_test.go::TestSignImageNoBackupSkipsBak`` and
``node/tests/sign-image.test.ts::signImage SDK option translation``.

These tests do NOT require a built haiipy wheel — the autouse
``_auto_mock_ffi`` fixture in ``conftest.py`` injects a fully mocked FFI.
"""

from __future__ import annotations

from typing import Any

import pytest

from haiai import AsyncHaiClient, HaiClient


def _last_sign_image_opts(mock_ffi: Any) -> dict[str, Any]:
    matches = [
        call for call in mock_ffi.calls if call[0] == "sign_image"
    ]
    assert matches, "sign_image was not called on the FFI adapter"
    # MockFFIAdapter records (method, args, kwargs); sign_image's positional
    # args are (in_path, out_path, opts).
    _, args, _ = matches[-1]
    assert len(args) == 3, f"unexpected sign_image args: {args!r}"
    return args[2]


def _stub_sign_image_response(mock_ffi: Any, out_path: str) -> None:
    mock_ffi.responses["sign_image"] = {
        "out_path": out_path,
        "signer_id": "test-signer",
        "format": "png",
        "robust": False,
        "backup_path": None,
    }


class TestSignImageOptionTranslationSync:
    """HaiClient.sign_image must shape `opts` for the FFI exactly as the
    Go SDK does (Issue 009)."""

    def test_no_backup_true_maps_to_backup_false(
        self, loaded_config: None
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        client.sign_image("/tmp/in.png", "/tmp/out.png", no_backup=True)

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("backup") is False
        assert "unsafe_bak_mode" not in opts

    def test_default_maps_to_backup_true(self, loaded_config: None) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        client.sign_image("/tmp/in.png", "/tmp/out.png")

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("backup") is True
        assert "unsafe_bak_mode" not in opts

    def test_unsafe_bak_mode_forwarded(self, loaded_config: None) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        client.sign_image(
            "/tmp/in.png",
            "/tmp/out.png",
            unsafe_bak_mode=0o644,
        )

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("unsafe_bak_mode") == 0o644

    def test_unsafe_bak_mode_omitted_when_none(
        self, loaded_config: None
    ) -> None:
        client = HaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        client.sign_image("/tmp/in.png", "/tmp/out.png")

        opts = _last_sign_image_opts(mock_ffi)
        assert "unsafe_bak_mode" not in opts


@pytest.mark.asyncio
class TestSignImageOptionTranslationAsync:
    """AsyncHaiClient.sign_image must shape `opts` identically to the
    sync wrapper (Issue 009 — cross-language parity)."""

    async def test_no_backup_true_maps_to_backup_false(
        self, loaded_config: None
    ) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        await client.sign_image(
            "/tmp/in.png",
            "/tmp/out.png",
            no_backup=True,
        )

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("backup") is False

    async def test_default_maps_to_backup_true(
        self, loaded_config: None
    ) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        await client.sign_image("/tmp/in.png", "/tmp/out.png")

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("backup") is True

    async def test_unsafe_bak_mode_forwarded(
        self, loaded_config: None
    ) -> None:
        client = AsyncHaiClient()
        mock_ffi = client._get_ffi()
        _stub_sign_image_response(mock_ffi, "/tmp/out.png")

        await client.sign_image(
            "/tmp/in.png",
            "/tmp/out.png",
            unsafe_bak_mode=0o644,
        )

        opts = _last_sign_image_opts(mock_ffi)
        assert opts.get("unsafe_bak_mode") == 0o644
