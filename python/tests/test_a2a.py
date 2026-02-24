"""Tests for `haisdk.a2a` wrapper module."""

from __future__ import annotations

import json
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


def test_register_and_mediated_helpers(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")

    calls: dict[str, Any] = {}

    class FakeIntegration:
        def __init__(self, _client: Any, trust_policy: str = "verified") -> None:
            calls.setdefault("ctor", []).append(trust_policy)

        def export_agent_card(self, agent_data: dict[str, Any]) -> dict[str, Any]:
            calls["export_agent_card"] = agent_data
            return {
                "name": "Demo",
                "supportedInterfaces": [{"protocolVersion": "1.0"}],
                "skills": [{"id": "s1"}],
                "capabilities": {},
                "metadata": {},
            }

        def sign_artifact(
            self,
            artifact: dict[str, Any],
            artifact_type: str,
            parent_signatures: list[dict[str, Any]] | None = None,
        ) -> dict[str, Any]:
            calls.setdefault("sign_artifact", []).append(
                {
                    "artifact": artifact,
                    "artifact_type": artifact_type,
                    "parent_signatures": parent_signatures,
                }
            )
            return {
                "jacsType": f"a2a-{artifact_type}",
                "a2aArtifact": artifact,
                "jacsSignature": {"agentID": "agent-1", "signature": "sig"},
            }

        def verify_wrapped_artifact(
            self,
            _wrapped_artifact: dict[str, Any],
            assess_trust: bool = False,
            trust_policy: str | None = None,
        ) -> dict[str, Any]:
            calls["verify_wrapped_artifact"] = {
                "assess_trust": assess_trust,
                "trust_policy": trust_policy,
            }
            return {"valid": True}

        def assess_remote_agent(
            self,
            _agent_card_json: str,
            policy: str | None = None,
        ) -> dict[str, Any]:
            calls["assess_remote_agent"] = {"policy": policy}
            return {"allowed": True, "reason": "ok"}

    _install_module(monkeypatch, "jacs.a2a", JACSA2AIntegration=FakeIntegration)

    merged = a2a_module.merge_agent_json_with_agent_card(
        {"jacsId": "agent-1"},
        {
            "supportedInterfaces": [{"protocolVersion": "1.0"}],
            "skills": [{"id": "s1"}],
            "capabilities": {},
            "metadata": {},
        },
    )
    merged_obj = json.loads(merged)
    assert merged_obj["metadata"]["a2aProfile"] == "1.0"
    assert merged_obj["metadata"]["a2aSkillsCount"] == 1

    class FakeHaiClient:
        def __init__(self) -> None:
            self.register_calls: list[dict[str, Any]] = []
            self.submit_calls: list[dict[str, Any]] = []
            self.email_calls: list[dict[str, Any]] = []
            self._connect_attempts = 0

        def register(
            self,
            hai_url: str,
            agent_json: str | None = None,
            public_key: str | None = None,
            owner_email: str | None = None,
        ) -> dict[str, Any]:
            self.register_calls.append(
                {
                    "hai_url": hai_url,
                    "agent_json": agent_json,
                    "public_key": public_key,
                    "owner_email": owner_email,
                }
            )
            return {"agent_id": "agent-1"}

        class _Event:
            def __init__(self, event_type: str, data: dict[str, Any]) -> None:
                self.event_type = event_type
                self.data = data

        def connect(self, _hai_url: str, *, transport: str = "sse"):  # noqa: ANN001
            self._connect_attempts += 1
            if self._connect_attempts == 1:
                raise RuntimeError("temporary stream failure")
            assert transport in {"sse", "ws"}
            yield self._Event(
                "benchmark_job",
                {
                    "job_id": "job-1",
                    "remoteAgentCard": {"metadata": {"jacsId": "remote-1"}},
                    "a2aTask": {"jacsType": "a2a-task"},
                },
            )
            yield self._Event("disconnect", {"reason": "done"})

        def submit_response(
            self,
            _hai_url: str,
            *,
            job_id: str,
            message: str,
            metadata: dict[str, Any],
            processing_time_ms: int,
        ) -> None:
            self.submit_calls.append(
                {
                    "job_id": job_id,
                    "message": message,
                    "metadata": metadata,
                    "processing_time_ms": processing_time_ms,
                }
            )

        def send_email(
            self,
            _hai_url: str,
            *,
            to: str,
            subject: str,
            body: str,
            in_reply_to: str | None = None,
        ) -> None:
            self.email_calls.append(
                {
                    "to": to,
                    "subject": subject,
                    "body": body,
                    "in_reply_to": in_reply_to,
                }
            )

    fake_hai = FakeHaiClient()
    reg = a2a_module.register_with_agent_card(
        fake_hai,
        object(),
        "https://hai.example",
        {"jacsId": "agent-1", "jacsName": "Agent One"},
        owner_email="owner@hai.ai",
        agent_json={"jacsId": "agent-1"},
        trust_policy="strict",
    )
    assert reg["registration"]["agent_id"] == "agent-1"
    assert fake_hai.register_calls

    a2a_module.on_mediated_benchmark_job(
        fake_hai,
        object(),
        "https://hai.example",
        lambda _task: {"message": "handled"},
        transport="ws",
        trust_policy="verified",
        verify_inbound_artifact=True,
        enforce_trust_policy=True,
        max_reconnect_attempts=1,
        notify_email="ops@hai.ai",
    )
    assert fake_hai._connect_attempts == 2
    assert len(fake_hai.submit_calls) == 1
    assert fake_hai.submit_calls[0]["metadata"]["a2aTask"]["jacsType"] == "a2a-task"
    assert len(fake_hai.email_calls) == 1
