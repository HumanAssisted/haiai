"""Async HAI client using ``httpx.AsyncClient``.

Provides the same API as ``HaiClient`` but with async methods suitable
for use in FastAPI, LangChain, CrewAI, AutoGen, and other async frameworks.

Usage::

    from haiai import config
    from haiai.async_client import AsyncHaiClient

    config.load("./jacs.config.json")

    async with AsyncHaiClient() as client:
        result = await client.hello_world("https://beta.hai.ai")
        print(result.message)
"""

from __future__ import annotations

import asyncio
import base64
import json
import logging
import time
from typing import Any, AsyncIterator, Optional, Union
from urllib.parse import quote

import httpx

from haiai._retry import RETRY_MAX_ATTEMPTS, backoff, should_retry
from haiai._sse import flatten_benchmark_job, parse_sse_lines
from haiai.signing import canonicalize_json, create_agent_document  # noqa: F401
from haiai.errors import (
    BenchmarkError,
    BodyTooLarge,
    EmailNotActive,
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RateLimited,
    RecipientNotFound,
    RegistrationError,
    SubjectTooLong,
)
from haiai.models import (
    BaselineRunResult,
    BenchmarkResult,
    ChainEntry,
    Contact,
    EmailDeliveryInfo,
    EmailMessage,
    EmailReputationInfo,
    EmailStatus,
    EmailVerificationResultV2,
    EmailVolumeInfo,
    FieldResult,
    FieldStatus,
    FreeChaoticResult,
    HaiEvent,
    HaiRegistrationPreview,
    HaiRegistrationResult,
    HaiStatusResult,
    HelloWorldResult,
    JobResponseResult,
    PublicKeyInfo,
    RotationResult,
    SendEmailResult,
    TranscriptMessage,
)
from haiai.signing import is_signed_event, sign_response, unwrap_signed_event

logger = logging.getLogger("haiai.async_client")


class AsyncHaiClient:
    """Async client for the HAI benchmark platform.

    Drop-in async replacement for ``HaiClient``.  All I/O methods are
    ``async`` and use ``httpx.AsyncClient`` internally.
    """

    def __init__(
        self,
        *,
        timeout: float = 30.0,
        max_retries: int = 3,
        verify_server_signatures: bool = False,
    ) -> None:
        self._timeout = timeout
        self._max_retries = max_retries
        self._verify_server_signatures = verify_server_signatures
        self._connected = False
        self._should_disconnect = False
        self._hai_url: Optional[str] = None
        self._http: Optional[httpx.AsyncClient] = None
        self._hai_agent_id: Optional[str] = None
        self._agent_email: Optional[str] = None

    @property
    def agent_email(self) -> Optional[str]:
        """Agent @hai.ai email, required for v2 email signing."""
        return self._agent_email

    def set_agent_email(self, email: str) -> None:
        """Set the agent @hai.ai email used in v2 email signing payloads."""
        self._agent_email = email

    async def _get_http(self) -> httpx.AsyncClient:
        if self._http is None or self._http.is_closed:
            self._http = httpx.AsyncClient(timeout=self._timeout)
        return self._http

    async def close(self) -> None:
        """Close the underlying HTTP client."""
        if self._http is not None and not self._http.is_closed:
            await self._http.aclose()
            self._http = None

    async def __aenter__(self) -> AsyncHaiClient:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    # ------------------------------------------------------------------
    # Internal helpers (reused from sync client logic)
    # ------------------------------------------------------------------

    @staticmethod
    def _make_url(base_url: str, path: str) -> str:
        base = base_url.rstrip("/")
        path = "/" + path.lstrip("/")
        return base + path

    @staticmethod
    def _escape_path_segment(value: str) -> str:
        return quote(value, safe="")

    def _get_jacs_id(self) -> str:
        from haiai.config import get_config
        cfg = get_config()
        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required in config for JACS authentication")
        return cfg.jacs_id

    def _get_hai_agent_id(self) -> str:
        """Return the HAI-assigned agent UUID for email URL paths."""
        return self._hai_agent_id or self._get_jacs_id()

    def _build_jacs_auth_header(self) -> str:
        from haiai.config import get_config, get_agent
        cfg = get_config()
        agent = get_agent()
        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required for JACS authentication")

        # Prefer JACS binding delegation
        if hasattr(agent, "build_auth_header"):
            return agent.build_auth_header()

        # Local construction using JACS sign_string
        if not hasattr(agent, "sign_string"):
            raise HaiError(
                "build_auth_header requires a JACS agent with sign_string support",
                code="JACS_NOT_LOADED",
                action="Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
            )

        timestamp = int(time.time())
        message = f"{cfg.jacs_id}:{timestamp}"
        signature = agent.sign_string(message)
        return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"

    def _build_auth_headers(self) -> dict[str, str]:
        from haiai.config import is_loaded, get_config
        if not (is_loaded() and get_config().jacs_id):
            raise HaiAuthError(
                "No JACS authentication available. "
                "Call haiai.config.load() with a config containing jacsId."
            )
        return {"Authorization": self._build_jacs_auth_header()}

    @staticmethod
    def _parse_transcript(raw_messages: list[dict[str, Any]]) -> list[TranscriptMessage]:
        return [
            TranscriptMessage(
                role=msg.get("role", "system"),
                content=msg.get("content", ""),
                timestamp=msg.get("timestamp", ""),
                annotations=msg.get("annotations", []),
            )
            for msg in raw_messages
        ]

    # ------------------------------------------------------------------
    # testconnection
    # ------------------------------------------------------------------

    async def testconnection(self, hai_url: str) -> bool:
        """Test connectivity to the HAI server."""
        http = await self._get_http()
        endpoints = ["/api/v1/health", "/health", "/api/health", "/"]
        for endpoint in endpoints:
            try:
                url = self._make_url(hai_url, endpoint)
                resp = await http.get(url, timeout=min(self._timeout, 10.0), follow_redirects=True)
                if 200 <= resp.status_code < 300:
                    return True
            except Exception:
                pass
        return False

    # ------------------------------------------------------------------
    # hello_world
    # ------------------------------------------------------------------

    async def hello_world(
        self, hai_url: str, include_test: bool = False
    ) -> HelloWorldResult:
        """Send a JACS-signed hello request."""
        http = await self._get_http()
        url = self._make_url(hai_url, "/api/v1/agents/hello")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {}
        if include_test:
            payload["include_test"] = True

        try:
            resp = await http.post(url, json=payload, headers=headers)
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")

        if resp.status_code in (401, 403):
            raise HaiAuthError(
                f"Hello auth failed: {resp.status_code}",
                status_code=resp.status_code,
                body=resp.text,
            )
        if resp.status_code == 429:
            raise HaiError("Rate limited -- too many hello requests", status_code=429)
        if resp.status_code not in (200, 201):
            raise HaiApiError(
                f"Hello request failed: {resp.status_code}",
                status_code=resp.status_code,
                body=resp.text,
            )

        data = resp.json()
        hai_sig_valid = False
        hai_ack_sig = data.get("hai_signed_ack", "")
        if hai_ack_sig:
            hai_sig_valid = self.verify_hai_message(
                message=json.dumps(data, sort_keys=True),
                signature=hai_ack_sig,
                hai_public_key=data.get("hai_public_key_fingerprint", ""),
                hai_url=hai_url,
            )
        return HelloWorldResult(
            success=True,
            timestamp=data.get("timestamp", ""),
            client_ip=data.get("client_ip", ""),
            hai_public_key_fingerprint=data.get("hai_public_key_fingerprint", ""),
            message=data.get("message", ""),
            hai_signature_valid=hai_sig_valid,
            hello_id=data.get("hello_id", ""),
            test_scenario=data.get("test_scenario"),
            raw_response=data,
        )

    def verify_hai_message(
        self,
        message: str,
        signature: str,
        hai_public_key: str = "",
        hai_url: Optional[str] = None,
    ) -> bool:
        """Verify a HAI-signed message with the same rules as the sync client."""
        from haiai.client import _verify_hai_message_impl

        return _verify_hai_message_impl(
            message=message,
            signature=signature,
            hai_public_key=hai_public_key,
            hai_url=hai_url,
        )

    # ------------------------------------------------------------------
    # register
    # ------------------------------------------------------------------

    async def register(
        self,
        hai_url: str,
        agent_json: Optional[str] = None,
        public_key: Optional[str] = None,
        preview: bool = False,
        owner_email: Optional[str] = None,
    ) -> Union[HaiRegistrationResult, HaiRegistrationPreview]:
        """Register a JACS agent with HAI."""
        from haiai.config import get_config

        http = await self._get_http()
        cfg = get_config()

        if agent_json is None:
            from haiai.config import get_agent
            from haiai.client import _read_public_key_pem

            agent = get_agent()
            pub_pem = _read_public_key_pem(cfg)
            agent_doc = create_agent_document(
                agent=agent, name=cfg.name, version=cfg.version,
            )
            agent_json = json.dumps(agent_doc, indent=2)
            if public_key is None:
                public_key = pub_pem

        payload: dict[str, Any] = {"agent_json": agent_json}
        if public_key is not None:
            payload["public_key"] = base64.b64encode(
                public_key.encode("utf-8")
            ).decode("utf-8")
        if owner_email is not None:
            payload["owner_email"] = owner_email

        url = self._make_url(hai_url, "/api/v1/agents/register")

        if preview:
            return HaiRegistrationPreview(
                agent_id=cfg.jacs_id or "",
                agent_name=cfg.name,
                payload_json=json.dumps(payload, indent=2),
                endpoint=url,
                headers={"Content-Type": "application/json", "Authorization": "JACS ***"},
            )

        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        last_error: Optional[Exception] = None
        for attempt in range(self._max_retries):
            try:
                resp = await http.post(url, json=payload, headers=headers)

                if resp.status_code in (200, 201):
                    data = resp.json()
                    agent_id = data.get("agent_id", "")
                    if agent_id:
                        self._hai_agent_id = agent_id
                    return HaiRegistrationResult(
                        success=True,
                        agent_id=agent_id,
                        registered_at=data.get("registered_at", ""),
                        raw_response=data,
                    )
                if resp.status_code in (401, 403):
                    raise HaiAuthError(
                        "Registration auth failed",
                        status_code=resp.status_code,
                        body=resp.text,
                    )
                if resp.status_code == 409:
                    raise RegistrationError(
                        "Agent is already registered",
                        status_code=resp.status_code,
                    )
                last_error = RegistrationError(
                    f"Registration failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                )
            except (httpx.ConnectError, httpx.TimeoutException) as exc:
                last_error = HaiConnectionError(f"Connection failed: {exc}")
            except HaiError:
                raise
            except Exception as exc:
                last_error = RegistrationError(f"Unexpected error: {exc}")

            if attempt < self._max_retries - 1:
                await asyncio.sleep(2**attempt)

        raise last_error or RegistrationError("Registration failed after all retries")

    # ------------------------------------------------------------------
    # status
    # ------------------------------------------------------------------

    async def status(self, hai_url: str) -> HaiStatusResult:
        """Check registration/verification status."""
        http = await self._get_http()
        jacs_id = self._get_jacs_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_jacs_id}/verify")
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, headers=headers)

            if resp.status_code == 200:
                data = resp.json()
                registrations = data.get("registrations", [])
                return HaiStatusResult(
                    registered=data.get("registered", True),
                    agent_id=data.get("jacs_id", jacs_id),
                    registered_at=data.get("registered_at", ""),
                    hai_signatures=[r.get("algorithm", "") for r in registrations],
                    raw_response=data,
                )
            if resp.status_code == 404:
                return HaiStatusResult(
                    registered=False,
                    agent_id=jacs_id,
                    raw_response=resp.json() if resp.text else {},
                )
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Status check auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            raise HaiError(
                f"Status check failed: HTTP {resp.status_code}",
                status_code=resp.status_code,
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Status check failed: {exc}")

    # ------------------------------------------------------------------
    # username APIs
    # ------------------------------------------------------------------

    async def check_username(self, hai_url: str, username: str) -> dict[str, Any]:
        """Check if a username is available for @hai.ai email."""
        http = await self._get_http()
        url = self._make_url(hai_url, "/api/v1/agents/username/check")

        try:
            resp = await http.get(url, params={"username": username}, timeout=self._timeout)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Username check failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username check failed: {exc}")

    async def claim_username(
        self, hai_url: str, agent_id: str, username: str
    ) -> dict[str, Any]:
        """Claim a username for an agent and cache returned @hai.ai email."""
        http = await self._get_http()
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        try:
            resp = await http.post(
                url,
                json={"username": username},
                headers=headers,
                timeout=self._timeout,
            )
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Username claim auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Username claim failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            data = resp.json()
            self._agent_email = data.get("email")
            return data
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username claim failed: {exc}")

    async def update_username(
        self, hai_url: str, agent_id: str, username: str
    ) -> dict[str, Any]:
        """Rename an existing username for an agent."""
        http = await self._get_http()
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
        headers = self._build_auth_headers()

        try:
            resp = await http.put(
                url,
                headers=headers,
                json={"username": username},
                timeout=self._timeout,
            )
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Username update auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Username update failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username update failed: {exc}")

    async def delete_username(self, hai_url: str, agent_id: str) -> dict[str, Any]:
        """Release a claimed username for an agent."""
        http = await self._get_http()
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
        headers = self._build_auth_headers()

        try:
            resp = await http.delete(url, headers=headers, timeout=self._timeout)
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Username delete auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Username delete failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username delete failed: {exc}")

    # ------------------------------------------------------------------
    # attestation
    # ------------------------------------------------------------------

    async def create_attestation(
        self,
        hai_url: str,
        agent_id: str,
        subject: dict,
        claims: list,
        evidence: list | None = None,
    ) -> dict:
        """Create a signed attestation document for a registered agent.

        HAI co-signs the attestation using its signing authority.

        Args:
            hai_url: Base URL of the HAI server.
            agent_id: The agent's JACS ID.
            subject: Attestation subject (type, id, digests).
            claims: Array of claim objects.
            evidence: Optional array of evidence references.

        Returns:
            Dict with attestation, hai_signature, and doc_id.
        """
        escaped = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{escaped}/attestations")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload = {
            "subject": subject,
            "claims": claims,
            "evidence": evidence or [],
        }

        http = await self._get_http()
        resp = await http.post(url, json=payload, headers=headers, timeout=self._timeout)
        if resp.status_code == 404:
            raise HaiError(f"Agent '{agent_id}' not registered with HAI")
        if resp.status_code in (401, 403):
            raise HaiAuthError(f"Authentication failed: {resp.text}")
        resp.raise_for_status()
        return resp.json()

    async def list_attestations(
        self,
        hai_url: str,
        agent_id: str,
        limit: int = 20,
        offset: int = 0,
    ) -> dict:
        """List attestations for a registered agent."""
        escaped = self._escape_path_segment(agent_id)
        url = self._make_url(
            hai_url,
            f"/api/v1/agents/{escaped}/attestations?limit={limit}&offset={offset}",
        )
        headers = self._build_auth_headers()
        http = await self._get_http()
        resp = await http.get(url, headers=headers, timeout=self._timeout)
        resp.raise_for_status()
        return resp.json()

    async def get_attestation(
        self,
        hai_url: str,
        agent_id: str,
        doc_id: str,
    ) -> dict:
        """Get a specific attestation document."""
        escaped_agent = self._escape_path_segment(agent_id)
        escaped_doc = self._escape_path_segment(doc_id)
        url = self._make_url(
            hai_url,
            f"/api/v1/agents/{escaped_agent}/attestations/{escaped_doc}",
        )
        headers = self._build_auth_headers()
        http = await self._get_http()
        resp = await http.get(url, headers=headers, timeout=self._timeout)
        if resp.status_code == 404:
            raise HaiError(f"Attestation '{doc_id}' not found for agent '{agent_id}'")
        resp.raise_for_status()
        return resp.json()

    async def verify_attestation(
        self,
        hai_url: str,
        document: str,
    ) -> dict:
        """Verify an attestation document via HAI."""
        url = self._make_url(hai_url, "/api/v1/attestations/verify")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        http = await self._get_http()
        resp = await http.post(
            url,
            json={"document": document},
            headers=headers,
            timeout=self._timeout,
        )
        resp.raise_for_status()
        return resp.json()

    # ------------------------------------------------------------------
    # benchmark
    # ------------------------------------------------------------------

    async def benchmark(
        self,
        hai_url: str,
        name: str = "mediator",
        tier: str = "free",
        timeout: Optional[float] = None,
    ) -> BenchmarkResult:
        """Run a benchmark via HAI."""
        http = await self._get_http()
        url = self._make_url(hai_url, "/api/benchmark/run")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload = {"name": name, "tier": tier}
        request_timeout = timeout or max(self._timeout, 120.0)

        try:
            resp = await http.post(url, json=payload, headers=headers, timeout=request_timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Benchmark auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise BenchmarkError(
                    f"Benchmark request failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                )

            data = resp.json()
            job_id = data.get("job_id") or data.get("jobId")
            if job_id:
                return await self._poll_benchmark_result(hai_url, job_id, request_timeout)

            return BenchmarkResult(
                success=data.get("success", True),
                suite=name,
                score=float(data.get("score", 0)),
                passed=int(data.get("passed", 0)),
                failed=int(data.get("failed", 0)),
                total=int(data.get("total", 0)),
                duration_ms=int(data.get("duration_ms", data.get("durationMs", 0))),
                results=data.get("results", []),
                raw_response=data,
            )
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise BenchmarkError(f"Benchmark execution failed: {exc}")

    async def _poll_benchmark_result(
        self, hai_url: str, job_id: str, timeout: float,
    ) -> BenchmarkResult:
        http = await self._get_http()
        safe_job_id = self._escape_path_segment(job_id)
        url = self._make_url(hai_url, f"/api/benchmark/jobs/{safe_job_id}")
        headers = self._build_auth_headers()

        start_time = time.time()
        poll_interval = 2.0

        while (time.time() - start_time) < timeout:
            resp = await http.get(url, headers=headers, timeout=30.0)
            if resp.status_code != 200:
                raise BenchmarkError(f"Poll failed: HTTP {resp.status_code}", status_code=resp.status_code)

            data = resp.json()
            status = data.get("status", "").lower()
            if status == "completed":
                return BenchmarkResult(
                    success=True,
                    suite=data.get("suite", ""),
                    score=float(data.get("score", 0)),
                    passed=int(data.get("passed", 0)),
                    failed=int(data.get("failed", 0)),
                    total=int(data.get("total", 0)),
                    duration_ms=int(data.get("duration_ms", 0)),
                    results=data.get("results", []),
                    raw_response=data,
                )
            if status in ("failed", "error"):
                raise BenchmarkError(data.get("error", "Benchmark job failed"), response_data=data)

            await asyncio.sleep(poll_interval)
            poll_interval = min(poll_interval * 1.5, 10.0)

        raise BenchmarkError(f"Benchmark timed out after {timeout}s")

    async def free_run(
        self,
        hai_url: str,
        transport: str = "sse",
    ) -> FreeChaoticResult:
        """Run a free benchmark (async).

        Connects to HAI and runs the canonical scenario with a cheap model.
        No judge evaluation, no scoring.

        Rate limited to 3 runs per JACS keypair per 24 hours.

        Args:
            hai_url: Base URL of the HAI server.
            transport: Transport protocol: "sse" (default) or "ws".

        Returns:
            FreeChaoticResult with transcript and annotations.
        """
        http = await self._get_http()
        jacs_id = self._get_jacs_id()
        url = self._make_url(hai_url, "/api/benchmark/run")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {
            "name": f"Free Run - {jacs_id[:8]}",
            "tier": "free",
            "transport": transport,
        }

        try:
            resp = await http.post(
                url, json=payload, headers=headers,
                timeout=max(self._timeout, 120.0),
            )

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Authentication failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code == 429:
                raise HaiError(
                    "Rate limited -- maximum 3 free chaotic runs per 24 hours",
                    status_code=429,
                )
            if resp.status_code == 402:
                raise HaiError("Payment required for this tier", status_code=402)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Free chaotic run failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            transcript = self._parse_transcript(data.get("transcript", []))

            return FreeChaoticResult(
                success=True,
                run_id=data.get("run_id", data.get("runId", "")),
                transcript=transcript,
                upsell_message=data.get(
                    "upsell_message", data.get("upsellMessage", "")
                ),
                raw_response=data,
            )
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Free chaotic run failed: {exc}")

    async def submit_benchmark_response(
        self,
        hai_url: str,
        job_id: str,
        message: str,
        metadata: Optional[dict[str, Any]] = None,
        processing_time_ms: int = 0,
    ) -> JobResponseResult:
        """Submit a benchmark job response (async).

        POST /api/v1/agents/jobs/{job_id}/response

        The response is wrapped as a JACS-signed document.

        Args:
            hai_url: Base URL of the HAI server.
            job_id: The job ID from the benchmark_job event.
            message: The mediator's response message.
            metadata: Optional metadata dict.
            processing_time_ms: Processing time in milliseconds.

        Returns:
            JobResponseResult with acknowledgment.
        """
        from haiai.config import get_config, get_agent

        http = await self._get_http()
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        response_body: dict[str, Any] = {"message": message}
        if metadata is not None:
            response_body["metadata"] = metadata
        response_body["processing_time_ms"] = processing_time_ms

        job_response_payload = {"response": response_body}

        cfg = get_config()
        payload: dict[str, Any] = sign_response(
            job_response_payload, get_agent(), cfg.jacs_id or "",
        )

        safe_job_id = self._escape_path_segment(job_id)
        url = self._make_url(hai_url, f"/api/v1/agents/jobs/{safe_job_id}/response")

        last_exc: Optional[Exception] = None
        for attempt in range(RETRY_MAX_ATTEMPTS):
            try:
                resp = await http.post(
                    url, json=payload, headers=headers, timeout=30.0,
                )

                if resp.status_code in (401, 403):
                    raise HaiAuthError(
                        f"Auth failed submitting response: {resp.status_code}",
                        status_code=resp.status_code,
                        body=resp.text,
                    )
                if resp.status_code == 404:
                    raise BenchmarkError(
                        f"Job not found: {job_id}",
                        status_code=404,
                    )
                if resp.status_code in (502, 503, 504) or resp.status_code == 429:
                    delay = backoff(attempt)
                    logger.warning(
                        "submit_benchmark_response got %d, retrying in %.1fs",
                        resp.status_code,
                        delay,
                    )
                    await asyncio.sleep(delay)
                    headers = self._build_auth_headers()
                    headers["Content-Type"] = "application/json"
                    continue

                resp.raise_for_status()
                data = resp.json()
                return JobResponseResult(
                    success=data.get("success", True),
                    job_id=data.get("job_id", data.get("jobId", job_id)),
                    message=data.get("message", "Response accepted"),
                    raw_response=data,
                )
            except (httpx.ConnectError, httpx.TimeoutException) as exc:
                last_exc = exc
                delay = backoff(attempt)
                await asyncio.sleep(delay)
            except HaiError:
                raise
            except Exception as exc:
                last_exc = exc
                break

        raise HaiConnectionError(
            f"submit_benchmark_response failed after retries: {last_exc}"
        )

    async def rotate_keys(
        self,
        hai_url: Optional[str] = None,
        register_with_hai: bool = True,
        config_path: Optional[str] = None,
        algorithm: str = "pq2025",
    ) -> RotationResult:
        """Rotate the agent's cryptographic keys (async).

        Delegates to the synchronous ``HaiClient.rotate_keys()`` in a
        thread executor, since the operation involves file I/O and JACS
        agent creation that are inherently synchronous.

        Args:
            hai_url: Base URL of the HAI server (required if
                ``register_with_hai=True``).
            register_with_hai: If True (default), re-register the agent
                with HAI after local rotation.
            config_path: Path to jacs.config.json.
            algorithm: Signing algorithm for the new key (default "pq2025").

        Returns:
            RotationResult with old/new versions, public key hash, and
            whether re-registration succeeded.
        """
        from haiai.client import HaiClient

        sync_client = HaiClient(
            timeout=self._timeout,
            verify_server_signatures=self._verify_server_signatures,
        )
        sync_client._hai_url = self._hai_url or hai_url or ''
        sync_client._hai_agent_id = self._hai_agent_id or ''

        return await asyncio.to_thread(
            sync_client.rotate_keys,
            hai_url=hai_url,
            register_with_hai=register_with_hai,
            config_path=config_path,
            algorithm=algorithm,
        )

    # ------------------------------------------------------------------
    # Email CRUD
    # ------------------------------------------------------------------

    async def send_email(
        self,
        hai_url: str,
        to: str,
        subject: str,
        body: str,
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
        cc: Optional[list[str]] = None,
        bcc: Optional[list[str]] = None,
        labels: Optional[list[str]] = None,
    ) -> SendEmailResult:
        """Send an email from this agent's @hai.ai address."""
        if self._agent_email is None:
            raise HaiError(
                "agent email not set -- call claim_username() first or set_agent_email()"
            )

        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/send")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        # Server handles JACS attachment signing (TASK_014/017).
        # Client only sends content fields.
        payload: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            payload["in_reply_to"] = in_reply_to
        if attachments:
            payload["attachments"] = [
                {
                    "filename": a["filename"],
                    "content_type": a["content_type"],
                    "data_base64": base64.b64encode(a["data"]).decode(),
                }
                for a in attachments
            ]
        if cc:
            payload["cc"] = cc
        if bcc:
            payload["bcc"] = bcc
        if labels:
            payload["labels"] = labels

        try:
            resp = await http.post(url, json=payload, headers=headers)
            # Parse structured error code if available
            try:
                err_data = resp.json()
                err_code = err_data.get("error_code", "")
            except (ValueError, KeyError):
                err_data = {}
                err_code = ""

            if resp.status_code == 403 and (err_code == "EMAIL_NOT_ACTIVE" or "allocated" in resp.text.lower()):
                raise EmailNotActive(
                    err_data.get("message", "Agent email is not active (status: allocated)"),
                    status_code=403, body=resp.text,
                )
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email send auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code == 400 and (err_code == "RECIPIENT_NOT_FOUND" or "Invalid recipient" in resp.text):
                raise RecipientNotFound(
                    err_data.get("message", "Recipient not found"),
                    status_code=400, body=resp.text,
                )
            if resp.status_code == 400 and err_code == "SUBJECT_TOO_LONG":
                raise SubjectTooLong(
                    err_data.get("message", "Subject too long"),
                    status_code=400, body=resp.text,
                )
            if resp.status_code == 400 and err_code == "BODY_TOO_LARGE":
                raise BodyTooLarge(
                    err_data.get("message", "Body too large"),
                    status_code=400, body=resp.text,
                )
            if resp.status_code == 429:
                raise RateLimited(
                    err_data.get("message", "Rate limited"),
                    status_code=429, body=resp.text,
                    resets_at=err_data.get("resets_at", ""),
                )
            if resp.status_code == 400:
                body_lower = resp.text.lower()
                if "recipient" in body_lower:
                    raise RecipientNotFound(f"Recipient not found: {resp.text}", status_code=400, body=resp.text)
                if "subject" in body_lower:
                    raise SubjectTooLong(f"Subject too long: {resp.text}", status_code=400, body=resp.text)
                if "body" in body_lower:
                    raise BodyTooLarge(f"Body too large: {resp.text}", status_code=400, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email send failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return SendEmailResult(message_id=data.get("message_id", ""), status=data.get("status", "sent"))
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email send failed: {exc}")

    async def send_signed_email(
        self,
        hai_url: str,
        to: str,
        subject: str,
        body: str,
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
        cc: Optional[list[str]] = None,
        bcc: Optional[list[str]] = None,
        labels: Optional[list[str]] = None,
    ) -> SendEmailResult:
        """Send an agent-signed email (async).

        .. deprecated::
            send_signed_email currently delegates to send_email. Use
            send_email directly.

        Args:
            hai_url: Base URL of the HAI server.
            to: Recipient address.
            subject: Email subject line.
            body: Plain text email body.
            in_reply_to: Optional Message-ID for threading.
            attachments: Optional list of attachment dicts.
            cc: Optional CC recipients.
            bcc: Optional BCC recipients.
            labels: Optional labels.

        Returns:
            SendEmailResult with message_id and status.
        """
        return await self.send_email(
            hai_url,
            to,
            subject,
            body,
            in_reply_to=in_reply_to,
            attachments=attachments,
            cc=cc,
            bcc=bcc,
            labels=labels,
        )

    async def sign_email(self, hai_url: str, raw_email: bytes) -> bytes:
        """Sign a raw RFC 5322 email with a JACS attachment via the HAI API.

        The server adds a ``jacs-signature.json`` MIME attachment containing
        the detached JACS signature. The returned bytes are the signed email.

        Also accepts ``email.message.EmailMessage`` objects -- they are
        automatically converted to bytes via ``as_bytes()``.

        Args:
            hai_url: Base URL of the HAI server.
            raw_email: Raw RFC 5322 email bytes (or EmailMessage).

        Returns:
            Signed email bytes with the JACS attachment added.
        """
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()

        http = await self._get_http()
        url = self._make_url(hai_url, "/api/v1/email/sign")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "message/rfc822"

        try:
            resp = await http.post(url, content=raw_email, headers=headers)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email sign failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.content
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email sign failed: {exc}")

    async def verify_email(self, hai_url: str, raw_email: bytes) -> EmailVerificationResultV2:
        """Verify a JACS-signed email via the HAI API.

        The server extracts the ``jacs-signature.json`` attachment, validates
        the cryptographic signature and content hashes, and returns a
        detailed verification result.

        Also accepts ``email.message.EmailMessage`` objects -- they are
        automatically converted to bytes via ``as_bytes()``.

        Args:
            hai_url: Base URL of the HAI server.
            raw_email: Raw RFC 5322 email bytes (or EmailMessage).

        Returns:
            EmailVerificationResultV2 with field-level verification results.
        """
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()

        http = await self._get_http()
        url = self._make_url(hai_url, "/api/v1/email/verify")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "message/rfc822"

        try:
            resp = await http.post(url, content=raw_email, headers=headers)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email verify failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            data = resp.json()
            return EmailVerificationResultV2(
                valid=data.get("valid", False),
                jacs_id=data.get("jacs_id", ""),
                algorithm=data.get("algorithm", ""),
                reputation_tier=data.get("reputation_tier", ""),
                dns_verified=data.get("dns_verified"),
                field_results=[
                    FieldResult(
                        field=fr.get("field", ""),
                        status=FieldStatus(fr.get("status", "unverifiable")),
                        original_hash=fr.get("original_hash"),
                        current_hash=fr.get("current_hash"),
                        original_value=fr.get("original_value"),
                        current_value=fr.get("current_value"),
                    )
                    for fr in data.get("field_results", [])
                ],
                chain=[
                    ChainEntry(
                        signer=ce.get("signer", ""),
                        jacs_id=ce.get("jacs_id", ""),
                        valid=ce.get("valid", False),
                        forwarded=ce.get("forwarded", False),
                    )
                    for ce in data.get("chain", [])
                ],
                error=data.get("error"),
                agent_status=data.get("agent_status"),
                benchmarks_completed=data.get("benchmarks_completed", []),
            )
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email verify failed: {exc}")

    async def list_messages(
        self,
        hai_url: str,
        limit: int = 20,
        offset: int = 0,
        direction: Optional[str] = None,
        is_read: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
    ) -> list[EmailMessage]:
        """List email messages for this agent."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/messages")
        headers = self._build_auth_headers()

        params: dict[str, Any] = {"limit": limit, "offset": offset}
        if direction is not None:
            params["direction"] = direction
        if is_read is not None:
            params["is_read"] = str(is_read).lower()
        if folder is not None:
            params["folder"] = folder
        if label is not None:
            params["label"] = label

        try:
            resp = await http.get(url, params=params, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email list auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email list failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            messages = data if isinstance(data, list) else data.get("messages", [])
            return [EmailMessage.from_dict(m) for m in messages]
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email list failed: {exc}")

    async def mark_read(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as read."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/read",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.post(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email mark_read auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(f"Email mark_read failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            return True
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email mark_read failed: {exc}")

    async def get_email_status(self, hai_url: str) -> EmailStatus:
        """Get email rate-limit and reputation status."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/status")
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email status auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email status failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return self._parse_email_status(data)
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email status failed: {exc}")

    @staticmethod
    def _parse_email_status(data: dict) -> EmailStatus:
        """Parse an EmailStatus from a JSON dict, including nested fields."""
        volume_data = data.get("volume")
        volume = (
            EmailVolumeInfo(
                sent_total=int(volume_data.get("sent_total", 0)),
                received_total=int(volume_data.get("received_total", 0)),
                sent_24h=int(volume_data.get("sent_24h", 0)),
            )
            if volume_data
            else None
        )

        delivery_data = data.get("delivery")
        delivery = (
            EmailDeliveryInfo(
                bounce_count=int(delivery_data.get("bounce_count", 0)),
                spam_report_count=int(delivery_data.get("spam_report_count", 0)),
                delivery_rate=float(delivery_data.get("delivery_rate", 0.0)),
            )
            if delivery_data
            else None
        )

        reputation_data = data.get("reputation")
        reputation = (
            EmailReputationInfo(
                score=float(reputation_data.get("score", 0.0)),
                tier=reputation_data.get("tier", ""),
                email_score=float(reputation_data.get("email_score", 0.0)),
                hai_score=(
                    float(reputation_data["hai_score"])
                    if reputation_data.get("hai_score") is not None
                    else None
                ),
            )
            if reputation_data
            else None
        )

        return EmailStatus(
            email=data.get("email", ""),
            status=data.get("status", ""),
            tier=data.get("tier", ""),
            billing_tier=data.get("billing_tier", ""),
            messages_sent_24h=int(data.get("messages_sent_24h", 0)),
            daily_limit=int(data.get("daily_limit", 0)),
            daily_used=int(data.get("daily_used", 0)),
            resets_at=data.get("resets_at", ""),
            messages_sent_total=int(data.get("messages_sent_total", 0)),
            external_enabled=bool(data.get("external_enabled", False)),
            external_sends_today=int(data.get("external_sends_today", 0)),
            last_tier_change=data.get("last_tier_change"),
            volume=volume,
            delivery=delivery,
            reputation=reputation,
        )

    async def get_message(self, hai_url: str, message_id: str) -> EmailMessage:
        """Get a single email message by ID."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email get_message auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code == 404:
                raise HaiApiError(f"Message not found: {message_id}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email get_message failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            m = resp.json()
            return EmailMessage.from_dict(m)
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email get_message failed: {exc}")

    async def delete_message(self, hai_url: str, message_id: str) -> bool:
        """Delete an email message."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.delete(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email delete_message auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code == 404:
                raise HaiApiError(f"Message not found: {message_id}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 204):
                raise HaiApiError(f"Email delete_message failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            return True
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email delete_message failed: {exc}")

    async def mark_unread(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as unread."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/unread",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.post(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email mark_unread auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(f"Email mark_unread failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            return True
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email mark_unread failed: {exc}")

    async def search_messages(
        self,
        hai_url: str,
        q: Optional[str] = None,
        direction: Optional[str] = None,
        from_address: Optional[str] = None,
        to_address: Optional[str] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
        is_read: Optional[bool] = None,
        jacs_verified: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
        limit: int = 20,
        offset: int = 0,
    ) -> list[EmailMessage]:
        """Search email messages."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/search")
        headers = self._build_auth_headers()

        params: dict[str, Any] = {"limit": limit, "offset": offset}
        if q is not None:
            params["q"] = q
        if direction is not None:
            params["direction"] = direction
        if from_address is not None:
            params["from_address"] = from_address
        if to_address is not None:
            params["to_address"] = to_address
        if since is not None:
            params["since"] = since
        if until is not None:
            params["until"] = until
        if is_read is not None:
            params["is_read"] = str(is_read).lower()
        if jacs_verified is not None:
            params["jacs_verified"] = str(jacs_verified).lower()
        if folder is not None:
            params["folder"] = folder
        if label is not None:
            params["label"] = label

        try:
            resp = await http.get(url, params=params, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email search auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email search failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            messages = data if isinstance(data, list) else data.get("messages", [])
            return [EmailMessage.from_dict(m) for m in messages]
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email search failed: {exc}")

    async def get_unread_count(self, hai_url: str) -> int:
        """Get the number of unread email messages."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/unread-count")
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email unread_count auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email unread_count failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return int(data.get("count", 0))
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email unread_count failed: {exc}")

    async def reply(
        self,
        hai_url: str,
        message_id: str,
        body: str,
        subject: Optional[str] = None,
    ) -> SendEmailResult:
        """Reply to an email message."""
        original = await self.get_message(hai_url, message_id)
        reply_subject = subject if subject is not None else f"Re: {original.subject}"
        return await self.send_email(
            hai_url,
            to=original.from_address,
            subject=reply_subject,
            body=body,
            in_reply_to=original.message_id,
        )

    async def forward(
        self,
        hai_url: str,
        message_id: str,
        to: str,
        comment: Optional[str] = None,
    ) -> SendEmailResult:
        """Forward an email message to another recipient."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/forward",
        )
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {"to": to}
        if comment is not None:
            payload["comment"] = comment

        try:
            resp = await http.post(url, json=payload, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email forward auth failed",
                    status_code=resp.status_code, body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email forward failed: HTTP {resp.status_code}",
                    status_code=resp.status_code, body=resp.text,
                )
            data = resp.json()
            return SendEmailResult(
                message_id=data.get("message_id", ""),
                status=data.get("status", "sent"),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email forward failed: {exc}")

    async def archive(self, hai_url: str, message_id: str) -> bool:
        """Archive an email message."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/archive",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.post(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email archive auth failed",
                    status_code=resp.status_code, body=resp.text,
                )
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(
                    f"Email archive failed: HTTP {resp.status_code}",
                    status_code=resp.status_code, body=resp.text,
                )
            return True
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email archive failed: {exc}")

    async def unarchive(self, hai_url: str, message_id: str) -> bool:
        """Unarchive an email message."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/unarchive",
        )
        headers = self._build_auth_headers()

        try:
            resp = await http.post(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email unarchive auth failed",
                    status_code=resp.status_code, body=resp.text,
                )
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(
                    f"Email unarchive failed: HTTP {resp.status_code}",
                    status_code=resp.status_code, body=resp.text,
                )
            return True
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email unarchive failed: {exc}")

    async def contacts(self, hai_url: str) -> list[Contact]:
        """List contacts derived from email history."""
        http = await self._get_http()
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/contacts")
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email contacts auth failed",
                    status_code=resp.status_code, body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email contacts failed: HTTP {resp.status_code}",
                    status_code=resp.status_code, body=resp.text,
                )
            data = resp.json()
            items = data if isinstance(data, list) else data.get("contacts", [])
            return [
                Contact(
                    email=c.get("email", ""),
                    display_name=c.get("display_name"),
                    last_contact=c.get("last_contact", ""),
                    jacs_verified=c.get("jacs_verified", False),
                    reputation_tier=c.get("reputation_tier"),
                )
                for c in items
            ]
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email contacts failed: {exc}")

    # ------------------------------------------------------------------
    # fetch_remote_key
    # ------------------------------------------------------------------

    async def fetch_remote_key(
        self, hai_url: str, jacs_id: str, version: str = "latest",
    ) -> PublicKeyInfo:
        """Fetch another agent's public key from HAI."""
        http = await self._get_http()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_version = self._escape_path_segment(version)
        url = self._make_url(hai_url, f"/jacs/v1/agents/{safe_jacs_id}/keys/{safe_version}")

        try:
            resp = await http.get(url)
            if resp.status_code == 404:
                raise HaiApiError(
                    f"No public key found for agent {jacs_id} version {version}",
                    status_code=404, body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Key lookup failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)

            warning = resp.headers.get("Warning")
            if warning:
                logger.warning("HAI key service: %s", warning)

            data = resp.json()
            return PublicKeyInfo(
                jacs_id=data.get("jacs_id", jacs_id),
                version=data.get("version", version),
                public_key=data.get("public_key", ""),
                public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""),
                public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""),
                dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    # ------------------------------------------------------------------
    # fetch_key_by_hash / fetch_key_by_email / fetch_key_by_domain / fetch_all_keys
    # ------------------------------------------------------------------

    async def fetch_key_by_hash(self, hai_url: str, public_key_hash: str) -> PublicKeyInfo:
        """Fetch an agent's public key by its SHA-256 hash."""
        http = await self._get_http()
        safe_hash = self._escape_path_segment(public_key_hash)
        url = self._make_url(hai_url, f"/jacs/v1/keys/by-hash/{safe_hash}")

        try:
            resp = await http.get(url)
            if resp.status_code == 404:
                raise HaiApiError(f"No key found for hash: {public_key_hash}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Key lookup failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""), version=data.get("version", ""),
                public_key=data.get("public_key", ""), public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""), public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""), dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    async def fetch_key_by_email(self, hai_url: str, email: str) -> PublicKeyInfo:
        """Fetch an agent's public key by their @hai.ai email address."""
        http = await self._get_http()
        safe_email = self._escape_path_segment(email)
        url = self._make_url(hai_url, f"/api/agents/keys/{safe_email}")

        try:
            resp = await http.get(url)
            if resp.status_code == 404:
                raise HaiApiError(f"No key found for email: {email}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Key lookup failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""), version=data.get("version", ""),
                public_key=data.get("public_key", ""), public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""), public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""), dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    async def fetch_key_by_domain(self, hai_url: str, domain: str) -> PublicKeyInfo:
        """Fetch the latest DNS-verified agent key for a domain."""
        http = await self._get_http()
        safe_domain = self._escape_path_segment(domain)
        url = self._make_url(hai_url, f"/jacs/v1/agents/by-domain/{safe_domain}")

        try:
            resp = await http.get(url)
            if resp.status_code == 404:
                raise HaiApiError(f"No verified agent for domain: {domain}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Key lookup failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            data = resp.json()
            return PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""), version=data.get("version", ""),
                public_key=data.get("public_key", ""), public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""), public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""), dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    async def fetch_all_keys(self, hai_url: str, jacs_id: str) -> dict:
        """Fetch all key versions for an agent."""
        http = await self._get_http()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/jacs/v1/agents/{safe_jacs_id}/keys")

        try:
            resp = await http.get(url)
            if resp.status_code == 404:
                raise HaiApiError(f"Agent not found: {jacs_id}", status_code=404, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Key history lookup failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key history lookup failed: {exc}")

    # ------------------------------------------------------------------
    # advanced verification endpoints
    # ------------------------------------------------------------------

    async def get_verification(
        self,
        hai_url: str,
        agent_id: str,
    ) -> dict[str, Any]:
        """Get advanced 3-level verification status for an agent."""
        http = await self._get_http()
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/verification")

        try:
            resp = await http.get(url, timeout=self._timeout)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Advanced verification failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Advanced verification failed: {exc}")

    async def verify_agent_document(
        self,
        hai_url: str,
        agent_json: str | dict[str, Any],
        *,
        public_key: str | None = None,
        domain: str | None = None,
    ) -> dict[str, Any]:
        """Verify an agent document via HAI's advanced verification endpoint."""
        http = await self._get_http()
        url = self._make_url(hai_url, "/api/v1/agents/verify")
        payload: dict[str, Any] = {
            "agent_json": agent_json if isinstance(agent_json, str) else json.dumps(agent_json),
        }
        if public_key is not None:
            payload["public_key"] = public_key
        if domain is not None:
            payload["domain"] = domain

        try:
            resp = await http.post(url, json=payload, timeout=self._timeout)
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Agent document verification failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Agent document verification failed: {exc}")

    # ------------------------------------------------------------------
    # connect (SSE async streaming)
    # ------------------------------------------------------------------

    async def connect(
        self, hai_url: str, *, transport: str = "sse",
    ) -> AsyncIterator[HaiEvent]:
        """Connect to HAI and yield events asynchronously."""
        if transport != "sse":
            raise ValueError(f"Async client only supports 'sse' transport, got '{transport}'")

        self._hai_url = hai_url
        self._should_disconnect = False
        self._connected = False

        url = self._make_url(hai_url, "/api/v1/agents/connect")
        headers = self._build_auth_headers()
        headers["Accept"] = "text/event-stream"

        http = await self._get_http()

        async with http.stream(
            "GET", url, headers=headers,
            timeout=httpx.Timeout(connect=10.0, read=90.0, write=10.0, pool=10.0),
        ) as response:
            if response.status_code in (401, 403):
                raise HaiAuthError(
                    f"Authentication failed: {response.status_code}",
                    status_code=response.status_code,
                )
            response.raise_for_status()
            self._connected = True

            buf: list[str] = []
            async for raw_line in response.aiter_lines():
                if self._should_disconnect:
                    break
                line = raw_line.rstrip("\n").rstrip("\r")
                if line == "":
                    parsed = parse_sse_lines(buf)
                    buf = []
                    if parsed is None:
                        continue
                    event_type, data_str = parsed
                    try:
                        data: Any = json.loads(data_str)
                    except json.JSONDecodeError:
                        data = data_str
                    if isinstance(data, dict) and is_signed_event(data):
                        payload, _ = unwrap_signed_event(
                            data, hai_url=self._hai_url,
                            verify=self._verify_server_signatures,
                        )
                        data = payload
                    if event_type == "benchmark_job" and isinstance(data, dict):
                        data = flatten_benchmark_job(data)
                    yield HaiEvent(event_type=event_type, data=data, raw=data_str)
                else:
                    buf.append(line)

        self._connected = False

    def disconnect(self) -> None:
        """Signal the SSE loop to stop."""
        self._should_disconnect = True
        self._connected = False

    @property
    def is_connected(self) -> bool:
        return self._connected
