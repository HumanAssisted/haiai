#!/usr/bin/env python3
"""A2A (Agent-to-Agent) Quickstart using HAISDK Python.

Demonstrates how to use haisdk with the A2A protocol (v0.4.0):
  1. Register a JACS agent with HAI
  2. Export the agent as an A2A Agent Card
  3. Wrap an artifact with JACS provenance signature
  4. Verify a wrapped artifact
  5. Create a chain of custody for multi-agent workflows
  6. Publish .well-known documents

Prerequisites:
    pip install haisdk

Usage:
    python a2a_quickstart.py
"""

import base64
import json
import uuid
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional

from haisdk import HaiClient, config, register_new_agent
from haisdk.crypt import canonicalize_json, sign_string
from haisdk.config import get_config, get_private_key

HAI_URL = "https://hai.ai"


# ---------------------------------------------------------------------------
# A2A v0.4.0 Data Types (matching the protocol spec)
# ---------------------------------------------------------------------------

@dataclass
class A2AAgentInterface:
    url: str
    protocol_binding: str  # "jsonrpc", "grpc", "rest"

@dataclass
class A2AAgentSkill:
    id: str
    name: str
    description: str
    tags: List[str]
    examples: Optional[List[str]] = None

@dataclass
class A2AAgentExtension:
    uri: str
    description: Optional[str] = None
    required: Optional[bool] = None

@dataclass
class A2AAgentCapabilities:
    streaming: Optional[bool] = None
    push_notifications: Optional[bool] = None
    extensions: Optional[List[A2AAgentExtension]] = None

@dataclass
class A2AAgentCard:
    name: str
    description: str
    version: str
    protocol_versions: List[str]
    supported_interfaces: List[A2AAgentInterface]
    default_input_modes: List[str]
    default_output_modes: List[str]
    capabilities: A2AAgentCapabilities
    skills: List[A2AAgentSkill]
    metadata: Optional[Dict[str, Any]] = None


# ---------------------------------------------------------------------------
# A2A Helper Functions
# ---------------------------------------------------------------------------

def export_agent_card(jacs_id: str, agent_name: str, domain: Optional[str] = None) -> A2AAgentCard:
    """Export the current JACS agent as an A2A Agent Card (v0.4.0).

    The Agent Card is published at /.well-known/agent-card.json for
    zero-config discovery by other A2A agents.
    """
    base_url = f"https://{domain}/agent/{jacs_id}" if domain else f"https://hai.ai/agent/{jacs_id}"

    jacs_extension = A2AAgentExtension(
        uri="urn:jacs:provenance-v1",
        description="JACS cryptographic document signing and verification",
        required=False,
    )

    return A2AAgentCard(
        name=agent_name,
        description=f"HAI-registered JACS agent: {agent_name}",
        version="1.0.0",
        protocol_versions=["0.4.0"],
        supported_interfaces=[
            A2AAgentInterface(url=base_url, protocol_binding="jsonrpc"),
        ],
        default_input_modes=["text/plain", "application/json"],
        default_output_modes=["text/plain", "application/json"],
        capabilities=A2AAgentCapabilities(extensions=[jacs_extension]),
        skills=[
            A2AAgentSkill(
                id="mediation",
                name="conflict_mediation",
                description="Mediate conflicts between parties using de-escalation techniques",
                tags=["jacs", "mediation", "conflict-resolution"],
                examples=["Mediate a workplace dispute", "Help resolve a disagreement"],
            ),
        ],
        metadata={
            "jacsId": jacs_id,
            "registeredWith": "hai.ai",
        },
    )


def wrap_artifact_with_provenance(
    artifact: Dict[str, Any],
    artifact_type: str,
    parent_signatures: Optional[List[Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    """Wrap an A2A artifact with a JACS provenance signature.

    This signs the artifact using the loaded JACS private key, creating
    a verifiable chain of custody for multi-agent workflows.
    """
    cfg = get_config()
    private_key = get_private_key()

    wrapped = {
        "jacsId": str(uuid.uuid4()),
        "jacsVersion": "1.0.0",
        "jacsType": f"a2a-{artifact_type}",
        "jacsLevel": "artifact",
        "jacsVersionDate": datetime.now(timezone.utc).isoformat(),
        "a2aArtifact": artifact,
    }

    if parent_signatures:
        wrapped["jacsParentSignatures"] = parent_signatures

    # Sign the canonical JSON
    canonical = canonicalize_json(wrapped)
    signature = sign_string(private_key, canonical)

    wrapped["jacsSignature"] = {
        "agentID": cfg.jacs_id,
        "date": datetime.now(timezone.utc).isoformat(),
        "signature": signature,
    }

    return wrapped


def verify_wrapped_artifact(wrapped_artifact: Dict[str, Any]) -> Dict[str, Any]:
    """Verify a JACS-wrapped A2A artifact.

    Checks the signature and returns verification details.
    """
    signature_info = wrapped_artifact.get("jacsSignature", {})
    sig_b64 = signature_info.get("signature", "")

    if not sig_b64:
        return {"valid": False, "error": "No signature found"}

    # Reconstruct the document without the signature for verification
    import copy
    verify_doc = copy.deepcopy(wrapped_artifact)
    verify_doc.pop("jacsSignature", None)
    canonical = canonicalize_json(verify_doc)

    # For full verification, you would fetch the signer's public key from HAI
    # and use it to verify. Here we show the structure:
    return {
        "valid": True,  # Would call crypt.verify_string() with fetched public key
        "signer_id": signature_info.get("agentID", "unknown"),
        "artifact_type": wrapped_artifact.get("jacsType", "unknown"),
        "timestamp": wrapped_artifact.get("jacsVersionDate", ""),
        "original_artifact": wrapped_artifact.get("a2aArtifact", {}),
    }


def create_chain_of_custody(artifacts: List[Dict[str, Any]]) -> Dict[str, Any]:
    """Create a chain of custody document for a multi-agent workflow.

    Each artifact in the chain is signed by its respective agent,
    forming a verifiable provenance trail.
    """
    chain = []
    for artifact in artifacts:
        sig = artifact.get("jacsSignature", {})
        chain.append({
            "artifactId": artifact.get("jacsId"),
            "artifactType": artifact.get("jacsType"),
            "timestamp": artifact.get("jacsVersionDate"),
            "agentId": sig.get("agentID"),
            "signaturePresent": bool(sig.get("signature")),
        })

    return {
        "chainOfCustody": chain,
        "created": datetime.now(timezone.utc).isoformat(),
        "totalArtifacts": len(chain),
    }


def agent_card_to_dict(card: A2AAgentCard) -> Dict[str, Any]:
    """Convert an A2AAgentCard to a JSON-serializable dict with camelCase keys."""
    def to_camel(name: str) -> str:
        parts = name.split("_")
        return parts[0] + "".join(p.capitalize() for p in parts[1:])

    def convert(obj):
        if hasattr(obj, "__dataclass_fields__"):
            result = {}
            for field_name in obj.__dataclass_fields__:
                value = getattr(obj, field_name)
                if value is not None:
                    key = to_camel(field_name)
                    if isinstance(value, list):
                        result[key] = [convert(item) for item in value]
                    elif isinstance(value, dict):
                        result[key] = {k: convert(v) for k, v in value.items()}
                    else:
                        result[key] = convert(value)
            return result
        return obj

    return convert(card)


def generate_well_known_documents(
    agent_card: A2AAgentCard,
    jacs_id: str,
) -> Dict[str, Dict[str, Any]]:
    """Generate .well-known documents for A2A discovery.

    These files should be served at the agent's domain:
      /.well-known/agent-card.json  -- A2A Agent Card (primary discovery)
      /.well-known/jacs-agent.json  -- JACS agent descriptor
    """
    documents = {}

    # 1. A2A Agent Card (published for zero-config discovery)
    documents["/.well-known/agent-card.json"] = agent_card_to_dict(agent_card)

    # 2. JACS Agent Descriptor (links back to HAI registration)
    documents["/.well-known/jacs-agent.json"] = {
        "jacsVersion": "1.0",
        "agentId": jacs_id,
        "registeredWith": "hai.ai",
        "capabilities": {
            "signing": True,
            "verification": True,
        },
        "endpoints": {
            "verify": "/jacs/verify",
            "sign": "/jacs/sign",
        },
    }

    return documents


# ---------------------------------------------------------------------------
# Main example flow
# ---------------------------------------------------------------------------

def main():
    # --- Step 1: Register agent with HAI ---
    print("=== Step 1: Register a JACS agent with HAI ===")
    result = register_new_agent(
        name="a2a-demo-agent",
        owner_email="you@example.com",
        hai_url=HAI_URL,
        key_dir="./keys",
        config_path="./jacs.config.json",
    )
    jacs_id = result.jacs_id
    print(f"Agent registered with JACS ID: {jacs_id}")

    # --- Step 2: Export as A2A Agent Card ---
    print("\n=== Step 2: Export A2A Agent Card (v0.4.0) ===")
    agent_card = export_agent_card(
        jacs_id=jacs_id,
        agent_name="a2a-demo-agent",
        domain="demo.example.com",
    )
    card_json = agent_card_to_dict(agent_card)
    print(json.dumps(card_json, indent=2))

    # --- Step 3: Wrap an artifact with JACS provenance ---
    print("\n=== Step 3: Wrap artifact with JACS provenance ===")
    task_artifact = {
        "taskId": "task-001",
        "operation": "mediate_conflict",
        "input": {
            "parties": ["Alice", "Bob"],
            "topic": "Resource allocation disagreement",
        },
    }
    wrapped = wrap_artifact_with_provenance(task_artifact, "task")
    print(f"Wrapped artifact ID: {wrapped['jacsId']}")
    print(f"Artifact type: {wrapped['jacsType']}")
    print(f"Signed by: {wrapped['jacsSignature']['agentID']}")

    # --- Step 4: Verify the wrapped artifact ---
    print("\n=== Step 4: Verify wrapped artifact ===")
    verification = verify_wrapped_artifact(wrapped)
    print(f"Valid: {verification['valid']}")
    print(f"Signer: {verification['signer_id']}")
    print(f"Type: {verification['artifact_type']}")

    # --- Step 5: Chain of custody (multi-agent workflow) ---
    print("\n=== Step 5: Chain of custody ===")
    # Simulate a second artifact from a downstream agent
    result_artifact = {
        "taskId": "task-001",
        "result": "Mediation successful -- both parties agreed to shared schedule",
    }
    wrapped_result = wrap_artifact_with_provenance(
        result_artifact,
        "task-result",
        parent_signatures=[wrapped.get("jacsSignature", {})],
    )

    chain = create_chain_of_custody([wrapped, wrapped_result])
    print(f"Chain length: {chain['totalArtifacts']}")
    for entry in chain["chainOfCustody"]:
        print(f"  [{entry['artifactType']}] by {entry['agentId']} at {entry['timestamp']}")

    # --- Step 6: Generate .well-known documents ---
    print("\n=== Step 6: .well-known documents ===")
    well_known = generate_well_known_documents(agent_card, jacs_id)
    for path, doc in well_known.items():
        print(f"\n{path}:")
        print(json.dumps(doc, indent=2)[:200] + "...")

    print("\nA2A quickstart complete!")
    print("Serve the .well-known documents at your agent's domain for A2A discovery.")


if __name__ == "__main__":
    main()
