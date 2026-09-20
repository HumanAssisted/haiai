"""Existing-identity registration through public facades and real FFI serializers."""

import asyncio
import json
from pathlib import Path

import pytest

from haiai import client as client_mod
from haiai._ffi_adapter import AsyncFFIAdapter, FFIAdapter
from haiai.async_client import AsyncHaiClient
from haiai.client import HaiClient


CONTRACT = json.loads(
    (Path(__file__).resolve().parents[2] / "fixtures/init_contract.json").read_text()
)["existing_identity_register"]


@pytest.mark.parametrize(
    "entrypoint,case",
    [
        pytest.param(entrypoint, case, id=f"{entrypoint}-{case['name']}")
        for entrypoint in ["sync", "async", "module"]
        for case in CONTRACT["cases"]
        # Module-level registration always loads a PEM; omission is only valid
        # when the caller supplies agent_json through the sync/async methods.
        if entrypoint != "module" or "public_key_pem" in case["request"]
    ],
)
@pytest.mark.parametrize("preview", [False, True], ids=["live_payload", "preview"])
def test_existing_identity_registration(
    case, entrypoint, preview, loaded_config, monkeypatch, capsys, caplog
):
    request = case["request"]
    captured = []

    class NativeRegistration:
        # Only existing-identity registration is available: bootstrap calls fail.
        def register_sync(self, options_json):
            captured.append(json.loads(options_json))
            return json.dumps(CONTRACT["response"])

        async def register(self, options_json):
            return self.register_sync(options_json)

    adapter_cls = AsyncFFIAdapter if entrypoint == "async" else FFIAdapter
    adapter = object.__new__(adapter_cls)
    adapter._native = NativeRegistration()
    client = AsyncHaiClient() if entrypoint == "async" else HaiClient()
    client._ffi = adapter
    kwargs = {"owner_email": request["owner_email"], "preview": preview}
    if "registration_key" in request:
        kwargs["registration_key"] = request["registration_key"]

    if entrypoint == "module":
        monkeypatch.setattr(client_mod, "_get_client", lambda: client)
        monkeypatch.setattr(
            client_mod, "_read_public_key_pem", lambda cfg: request["public_key_pem"]
        )
        monkeypatch.setattr(
            client_mod,
            "create_agent_document",
            lambda **kwargs: json.loads(request["agent_json"]),
        )
        result = client_mod.register("https://hai.example", **kwargs)
    else:
        kwargs["agent_json"] = request["agent_json"]
        if "public_key_pem" in request:
            kwargs["public_key"] = request["public_key_pem"]
        result = client.register("https://hai.example", **kwargs)
        if entrypoint == "async":
            result = asyncio.run(result)

    if preview:
        assert captured == []
        expected = dict(request)
        if "registration_key" in expected:
            expected["registration_key"] = "***"
        assert json.loads(result.payload_json) == expected
        print(result)
    else:
        assert captured == [request]
        assert result.agent_id == CONTRACT["response"]["agent_id"]

    output = capsys.readouterr()
    if request.get("registration_key"):
        if request["registration_key"] in output.out + output.err + caplog.text:
            pytest.fail("registration key was disclosed in output")
