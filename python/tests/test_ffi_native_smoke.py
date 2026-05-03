"""Real-FFI smoke tests for haiipy.

Two tests, one per backend, both loading the real haiipy native binding
(PyO3 cdylib) and exercising `save_memory("...")` end-to-end:

1. **Remote** (`test_save_memory_round_trips_through_native_binding`) —
   hosted production path. Sets ``JACS_DEFAULT_STORAGE=remote`` so the FFI
   builds a `RemoteJacsProvider`, signs locally, POSTs to a stdlib
   `http.server.HTTPServer` mock, and reads the server-issued key from the
   response. Verifies the mock saw exactly one `POST /api/v1/records` with
   `application/json` and a `"jacsType":"memory"` body.

2. **Local** (`test_save_memory_local_path_through_native_binding`) — dev
   default path (`haiai init` writes ``default_storage: "fs"``). Sets
   ``JACS_DEFAULT_STORAGE=fs`` (or leaves it unset on a fs-config) so the
   FFI builds a `LocalJacsProvider`, signs locally, writes to disk, and
   returns a client-side ``{jacsId}:{jacsVersion}`` key. Verifies the doc
   round-trips via `get_record_bytes(key)`.

Together these two tests cover the only two backends production and dev
users actually exercise — and would have caught a regression in either
the remote routing path OR the local FS routing path.

Skipped cleanly when:
- haiipy is not built / installable (`importorskip`).
- The CI environment lacks the JACS toolchain to bootstrap a test agent.

Marker: `@pytest.mark.native_smoke`. Run with:

    pytest -m native_smoke python/tests/

Per PRD docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: stdlib
`http.server.HTTPServer` (no `respx`/`httpx`-level mock). The traffic is
Rust `reqwest` running INSIDE the haiipy native binding, which only a real
listening socket can intercept.
"""

from __future__ import annotations

import json
import os
import re
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
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
            assert "application/json" in ct, f"expected application/json in {ct!r}"
            assert b'"jacsType":"memory"' in req["body"]
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

    Asserts the returned key has the local ``{jacsId}:{jacsVersion}`` UUID
    shape (not a server-issued string), and that the just-stored document
    round-trips back via `get_record_bytes(key)`.
    """
    _restore_smoke_password(monkeypatch)
    monkeypatch.setenv("JACS_DEFAULT_STORAGE", "fs")

    with tempfile.TemporaryDirectory() as workdir:
        config_path = _bootstrap_jacs_agent(workdir)

        # No mock HTTP server: the local path must not make any network
        # calls, and binding the FFI to an unreachable URL surfaces that
        # invariant if the routing decision ever regresses.
        ffi_config = json.dumps(
            {
                "base_url": "http://127.0.0.1:1",  # unreachable on purpose
                "jacs_config_path": config_path,
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
