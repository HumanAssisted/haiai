#!/usr/bin/env python3
"""Exercise source identity and independently versioned JACS manifests."""

from pathlib import Path
import subprocess
import tempfile
import unittest


CHECK = Path(__file__).with_name("check_jacs_source.sh")
NATIVE_CRATES = (
    "jacs-media", "jacs", "binding-core", "jacs-mcp", "jacs-cli",
    "jacspy", "jacsnpm", "jacsgo/lib",
)


class JacsSourceCheck(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="haiai-source-check-")
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name)
        self.write_manifest("jacs-core", "0.14.0")
        for crate in NATIVE_CRATES:
            self.write_manifest(f"archive/native/{crate}", "0.13.0")
        self.git("init", "--quiet")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Synthetic source manifest fixture")
        self.ref = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-c", "user.name=Source fixture", "-c",
             "user.email=source@example.test", "-c", "commit.gpgsign=false",
             "-C", str(self.source), *args], text=True,
        )

    def write_manifest(self, crate, version):
        path = self.source / crate / "Cargo.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'[package]\nname = "fixture"\nversion = "{version}"\n')

    def check(self, *, ref=None, core="0.14.0", source=True):
        return subprocess.run(
            ["bash", str(CHECK), ref or self.ref, "0.13.0",
             str(self.source) if source else "", core],
            capture_output=True, text=True, check=False,
        )

    def assert_refused(self, result, reason):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(reason, result.stderr)

    def test_accepts_core_014_with_native_013(self):
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("core matches 0.14.0", result.stdout)
        self.assertIn("all 8 native manifests match 0.13.0", result.stdout)

    def test_refuses_core_version_drift(self):
        self.write_manifest("jacs-core", "0.13.0")
        self.assert_refused(self.check(), "jacs-core version 0.13.0")

    def test_refuses_each_native_adapter_version_drift(self):
        for crate in NATIVE_CRATES:
            with self.subTest(crate=crate):
                self.write_manifest(f"archive/native/{crate}", "0.14.0")
                self.assert_refused(self.check(), f"{crate} version 0.14.0")
                self.write_manifest(f"archive/native/{crate}", "0.13.0")

    def test_refuses_missing_manifest(self):
        (self.source / "jacs-core/Cargo.toml").unlink()
        self.assert_refused(self.check(), "missing JACS source manifest")

    def test_refuses_other_source_commit(self):
        self.assert_refused(self.check(ref="0" * 40), "does not match")

    def test_refuses_floating_source_ref(self):
        self.assert_refused(self.check(ref="main"), "JACS_REF must be a full commit SHA")

    def test_refuses_invalid_core_configuration_without_checkout(self):
        for core in ("", "latest", "0.14"):
            with self.subTest(core=core):
                self.assert_refused(
                    self.check(core=core, source=False),
                    "invalid expected JACS core version",
                )

    def test_preserves_native_release_tag_validation(self):
        self.git("tag", "crate/v0.13.0")
        result = self.check(ref="crate/v0.13.0")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_preserves_three_argument_coupled_version_callers(self):
        self.write_manifest("jacs-core", "0.13.0")
        result = subprocess.run(
            ["bash", str(CHECK), self.ref, "0.13.0", str(self.source)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
