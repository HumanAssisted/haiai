"""Real-FFI smoke test for haiipy.

Loads the real haiipy native binding (PyO3 cdylib) and round-trips
`save_memory("smoke")` against a local stdlib HTTP server. This is the one
test that would have caught the regression where the FFI surface declared
methods that the native binding never exposed.

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
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

import pytest

haiipy = pytest.importorskip("haiipy", reason="haiipy native binding not built")


pytestmark = pytest.mark.native_smoke


class _RecordsHandler(BaseHTTPRequestHandler):
    """Minimal mock that handles POST /api/v1/records.

    Records the request body and headers on the parent server so the test
    can assert what the FFI sent.
    """

    # Suppress the default stderr access log so test output stays clean.
    def log_message(self, format: str, *args: Any) -> None:  # type: ignore[override]
        del format, args

    def do_POST(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler dispatch name
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""
        # Stash for the test to assert against.
        captured = getattr(self.server, "captured", [])
        captured.append(
            {
                "path": self.path,
                "headers": dict(self.headers.items()),
                "body": body,
            }
        )
        self.server.captured = captured  # type: ignore[attr-defined]

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
    """Create a JACS agent via the haiipy registration path (keygen only).

    Returns the absolute path to `jacs.config.json`. Skips the test when
    the JACS toolchain isn't available (which happens when the bundled
    JACS Python wheel is not on the path).
    """
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


def test_save_memory_round_trips_through_native_binding() -> None:
    """End-to-end smoke: haiipy.HaiClient.save_memory_sync hits a real socket."""
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
            assert len(captured) == 1, f"expected 1 POST, saw {len(captured)}"
            req = captured[0]
            assert req["path"] == "/api/v1/records"
            assert "application/json" in req["headers"].get("Content-Type", "")
            assert b'"jacsType":"memory"' in req["body"]
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
