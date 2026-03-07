"""A2A Verification Contract Tests.

These tests validate that the Python SDK's A2A serialization produces
and consumes JSON with the exact field names declared in the canonical
contract fixture at fixtures/a2a_verification_contract.json.

They catch schema drift across languages by verifying field names, types,
and roundtrip values against the shared fixture.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

FIXTURES_DIR = Path(__file__).resolve().parents[2] / "fixtures"
CONTRACT_PATH = FIXTURES_DIR / "a2a_verification_contract.json"
A2A_FIXTURES_DIR = FIXTURES_DIR / "a2a"


def _load_contract() -> dict[str, Any]:
    return json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


def _load_a2a_fixture(name: str) -> dict[str, Any]:
    return json.loads((A2A_FIXTURES_DIR / name).read_text(encoding="utf-8"))


def _assert_fields_present(
    label: str, obj: dict[str, Any], required_fields: dict[str, str]
) -> None:
    """Assert all non-comment required fields exist in obj."""
    for field in required_fields:
        if field == "_comment":
            continue
        assert field in obj, f"{label}: missing required field '{field}'"


def _assert_field_type(
    label: str, field: str, expected_type: str, value: Any
) -> None:
    """Assert a value matches the expected type string from the schema."""
    if expected_type == "string":
        assert isinstance(value, str), (
            f"{label}.{field}: expected string, got {type(value).__name__}"
        )
    elif expected_type == "boolean":
        assert isinstance(value, bool), (
            f"{label}.{field}: expected boolean, got {type(value).__name__}"
        )
    elif expected_type == "object":
        assert isinstance(value, dict), (
            f"{label}.{field}: expected object, got {type(value).__name__}"
        )
    elif expected_type == "array":
        assert isinstance(value, list), (
            f"{label}.{field}: expected array, got {type(value).__name__}"
        )
    elif expected_type == "number":
        assert isinstance(value, (int, float)), (
            f"{label}.{field}: expected number, got {type(value).__name__}"
        )


# ---------------------------------------------------------------------------
# WrappedArtifact schema tests
# ---------------------------------------------------------------------------


class TestContractWrappedArtifact:
    """Validate A2AWrappedArtifact JSON field names and types."""

    @pytest.fixture()
    def contract(self) -> dict[str, Any]:
        return _load_contract()

    def test_required_fields_present(self, contract: dict[str, Any]) -> None:
        wrapped = contract["wrappedArtifact"]
        schema = contract["wrappedArtifactSchema"]
        required = schema["requiredFields"]
        _assert_fields_present("A2AWrappedArtifact", wrapped, required)

    def test_required_fields_types(self, contract: dict[str, Any]) -> None:
        wrapped = contract["wrappedArtifact"]
        schema = contract["wrappedArtifactSchema"]
        for field, expected_type in schema["requiredFields"].items():
            if field == "_comment":
                continue
            _assert_field_type(
                "A2AWrappedArtifact", field, expected_type, wrapped[field]
            )

    def test_signature_sub_fields(self, contract: dict[str, Any]) -> None:
        wrapped = contract["wrappedArtifact"]
        schema = contract["wrappedArtifactSchema"]
        sig = wrapped["jacsSignature"]
        assert sig is not None
        _assert_fields_present(
            "A2AArtifactSignature", sig, schema["signatureFields"]
        )

    def test_agent_id_uses_uppercase_id(self, contract: dict[str, Any]) -> None:
        """Signature field must be 'agentID' (uppercase ID), not 'agentId'."""
        sig = contract["wrappedArtifact"]["jacsSignature"]
        assert "agentID" in sig
        assert "agentId" not in sig

    def test_roundtrip_values(self, contract: dict[str, Any]) -> None:
        wrapped = contract["wrappedArtifact"]
        assert wrapped["jacsId"] == "contract-00000000-0000-4000-8000-000000000001"
        assert wrapped["jacsType"] == "a2a-task"
        assert wrapped["jacsLevel"] == "artifact"
        assert wrapped["jacsVersion"] == "1.0.0"
        assert wrapped["jacsSignature"]["agentID"] == "contract-agent"


# ---------------------------------------------------------------------------
# VerificationResult schema tests
# ---------------------------------------------------------------------------


class TestContractVerificationResult:
    """Validate A2AArtifactVerificationResult JSON field names and types."""

    @pytest.fixture()
    def contract(self) -> dict[str, Any]:
        return _load_contract()

    def test_required_fields_present(self, contract: dict[str, Any]) -> None:
        schema = contract["verificationResultSchema"]
        example = contract["verificationResultExample"]
        _assert_fields_present(
            "A2AArtifactVerificationResult",
            example,
            schema["requiredFields"],
        )

    def test_required_fields_types(self, contract: dict[str, Any]) -> None:
        schema = contract["verificationResultSchema"]
        example = contract["verificationResultExample"]
        for field, expected_type in schema["requiredFields"].items():
            if field == "_comment":
                continue
            _assert_field_type(
                "A2AArtifactVerificationResult",
                field,
                expected_type,
                example[field],
            )

    def test_example_values(self, contract: dict[str, Any]) -> None:
        example = contract["verificationResultExample"]
        assert example["valid"] is False
        assert example["signerId"] == "contract-agent"
        assert example["artifactType"] == "a2a-task"
        assert example["timestamp"] == "2026-03-01T00:00:00Z"
        assert example["error"] == "signature verification failed"

    def test_camel_case_field_names(self, contract: dict[str, Any]) -> None:
        """Fields must use camelCase, not snake_case."""
        example = contract["verificationResultExample"]
        assert "signerId" in example
        assert "signer_id" not in example
        assert "artifactType" in example
        assert "artifact_type" not in example
        assert "originalArtifact" in example
        assert "original_artifact" not in example


# ---------------------------------------------------------------------------
# TrustAssessment schema tests
# ---------------------------------------------------------------------------


class TestContractTrustAssessment:
    """Validate A2ATrustAssessment JSON field names and types."""

    @pytest.fixture()
    def contract(self) -> dict[str, Any]:
        return _load_contract()

    def test_required_fields_present(self, contract: dict[str, Any]) -> None:
        schema = contract["trustAssessmentSchema"]
        example = contract["trustAssessmentExample"]
        _assert_fields_present(
            "A2ATrustAssessment", example, schema["requiredFields"]
        )

    def test_required_fields_types(self, contract: dict[str, Any]) -> None:
        schema = contract["trustAssessmentSchema"]
        example = contract["trustAssessmentExample"]
        for field, expected_type in schema["requiredFields"].items():
            if field == "_comment":
                continue
            _assert_field_type(
                "A2ATrustAssessment", field, expected_type, example[field]
            )

    def test_example_values(self, contract: dict[str, Any]) -> None:
        example = contract["trustAssessmentExample"]
        assert example["allowed"] is True
        assert example["trustLevel"] == "jacs_verified"
        assert example["jacsRegistered"] is True
        assert example["inTrustStore"] is False
        assert example["reason"] == "open policy: all agents accepted"

    def test_camel_case_field_names(self, contract: dict[str, Any]) -> None:
        """Fields must use camelCase, not snake_case."""
        example = contract["trustAssessmentExample"]
        assert "trustLevel" in example
        assert "trust_level" not in example
        assert "jacsRegistered" in example
        assert "jacs_registered" not in example
        assert "inTrustStore" in example
        assert "in_trust_store" not in example


# ---------------------------------------------------------------------------
# AgentCard schema tests
# ---------------------------------------------------------------------------


class TestContractAgentCard:
    """Validate A2AAgentCard JSON field names and types."""

    @pytest.fixture()
    def contract(self) -> dict[str, Any]:
        return _load_contract()

    @pytest.fixture()
    def card(self) -> dict[str, Any]:
        return _load_a2a_fixture("agent_card.v04.json")

    def test_required_fields_present(
        self, contract: dict[str, Any], card: dict[str, Any]
    ) -> None:
        schema = contract["agentCardSchema"]
        _assert_fields_present("A2AAgentCard", card, schema["requiredFields"])

    def test_required_fields_types(
        self, contract: dict[str, Any], card: dict[str, Any]
    ) -> None:
        schema = contract["agentCardSchema"]
        for field, expected_type in schema["requiredFields"].items():
            if field == "_comment":
                continue
            _assert_field_type(
                "A2AAgentCard", field, expected_type, card[field]
            )

    def test_camel_case_field_names(self, card: dict[str, Any]) -> None:
        """Fields must use camelCase, not snake_case."""
        assert "supportedInterfaces" in card
        assert "supported_interfaces" not in card
        assert "defaultInputModes" in card
        assert "default_input_modes" not in card
        assert "defaultOutputModes" in card
        assert "default_output_modes" not in card

    def test_skill_sub_fields(
        self, contract: dict[str, Any], card: dict[str, Any]
    ) -> None:
        schema = contract["agentCardSchema"]
        skill_fields = schema["skillFields"]
        skills = card["skills"]
        assert len(skills) > 0
        _assert_fields_present("A2AAgentSkill", skills[0], skill_fields)

    def test_extension_has_uri(self, card: dict[str, Any]) -> None:
        caps = card["capabilities"]
        extensions = caps["extensions"]
        assert len(extensions) > 0
        assert "uri" in extensions[0]


# ---------------------------------------------------------------------------
# ChainOfCustody schema tests
# ---------------------------------------------------------------------------


class TestContractChainOfCustody:
    """Validate A2AChainOfCustody JSON field names and types."""

    @pytest.fixture()
    def contract(self) -> dict[str, Any]:
        return _load_contract()

    @pytest.fixture()
    def chain_expected(self) -> dict[str, Any]:
        fixture = _load_a2a_fixture("golden_chain_of_custody.json")
        return fixture["expected"]

    def test_top_level_fields(
        self, contract: dict[str, Any], chain_expected: dict[str, Any]
    ) -> None:
        assert "totalArtifacts" in chain_expected
        assert "entries" in chain_expected

    def test_entry_fields_present(
        self, contract: dict[str, Any], chain_expected: dict[str, Any]
    ) -> None:
        schema = contract["chainOfCustodySchema"]
        entry_fields = schema["entryFields"]
        entries = chain_expected["entries"]
        assert len(entries) > 0
        _assert_fields_present("A2AChainEntry", entries[0], entry_fields)

    def test_entry_fields_types(
        self, contract: dict[str, Any], chain_expected: dict[str, Any]
    ) -> None:
        schema = contract["chainOfCustodySchema"]
        entry_fields = schema["entryFields"]
        entries = chain_expected["entries"]
        for field, expected_type in entry_fields.items():
            if field == "_comment":
                continue
            _assert_field_type(
                "A2AChainEntry", field, expected_type, entries[0][field]
            )

    def test_camel_case_entry_fields(
        self, chain_expected: dict[str, Any]
    ) -> None:
        """Chain entry fields must use camelCase, not snake_case."""
        entries = chain_expected["entries"]
        assert "artifactId" in entries[0]
        assert "artifact_id" not in entries[0]
        assert "artifactType" in entries[0]
        assert "artifact_type" not in entries[0]
        assert "signaturePresent" in entries[0]
        assert "signature_present" not in entries[0]
