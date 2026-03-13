"""
Locate and execute the bundled haiai Rust CLI binary.

Resolution order:
1. HAIAI_BINARY_PATH environment variable
2. Bundled binary in haiai/bin/ (installed via platform-specific wheel)
3. Binary on system PATH
"""

import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

# Maps (system, machine) to the binary name used in the wheel
_PLATFORM_BINARY = {
    ("Darwin", "arm64"): "haiai",
    ("Darwin", "x86_64"): "haiai",
    ("Linux", "x86_64"): "haiai",
    ("Linux", "aarch64"): "haiai",
    ("Windows", "AMD64"): "haiai.exe",
}


def find_binary() -> Path | None:
    """Find the haiai binary, returning its path or None."""
    # 1. Environment override
    env_path = os.environ.get("HAIAI_BINARY_PATH")
    if env_path:
        p = Path(env_path)
        if p.is_file():
            return p

    # 2. Bundled binary in package
    system = platform.system()
    machine = platform.machine()
    bin_name = _PLATFORM_BINARY.get((system, machine))
    if bin_name:
        bundled = Path(__file__).parent / "bin" / bin_name
        if bundled.is_file():
            return bundled

    # 3. System PATH
    which = shutil.which("haiai")
    if which:
        return Path(which)

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
    binary = find_binary()
    if binary is not None:
        sys.exit(run_binary())
    else:
        # Fall back to the Python CLI
        from haiai.cli import main as cli_main
        cli_main()


def main_mcp() -> None:
    """Entry point for `haiai mcp` — delegates to `haiai mcp`."""
    binary = find_binary()
    if binary is not None:
        sys.exit(run_binary(extra_args=["mcp"] + sys.argv[1:]))
    else:
        from haiai.mcp_server import main as mcp_main
        mcp_main()
