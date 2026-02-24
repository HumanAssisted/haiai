"""A2A integration wrappers for HAISDK.

These helpers expose JACS A2A functionality from the `haisdk` namespace while
keeping implementation ownership in `jacs`.
"""

from __future__ import annotations

import importlib
from typing import Any


def _load_optional(module_name: str, *, feature: str, install_hint: str) -> Any:
    try:
        return importlib.import_module(module_name)
    except ImportError as exc:
        raise ImportError(
            f"{feature} requires optional dependency '{module_name}'. {install_hint}"
        ) from exc


def _get_integration_class() -> type:
    module = _load_optional(
        "jacs.a2a",
        feature="A2A integration",
        install_hint='Install with: pip install "haisdk[a2a]"',
    )
    cls = getattr(module, "JACSA2AIntegration", None)
    if cls is None:
        raise ImportError(
            "jacs.a2a is available but missing JACSA2AIntegration. "
            "Install/upgrade JACS with A2A support: pip install -U 'jacs[a2a]'"
        )
    return cls


def get_a2a_integration(client: Any, trust_policy: str = "verified") -> Any:
    """Create a JACS A2A integration bound to a caller-provided JACS client."""
    cls = _get_integration_class()
    return cls(client, trust_policy=trust_policy)


def quickstart_a2a(
    algorithm: str | None = None,
    config_path: str | None = None,
    url: str | None = None,
) -> Any:
    """Delegate to ``JACSA2AIntegration.quickstart(...)``."""
    cls = _get_integration_class()
    quickstart = getattr(cls, "quickstart", None)
    if not callable(quickstart):
        raise ImportError(
            "jacs.a2a.JACSA2AIntegration.quickstart is missing. "
            "Install/upgrade JACS with A2A support: pip install -U 'jacs[a2a]'"
        )
    return quickstart(algorithm=algorithm, config_path=config_path, url=url)


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


def __getattr__(name: str) -> Any:
    """Lazy passthrough so stable `jacs.a2a` symbols are reachable via `haisdk.a2a`."""
    module = _load_optional(
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
]
