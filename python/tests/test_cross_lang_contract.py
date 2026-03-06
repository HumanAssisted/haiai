from __future__ import annotations

import json
from pathlib import Path

import pytest

from jacs.hai.client import HaiClient
from jacs.hai.signing import canonicalize_json


def _load_fixture() -> dict[str, object]:
    fixture_path = Path(__file__).resolve().parents[2] / "fixtures" / "cross_lang_test.json"
    return json.loads(fixture_path.read_text())


def test_cross_lang_canonical_json_cases() -> None:
    fixture = _load_fixture()
    for case in fixture["canonical_json_cases"]:
        case = case
        assert canonicalize_json(case["input"]) == case["expected"]


def test_cross_lang_auth_header_contract(monkeypatch: pytest.MonkeyPatch) -> None:
    fixture = _load_fixture()
    auth = fixture["auth_header"]
    example = auth["example"]
    seen: dict[str, str] = {}

    class _Config:
        jacs_id = example["jacs_id"]

    class _Agent:
        def sign_string(self, message: str) -> str:
            seen["message"] = message
            return example["stub_signature_base64"]

    monkeypatch.setattr("jacs.hai.client.get_config", lambda: _Config())
    monkeypatch.setattr("jacs.hai.client.get_agent", lambda: _Agent())
    monkeypatch.setattr("time.time", lambda: example["timestamp"])

    header = HaiClient()._build_jacs_auth_header()

    assert auth["scheme"] == "JACS"
    assert auth["parts"] == ["jacs_id", "timestamp", "signature_base64"]
    assert header == example["expected_header"]
    assert seen["message"] == auth["signed_message_template"].replace(
        "{jacs_id}", example["jacs_id"]
    ).replace("{timestamp}", str(example["timestamp"]))
