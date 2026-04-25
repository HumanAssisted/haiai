"""Per-language signing-side tests for the Python SDK (Issue 002).

Mirrors `rust/haiai-cli/tests/cli_image_tests.rs` / `rust/hai-binding-core`
wrapper tests but exercises the full Python SDK code path:
  HaiClient.sign_image -> _ffi_adapter -> haiipy FFI -> binding-core ->
  LocalJacsProvider -> JACS.

Skipped when `haiipy` is missing or the installed wheel predates the Layer-8
media exports (matches `test_cross_lang_media.py` skip semantics).
"""
from __future__ import annotations

import json
import os
import shutil
import tempfile
from pathlib import Path
from typing import Any, Iterator

import pytest


# ---------------------------------------------------------------------------
# Paths and constants — mirror test_cross_lang_media.py.
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[2]
JACS_AGENT_DIR = REPO_ROOT / "fixtures" / "jacs-agent"
FIXTURE_AGENT_PASSWORD = "secretpassord"


def _haiipy_has_media_methods() -> bool:
    """Return True iff the installed haiipy wheel exposes Layer-8 methods."""
    try:
        import haiipy  # type: ignore[import-untyped]
    except ImportError:
        return False
    try:
        c = haiipy.HaiClient('{"jacs_id":"x"}')
    except Exception:
        return False
    return all(
        hasattr(c, attr)
        for attr in (
            "sign_image_sync",
            "verify_image_sync",
            "sign_text_sync",
            "verify_text_sync",
            "extract_media_signature_sync",
        )
    )


pytestmark = pytest.mark.skipif(
    not _haiipy_has_media_methods(),
    reason=(
        "haiipy wheel missing Layer-8 media methods. "
        "Rebuild via `cd rust && maturin develop -p haiipy --release`."
    ),
)


# ---------------------------------------------------------------------------
# Fixture staging — mirror cross_lang_media.py._stage_fixture_agent.
# ---------------------------------------------------------------------------


def _copy_with_colons(src: Path, dst: Path) -> None:
    dst.mkdir(parents=True, exist_ok=True)
    for entry in src.iterdir():
        new_name = entry.name.replace("_", ":")
        target = dst / new_name
        if entry.is_dir():
            _copy_with_colons(entry, target)
        else:
            shutil.copy2(entry, target)


def _stage_fixture_agent(tmp: Path) -> Path:
    os.environ["JACS_PRIVATE_KEY_PASSWORD"] = FIXTURE_AGENT_PASSWORD
    src_cfg = JACS_AGENT_DIR / "jacs.config.json"
    cfg = json.loads(src_cfg.read_text())

    src_keys = JACS_AGENT_DIR / cfg["jacs_key_directory"]
    tmp_keys = tmp / "keys"
    tmp_keys.mkdir()
    for entry in src_keys.iterdir():
        shutil.copy2(entry, tmp_keys / entry.name)

    src_data = JACS_AGENT_DIR / cfg["jacs_data_directory"]
    tmp_data = tmp / "data"
    _copy_with_colons(src_data, tmp_data)

    cfg["jacs_data_directory"] = str(tmp_data)
    cfg["jacs_key_directory"] = str(tmp_keys)

    staged_cfg = tmp / "jacs.config.json"
    staged_cfg.write_text(json.dumps(cfg, indent=2))
    return staged_cfg


def _build_client() -> tuple[Any, Path]:
    """Build a haiai.HaiClient (NOT the raw haiipy FFI) so the Python SDK
    sign_image / verify_image / extract_media_signature paths are exercised
    in full (Issue 002 acceptance criterion). Returns the SDK client AND
    the tempdir so the caller keeps it alive.

    Bypasses the project-wide `_auto_mock_ffi` autouse fixture in
    `conftest.py` by attaching a real FFIAdapter directly. This tests the
    full Python SDK path (HaiClient.sign_image -> _ffi_adapter -> haiipy
    -> binding-core -> JACS).
    """
    from haiai import HaiClient, config as haiai_config
    from haiai._ffi_adapter import FFIAdapter
    from haiai.client import _build_ffi_config

    tmpdir = Path(tempfile.mkdtemp(prefix="haiai-sign-image-"))
    config_path = _stage_fixture_agent(tmpdir)
    os.environ["JACS_CONFIG_PATH"] = str(config_path)
    haiai_config.load(str(config_path))
    client = HaiClient()
    # Force the real FFI adapter (conftest's _auto_mock_ffi would otherwise
    # inject MockFFIAdapter, defeating the round-trip the test is checking).
    client._ffi = FFIAdapter(_build_ffi_config())
    return client, tmpdir


# ---------------------------------------------------------------------------
# Minimal in-memory PNG bytes — copied from `fixtures/media/_source/source.png`
# at module import. Matches what the Rust tests use, so PIL is not needed.
# ---------------------------------------------------------------------------


_SOURCE_PNG_PATH = REPO_ROOT / "fixtures" / "media" / "_source" / "source.png"


def _make_test_png(width: int = 32, height: int = 32) -> bytes:
    """Return PNG bytes. The width/height args are ignored — we use the
    fixture source PNG. Tests don't depend on the actual dimensions; they
    only need a valid PNG that JACS can sign and verify.
    """
    return _SOURCE_PNG_PATH.read_bytes()


# ---------------------------------------------------------------------------
# Per-test fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _fixture_agent_password(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", FIXTURE_AGENT_PASSWORD)


@pytest.fixture(scope="module")
def sdk_client() -> Iterator[Any]:
    os.environ["JACS_PRIVATE_KEY_PASSWORD"] = FIXTURE_AGENT_PASSWORD
    client, tmpdir = _build_client()
    try:
        yield client
    finally:
        if tmpdir.exists():
            shutil.rmtree(tmpdir, ignore_errors=True)


@pytest.fixture()
def stage_dir() -> Iterator[Path]:
    d = Path(tempfile.mkdtemp(prefix="haiai-sign-image-stage-"))
    try:
        yield d
    finally:
        shutil.rmtree(d, ignore_errors=True)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_sign_image_round_trip(sdk_client: Any, stage_dir: Path) -> None:
    in_path = stage_dir / "in.png"
    in_path.write_bytes(_make_test_png())
    out_path = stage_dir / "out.png"

    signed = sdk_client.sign_image(str(in_path), str(out_path))
    assert signed.format == "png"
    assert signed.signer_id  # non-empty
    assert signed.out_path == str(out_path)

    result = sdk_client.verify_image(str(out_path))
    assert result.status == "valid", result
    assert result.signer_id == signed.signer_id


def test_sign_image_robust_round_trip(sdk_client: Any, stage_dir: Path) -> None:
    """Robust LSB embedding round-trip — uses the larger fixture source PNG
    so the JACS payload fits the pixel capacity. Skipped if no large source
    PNG is committed under fixtures/media/_source/.
    """
    # Need a PNG large enough to carry the JACS payload via LSB. The fixture
    # signed.png (post-regen) is large enough; the unsigned _source/source.png
    # is sized for metadata-only signing. Fall back to a generated PNG if PIL
    # is available; otherwise skip rather than hard-fail (Issue 002 contract
    # is the round-trip, not the dependency on a particular fixture).
    try:
        from PIL import Image  # type: ignore[import-untyped]
    except ImportError:
        pytest.skip("PIL not available for generating a robust-fitting PNG")
    img = Image.new("RGBA", (256, 256), (32, 64, 128, 255))
    in_path = stage_dir / "in.png"
    img.save(in_path, format="PNG")
    out_path = stage_dir / "out_robust.png"

    signed = sdk_client.sign_image(str(in_path), str(out_path), robust=True)
    assert signed.robust is True
    assert signed.format == "png"

    # verify_image with robust=True scans the LSB channel; metadata is still
    # present (robust embedding writes BOTH channels), so verify works either
    # way. The embedding_channels field hints which channel(s) carried the
    # payload at decode time.
    result = sdk_client.verify_image(str(out_path), robust=True)
    assert result.status == "valid", result
    assert result.embedding_channels in {"metadata", "metadata+lsb"}, result


def test_verify_image_tampered_returns_hash_mismatch(
    sdk_client: Any, stage_dir: Path
) -> None:
    in_path = stage_dir / "in.png"
    in_path.write_bytes(_make_test_png())
    out_path = stage_dir / "out.png"
    sdk_client.sign_image(str(in_path), str(out_path))

    buf = bytearray(out_path.read_bytes())
    idat = buf.find(b"IDAT")
    assert idat != -1
    buf[idat + 6] ^= 0x01
    out_path.write_bytes(buf)

    result = sdk_client.verify_image(str(out_path))
    assert result.status in {"hash_mismatch", "invalid_signature"}, result


def test_extract_media_signature_returns_decoded_json(
    sdk_client: Any, stage_dir: Path
) -> None:
    in_path = stage_dir / "in.png"
    in_path.write_bytes(_make_test_png())
    out_path = stage_dir / "out.png"
    sdk_client.sign_image(str(in_path), str(out_path))

    extracted = sdk_client.extract_media_signature(str(out_path))
    assert extracted.present is True
    assert extracted.payload is not None
    parsed = json.loads(extracted.payload)
    assert isinstance(parsed, dict)


def test_extract_media_signature_raw_returns_base64url(
    sdk_client: Any, stage_dir: Path
) -> None:
    in_path = stage_dir / "in.png"
    in_path.write_bytes(_make_test_png())
    out_path = stage_dir / "out.png"
    sdk_client.sign_image(str(in_path), str(out_path))

    raw = sdk_client.extract_media_signature(str(out_path), raw_payload=True)
    assert raw.present is True
    assert raw.payload is not None
    # base64url-no-pad: ASCII letters, digits, '-', '_'.
    allowed = set(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    )
    assert all(ch in allowed for ch in raw.payload)


def test_extract_media_signature_unsigned_returns_present_false(
    sdk_client: Any, stage_dir: Path
) -> None:
    """Issue 005 / Issue 002 cross-check: an unsigned PNG returns present=False."""
    in_path = stage_dir / "unsigned.png"
    in_path.write_bytes(_make_test_png())

    extracted = sdk_client.extract_media_signature(str(in_path))
    assert extracted.present is False
    assert extracted.payload is None
