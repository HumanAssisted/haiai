"""Fixture shape checks for shared A2A contracts."""

from __future__ import annotations

import json
from pathlib import Path

from haisdk import a2a as a2a_module

FIXTURES_DIR = Path(__file__).resolve().parents[2] / "fixtures" / "a2a"


def _load(name: str) -> dict:
    return json.loads((FIXTURES_DIR / name).read_text(encoding="utf-8"))


def test_agent_card_fixtures_load() -> None:
    card_v04 = _load("agent_card.v04.json")
    card_v10 = _load("agent_card.v10.json")

    assert card_v04["name"] == "HAISDK Demo Agent"
    assert card_v04["protocolVersions"] == ["0.4.0"]
    assert card_v10["name"] == "HAISDK Demo Agent"
    assert card_v10["supportedInterfaces"][0]["protocolVersion"] == "1.0"


def test_wrapped_and_trust_fixtures_load() -> None:
    wrapped = _load("wrapped_task.with_parents.json")
    trust_cases = _load("trust_assessment_cases.json")

    assert wrapped["jacsType"] == "a2a-task-result"
    assert len(wrapped["jacsParentSignatures"]) == 1
    assert isinstance(trust_cases["cases"], list)
    assert trust_cases["cases"]


def test_golden_profile_and_chain_fixtures_load() -> None:
    profiles = _load("golden_profile_normalization.json")
    chain = _load("golden_chain_of_custody.json")

    assert isinstance(profiles["cases"], list)
    assert profiles["cases"]
    assert profiles["cases"][0]["expected"]["a2aProfile"] in {"1.0", "0.4.0"}

    assert chain["expected"]["totalArtifacts"] == 2
    assert len(chain["expected"]["entries"]) == 2
    assert chain["expected"]["entries"][0]["artifactType"] == "a2a-task"


def test_golden_profile_normalization_behavior() -> None:
    profiles = _load("golden_profile_normalization.json")
    for case in profiles["cases"]:
        merged = a2a_module.merge_agent_json_with_agent_card(
            case["agentJson"],
            case["card"],
        )
        merged_obj = json.loads(merged)
        assert merged_obj["metadata"]["a2aProfile"] == case["expected"]["a2aProfile"]
