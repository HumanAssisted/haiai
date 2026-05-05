"""Real-FFI smoke tests for haiipy.

Two tests, one per backend, both loading the real haiipy native binding
(PyO3 cdylib) and exercising `save_memory("...")` end-to-end:

1. **Remote** (`test_save_memory_round_trips_through_native_binding`) —
   hosted production path. Sets ``JACS_DEFAULT_STORAGE=remote`` so the FFI
   builds a `RemoteJacsProvider`, signs locally, POSTs to a stdlib
   `http.server.HTTPServer` mock, and reads the server-issued key from the
   response. Verifies the mock saw a `POST /api/v1/records` with signed
   markdown bytes (plaintext plus the JACS signature footer).

2. **Local** (`test_save_memory_local_path_through_native_binding`) — dev
   default path (`haiai init` writes ``default_storage: "fs"``). Bootstraps
   a fresh agent, sets ``JACS_DEFAULT_STORAGE=fs``, signs locally, writes to
   disk, and returns a client-side ``{jacsId}:{jacsVersion}`` key. Verifies
   the doc round-trips via `get_record_bytes(key)`.

Together these two tests cover the only two backends production and dev
users actually exercise — and would have caught a regression in either
the remote routing path OR the local FS routing path.

Skipped cleanly when:
- haiipy is not built / installable (`importorskip`).
- The CI environment lacks the JACS toolchain to bootstrap a test agent.

Marker: `@pytest.mark.native_smoke`. Run with:

    pytest -m native_smoke python/tests/

Per PRD docs/haiai/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: stdlib
`http.server.HTTPServer` (no `respx`/`httpx`-level mock). The traffic is
Rust `reqwest` running INSIDE the haiipy native binding, which only a real
listening socket can intercept.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any

import pytest

haiipy = pytest.importorskip("haiipy", reason="haiipy native binding not built")


pytestmark = pytest.mark.native_smoke


# ``LocalJacsProvider::store_signed_text`` returns the key as
# ``{jacsId}:{jacsVersion}`` where both halves are JACS UUIDs. This regex
# matches that exact shape so the local-path test asserts on the key
# *structure* (not a specific value, which would change every run).
_LOCAL_KEY_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
    r":[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
)


def _locate_haiai_cli() -> str | None:
    """Return the path to the haiai CLI binary built by the smoke-tests
    workflow, or ``None`` when it isn't available.

    Search order:
    1. ``HAIAI_CLI`` env var (explicit override).
    2. ``rust/target/release/haiai`` at the repo root (the smoke-tests
       workflow's `Build haiai CLI` step writes here). Resolved by walking
       up from this test file.
    3. ``haiai`` on ``PATH`` (local dev with the cli installed).
    """
    explicit = os.environ.get("HAIAI_CLI")
    if explicit and os.access(explicit, os.X_OK):
        return explicit

    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "rust" / "target" / "release" / "haiai"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
        if (parent / ".git").exists():
            break  # don't walk past the repo root

    on_path = shutil.which("haiai")
    return on_path


def _bootstrap_fresh_jacs_agent(workdir: str) -> str:
    """Create a brand-new JACS agent in ``workdir`` via the ``haiai init``
    subprocess. Returns the absolute path to the freshly-written
    ``jacs.config.json``.

    This is the hermetic alternative to ``_bootstrap_jacs_agent`` for tests
    that must NOT share a ``data_directory`` with sibling tests in the same
    pytest session — most importantly the local-path smoke test, whose
    ``find_document(jacs_type="memory", singleton)`` would otherwise pick
    up state written by an earlier test in the shared smoke-agent dir.

    Skips cleanly when the ``haiai`` CLI isn't on disk (e.g. local dev
    without a release build).
    """
    cli = _locate_haiai_cli()
    if not cli:
        pytest.skip(
            "haiai CLI binary not found; cannot bootstrap a fresh JACS "
            "agent for the local-path smoke test"
        )

    workdir = os.path.realpath(workdir)
    config_path = os.path.join(workdir, "jacs.config.json")
    data_dir = os.path.join(workdir, "data")
    key_dir = os.path.join(workdir, "keys")

    # Use the same password the rest of the smoke-tests lane uses so the
    # parent process's `JACS_PRIVATE_KEY_PASSWORD` (which the FFI reads at
    # signing time) decrypts the agent's freshly-minted private key.
    password = (
        os.environ.get("_HAISDK_SMOKE_PASSWORD")
        or os.environ.get("JACS_PRIVATE_KEY_PASSWORD")
        or "smoke-password"
    )

    env = os.environ.copy()
    env["JACS_PRIVATE_KEY_PASSWORD"] = password

    result = subprocess.run(
        [
            cli,
            "init",
            "--quiet",
            "--name",
            "local-smoke-agent",
            "--register",
            "false",
            "--data-dir",
            data_dir,
            "--key-dir",
            key_dir,
            "--config-path",
            config_path,
        ],
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        pytest.skip(
            f"haiai init failed (rc={result.returncode}): "
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )

    if not os.path.exists(config_path):
        pytest.skip(
            f"haiai init succeeded but {config_path} was not written"
        )

    return config_path


def _restore_smoke_password(monkeypatch: pytest.MonkeyPatch) -> None:
    """Restore the password the agent was created with.

    The conftest's autouse `password_env` fixture sets
    ``JACS_PRIVATE_KEY_PASSWORD`` to a generic test value. Smoke tests need
    the *real* password the pre-baked agent was created with (CI sets
    ``_HAISDK_SMOKE_PASSWORD=smoke-password`` end-to-end).
    """
    pre_conftest_password = (
        os.environ.get("_HAISDK_SMOKE_PASSWORD")
        or os.environ.get("JACS_PRIVATE_KEY_PASSWORD")
    )
    if pre_conftest_password:
        monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", pre_conftest_password)


class _RecordsHandler(BaseHTTPRequestHandler):
    """Mock for `/api/v1/records`. Handles two methods used by the remote
    `save_memory` flow:

    - ``GET /api/v1/records?type=memory&...`` — issued by the FFI's
      ``find_document`` step (singleton resolution) before POSTing. Returns
      an empty ``items`` list so the FFI takes the "no existing singleton →
      create" branch.
    - ``POST /api/v1/records`` — issued by the FFI to persist the signed
      memory artifact. Returns the canned ``key: "smoke:v1"`` envelope the
      test asserts on.

    Records the request body and headers on the parent server so the test
    can assert what the FFI sent.
    """

    # Suppress the default stderr access log so test output stays clean.
    def log_message(self, format: str, *args: Any) -> None:  # type: ignore[override]
        del format, args

    def _capture(self, body: bytes) -> None:
        captured = getattr(self.server, "captured", [])
        captured.append(
            {
                "method": self.command,
                "path": self.path,
                "headers": dict(self.headers.items()),
                "body": body,
            }
        )
        self.server.captured = captured  # type: ignore[attr-defined]

    def do_GET(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler dispatch name
        self._capture(b"")
        # The FFI's find_document(singleton) issues GET with a `type=` query
        # param. Return an empty items list so the caller takes the
        # "no existing singleton → create" branch.
        if self.path.startswith("/api/v1/records"):
            payload = {"items": [], "next_cursor": None}
            data = json.dumps(payload).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler dispatch name
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""
        self._capture(body)

        if self.path == "/api/v1/records":
            payload = {
                "key": "smoke:v1",
                "id": "smoke",
                "version": "v1",
                "jacsType": "memory",
                "jacsVersionDate": "2026-01-01T00:00:00Z",
            }
            data = json.dumps(payload).encode("utf-8")
            self.send_response(201)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()


def _bootstrap_jacs_agent(workdir: str) -> str:
    """Resolve a JACS agent config path for the smoke test.

    Two paths:
    1. **Pre-baked agent (preferred for CI).** If `JACS_SMOKE_AGENT_DIR` is set
       and contains a `jacs.config.json`, use it directly. CI bootstraps the
       agent once via `haiai init --register false` and shares it across all
       three smoke tests (Issue 003).
    2. **In-process bootstrap (for local dev).** Fall back to creating an
       agent via `haiai.config.create_agent`. Skips when the JACS toolchain
       isn't available.

    Returns the absolute path to `jacs.config.json`.
    """
    # Path 1: pre-baked agent dir (CI).
    agent_dir = os.environ.get("JACS_SMOKE_AGENT_DIR")
    if agent_dir:
        prebaked = os.path.join(agent_dir, "jacs.config.json")
        if os.path.exists(prebaked):
            return prebaked
        pytest.skip(
            f"JACS_SMOKE_AGENT_DIR={agent_dir} but jacs.config.json not found"
        )

    # Path 2: in-process bootstrap (local dev).
    try:
        from haiai.config import create_agent  # type: ignore[import-not-found]
    except Exception:  # pragma: no cover — environment-specific
        pytest.skip("haiai.config.create_agent unavailable; cannot bootstrap JACS agent")

    config_path = os.path.join(workdir, "jacs.config.json")
    try:
        result = create_agent(
            agent_name="smoke-agent",
            password="smoke-password",
            data_directory=workdir,
            key_directory=workdir,
            config_path=config_path,
        )
    except Exception as exc:  # pragma: no cover — environment-specific
        pytest.skip(f"JACS agent creation failed (not a binding bug): {exc}")

    # `create_agent` may write to a slightly different location on disk;
    # fall back to whatever path it reports if our preferred path is empty.
    if not os.path.exists(config_path):
        result_path = getattr(result, "config_path", None) or (
            result.get("config_path") if isinstance(result, dict) else None
        )
        if result_path and os.path.exists(result_path):
            return result_path
        pytest.skip("JACS agent created but config path not found")
    return config_path


def test_save_memory_round_trips_through_native_binding(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """End-to-end smoke (REMOTE backend): haiipy.HaiClient.save_memory_sync
    hits a real HTTP socket.

    Mirrors hosted production: ``JACS_DEFAULT_STORAGE=remote`` causes the
    FFI to build a `RemoteJacsProvider`, sign locally, POST to
    ``base_url/api/v1/records``, and return the server-issued key.

    The agent's bootstrapped key is decrypted with the password the agent
    was actually created with — either ``_HAISDK_SMOKE_PASSWORD`` /
    ``JACS_PRIVATE_KEY_PASSWORD`` in the parent process (CI:
    ``smoke-password``), or the conftest's
    ``"test-private-key-password"`` for local in-process bootstrap.
    """
    _restore_smoke_password(monkeypatch)
    # Force remote routing for THIS test only. Without this, the FFI's
    # `build_document_provider` falls through to ``default_storage: "fs"``
    # in jacs.config.json (set by `haiai init`), routes to LocalJacsProvider,
    # and never makes the HTTP call this test was written to verify.
    monkeypatch.setenv("JACS_DEFAULT_STORAGE", "remote")

    server = HTTPServer(("127.0.0.1", 0), _RecordsHandler)
    server.captured = []  # type: ignore[attr-defined]
    port = server.server_port
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    try:
        with tempfile.TemporaryDirectory() as workdir:
            config_path = _bootstrap_jacs_agent(workdir)
            ffi_config = json.dumps(
                    {
                        "base_url": f"http://127.0.0.1:{port}",
                        "jacs_config_path": config_path,
                        "jacs_storage_backend": "remote",
                        "client_type": "python",
                        "timeout_secs": 5,
                    "max_retries": 0,
                }
            )

            client = haiipy.HaiClient(ffi_config)
            key = client.save_memory_sync("smoke-content")
            assert key == "smoke:v1"

            captured = server.captured  # type: ignore[attr-defined]
            # The FFI does at least: 1 GET (find_document singleton check)
            # + 1 POST (sign+store). Assert the POST is what we expect; the
            # GET count can vary with future routing tweaks.
            posts = [r for r in captured if r["method"] == "POST"]
            assert len(posts) == 1, (
                f"expected exactly 1 POST to /api/v1/records, saw {len(posts)} "
                f"in captured={[r['method'] + ' ' + r['path'] for r in captured]}"
            )
            req = posts[0]
            assert req["path"] == "/api/v1/records"
            # HTTP headers are case-insensitive; BaseHTTPRequestHandler
            # normalizes to lowercase. Look up in a case-insensitive way.
            ct = ""
            for hk, hv in req["headers"].items():
                if hk.lower() == "content-type":
                    ct = hv
                    break
            # save_memory POSTs the SIGNED MARKDOWN body (plaintext + JACS
            # footer), not a JSON envelope — see jacs.rs::save_memory which
            # passes ``content_type: "text/markdown; profile=jacs-text-v1"``.
            assert "text/markdown" in ct, (
                f"expected text/markdown content-type for signed-text POST, got {ct!r}"
            )
            # Body shape: original plaintext + JACS signature footer block.
            # The footer is YAML, not JSON, so search for the canonical
            # marker line + the original plaintext rather than for a JSON
            # `"jacsType":"memory"` substring (which would never appear).
            assert b"smoke-content" in req["body"], (
                "expected the original plaintext to be present in the signed-markdown body"
            )
            assert b"-----BEGIN JACS SIGNATURE-----" in req["body"], (
                "expected the JACS inline-text footer marker in the POST body — the "
                "signed bytes must include the signature block per the inline-text "
                "signing format"
            )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def test_save_memory_local_path_through_native_binding(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """End-to-end smoke (LOCAL backend): haiipy.HaiClient.save_memory_sync
    signs and writes to disk without any HTTP traffic.

    Mirrors the dev default — ``haiai init`` writes
    ``default_storage: "fs"`` to ``jacs.config.json``, so most local users
    never make a network call when calling `save_memory`. We force ``fs``
    here even though the pre-baked smoke agent already defaults to it,
    so this test is hermetic against future changes to the bootstrap
    defaults or to ``JACS_DEFAULT_STORAGE`` leaking in from a parent shell.

    Bootstraps a brand-new JACS agent in a per-test ``data_directory``
    rather than reusing the smoke-tests lane's pre-baked agent. The local
    path's ``find_document(jacs_type="memory", singleton)`` would otherwise
    pick up state written by an earlier test (or by JACS init internals)
    in the shared smoke-agent dir, sending ``save_memory`` down the
    update branch and tripping ``sign_text_update``.

    Asserts the returned key has the local ``{jacsId}:{jacsVersion}`` UUID
    shape (not a server-issued string), and that the just-stored document
    round-trips back via `get_record_bytes(key)`.
    """
    _restore_smoke_password(monkeypatch)
    monkeypatch.setenv("JACS_DEFAULT_STORAGE", "fs")

    with tempfile.TemporaryDirectory() as workdir:
        config_path = _bootstrap_fresh_jacs_agent(workdir)

        # No mock HTTP server: the local path must not make any network
        # calls, and binding the FFI to an unreachable URL surfaces that
        # invariant if the routing decision ever regresses.
        ffi_config = json.dumps(
                {
                    "base_url": "http://127.0.0.1:1",  # unreachable on purpose
                    "jacs_config_path": config_path,
                    "jacs_storage_backend": "fs",
                    "client_type": "python",
                    "timeout_secs": 5,
                "max_retries": 0,
            }
        )

        client = haiipy.HaiClient(ffi_config)
        key = client.save_memory_sync("local-smoke-content")

        assert _LOCAL_KEY_PATTERN.match(key), (
            f"expected local key to match `{{jacsId}}:{{jacsVersion}}` UUID "
            f"shape, got {key!r}"
        )

        # Round-trip: fetch the just-stored document by key. The FFI returns
        # the raw bytes of the signed text artifact. The original plaintext
        # we passed in must be present in those bytes.
        record_bytes = client.get_record_bytes_sync(key)
        assert isinstance(record_bytes, (bytes, bytearray))
        assert b"local-smoke-content" in bytes(record_bytes), (
            "expected the stored signed-text artifact to contain the "
            "original plaintext we just saved"
        )
