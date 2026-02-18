"""Tests for jacs.hai.client module."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import patch

import httpx
import pytest
import respx

from jacs.hai.client import (
    HaiClient,
    _get_client,
    connect,
    disconnect,
    hello_world,
    register,
    register_new_agent,
    status,
    verify_agent,
)
from jacs.hai.client import testconnection as _testconnection
from jacs.hai.errors import (
    BenchmarkError,
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RegistrationError,
)
from jacs.hai.models import (
    HaiRegistrationPreview,
    HaiRegistrationResult,
    HaiStatusResult,
    HelloWorldResult,
)


# ---------------------------------------------------------------------------
# HaiClient._make_url
# ---------------------------------------------------------------------------


class TestMakeUrl:
    def test_basic(self) -> None:
        assert HaiClient._make_url("https://hai.ai", "/api/v1/health") == "https://hai.ai/api/v1/health"

    def test_trailing_slash(self) -> None:
        assert HaiClient._make_url("https://hai.ai/", "/api/v1/health") == "https://hai.ai/api/v1/health"

    def test_no_leading_slash(self) -> None:
        assert HaiClient._make_url("https://hai.ai", "api/v1/health") == "https://hai.ai/api/v1/health"


# ---------------------------------------------------------------------------
# HaiClient.testconnection
# ---------------------------------------------------------------------------


class TestTestConnection:
    @respx.mock
    def test_success(self) -> None:
        respx.get("https://hai.ai/api/v1/health").mock(
            return_value=httpx.Response(200, json={"status": "ok"})
        )
        client = HaiClient()
        assert client.testconnection("https://hai.ai")

    @respx.mock
    def test_all_endpoints_fail(self) -> None:
        respx.get("https://hai.ai/api/v1/health").mock(
            return_value=httpx.Response(500)
        )
        respx.get("https://hai.ai/health").mock(return_value=httpx.Response(500))
        respx.get("https://hai.ai/api/health").mock(return_value=httpx.Response(500))
        respx.get("https://hai.ai/").mock(return_value=httpx.Response(500))
        client = HaiClient()
        assert not client.testconnection("https://hai.ai")

    @respx.mock
    def test_fallback_to_second_endpoint(self) -> None:
        respx.get("https://hai.ai/api/v1/health").mock(
            return_value=httpx.Response(404)
        )
        respx.get("https://hai.ai/health").mock(
            return_value=httpx.Response(200, json={})
        )
        client = HaiClient()
        assert client.testconnection("https://hai.ai")


# ---------------------------------------------------------------------------
# HaiClient._build_auth_headers
# ---------------------------------------------------------------------------


class TestBuildAuthHeaders:
    def test_no_config_raises(self) -> None:
        from jacs.hai.config import reset
        reset()
        client = HaiClient()
        with pytest.raises(HaiAuthError, match="No JACS authentication"):
            client._build_auth_headers()

    def test_valid_headers(self, loaded_config: None) -> None:
        client = HaiClient()
        headers = client._build_auth_headers()
        auth = headers["Authorization"]
        assert auth.startswith("JACS test-jacs-id-1234:")
        parts = auth[len("JACS "):].split(":")
        assert len(parts) == 3  # jacs_id:timestamp:signature


# ---------------------------------------------------------------------------
# HaiClient.hello_world
# ---------------------------------------------------------------------------


class TestHelloWorld:
    @respx.mock
    def test_success(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/hello").mock(
            return_value=httpx.Response(200, json={
                "timestamp": "2024-01-01T00:00:00Z",
                "client_ip": "1.2.3.4",
                "message": "Hello from HAI",
            })
        )
        client = HaiClient()
        result = client.hello_world("https://hai.ai")
        assert isinstance(result, HelloWorldResult)
        assert result.success
        assert result.message == "Hello from HAI"
        assert result.client_ip == "1.2.3.4"

    @respx.mock
    def test_auth_failure(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/hello").mock(
            return_value=httpx.Response(401, text="unauthorized")
        )
        client = HaiClient()
        with pytest.raises(HaiAuthError, match="401"):
            client.hello_world("https://hai.ai")

    @respx.mock
    def test_rate_limited(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/hello").mock(
            return_value=httpx.Response(429, text="too many")
        )
        client = HaiClient()
        with pytest.raises(HaiError, match="Rate limited"):
            client.hello_world("https://hai.ai")


# ---------------------------------------------------------------------------
# HaiClient.register
# ---------------------------------------------------------------------------


class TestRegister:
    @respx.mock
    def test_success(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/register").mock(
            return_value=httpx.Response(201, json={
                "agent_id": "a-1",
                "hai_signature": "sig123",
                "registration_id": "reg-1",
                "registered_at": "2024-01-01",
            })
        )
        client = HaiClient()
        result = client.register("https://hai.ai")
        assert isinstance(result, HaiRegistrationResult)
        assert result.success
        assert result.agent_id == "a-1"

    def test_preview(self, loaded_config: None) -> None:
        client = HaiClient()
        result = client.register("https://hai.ai", preview=True)
        assert isinstance(result, HaiRegistrationPreview)
        assert result.agent_id == "test-jacs-id-1234"
        assert "JACS" in result.headers["Authorization"]

    @respx.mock
    def test_already_registered(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/register").mock(
            return_value=httpx.Response(409, text="already exists")
        )
        client = HaiClient(max_retries=1)
        with pytest.raises(RegistrationError, match="already registered"):
            client.register("https://hai.ai")

    @respx.mock
    def test_auth_failure(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/register").mock(
            return_value=httpx.Response(401, text="unauthorized")
        )
        client = HaiClient(max_retries=1)
        with pytest.raises(HaiAuthError):
            client.register("https://hai.ai")


# ---------------------------------------------------------------------------
# HaiClient.status
# ---------------------------------------------------------------------------


class TestStatus:
    @respx.mock
    def test_registered(self, loaded_config: None, jacs_id: str) -> None:
        respx.get(f"https://hai.ai/api/v1/agents/{jacs_id}/verify").mock(
            return_value=httpx.Response(200, json={
                "jacs_id": jacs_id,
                "registered": True,
                "registrations": [
                    {"key_id": "k1", "algorithm": "ed25519",
                     "signature_json": "{}", "signed_at": "2024-01-01"},
                ],
                "dns_verified": False,
                "registered_at": "2024-01-01",
            })
        )
        client = HaiClient()
        result = client.status("https://hai.ai")
        assert isinstance(result, HaiStatusResult)
        assert result.registered
        assert result.hai_signatures == ["ed25519"]

    @respx.mock
    def test_not_registered(self, loaded_config: None, jacs_id: str) -> None:
        respx.get(f"https://hai.ai/api/v1/agents/{jacs_id}/verify").mock(
            return_value=httpx.Response(404, json={"message": "not found"})
        )
        client = HaiClient()
        result = client.status("https://hai.ai")
        assert not result.registered


# ---------------------------------------------------------------------------
# HaiClient.get_agent_attestation
# ---------------------------------------------------------------------------


class TestGetAgentAttestation:
    @respx.mock
    def test_registered_agent(self, loaded_config: None) -> None:
        respx.get("https://hai.ai/api/v1/agents/other-id/verify").mock(
            return_value=httpx.Response(200, json={
                "jacs_id": "other-id",
                "registered": True,
                "registrations": [
                    {"key_id": "k1", "algorithm": "ed25519",
                     "signature_json": "{}", "signed_at": "2024-01-01"},
                ],
                "dns_verified": False,
                "registered_at": "2024-01-01",
            })
        )
        client = HaiClient()
        result = client.get_agent_attestation("https://hai.ai", "other-id")
        assert result.registered
        assert result.hai_signatures == ["ed25519"]

    @respx.mock
    def test_unregistered_agent(self, loaded_config: None) -> None:
        respx.get("https://hai.ai/api/v1/agents/unknown/verify").mock(
            return_value=httpx.Response(404, json={})
        )
        client = HaiClient()
        result = client.get_agent_attestation("https://hai.ai", "unknown")
        assert not result.registered


# ---------------------------------------------------------------------------
# HaiClient.submit_benchmark_response
# ---------------------------------------------------------------------------


class TestSubmitBenchmarkResponse:
    @respx.mock
    def test_success(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/jobs/j-1/response").mock(
            return_value=httpx.Response(200, json={
                "success": True,
                "job_id": "j-1",
                "message": "accepted",
            })
        )
        client = HaiClient()
        result = client.submit_benchmark_response(
            "https://hai.ai", "j-1", "looks fine", processing_time_ms=500,
        )
        assert result.success
        assert result.job_id == "j-1"

    @respx.mock
    def test_job_not_found(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/jobs/bad/response").mock(
            return_value=httpx.Response(404, text="not found")
        )
        client = HaiClient()
        with pytest.raises(BenchmarkError, match="Job not found"):
            client.submit_benchmark_response("https://hai.ai", "bad", "x")


# ---------------------------------------------------------------------------
# HaiClient.free_chaotic_run
# ---------------------------------------------------------------------------


class TestFreeChaoticRun:
    @respx.mock
    def test_success(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/benchmark/run").mock(
            return_value=httpx.Response(200, json={
                "run_id": "r-1",
                "transcript": [
                    {"role": "party_a", "content": "I disagree"},
                    {"role": "mediator", "content": "Let's discuss"},
                ],
                "upsell_message": "Upgrade for scores!",
            })
        )
        client = HaiClient()
        result = client.free_chaotic_run("https://hai.ai")
        assert result.success
        assert len(result.transcript) == 2
        assert result.upsell_message == "Upgrade for scores!"

    @respx.mock
    def test_rate_limited(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/benchmark/run").mock(
            return_value=httpx.Response(429, text="slow down")
        )
        client = HaiClient()
        with pytest.raises(HaiError, match="Rate limited"):
            client.free_chaotic_run("https://hai.ai")


# ---------------------------------------------------------------------------
# HaiClient.benchmark
# ---------------------------------------------------------------------------


class TestBenchmark:
    @respx.mock
    def test_sync_result(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/benchmark/run").mock(
            return_value=httpx.Response(200, json={
                "success": True,
                "score": 85.0,
                "passed": 8,
                "failed": 2,
                "total": 10,
            })
        )
        client = HaiClient()
        result = client.benchmark("https://hai.ai")
        assert result.success
        assert result.score == 85.0
        assert result.passed == 8

    @respx.mock
    def test_auth_failure(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/benchmark/run").mock(
            return_value=httpx.Response(403, text="forbidden")
        )
        client = HaiClient()
        with pytest.raises(HaiAuthError):
            client.benchmark("https://hai.ai")


# ---------------------------------------------------------------------------
# HaiClient.sign_benchmark_result
# ---------------------------------------------------------------------------


class TestSignBenchmarkResult:
    def test_produces_signed_doc(self, loaded_config: None) -> None:
        client = HaiClient()
        result = client.sign_benchmark_result("run-123", score=92.0, tier="baseline")
        assert "signed_document" in result
        assert result["agent_jacs_id"] == "test-jacs-id-1234"
        doc = json.loads(result["signed_document"])
        assert doc["document_type"] == "job_response"


# ---------------------------------------------------------------------------
# HaiClient.verify_hai_message
# ---------------------------------------------------------------------------


class TestVerifyHaiMessage:
    def test_empty_signature(self) -> None:
        client = HaiClient()
        assert not client.verify_hai_message("msg", "")

    def test_empty_message(self) -> None:
        client = HaiClient()
        assert not client.verify_hai_message("", "sig")

    def test_no_public_key(self) -> None:
        client = HaiClient()
        assert not client.verify_hai_message("msg", "sig")

    def test_valid_signature(self, ed25519_keypair: tuple) -> None:
        from cryptography.hazmat.primitives.serialization import (
            Encoding,
            PublicFormat,
        )
        from jacs.hai.crypt import sign_string

        private_key, _ = ed25519_keypair
        pub_pem = private_key.public_key().public_bytes(
            Encoding.PEM, PublicFormat.SubjectPublicKeyInfo
        ).decode()
        message = "hello world"
        sig = sign_string(private_key, message)

        client = HaiClient()
        assert client.verify_hai_message(message, sig, hai_public_key=pub_pem)

    def test_invalid_signature(self, ed25519_keypair: tuple) -> None:
        from cryptography.hazmat.primitives.serialization import (
            Encoding,
            PublicFormat,
        )

        private_key, _ = ed25519_keypair
        pub_pem = private_key.public_key().public_bytes(
            Encoding.PEM, PublicFormat.SubjectPublicKeyInfo
        ).decode()

        client = HaiClient()
        assert not client.verify_hai_message("msg", "badsig", hai_public_key=pub_pem)


# ---------------------------------------------------------------------------
# HaiClient.disconnect / is_connected
# ---------------------------------------------------------------------------


class TestDisconnect:
    def test_disconnect_when_not_connected(self) -> None:
        client = HaiClient()
        client.disconnect()  # should not raise
        assert not client.is_connected

    def test_connect_sets_transport_choice(self) -> None:
        client = HaiClient()
        with pytest.raises(ValueError, match="transport must be"):
            list(client.connect("https://hai.ai", transport="invalid"))


# ---------------------------------------------------------------------------
# Module-level convenience functions
# ---------------------------------------------------------------------------


class TestConvenienceFunctions:
    @respx.mock
    def test_testconnection(self) -> None:
        respx.get("https://hai.ai/api/v1/health").mock(
            return_value=httpx.Response(200, json={})
        )
        assert _testconnection("https://hai.ai")

    @respx.mock
    def test_hello_world(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/hello").mock(
            return_value=httpx.Response(200, json={"message": "hi"})
        )
        result = hello_world("https://hai.ai")
        assert result.success

    @respx.mock
    def test_register(self, loaded_config: None) -> None:
        respx.post("https://hai.ai/api/v1/agents/register").mock(
            return_value=httpx.Response(201, json={"agent_id": "a1"})
        )
        result = register("https://hai.ai")
        assert isinstance(result, HaiRegistrationResult)

    @respx.mock
    def test_status(self, loaded_config: None, jacs_id: str) -> None:
        respx.get(f"https://hai.ai/api/v1/agents/{jacs_id}/verify").mock(
            return_value=httpx.Response(200, json={
                "jacs_id": jacs_id,
                "registered": True,
                "registrations": [],
                "dns_verified": False,
                "registered_at": "2024-01-01",
            })
        )
        result = status("https://hai.ai")
        assert result.registered


# ---------------------------------------------------------------------------
# verify_agent (standalone)
# ---------------------------------------------------------------------------


class TestVerifyAgent:
    def test_valid_level_1(self, ed25519_keypair: tuple) -> None:
        from jacs.hai.crypt import create_agent_document

        private_key, public_pem = ed25519_keypair
        doc = create_agent_document(
            name="Bot", version="1.0",
            public_key_pem=public_pem, private_key=private_key,
        )
        result = verify_agent(doc, min_level=1)
        assert result.valid
        assert result.level >= 1
        assert result.jacs_valid

    def test_invalid_json(self) -> None:
        result = verify_agent("not json at all", min_level=1)
        assert not result.valid
        assert result.level == 0

    def test_missing_signature(self) -> None:
        doc = {"jacsId": "x", "jacsPublicKey": ""}
        result = verify_agent(doc, min_level=1)
        assert not result.valid
        assert "Missing jacsSignature" in result.errors[0]
