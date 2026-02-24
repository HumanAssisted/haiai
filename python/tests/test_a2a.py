"""Tests for `haisdk.a2a` wrapper module."""

from __future__ import annotations

import sys
import types
from typing import Any

import pytest

from haisdk import a2a as a2a_module


def _install_module(monkeypatch: pytest.MonkeyPatch, module_name: str, **attrs: Any) -> types.ModuleType:
    module = types.ModuleType(module_name)
    for key, value in attrs.items():
        setattr(module, key, value)
    monkeypatch.setitem(sys.modules, module_name, module)
    return module


def _install_package(monkeypatch: pytest.MonkeyPatch, package_name: str) -> types.ModuleType:
    module = types.ModuleType(package_name)
    module.__path__ = []  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, package_name, module)
    return module


def test_missing_dependency_error_has_install_hint(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delitem(sys.modules, "jacs.a2a", raising=False)
    monkeypatch.delitem(sys.modules, "jacs", raising=False)

    with pytest.raises(ImportError, match=r"haisdk\[a2a\]"):
        a2a_module.get_a2a_integration(client=object())


def test_get_a2a_integration_and_wrappers_delegate(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")

    calls: dict[str, Any] = {}

    class FakeIntegration:
        def __init__(self, client: Any, trust_policy: str = "verified") -> None:
            calls["ctor"] = {"client": client, "trust_policy": trust_policy}

        @classmethod
        def quickstart(
            cls,
            algorithm: str | None = None,
            config_path: str | None = None,
            url: str | None = None,
        ) -> dict[str, Any]:
            calls["quickstart"] = {
                "algorithm": algorithm,
                "config_path": config_path,
                "url": url,
            }
            return {"quickstart": True}

        def export_agent_card(self, agent_data: dict[str, Any]) -> dict[str, Any]:
            calls["export_agent_card"] = agent_data
            return {"op": "export_agent_card"}

        def sign_artifact(
            self,
            artifact: dict[str, Any],
            artifact_type: str,
            parent_signatures: list[dict[str, Any]] | None = None,
        ) -> dict[str, Any]:
            calls["sign_artifact"] = {
                "artifact": artifact,
                "artifact_type": artifact_type,
                "parent_signatures": parent_signatures,
            }
            return {"op": "sign_artifact"}

        def verify_wrapped_artifact(
            self,
            wrapped_artifact: dict[str, Any],
            assess_trust: bool = False,
            trust_policy: str | None = None,
        ) -> dict[str, Any]:
            calls["verify_wrapped_artifact"] = {
                "wrapped_artifact": wrapped_artifact,
                "assess_trust": assess_trust,
                "trust_policy": trust_policy,
            }
            return {"op": "verify_wrapped_artifact"}

        def create_chain_of_custody(self, artifacts: list[dict[str, Any]]) -> dict[str, Any]:
            calls["create_chain_of_custody"] = artifacts
            return {"op": "create_chain_of_custody"}

        def generate_well_known_documents(
            self,
            agent_card: dict[str, Any],
            jws_signature: str,
            public_key_b64: str,
            agent_data: dict[str, Any],
        ) -> dict[str, Any]:
            calls["generate_well_known_documents"] = {
                "agent_card": agent_card,
                "jws_signature": jws_signature,
                "public_key_b64": public_key_b64,
                "agent_data": agent_data,
            }
            return {"op": "generate_well_known_documents"}

        def assess_remote_agent(
            self,
            agent_card_json: str,
            policy: str | None = None,
        ) -> dict[str, Any]:
            calls["assess_remote_agent"] = {
                "agent_card_json": agent_card_json,
                "policy": policy,
            }
            return {"op": "assess_remote_agent"}

        def trust_a2a_agent(self, agent_card_json: str) -> str:
            calls["trust_a2a_agent"] = agent_card_json
            return "trusted-a2a-agent"

    _install_module(monkeypatch, "jacs.a2a", JACSA2AIntegration=FakeIntegration, MAGIC="ok")

    fake_client = object()
    integration = a2a_module.get_a2a_integration(fake_client, trust_policy="strict")
    assert isinstance(integration, FakeIntegration)
    assert calls["ctor"]["client"] is fake_client
    assert calls["ctor"]["trust_policy"] == "strict"

    assert a2a_module.quickstart_a2a(algorithm="pq2025", config_path="cfg.json", url="https://a2a.example") == {
        "quickstart": True,
    }
    assert calls["quickstart"] == {
        "algorithm": "pq2025",
        "config_path": "cfg.json",
        "url": "https://a2a.example",
    }

    assert a2a_module.export_agent_card(fake_client, {"jacsId": "agent-1"}) == {
        "op": "export_agent_card",
    }
    assert calls["export_agent_card"] == {"jacsId": "agent-1"}

    assert a2a_module.sign_artifact(fake_client, {"taskId": "t-1"}, "task", [{"p": 1}]) == {
        "op": "sign_artifact",
    }
    assert calls["sign_artifact"] == {
        "artifact": {"taskId": "t-1"},
        "artifact_type": "task",
        "parent_signatures": [{"p": 1}],
    }

    assert a2a_module.verify_artifact(
        fake_client,
        {"wrapped": True},
        assess_trust=True,
        policy="strict",
    ) == {"op": "verify_wrapped_artifact"}
    assert calls["verify_wrapped_artifact"] == {
        "wrapped_artifact": {"wrapped": True},
        "assess_trust": True,
        "trust_policy": "strict",
    }

    assert a2a_module.create_chain_of_custody(fake_client, [{"step": 1}, {"step": 2}]) == {
        "op": "create_chain_of_custody",
    }
    assert calls["create_chain_of_custody"] == [{"step": 1}, {"step": 2}]

    assert a2a_module.generate_well_known_documents(
        fake_client,
        {"name": "Agent"},
        "jws",
        "pubkey-b64",
        {"jacsId": "agent-1"},
    ) == {"op": "generate_well_known_documents"}
    assert calls["generate_well_known_documents"] == {
        "agent_card": {"name": "Agent"},
        "jws_signature": "jws",
        "public_key_b64": "pubkey-b64",
        "agent_data": {"jacsId": "agent-1"},
    }

    assert a2a_module.assess_remote_agent(fake_client, '{"card":true}', policy="verified") == {
        "op": "assess_remote_agent",
    }
    assert calls["assess_remote_agent"] == {
        "agent_card_json": '{"card":true}',
        "policy": "verified",
    }

    assert a2a_module.trust_a2a_agent(fake_client, '{"card":true}') == "trusted-a2a-agent"
    assert calls["trust_a2a_agent"] == '{"card":true}'


def test_module_getattr_passthrough(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")
    _install_module(monkeypatch, "jacs.a2a", JACSA2AIntegration=object, EXPORTED_SYMBOL=123)

    assert a2a_module.EXPORTED_SYMBOL == 123
