"""Per-language sign_text / verify_text tests for the Python SDK (Issue 002).

Mirrors `rust/haiai-cli/tests/cli_text_tests.rs` and `rust/hai-binding-core`
sign_text round-trip tests. Exercises the full Python SDK code path.
"""
from __future__ import annotations

import json
import os
import shutil
import tempfile
from pathlib import Path
from typing import Any, Iterator

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
JACS_AGENT_DIR = REPO_ROOT / "fixtures" / "jacs-agent"
FIXTURE_AGENT_PASSWORD = "secretpassord"


def _haiipy_has_media_methods() -> bool:
    try:
        import haiipy  # type: ignore[import-untyped]
    except ImportError:
        return False
    try:
        c = haiipy.HaiClient('{"jacs_id":"x"}')
    except Exception:
        return False
    return hasattr(c, "sign_text_sync") and hasattr(c, "verify_text_sync")


pytestmark = pytest.mark.skipif(
    not _haiipy_has_media_methods(),
    reason=(
        "haiipy wheel missing Layer-8 media methods. "
        "Rebuild via `cd rust && maturin develop -p haiipy --release`."
    ),
)


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
    """Build a haiai.HaiClient with a real FFI adapter, bypassing the
    project-wide `_auto_mock_ffi` fixture in conftest.py (which would
    otherwise inject MockFFIAdapter and defeat the round-trip).
    """
    from haiai import HaiClient, config as haiai_config
    from haiai._ffi_adapter import FFIAdapter
    from haiai.client import _build_ffi_config

    tmpdir = Path(tempfile.mkdtemp(prefix="haiai-sign-text-"))
    config_path = _stage_fixture_agent(tmpdir)
    os.environ["JACS_CONFIG_PATH"] = str(config_path)
    haiai_config.load(str(config_path))
    client = HaiClient()
    client._ffi = FFIAdapter(_build_ffi_config())
    return client, tmpdir


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
    d = Path(tempfile.mkdtemp(prefix="haiai-sign-text-stage-"))
    try:
        yield d
    finally:
        shutil.rmtree(d, ignore_errors=True)


def test_sign_text_round_trip(sdk_client: Any, stage_dir: Path) -> None:
    path = stage_dir / "hello.md"
    path.write_text("# Hello\n")

    outcome = sdk_client.sign_text(str(path))
    assert outcome.signers_added == 1

    result = sdk_client.verify_text(str(path))
    assert result.status == "signed", result
    assert len(result.signatures) == 1
    assert result.signatures[0].status == "valid"


def test_sign_text_default_backup_creates_bak_file(
    sdk_client: Any, stage_dir: Path
) -> None:
    """Default backup=True writes `<path>.bak` (Issue 003 / 002 cross-check)."""
    path = stage_dir / "with-bak.md"
    path.write_text("# original\n")

    sdk_client.sign_text(str(path))
    assert (stage_dir / "with-bak.md.bak").exists()


def test_sign_text_no_backup_skips_bak(sdk_client: Any, stage_dir: Path) -> None:
    """`no_backup=True` skips the `.bak` file."""
    path = stage_dir / "no-bak.md"
    path.write_text("# no backup\n")

    sdk_client.sign_text(str(path), no_backup=True)
    assert not (stage_dir / "no-bak.md.bak").exists()


def test_verify_text_strict_missing_signature_raises_or_returns_status(
    sdk_client: Any, stage_dir: Path
) -> None:
    """Strict mode on a missing signature: SDK either raises an error OR returns
    a non-"signed" status. Mirrors the JACS strict-mode behavior — the SDK
    surface is allowed to choose either, but it MUST signal the failure.
    """
    path = stage_dir / "unsigned.md"
    path.write_text("# untouched\n")

    try:
        result = sdk_client.verify_text(str(path), strict=True)
    except Exception:
        # JACS strict mode raises through the FFI — acceptable.
        return

    # Otherwise the SDK should not report "signed".
    assert result.status != "signed", result
