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


OUTCOMES = json.loads(
    (Path(__file__).resolve().parents[2] / "fixtures/init_contract.json").read_text()
)["registration_outcomes"]


@pytest.mark.parametrize("case", OUTCOMES["cases"], ids=lambda case: case["name"])
@pytest.mark.parametrize("entrypoint", ["sync", "async", "module", "bootstrap"])
def test_registration_outcomes(
    case, entrypoint, loaded_config, monkeypatch, tmp_path, capsys
):
    calls = []

    class NativeRegistration:
        def register_sync(self, options_json):
            calls.append(json.loads(options_json))
            if case["http_status"] >= 400:
                raise RuntimeError("ApiError: " + case["response"]["message"])
            return json.dumps(case["response"])

        async def register(self, options_json):
            return self.register_sync(options_json)

        def register_new_agent_sync(self, options_json):
            return self.register_sync(options_json)

    adapter_cls = AsyncFFIAdapter if entrypoint == "async" else FFIAdapter
    adapter = object.__new__(adapter_cls)
    adapter._native = NativeRegistration()
    client = AsyncHaiClient() if entrypoint == "async" else HaiClient()
    client._ffi = adapter
    monkeypatch.setattr(client_mod, "_get_client", lambda: client)
    monkeypatch.setattr(client_mod, "_read_public_key_pem", lambda cfg: "fixture PEM")
    monkeypatch.setattr(client_mod, "FFIAdapter", lambda config: adapter)
    monkeypatch.setenv("JACS_PRIVATE_KEY_PASSWORD", "synthetic-registration-password")
    monkeypatch.delenv("JACS_PASSWORD_FILE", raising=False)

    def invoke():
        if entrypoint == "bootstrap":
            return client_mod.register_new_agent(
                OUTCOMES["requested_name"],
                "owner@example.test",
                key_dir=str(tmp_path / "keys"),
                config_path=str(tmp_path / "config.json"),
            )
        if entrypoint == "module":
            return client_mod.register(
                "https://hai.example", owner_email="owner@example.test"
            )
        result = client.register(
            "https://hai.example", agent_json='{"jacsId":"existing"}'
        )
        return asyncio.run(result) if entrypoint == "async" else result

    if case["http_status"] >= 400:
        with pytest.raises(Exception, match="Registration key was not accepted"):
            invoke()
    else:
        result = invoke()
        assert result.agent_id == case["response"]["agent_id"]
        assert result.registration_status == case["expected_status"]
        assert result.email == case["expected_email"]
        if entrypoint != "bootstrap":
            assert result.raw_response == case["response"]
        if entrypoint == "bootstrap":
            output = capsys.readouterr().out
            assert (
                f"Registration status: {case['expected_status'] or 'unknown'}" in output
            )
            assert "requested-agent@hai.ai" not in output
            assert "verification email has been sent" not in output.lower()
            if case["expected_email"]:
                assert f"Assigned email: {case['expected_email']}" in output
            else:
                assert "Assigned email:" not in output
    assert len(calls) == 1
