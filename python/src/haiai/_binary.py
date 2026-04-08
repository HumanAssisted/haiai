"""
Locate and execute the bundled haiai Rust CLI binary.

Resolution order:
1. HAIAI_BINARY_PATH environment variable
2. Bundled binary in haiai/bin/ (installed via platform-specific wheel)
3. Project-local binary: walk up from cwd looking for .cargo-local/bin/haiai
4. Binary on system PATH (only if version matches the SDK)
"""

import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path

# Must match the version in pyproject.toml.
_SDK_VERSION = "0.2.2"

# Maps (system, machine) to the binary name used in the wheel
_PLATFORM_BINARY = {
    ("Darwin", "arm64"): "haiai",
    ("Darwin", "x86_64"): "haiai",
    ("Linux", "x86_64"): "haiai",
    ("Linux", "aarch64"): "haiai",
    ("Windows", "AMD64"): "haiai.exe",
}

_VERSION_RE = re.compile(r"haiai (\d+\.\d+\.\d+)")


def _get_binary_version(binary: Path) -> str | None:
    """Run `haiai --version` and return the version string, or None on failure."""
    try:
        out = subprocess.run(
            [str(binary), "--version"],
            capture_output=True, text=True, timeout=5,
        )
        m = _VERSION_RE.search(out.stdout)
        return m.group(1) if m else None
    except Exception:
        return None


def _walk_up_for_cargo_local(bin_name: str) -> Path | None:
    """Walk up from cwd looking for .cargo-local/bin/<bin_name>."""
    cur = Path.cwd().resolve()
    for _ in range(20):  # cap depth
        candidate = cur / ".cargo-local" / "bin" / bin_name
        if candidate.is_file():
            return candidate
        parent = cur.parent
        if parent == cur:
            break
        cur = parent
    return None


def find_binary() -> Path | None:
    """Find the haiai binary, returning its path or None."""
    bin_name = _PLATFORM_BINARY.get(
        (platform.system(), platform.machine()), "haiai"
    )

    # 1. Environment override (trusted -- skip version check)
    env_path = os.environ.get("HAIAI_BINARY_PATH")
    if env_path:
        p = Path(env_path)
        if p.is_file():
            return p

    # 2. Bundled binary in package
    bundled = Path(__file__).parent / "bin" / bin_name
    if bundled.is_file():
        return bundled

    # 3. Project-local .cargo-local/bin/haiai
    local = _walk_up_for_cargo_local(bin_name)
    if local:
        return local

    # 4. System PATH -- only accept if major.minor matches the SDK
    which = shutil.which("haiai")
    if which:
        p = Path(which)
        ver = _get_binary_version(p)
        sdk_major_minor = ".".join(_SDK_VERSION.split(".")[:2])
        if ver and ver.startswith(sdk_major_minor):
            return p
        # Version mismatch -- warn and skip
        if ver:
            print(
                f"haiai: skipping PATH binary {p} (v{ver}) -- "
                f"SDK requires v{sdk_major_minor}.x",
                file=sys.stderr,
            )

    return None


def run_binary(extra_args: list[str] | None = None) -> int:
    """Find and exec the haiai binary with sys.argv (or extra_args).

    Returns the process exit code. Raises FileNotFoundError if no binary found.
    """
    binary = find_binary()
    if binary is None:
        raise FileNotFoundError(
            f"haiai binary not found for {platform.system()}-{platform.machine()}. "
            "Install a platform-specific wheel or set HAIAI_BINARY_PATH."
        )

    args = extra_args if extra_args is not None else sys.argv[1:]

    if sys.platform != "win32":
        # On Unix, exec replaces the process
        os.execv(str(binary), [str(binary)] + args)
        # unreachable
    else:
        # On Windows, use subprocess
        result = subprocess.run([str(binary)] + args)
        return result.returncode

    return 0  # unreachable on Unix


def main() -> None:
    """Entry point for the `haiai` console script — delegates to the Rust binary."""
    sys.exit(run_binary())


def main_mcp() -> None:
    """Entry point for `haiai mcp` — delegates to `haiai mcp`."""
    sys.exit(run_binary(extra_args=["mcp"] + sys.argv[1:]))
