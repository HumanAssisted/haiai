"""Fixture shape checks for shared A2A contracts."""

from __future__ import annotations

import json
from pathlib import Path


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
