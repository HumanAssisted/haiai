#!/usr/bin/env python3
"""A2A quickstart using HAISDK facade APIs.

Demonstrates:
1. Initialize JACS + HAI clients
2. Export an A2A agent card via `haisdk.a2a`
3. Sign and verify task artifacts
4. Build chain-of-custody output
5. Generate .well-known discovery documents

Prerequisites:
    pip install haisdk jacs

Usage:
    python python/examples/a2a_quickstart.py
"""

from __future__ import annotations

import base64
import json
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from jacs.client import JacsClient
from jacs.hai.config import get_private_key

from haisdk import HaiClient
from haisdk.a2a import (
    create_chain_of_custody,
    generate_well_known_documents,
    register_with_agent_card,
    sign_artifact,
    verify_artifact,
)

HAI_URL = "https://hai.ai"
TRUST_POLICY = "verified"


def _public_key_b64() -> str:
    private_key = get_private_key()
    public_pem = private_key.public_key().public_bytes(
        Encoding.PEM,
        PublicFormat.SubjectPublicKeyInfo,
    )
    return base64.urlsafe_b64encode(public_pem).decode("utf-8").rstrip("=")


def main() -> None:
    print("=== Step 1: Initialize JACS + HAI clients ===")
    jacs = JacsClient.quickstart(
        name="hai-agent",
        domain="agent.example.com",
        description="HAISDK agent",
        algorithm="pq2025",
    )
    hai = HaiClient()
    print("\n=== Step 2: Register with embedded A2A agent card metadata ===")
    local_jacs_id = getattr(hai, "_get_jacs_id")()
    agent_data = {
        "jacsId": local_jacs_id,
        "jacsName": "a2a-demo-agent",
        "jacsVersion": "1.0.0",
        "jacsAgentDomain": "demo.example.com",
        "a2aProfile": "1.0",
        "jacsServices": [
            {
                "name": "conflict_mediation",
                "serviceDescription": "Mediate disputes with signed provenance artifacts.",
            },
        ],
    }
    registered = register_with_agent_card(
        hai,
        jacs,
        HAI_URL,
        agent_data,
        owner_email="you@example.com",
        agent_json={
            "jacsId": local_jacs_id,
            "name": "a2a-demo-agent",
        },
        trust_policy=TRUST_POLICY,
    )
    registration = registered["registration"]
    jacs_id = getattr(registration, "agent_id", "") or local_jacs_id
    agent_card = registered["agent_card"]
    print(f"Agent registered with ID: {jacs_id}")
    print(json.dumps(agent_card, indent=2))

    print("\n=== Step 3: Sign and verify task artifact ===")
    task_artifact = {
        "taskId": "task-001",
        "operation": "mediate_conflict",
        "input": {
            "parties": ["Alice", "Bob"],
            "topic": "Resource allocation disagreement",
        },
    }
    wrapped_task = sign_artifact(jacs, task_artifact, "task", trust_policy=TRUST_POLICY)
    verification = verify_artifact(jacs, wrapped_task, trust_policy=TRUST_POLICY)
    print(f"Valid: {verification.get('valid')}")
    print(f"Signer: {verification.get('signerId') or verification.get('signer_id')}")
    print(f"Type: {verification.get('artifactType') or verification.get('artifact_type')}")

    print("\n=== Step 4: Chain of custody ===")
    result_artifact = {
        "taskId": "task-001",
        "result": "Mediation successful -- both parties agreed to a shared schedule.",
    }
    wrapped_result = sign_artifact(
        jacs,
        result_artifact,
        "task-result",
        parent_signatures=[wrapped_task],
        trust_policy=TRUST_POLICY,
    )
    chain = create_chain_of_custody(
        jacs,
        [wrapped_task, wrapped_result],
        trust_policy=TRUST_POLICY,
    )
    print(json.dumps(chain, indent=2))

    print("\n=== Step 5: .well-known document bundle ===")
    well_known = generate_well_known_documents(
        jacs,
        agent_card,
        "",
        _public_key_b64(),
        agent_data,
        trust_policy=TRUST_POLICY,
    )
    for path, doc in well_known.items():
        preview = json.dumps(doc, indent=2)
        print(f"\n{path}:")
        print(preview[:220] + ("..." if len(preview) > 220 else ""))

    print("\nA2A quickstart complete.")


if __name__ == "__main__":
    main()
