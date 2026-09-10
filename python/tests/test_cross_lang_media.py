"""Cross-language media verify-parity tests (Python side).

Mirrors `rust/haiai/tests/cross_lang_contract.rs` (`cross_lang_signed_image_*`
and `cross_lang_signed_text_md_*`). Loads the same pre-signed fixtures from
`fixtures/media/signed.{png,jpg,webp,md}` (signed once by the Rust regenerator
in `rust/haiai/tests/regen_media_fixtures.rs`, with its public key committed
beside the media) and asserts that the Python `HaiClient.verify_image`
/ `.verify_text` paths produce the same Valid / HashMismatch verdicts as Rust.

The legacy fixture agent config is intentionally not loaded: it predates signed
agent configs. Verification uses a fresh signed local agent and resolves the
fixture signer through its staged public key.

Any drift between languages here MUST be a parity bug, not a test-only quirk.
That is the entire point of this suite (PRD §5.5, TASK_011).

Skips gracefully when the installed `haiipy` wheel does not yet expose the
media methods (i.e., it predates TASK_007 PyO3 bindings — rebuild via
`maturin develop` to re-run).
"""

from __future__ import annotations

import json
import os
import shutil
import tempfile
from hashlib import sha256
from pathlib import Path
from typing import Any, Iterator

import pytest

from tests.jacs_test_utils import create_signed_jacs_agent

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[2]
MEDIA_DIR = REPO_ROOT / "fixtures" / "media"
SIGNER_FIXTURE_PATH = MEDIA_DIR / "SIGNER.json"
CHECKSUMS_PATH = MEDIA_DIR / "CHECKSUMS.txt"

# Password for the fresh local verifier agent.
VERIFIER_AGENT_PASSWORD = "secretpassord"


# ---------------------------------------------------------------------------
# FFI availability — skip when the installed haiipy wheel pre-dates TASK_007.
# ---------------------------------------------------------------------------


def _haiipy_has_media_methods() -> bool:
    """Return True iff the installed haiipy wheel exposes media methods."""
    try:
        import haiipy  # type: ignore[import-untyped]
    except ImportError:
        return False
    # Probe via a throwaway client; constructor accepts any JSON.
    try:
        c = haiipy.HaiClient('{"jacs_id":"x"}')
    except Exception:
        return False
    return all(
        hasattr(c, attr)
        for attr in (
            "verify_image_sync",
            "verify_text_sync",
            "sign_image_sync",
            "sign_text_sync",
            "extract_media_signature_sync",
        )
    )


# Apply at module load. If false, every test in the module is skipped.
pytestmark = pytest.mark.skipif(
    not _haiipy_has_media_methods(),
    reason=(
        "Installed haiipy wheel does not expose the Layer-8 media methods. "
        "Rebuild with `cd rust && maturin develop -p haiipy --release` "
        "(or rerun `pip install -e python/`) once JACS 0.10.0 is available."
    ),
)


# ---------------------------------------------------------------------------
# Fixture loading helpers — mirror Rust `cross_lang_contract.rs`.
# ---------------------------------------------------------------------------


def _load_signer() -> dict[str, str]:
    return json.loads(SIGNER_FIXTURE_PATH.read_text())


def _load_checksum(name: str) -> str:
    for line in CHECKSUMS_PATH.read_text().splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1] == name:
            return parts[0]
    raise AssertionError(f"no checksum for {name} in CHECKSUMS.txt")


def _read_signed_with_checksum(name: str) -> bytes:
    bytes_ = (MEDIA_DIR / name).read_bytes()
    expected = _load_checksum(name)
    got = sha256(bytes_).hexdigest()
    assert got == expected, (
        f"checksum drift on fixtures/media/{name}: got {got}, expected {expected}"
    )
    return bytes_


def _build_ffi_client() -> tuple[Any, str, Path]:
    """Construct a `haiipy.HaiClient` with strict-safe verifier state.

    Goes through the FFI directly (not `haiai.HaiClient`) because the test
    only needs the verify methods; this avoids the SimpleAgent.load round-trip
    and keeps the test scoped to the parity contract.

    The pre-signed media keeps its historical PQ signer, whose public key is
    supplied through JACS' explicit key-directory verification option. The
    local agent is generated and signed at its final paths; no unsigned-config
    compatibility escape is used.

    Returns the raw FFI client, verification options JSON, and tempdir path so
    the caller can keep the files alive (PyO3 objects reject attributes).
    """
    import haiipy  # type: ignore[import-untyped]

    tmpdir = Path(tempfile.mkdtemp(prefix="haiai-media-parity-"))
    verifier = create_signed_jacs_agent(
        tmpdir / "verifier-agent",
        name="media-fixture-verifier",
        password=VERIFIER_AGENT_PASSWORD,
    )

    signer = _load_signer()
    verification_keys = tmpdir / "verification-keys"
    verification_keys.mkdir()
    shutil.copy2(
        REPO_ROOT / signer["public_key_file"],
        verification_keys / f"{signer['signer_id']}.public.pem",
    )

    ffi_config = json.dumps(
        {
            "jacs_id": verifier.jacs_id,
            "agent_name": "MediaFixtureVerifier",
            "agent_version": "1.0.0",
            "key_dir": str(verifier.key_dir),
            "jacs_config_path": str(verifier.config_path),
            "base_url": "http://localhost:1",  # never used; verify is local-only
        }
    )
    client = haiipy.HaiClient(ffi_config)
    verify_options = json.dumps({"key_dir": str(verification_keys)})
    return client, verify_options, tmpdir


# ---------------------------------------------------------------------------
# Tampering helpers — mirror Rust `tamper_after` + `tamper_text_body`.
# ---------------------------------------------------------------------------


def _tamper_after(buf: bytearray, marker: bytes, offset: int) -> None:
    idx = buf.find(marker)
    assert idx != -1, f"marker {marker!r} not found"
    target = idx + len(marker) + offset
    buf[target] ^= 0x01


def _tamper_text_body(buf: bytearray) -> None:
    """Flip case on a body byte BEFORE the JACS signature block."""
    marker = b"-----BEGIN JACS SIGNATURE-----"
    body_end = buf.find(marker)
    assert body_end != -1, "BEGIN marker present in signed.md"
    # Walk back to a printable ASCII letter and toggle case bit (0x20).
    for i in range(body_end - 1, -1, -1):
        c = buf[i]
        if (0x41 <= c <= 0x5A) or (0x61 <= c <= 0x7A):
            buf[i] ^= 0b0010_0000
            return
    raise AssertionError("no ASCII letter found before signature block")


# ---------------------------------------------------------------------------
# Per-test fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(scope="module")
def signer() -> dict[str, str]:
    return _load_signer()


@pytest.fixture(autouse=True)
def _fixture_agent_password(monkeypatch: pytest.MonkeyPatch) -> None:
    """Override the project-wide `password_env` autouse with the verifier
    agent's password. This must run on every test that uses the FFI client,
    because conftest.py's `password_env` fixture (autouse, function-scoped)
    sets `JACS_PRIVATE_KEY_PASSWORD` to a different value otherwise.
    """
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", VERIFIER_AGENT_PASSWORD)


@pytest.fixture(scope="module")
def ffi_client() -> Iterator[tuple[Any, str]]:
    # Set the password before the FFI client is constructed (module scope).
    # The function-scoped autouse `_fixture_agent_password` keeps it set for
    # each subsequent test invocation that calls into the FFI.
    os.environ["JACS_PRIVATE_KEY_PASSWORD"] = VERIFIER_AGENT_PASSWORD
    client, verify_options, tmpdir = _build_ffi_client()
    try:
        yield client, verify_options
    finally:
        if tmpdir.exists():
            shutil.rmtree(tmpdir, ignore_errors=True)


@pytest.fixture()
def stage_dir() -> Iterator[Path]:
    """Per-test tempdir for staging the (possibly tampered) signed bytes."""
    d = Path(tempfile.mkdtemp(prefix="haiai-media-stage-"))
    try:
        yield d
    finally:
        shutil.rmtree(d, ignore_errors=True)


# ---------------------------------------------------------------------------
# verify_image — Python parity (mirror Rust assert_image_valid / tampered)
# ---------------------------------------------------------------------------


def _verify_image(client_with_options: tuple[Any, str], path: Path) -> dict[str, Any]:
    client, verify_options = client_with_options
    raw = client.verify_image_sync(str(path), verify_options)
    return json.loads(raw)


def _verify_text(client_with_options: tuple[Any, str], path: Path) -> dict[str, Any]:
    client, verify_options = client_with_options
    raw = client.verify_text_sync(str(path), verify_options)
    return json.loads(raw)


def _stage_signed(name: str, dest: Path) -> Path:
    target = dest / name
    target.write_bytes(_read_signed_with_checksum(name))
    return target


def _stage_tampered(name: str, marker: bytes, offset: int, dest: Path) -> Path:
    target = dest / name
    buf = bytearray(_read_signed_with_checksum(name))
    _tamper_after(buf, marker, offset)
    target.write_bytes(buf)
    return target


def test_signed_image_png_verifies(
    ffi_client: Any, signer: dict[str, str], stage_dir: Path
) -> None:
    path = _stage_signed("signed.png", stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "valid", result
    assert result.get("signer_id") == signer["signer_id"], result


def test_signed_image_png_tampered_returns_hash_mismatch(
    ffi_client: Any, stage_dir: Path
) -> None:
    path = _stage_tampered("signed.png", b"IDAT", 6, stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "hash_mismatch", result


def test_signed_image_jpeg_verifies(
    ffi_client: Any, signer: dict[str, str], stage_dir: Path
) -> None:
    path = _stage_signed("signed.jpg", stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "valid", result
    assert result.get("signer_id") == signer["signer_id"], result


def test_signed_image_jpeg_tampered_returns_hash_mismatch(
    ffi_client: Any, stage_dir: Path
) -> None:
    path = _stage_tampered("signed.jpg", b"\xff\xda", 4, stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "hash_mismatch", result


def test_signed_image_webp_verifies(
    ffi_client: Any, signer: dict[str, str], stage_dir: Path
) -> None:
    path = _stage_signed("signed.webp", stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "valid", result
    assert result.get("signer_id") == signer["signer_id"], result


def test_signed_image_webp_tampered_returns_hash_mismatch(
    ffi_client: Any, stage_dir: Path
) -> None:
    path = _stage_tampered("signed.webp", b"VP8L", 4, stage_dir)
    result = _verify_image(ffi_client, path)
    assert result["status"] == "hash_mismatch", result


# ---------------------------------------------------------------------------
# verify_text — Python parity
# ---------------------------------------------------------------------------


def test_signed_text_md_verifies(
    ffi_client: Any, signer: dict[str, str], stage_dir: Path
) -> None:
    target = stage_dir / "signed.md"
    target.write_bytes(_read_signed_with_checksum("signed.md"))
    result = _verify_text(ffi_client, target)
    assert result["status"] == "signed", result
    sigs = result.get("signatures") or []
    assert len(sigs) == 1, f"expected 1 signature, got {len(sigs)}: {sigs}"
    assert sigs[0]["status"] == "valid", sigs[0]
    assert sigs[0]["signer_id"] == signer["signer_id"], sigs[0]


def test_signed_text_md_tampered_returns_hash_mismatch(
    ffi_client: Any, stage_dir: Path
) -> None:
    target = stage_dir / "signed.md"
    buf = bytearray(_read_signed_with_checksum("signed.md"))
    _tamper_text_body(buf)
    target.write_bytes(buf)

    result = _verify_text(ffi_client, target)
    assert result["status"] == "signed", result
    sigs = result.get("signatures") or []
    assert len(sigs) == 1, f"expected 1 signature, got {len(sigs)}: {sigs}"
    assert sigs[0]["status"] == "hash_mismatch", sigs[0]
