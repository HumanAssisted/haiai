#!/usr/bin/env python3
"""
Embed a platform-specific haiai binary into the Python package for wheel building.

Usage:
    # From CI — embed binary from artifacts directory:
    python embed_binary.py <artifacts-dir> <version>

    # Local development — embed a single binary for current platform:
    python embed_binary.py --local <path-to-binary>

The binary is placed at src/haiai/bin/<binary-name> so hatchling includes it
in the wheel.
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

PACKAGE_BIN_DIR = Path(__file__).parent.parent / "src" / "haiai" / "bin"

# Maps CI asset suffix to (system, machine, binary_name)
PLATFORM_MAP = {
    "darwin-arm64": ("Darwin", "arm64", "haiai"),
    "darwin-x64": ("Darwin", "x86_64", "haiai"),
    "linux-x64": ("Linux", "x86_64", "haiai"),
    "linux-arm64": ("Linux", "aarch64", "haiai"),
    "windows-x64": ("Windows", "AMD64", "haiai.exe"),
}

# Reverse: (system, machine) -> asset suffix
CURRENT_PLATFORM = {
    ("Darwin", "arm64"): "darwin-arm64",
    ("Darwin", "x86_64"): "darwin-x64",
    ("Linux", "x86_64"): "linux-x64",
    ("Linux", "aarch64"): "linux-arm64",
    ("Windows", "AMD64"): "windows-x64",
}


def embed_from_artifacts(artifacts_dir: str, version: str, target_platform: str | None = None):
    """Extract binary from CI artifact and place in package bin dir."""
    artifacts = Path(artifacts_dir)
    PACKAGE_BIN_DIR.mkdir(parents=True, exist_ok=True)

    if target_platform:
        platforms = {target_platform: PLATFORM_MAP[target_platform]}
    else:
        # Current platform only
        key = CURRENT_PLATFORM.get((platform.system(), platform.machine()))
        if not key:
            print(f"Unsupported platform: {platform.system()}-{platform.machine()}", file=sys.stderr)
            sys.exit(1)
        platforms = {key: PLATFORM_MAP[key]}

    for suffix, (_, _, bin_name) in platforms.items():
        is_windows = suffix.startswith("windows")
        ext = "zip" if is_windows else "tar.gz"
        archive_name = f"haiai-cli-{version}-{suffix}.{ext}"

        # Search for archive (CI may put in subdirectories)
        archive_path = artifacts / archive_name
        if not archive_path.exists():
            # glob search
            matches = list(artifacts.rglob(archive_name))
            if not matches:
                print(f"Warning: {archive_name} not found in {artifacts_dir}", file=sys.stderr)
                continue
            archive_path = matches[0]

        with tempfile.TemporaryDirectory() as tmpdir:
            if is_windows:
                subprocess.run(["unzip", "-o", str(archive_path), "-d", tmpdir], check=True)
            else:
                subprocess.run(["tar", "xzf", str(archive_path), "-C", tmpdir], check=True)

            # Find the binary (may be named haiai-cli or haiai)
            tmp = Path(tmpdir)
            for candidate in ["haiai-cli", "haiai", "haiai-cli.exe", "haiai.exe"]:
                src = tmp / candidate
                if src.exists():
                    dest = PACKAGE_BIN_DIR / bin_name
                    shutil.copy2(src, dest)
                    dest.chmod(0o755)
                    print(f"Embedded {src.name} -> {dest}")
                    break
            else:
                print(f"Binary not found in {archive_name}", file=sys.stderr)
                sys.exit(1)


def embed_local(binary_path: str):
    """Copy a local binary into the package bin dir."""
    src = Path(binary_path)
    if not src.is_file():
        print(f"Binary not found: {binary_path}", file=sys.stderr)
        sys.exit(1)

    PACKAGE_BIN_DIR.mkdir(parents=True, exist_ok=True)
    bin_name = "haiai.exe" if platform.system() == "Windows" else "haiai"
    dest = PACKAGE_BIN_DIR / bin_name
    shutil.copy2(src, dest)
    dest.chmod(0o755)
    print(f"Embedded {src} -> {dest}")


def main():
    parser = argparse.ArgumentParser(description="Embed haiai binary into Python package")
    parser.add_argument("--local", metavar="BINARY", help="Path to local binary")
    parser.add_argument("--platform", metavar="PLATFORM",
                        help="Target platform (e.g. darwin-arm64)")
    parser.add_argument("artifacts_dir", nargs="?", help="Directory with CI artifacts")
    parser.add_argument("version", nargs="?", help="Version string")
    args = parser.parse_args()

    if args.local:
        embed_local(args.local)
    elif args.artifacts_dir and args.version:
        embed_from_artifacts(args.artifacts_dir, args.version, args.platform)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
