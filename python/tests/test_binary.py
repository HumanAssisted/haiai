"""Tests for the binary wrapper module."""

import platform
from pathlib import Path
from unittest.mock import patch

import pytest


def test_binary_module_imports():
    """_binary module should import without errors."""
    from haiai._binary import find_binary, run_binary, main


def test_find_binary_env_override(tmp_path):
    """HAIAI_BINARY_PATH env var should take precedence."""
    from haiai._binary import find_binary

    fake_bin = tmp_path / "haiai"
    fake_bin.write_text("#!/bin/sh\necho test")
    fake_bin.chmod(0o755)

    with patch.dict("os.environ", {"HAIAI_BINARY_PATH": str(fake_bin)}):
        result = find_binary()
        assert result == fake_bin


def test_find_binary_env_override_missing():
    """HAIAI_BINARY_PATH pointing to nonexistent file should be skipped."""
    from haiai._binary import find_binary

    with patch.dict("os.environ", {"HAIAI_BINARY_PATH": "/nonexistent/haiai"}):
        # Should not return the nonexistent path — may return None or a fallback
        result = find_binary()
        assert result is None or str(result) != "/nonexistent/haiai"


def test_find_binary_bundled(tmp_path):
    """Should find binary in the package bin/ directory."""
    from haiai import _binary

    system = platform.system()
    machine = platform.machine()
    bin_name = _binary._PLATFORM_BINARY.get((system, machine))
    if bin_name is None:
        pytest.skip(f"No binary mapping for {system}-{machine}")

    fake_bin = tmp_path / bin_name
    fake_bin.write_text("#!/bin/sh\necho test")
    fake_bin.chmod(0o755)

    with patch.dict("os.environ", {}, clear=False):
        # Remove env override if set
        import os
        os.environ.pop("HAIAI_BINARY_PATH", None)

        with patch.object(_binary, "__file__", str(tmp_path / "_binary.py")):
            # Patch __file__ so bin/ resolves to tmp_path/bin/
            # Actually, _binary looks for Path(__file__).parent / "bin" / bin_name
            # So we need tmp_path / "bin" / bin_name
            bin_dir = tmp_path / "bin"
            bin_dir.mkdir()
            bundled = bin_dir / bin_name
            bundled.write_text("#!/bin/sh\necho test")
            bundled.chmod(0o755)

            result = _binary.find_binary()
            assert result is not None


def test_find_binary_returns_none_when_nothing_available():
    """Should return None when no binary is available anywhere."""
    from haiai._binary import find_binary

    with patch.dict("os.environ", {}, clear=False):
        import os
        os.environ.pop("HAIAI_BINARY_PATH", None)

        with patch("shutil.which", return_value=None):
            # Also ensure bundled doesn't exist
            with patch("pathlib.Path.is_file", return_value=False):
                result = find_binary()
                assert result is None


def test_platform_binary_covers_common_platforms():
    """All 5 target platforms should have binary name mappings."""
    from haiai._binary import _PLATFORM_BINARY

    expected = [
        ("Darwin", "arm64"),
        ("Darwin", "x86_64"),
        ("Linux", "x86_64"),
        ("Linux", "aarch64"),
        ("Windows", "AMD64"),
    ]
    for key in expected:
        assert key in _PLATFORM_BINARY, f"Missing platform: {key}"


def test_main_raises_when_no_binary():
    """main() should raise FileNotFoundError when no binary exists."""
    from haiai._binary import main

    with patch("haiai._binary.find_binary", return_value=None):
        with pytest.raises(FileNotFoundError, match="haiai binary not found"):
            main()
