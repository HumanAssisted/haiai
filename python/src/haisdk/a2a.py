"""A2A integration wrappers for HAISDK.

These helpers expose JACS A2A functionality from the `haisdk` namespace while
keeping implementation ownership in `jacs`.
"""

from __future__ import annotations

import json
from typing import Any

from haisdk._optional import load_optional_module, require_attr


def _get_integration_class() -> type:
    module = load_optional_module(
        "jacs.a2a",
        feature="A2A integration",
        install_hint='Install with: pip install "haisdk[a2a]"',
    )
    return require_attr(
        module,
        "JACSA2AIntegration",
        owner_name="jacs.a2a",
        upgrade_hint="Install/upgrade JACS with A2A support: pip install -U 'jacs[a2a]'",
    )


def get_a2a_integration(client: Any, trust_policy: str = "verified") -> Any:
    """Create a JACS A2A integration bound to a caller-provided JACS client."""
    cls = _get_integration_class()
    return cls(client, trust_policy=trust_policy)


def quickstart_a2a(
    name: str,
    domain: str,
    description: str,
    algorithm: str = "pq2025",
    config_path: str | None = None,
    url: str | None = None,
) -> Any:
    """Delegate to ``JACSA2AIntegration.quickstart(...)``."""
    cls = _get_integration_class()
    quickstart = require_attr(
        cls,
        "quickstart",
        owner_name="jacs.a2a.JACSA2AIntegration",
        upgrade_hint="Install/upgrade JACS with A2A support: pip install -U 'jacs[a2a]'",
    )
    if not callable(quickstart):
        raise ImportError(
            "jacs.a2a.JACSA2AIntegration.quickstart is missing. "
            "Install/upgrade JACS with A2A support: pip install -U 'jacs[a2a]'"
        )
    return quickstart(
        name=name,
        domain=domain,
        description=description,
        algorithm=algorithm,
        config_path=config_path,
        url=url,
    )


def export_agent_card(
    client: Any,
    agent_data: dict[str, Any],
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.export_agent_card(agent_data)


def sign_artifact(
    client: Any,
    artifact: dict[str, Any],
    artifact_type: str,
    parent_signatures: list[dict[str, Any]] | None = None,
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.sign_artifact(artifact, artifact_type, parent_signatures)


def verify_artifact(
    client: Any,
    wrapped_artifact: dict[str, Any],
    assess_trust: bool = False,
    policy: str | None = None,
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.verify_wrapped_artifact(
        wrapped_artifact,
        assess_trust=assess_trust,
        trust_policy=policy,
    )


def create_chain_of_custody(
    client: Any,
    artifacts: list[dict[str, Any]],
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.create_chain_of_custody(artifacts)


def generate_well_known_documents(
    client: Any,
    agent_card: Any,
    jws_signature: str,
    public_key_b64: str,
    agent_data: dict[str, Any],
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.generate_well_known_documents(
        agent_card,
        jws_signature,
        public_key_b64,
        agent_data,
    )


def assess_remote_agent(
    client: Any,
    agent_card_json: str,
    policy: str | None = None,
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.assess_remote_agent(agent_card_json, policy=policy)


def trust_a2a_agent(
    client: Any,
    agent_card_json: str,
    trust_policy: str = "verified",
) -> Any:
    integration = get_a2a_integration(client, trust_policy=trust_policy)
    return integration.trust_a2a_agent(agent_card_json)


def _resolve_card_profile(agent_card: dict[str, Any]) -> str:
    metadata = agent_card.get("metadata")
    if isinstance(metadata, dict):
        profile = metadata.get("a2aProfile")
        if isinstance(profile, str) and profile.strip():
            return profile

    versions = agent_card.get("protocolVersions")
    if isinstance(versions, list) and versions:
        first = versions[0]
        if isinstance(first, str) and first.strip():
            return first

    interfaces = agent_card.get("supportedInterfaces")
    if isinstance(interfaces, list):
        for interface in interfaces:
            if not isinstance(interface, dict):
                continue
            version = interface.get("protocolVersion")
            if isinstance(version, str) and version.strip():
                return version

    return "0.4.0"


def merge_agent_json_with_agent_card(
    agent_json: str | dict[str, Any],
    agent_card: dict[str, Any],
) -> str:
    if isinstance(agent_json, str):
        base = json.loads(agent_json)
    else:
        base = dict(agent_json)

    if not isinstance(base, dict):
        raise ValueError("agent_json must decode to an object")
    if not isinstance(agent_card, dict):
        raise ValueError("agent_card must be an object")

    base["a2aAgentCard"] = agent_card
    if "skills" not in base and isinstance(agent_card.get("skills"), list):
        base["skills"] = agent_card["skills"]
    if "capabilities" not in base and isinstance(agent_card.get("capabilities"), dict):
        base["capabilities"] = agent_card["capabilities"]

    metadata = base.get("metadata")
    if not isinstance(metadata, dict):
        metadata = {}
    metadata["a2aProfile"] = _resolve_card_profile(agent_card)
    metadata["a2aSkillsCount"] = (
        len(agent_card.get("skills", []))
        if isinstance(agent_card.get("skills"), list)
        else 0
    )
    base["metadata"] = metadata
    return json.dumps(base)


def register_with_agent_card(
    hai_client: Any,
    jacs_client: Any,
    hai_url: str,
    agent_data: dict[str, Any],
    *,
    owner_email: str | None = None,
    domain: str | None = None,
    description: str | None = None,
    agent_json: str | dict[str, Any] | None = None,
    public_key: str | None = None,
    trust_policy: str = "verified",
) -> dict[str, Any]:
    card = export_agent_card(jacs_client, agent_data, trust_policy=trust_policy)

    base_agent_json: str | dict[str, Any]
    if agent_json is not None:
        base_agent_json = agent_json
    else:
        base_agent_json = {
            "jacsId": agent_data.get("jacsId"),
            "name": agent_data.get("jacsName"),
            "jacsVersion": agent_data.get("jacsVersion", "1.0.0"),
        }

    merged_agent_json = merge_agent_json_with_agent_card(base_agent_json, card)
    registration = hai_client.register(
        hai_url,
        agent_json=merged_agent_json,
        public_key=public_key,
        owner_email=owner_email,
    )
    return {
        "registration": registration,
        "agent_card": card,
        "agent_json": merged_agent_json,
        "domain": domain,
        "description": description,
    }


def on_mediated_benchmark_job(
    hai_client: Any,
    jacs_client: Any,
    hai_url: str,
    handler: Any,
    *,
    transport: str = "sse",
    trust_policy: str = "verified",
    verify_inbound_artifact: bool = False,
    enforce_trust_policy: bool = False,
    max_reconnect_attempts: int = 0,
    notify_email: str | None = None,
    email_subject: str | None = None,
) -> None:
    attempts = 0
    while True:
        try:
            for event in hai_client.connect(hai_url, transport=transport):
                event_type = getattr(event, "event_type", None)
                if event_type != "benchmark_job":
                    continue

                data = getattr(event, "data", {})
                if not isinstance(data, dict):
                    data = {}

                job_id = (
                    data.get("job_id")
                    or data.get("run_id")
                    or data.get("jobId")
                    or data.get("runId")
                )
                if not isinstance(job_id, str) or not job_id:
                    raise ValueError("benchmark job missing job_id/run_id")

                if enforce_trust_policy:
                    remote_card = (
                        data.get("remoteAgentCard")
                        or data.get("remote_agent_card")
                        or data.get("agentCard")
                    )
                    if remote_card is None:
                        raise ValueError(
                            "remote agent card required when trust enforcement is enabled"
                        )
                    remote_card_json = (
                        remote_card
                        if isinstance(remote_card, str)
                        else json.dumps(remote_card)
                    )
                    trust = assess_remote_agent(
                        jacs_client,
                        remote_card_json,
                        trust_policy=trust_policy,
                    )
                    if not trust.get("allowed", False):
                        raise ValueError(
                            f"trust policy rejected remote agent: {trust.get('reason', 'unknown')}"
                        )

                if verify_inbound_artifact:
                    inbound = data.get("a2aTask") or data.get("a2a_task")
                    if inbound is None:
                        raise ValueError(
                            "inbound a2a task required when signature verification is enabled"
                        )
                    verify = verify_artifact(
                        jacs_client,
                        inbound if isinstance(inbound, dict) else json.loads(str(inbound)),
                        trust_policy=trust_policy,
                    )
                    if not verify.get("valid", False):
                        raise ValueError(
                            "inbound a2a task signature invalid: "
                            + str(verify.get("error", "unknown verification failure"))
                        )

                task_payload = {
                    "type": "benchmark_job",
                    "jobId": job_id,
                    "scenarioId": data.get("scenario_id") or data.get("scenarioId"),
                    "config": data.get("config"),
                }
                task_artifact = sign_artifact(
                    jacs_client,
                    task_payload,
                    "task",
                    trust_policy=trust_policy,
                )
                result_payload = handler(task_artifact)
                if not isinstance(result_payload, dict):
                    raise ValueError("handler must return a dict payload")

                result_artifact = sign_artifact(
                    jacs_client,
                    result_payload,
                    "task-result",
                    parent_signatures=[task_artifact],
                    trust_policy=trust_policy,
                )

                message = result_payload.get("message")
                if not isinstance(message, str):
                    message = json.dumps(result_payload)

                hai_client.submit_response(
                    hai_url,
                    job_id=job_id,
                    message=message,
                    metadata={
                        "a2aTask": task_artifact,
                        "a2aResult": result_artifact,
                    },
                    processing_time_ms=0,
                )

                if notify_email:
                    hai_client.send_email(
                        hai_url,
                        to=notify_email,
                        subject=email_subject or f"A2A mediated result for job {job_id}",
                        body="Signed A2A artifact:\n\n"
                        + json.dumps(result_artifact, indent=2),
                    )
            return
        except Exception:
            if attempts >= max_reconnect_attempts:
                raise
            attempts += 1


def __getattr__(name: str) -> Any:
    """Lazy passthrough so stable `jacs.a2a` symbols are reachable via `haisdk.a2a`."""
    module = load_optional_module(
        "jacs.a2a",
        feature="A2A integration",
        install_hint='Install with: pip install "haisdk[a2a]"',
    )
    try:
        return getattr(module, name)
    except AttributeError as exc:
        raise AttributeError(f"module 'haisdk.a2a' has no attribute {name!r}") from exc


__all__ = [
    "get_a2a_integration",
    "quickstart_a2a",
    "export_agent_card",
    "sign_artifact",
    "verify_artifact",
    "create_chain_of_custody",
    "generate_well_known_documents",
    "assess_remote_agent",
    "trust_a2a_agent",
    "merge_agent_json_with_agent_card",
    "register_with_agent_card",
    "on_mediated_benchmark_job",
]
