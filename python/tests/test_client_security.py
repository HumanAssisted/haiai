"""Regression tests for security/correctness-sensitive client behavior."""

from __future__ import annotations

import base64
import json
import os
import stat
from pathlib import Path

import httpx
import pytest

from jacs.hai.client import HaiClient, register_new_agent


class _FakeResponse:
    def __init__(self, status_code: int, payload: dict, text: str = "") -> None:
        self.status_code = status_code
        self._payload = payload
        self.text = text

    def json(self) -> dict:
        return self._payload

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise httpx.HTTPStatusError(
                "error",
                request=httpx.Request("POST", "https://hai.ai"),
                response=httpx.Response(self.status_code, text=self.text),
            )


def test_verify_hai_message_supports_key_id_lookup(
    ed25519_keypair: tuple,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    private_key, _ = ed25519_keypair
    message = '{"hello":"world"}'
    signature = base64.b64encode(private_key.sign(message.encode("utf-8"))).decode("ascii")

    from jacs.hai import signing as signing_mod

    monkeypatch.setattr(
        signing_mod,
        "fetch_server_keys",
        lambda _hai_url: [
            signing_mod._CachedKey(  # noqa: SLF001 - test-only internal fixture
                key_id="fingerprint-123",
                algorithm="ed25519",
                public_key=private_key.public_key(),
                public_key_pem="",
            )
        ],
    )

    client = HaiClient()
    assert client.verify_hai_message(
        message=message,
        signature=signature,
        hai_public_key="fingerprint-123",
        hai_url="https://hai.ai",
    )
    assert not client.verify_hai_message(
        message=message,
        signature=signature,
        hai_public_key="fingerprint-123",
        hai_url=None,
    )


def test_hello_world_passes_hai_url_to_verifier(
    loaded_config: None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, str] = {}

    def fake_verify(
        self: HaiClient,
        message: str,
        signature: str,
        hai_public_key: str = "",
        hai_url: str | None = None,
    ) -> bool:
        captured["hai_url"] = hai_url or ""
        return True

    monkeypatch.setattr(HaiClient, "verify_hai_message", fake_verify)

    monkeypatch.setattr(
        httpx,
        "post",
        lambda *_args, **_kwargs: _FakeResponse(
            status_code=200,
            payload={
                "timestamp": "2026-01-01T00:00:00Z",
                "client_ip": "127.0.0.1",
                "hai_public_key_fingerprint": "fingerprint-123",
                "hai_signed_ack": "abc",
                "message": "ok",
                "hello_id": "h1",
            },
        ),
    )

    result = HaiClient().hello_world("https://hai.ai")
    assert result.success
    assert captured["hai_url"] == "https://hai.ai"


def test_register_new_agent_writes_private_key_with_0600(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    if os.name == "nt":
        pytest.skip("POSIX permission bits are not reliable on Windows")

    key_dir = tmp_path / "keys"
    config_path = tmp_path / "jacs.config.json"

    monkeypatch.setattr(
        httpx,
        "post",
        lambda *_args, **_kwargs: _FakeResponse(
            status_code=201,
            payload={
                "agent_id": "agent-123",
                "jacs_id": "jacs-123",
            },
        ),
    )

    try:
        register_new_agent(
            name="Agent",
            owner_email="owner@hai.ai",
            hai_url="https://hai.ai",
            key_dir=str(key_dir),
            config_path=str(config_path),
            quiet=True,
        )
    finally:
        from jacs.hai.config import reset

        reset()

    private_key_path = key_dir / "agent_private_key.pem"
    mode = stat.S_IMODE(private_key_path.stat().st_mode)
    assert mode == 0o600


def test_register_new_agent_defaults_to_secure_key_dir(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_payload: dict[str, object] = {}
    monkeypatch.setenv("HOME", str(tmp_path))

    def fake_post(*_args, **kwargs):
        captured_payload.update(kwargs.get("json", {}))
        return _FakeResponse(
            status_code=201,
            payload={
                "agent_id": "agent-123",
                "jacs_id": "jacs-123",
            },
        )

    monkeypatch.setattr(httpx, "post", fake_post)
    config_path = tmp_path / "jacs.config.json"

    try:
        register_new_agent(
            name="Agent",
            owner_email="owner@hai.ai",
            hai_url="https://hai.ai",
            config_path=str(config_path),
            domain="agent.example",
            description="Agent description",
            quiet=True,
        )
    finally:
        from jacs.hai.config import reset

        reset()

    expected_key_dir = (tmp_path / ".jacs" / "keys").resolve()
    assert (expected_key_dir / "agent_private_key.pem").is_file()
    assert (expected_key_dir / "agent_public_key.pem").is_file()
    if os.name != "nt":
        assert stat.S_IMODE(expected_key_dir.stat().st_mode) == 0o700

    cfg = json.loads(config_path.read_text())
    assert Path(cfg["jacsKeyDir"]) == expected_key_dir
    assert captured_payload.get("domain") == "agent.example"
    doc = json.loads(str(captured_payload["agent_json"]))
    assert doc["description"] == "Agent description"
    assert doc["domain"] == "agent.example"
