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


# ---------------------------------------------------------------------------
# Resolution logic tests
# ---------------------------------------------------------------------------

import sys


def _make_stub(p: Path, version: str = "0.2.0") -> Path:
    """Create a stub haiai binary that prints the given version string."""
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(f'#!/bin/sh\necho "haiai {version}"\n')
    p.chmod(0o755)
    return p


class TestWalkUpForCargoLocal:
    """Tests for _walk_up_for_cargo_local (step 3)."""

    def test_finds_in_cwd(self, tmp_path):
        from haiai._binary import _walk_up_for_cargo_local

        stub = _make_stub(tmp_path / ".cargo-local" / "bin" / "haiai")

        with patch.object(Path, "cwd", return_value=tmp_path):
            assert _walk_up_for_cargo_local("haiai") == stub

    def test_finds_in_ancestor(self, tmp_path):
        from haiai._binary import _walk_up_for_cargo_local

        stub = _make_stub(tmp_path / ".cargo-local" / "bin" / "haiai")
        nested = tmp_path / "a" / "b" / "c"
        nested.mkdir(parents=True)

        with patch.object(Path, "cwd", return_value=nested):
            assert _walk_up_for_cargo_local("haiai") == stub

    def test_returns_none_when_absent(self, tmp_path):
        from haiai._binary import _walk_up_for_cargo_local

        with patch.object(Path, "cwd", return_value=tmp_path):
            assert _walk_up_for_cargo_local("haiai") is None

    def test_skips_directory_named_haiai(self, tmp_path):
        from haiai._binary import _walk_up_for_cargo_local

        # .cargo-local/bin/haiai exists but is a directory, not a file
        (tmp_path / ".cargo-local" / "bin" / "haiai").mkdir(parents=True)

        with patch.object(Path, "cwd", return_value=tmp_path):
            assert _walk_up_for_cargo_local("haiai") is None


class TestGetBinaryVersion:
    """Tests for _get_binary_version."""

    @pytest.mark.skipif(sys.platform == "win32", reason="Shell stubs need Unix")
    def test_parses_version(self, tmp_path):
        from haiai._binary import _get_binary_version

        stub = _make_stub(tmp_path / "haiai", "1.23.456")
        assert _get_binary_version(stub) == "1.23.456"

    @pytest.mark.skipif(sys.platform == "win32", reason="Shell stubs need Unix")
    def test_returns_none_on_garbage(self, tmp_path):
        from haiai._binary import _get_binary_version

        stub = tmp_path / "haiai"
        stub.write_text('#!/bin/sh\necho "not a version"\n')
        stub.chmod(0o755)
        assert _get_binary_version(stub) is None

    def test_returns_none_for_missing(self):
        from haiai._binary import _get_binary_version

        assert _get_binary_version(Path("/nonexistent/haiai")) is None


class TestFindBinaryResolution:
    """Integration tests for find_binary resolution priority and version guard."""

    def _no_env(self):
        """Context manager: clear HAIAI_BINARY_PATH."""
        import os
        return patch.dict("os.environ", {k: v for k, v in os.environ.items()
                                          if k != "HAIAI_BINARY_PATH"}, clear=True)

    def _no_bundled(self):
        """Context manager: make bundled-binary check fail."""
        from haiai import _binary
        return patch.object(_binary, "__file__", "/nonexistent/_binary.py")

    # -- Priority --

    def test_env_beats_cargo_local(self, tmp_path):
        from haiai._binary import find_binary

        env_bin = _make_stub(tmp_path / "env" / "haiai")
        _make_stub(tmp_path / ".cargo-local" / "bin" / "haiai")

        with patch.dict("os.environ", {"HAIAI_BINARY_PATH": str(env_bin)}):
            with patch.object(Path, "cwd", return_value=tmp_path):
                assert find_binary() == env_bin

    def test_cargo_local_beats_path(self, tmp_path):
        from haiai import _binary

        cargo_bin = _make_stub(
            tmp_path / "project" / ".cargo-local" / "bin" / "haiai"
        )
        path_bin = _make_stub(tmp_path / "sys" / "haiai")

        with self._no_env(), self._no_bundled():
            with patch.object(Path, "cwd", return_value=tmp_path / "project"):
                with patch("shutil.which", return_value=str(path_bin)):
                    assert _binary.find_binary() == cargo_bin

    # -- Version guard (step 4) --

    def test_path_accepted_when_version_matches(self, tmp_path):
        from haiai import _binary

        path_bin = _make_stub(tmp_path / "sys" / "haiai")

        with self._no_env(), self._no_bundled():
            with patch.object(Path, "cwd", return_value=tmp_path):
                with patch("shutil.which", return_value=str(path_bin)):
                    with patch(
                        "haiai._binary._get_binary_version",
                        return_value=_binary._SDK_VERSION,
                    ):
                        assert _binary.find_binary() == Path(str(path_bin))

    def test_path_rejected_when_version_mismatches(self, tmp_path):
        from haiai import _binary

        old_bin = _make_stub(tmp_path / "sys" / "haiai", "0.1.4")

        with self._no_env(), self._no_bundled():
            with patch.object(Path, "cwd", return_value=tmp_path):
                with patch("shutil.which", return_value=str(old_bin)):
                    with patch(
                        "haiai._binary._get_binary_version",
                        return_value="0.1.4",
                    ):
                        assert _binary.find_binary() is None

    def test_path_mismatch_warns_on_stderr(self, tmp_path, capsys):
        from haiai import _binary

        old_bin = _make_stub(tmp_path / "sys" / "haiai", "0.1.4")

        with self._no_env(), self._no_bundled():
            with patch.object(Path, "cwd", return_value=tmp_path):
                with patch("shutil.which", return_value=str(old_bin)):
                    with patch(
                        "haiai._binary._get_binary_version",
                        return_value="0.1.4",
                    ):
                        _binary.find_binary()
                        err = capsys.readouterr().err
                        assert "skipping PATH binary" in err
                        assert "0.1.4" in err

    def test_path_skipped_when_version_unknown(self, tmp_path):
        """PATH binary that returns no parseable version should be skipped."""
        from haiai import _binary

        with self._no_env(), self._no_bundled():
            with patch.object(Path, "cwd", return_value=tmp_path):
                with patch("shutil.which", return_value="/usr/bin/haiai"):
                    with patch(
                        "haiai._binary._get_binary_version",
                        return_value=None,
                    ):
                        assert _binary.find_binary() is None


def test_sdk_version_matches_pyproject():
    """_SDK_VERSION must stay in sync with pyproject.toml."""
    import re as _re
    from haiai._binary import _SDK_VERSION

    pyproject = Path(__file__).resolve().parent.parent / "pyproject.toml"
    if not pyproject.is_file():
        pytest.skip("pyproject.toml not found")

    m = _re.search(r'^version\s*=\s*"([^"]+)"', pyproject.read_text(), _re.MULTILINE)
    assert m is not None, "version not found in pyproject.toml"
    assert _SDK_VERSION == m.group(1), (
        f"_SDK_VERSION={_SDK_VERSION!r} != pyproject.toml version={m.group(1)!r}"
    )
