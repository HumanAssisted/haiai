"""HaiClient -- full-featured client for the HAI benchmark platform.

Ports every public method from the JACS monolith (jacs.hai) with:
  - JACS-only authentication (no API key / Bearer fallback)
  - Local Ed25519 signing via jacs.hai.crypt (no PyO3 dependency)
  - SSE and WebSocket transports
  - Retry with exponential backoff
"""

from __future__ import annotations

import base64
import json
import logging
import os
import time
import webbrowser
from pathlib import Path
from typing import Any, Generator, Iterator, Optional, Union

import httpx

from jacs.hai._retry import RETRY_MAX_ATTEMPTS, backoff, should_retry
from jacs.hai._sse import flatten_benchmark_job, parse_sse_lines
from jacs.hai.crypt import canonicalize_json, create_agent_document, sign_string
from jacs.hai.errors import (
    BenchmarkError,
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RegistrationError,
    SSEError,
    WebSocketError,
)
from jacs.hai.models import (
    AgentConfig,
    AgentVerificationResult,
    BaselineRunResult,
    BenchmarkResult,
    EmailMessage,
    EmailStatus,
    FreeChaoticResult,
    HaiEvent,
    HaiRegistrationPreview,
    HaiRegistrationResult,
    HaiStatusResult,
    HelloWorldResult,
    JobResponseResult,
    PublicKeyInfo,
    RegistrationResult,
    SendEmailResult,
    TranscriptMessage,
)
from jacs.hai.signing import is_signed_event, sign_response, unwrap_signed_event

logger = logging.getLogger("jacs.hai.client")

# Verify link constants (HAI / public verification URLs)
MAX_VERIFY_URL_LEN = 2048
MAX_VERIFY_DOCUMENT_BYTES = 1515


# ---------------------------------------------------------------------------
# HaiClient
# ---------------------------------------------------------------------------


class HaiClient:
    """Client for the HAI benchmark platform.

    Handles JACS-signed authentication and event streaming over SSE or
    WebSocket.  All operations require a loaded JACS config (via
    ``jacs.hai.config.load()``).  There is **no API-key fallback**.

    Example::

        from jacs.hai import config, HaiClient

        config.load("./jacs.config.json")
        client = HaiClient()
        if client.testconnection("https://hai.ai"):
            result = client.hello_world("https://hai.ai")
            print(result.message)
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
        self._ws: Any = None
        self._sse_connection: Any = None
        self._hai_url: Optional[str] = None
        self._last_event_id: Optional[str] = None

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _make_url(base_url: str, path: str) -> str:
        """Construct a full URL from base and path."""
        base = base_url.rstrip("/")
        path = "/" + path.lstrip("/")
        return base + path

    def _get_jacs_id(self) -> str:
        """Return the loaded JACS ID, raising if not available."""
        from jacs.hai.config import get_config

        cfg = get_config()
        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required in config for JACS authentication")
        return cfg.jacs_id

    def _build_jacs_auth_header(self) -> str:
        """Build ``Authorization: JACS {jacsId}:{timestamp}:{signature}``.

        The signed message is ``"{jacsId}:{timestamp}"`` matching the Rust
        ``extract_jacs_credentials`` parser.
        """
        from jacs.hai.config import get_config, get_private_key

        cfg = get_config()
        key = get_private_key()

        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required for JACS authentication")

        timestamp = int(time.time())
        message = f"{cfg.jacs_id}:{timestamp}"
        signature = sign_string(key, message)
        return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"

    def _build_auth_headers(self) -> dict[str, str]:
        """Return auth headers using JACS signature authentication."""
        from jacs.hai.config import is_loaded, get_config

        if not (is_loaded() and get_config().jacs_id):
            raise HaiAuthError(
                "No JACS authentication available. "
                "Call jacs.hai.config.load() with a config containing jacsId."
            )
        return {"Authorization": self._build_jacs_auth_header()}

    @staticmethod
    def _parse_transcript(
        raw_messages: list[dict[str, Any]],
    ) -> list[TranscriptMessage]:
        """Parse raw transcript messages from API response."""
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

    def testconnection(self, hai_url: str) -> bool:
        """Test connectivity to the HAI server.

        Tries multiple health endpoints and returns True if any respond 2xx.

        Args:
            hai_url: Base URL of the HAI server.

        Returns:
            True if the server is reachable.
        """
        endpoints = ["/api/v1/health", "/health", "/api/health", "/"]

        for endpoint in endpoints:
            try:
                url = self._make_url(hai_url, endpoint)
                resp = httpx.get(
                    url,
                    timeout=min(self._timeout, 10.0),
                    follow_redirects=True,
                )
                if 200 <= resp.status_code < 300:
                    logger.info("Connection successful to %s", url)
                    return True
            except Exception as exc:
                logger.debug("Connection failed to %s: %s", endpoint, exc)
        logger.warning("All connection attempts to %s failed", hai_url)
        return False

    # ------------------------------------------------------------------
    # hello_world
    # ------------------------------------------------------------------

    def hello_world(
        self,
        hai_url: str,
        include_test: bool = False,
    ) -> HelloWorldResult:
        """Send a JACS-signed hello request to HAI and get a signed ACK.

        Args:
            hai_url: Base URL of the HAI server.
            include_test: If True, include a test scenario preview.

        Returns:
            HelloWorldResult with HAI's signed acknowledgment.

        Raises:
            HaiAuthError: If JACS config is not loaded.
            HaiApiError: On any non-2xx response.
        """
        url = self._make_url(hai_url, "/api/v1/agents/hello")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {}
        if include_test:
            payload["include_test"] = True

        try:
            resp = httpx.post(url, json=payload, headers=headers, timeout=self._timeout)
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")

        if resp.status_code in (401, 403):
            raise HaiAuthError(
                f"Hello auth failed: {resp.status_code}",
                status_code=resp.status_code,
                body=resp.text,
            )

        if resp.status_code == 429:
            raise HaiError(
                "Rate limited -- too many hello requests",
                status_code=429,
            )

        if resp.status_code not in (200, 201):
            raise HaiApiError(
                f"Hello request failed: {resp.status_code}",
                status_code=resp.status_code,
                body=resp.text,
            )

        data = resp.json()

        # Verify HAI's signature on the ACK
        # API returns: hai_signed_ack (not hai_ack_signature)
        #              hai_public_key_fingerprint (not hai_public_key)
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

    # ------------------------------------------------------------------
    # verify_hai_message
    # ------------------------------------------------------------------

    def verify_hai_message(
        self,
        message: str,
        signature: str,
        hai_public_key: str = "",
        hai_url: Optional[str] = None,
    ) -> bool:
        """Verify a message signed by HAI.

        Args:
            message: The message string that was signed.
            signature: Base64-encoded signature.
            hai_public_key: HAI's public key (PEM or base64).

        Returns:
            True if signature is valid.
        """
        if not signature or not message:
            return False

        if not hai_public_key:
            return False

        try:
            import base64

            from cryptography.hazmat.primitives.asymmetric.ed25519 import (
                Ed25519PublicKey,
            )
            from cryptography.hazmat.primitives.serialization import (
                load_pem_public_key,
            )

            try:
                sig_bytes = base64.b64decode(signature)
            except Exception:
                sig_bytes = signature.encode("utf-8")

            msg_bytes = message.encode("utf-8")

            pub_key: Any
            if hai_public_key.startswith("-----"):
                pub_key = load_pem_public_key(hai_public_key.encode("utf-8"))
            else:
                try:
                    key_bytes = base64.b64decode(hai_public_key)
                    pub_key = Ed25519PublicKey.from_public_bytes(key_bytes)
                except Exception:
                    # Treat non-key values as key IDs/fingerprints and look up
                    # active HAI signing keys from the server.
                    if not hai_url:
                        return False
                    from jacs.hai.signing import fetch_server_keys

                    keys = fetch_server_keys(hai_url)
                    match = next((k for k in keys if k.key_id == hai_public_key), None)
                    if match is None:
                        return False
                    pub_key = match.public_key

            pub_key.verify(sig_bytes, msg_bytes)  # type: ignore[union-attr]
            return True
        except Exception as exc:
            logger.debug("Ed25519 verification failed: %s", exc)
            return False

    # ------------------------------------------------------------------
    # register (existing agent)
    # ------------------------------------------------------------------

    def register(
        self,
        hai_url: str,
        agent_json: Optional[str] = None,
        public_key: Optional[str] = None,
        preview: bool = False,
        owner_email: Optional[str] = None,
    ) -> Union[HaiRegistrationResult, HaiRegistrationPreview]:
        """Register a JACS agent with HAI.

        This replaces the legacy ``jacs.simple.register_with_hai()`` from the
        JACS monolith.  Key differences: uses JACS-signature authentication
        (not API keys), auto-builds the agent document from config, and
        supports preview mode and retry.

        Sends ``POST /api/v1/agents/register`` with
        ``{agent_json, public_key}``.

        If *agent_json* is not provided, a self-signed agent document is
        built from the loaded config and keypair automatically.

        Args:
            hai_url: Base URL of the HAI server.
            agent_json: Signed JACS agent document as a JSON string.
            public_key: PEM-encoded public key (optional).
            preview: If True, return preview without actually registering.
            owner_email: Owner's email for linking agent to a HAI user.

        Returns:
            HaiRegistrationResult or HaiRegistrationPreview.

        Raises:
            RegistrationError: If registration fails.
            HaiAuthError: If auth fails.
        """
        from jacs.hai.config import get_config

        cfg = get_config()

        # Build agent_json from config if not provided
        if agent_json is None:
            from jacs.hai.config import get_private_key

            priv_key = get_private_key()
            from cryptography.hazmat.primitives.serialization import (
                Encoding,
                PublicFormat,
            )
            pub_pem = priv_key.public_key().public_bytes(
                Encoding.PEM, PublicFormat.SubjectPublicKeyInfo,
            ).decode()
            agent_doc = create_agent_document(
                name=cfg.name,
                version=cfg.version,
                public_key_pem=pub_pem,
                private_key=priv_key,
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
                resp = httpx.post(
                    url, json=payload, headers=headers, timeout=self._timeout,
                )

                if resp.status_code in (200, 201):
                    data = resp.json()
                    # RegisterAgentResponse fields:
                    # agent_id, jacs_id, jacs_version, registrations,
                    # dns_verified, registered_at, a2a_detected, a2a_skills_count
                    return HaiRegistrationResult(
                        success=True,
                        agent_id=data.get("agent_id", ""),
                        hai_signature="",
                        registration_id="",
                        registered_at=data.get("registered_at", ""),
                        capabilities=[],
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
                time.sleep(2**attempt)

        raise last_error or RegistrationError("Registration failed after all retries")

    # ------------------------------------------------------------------
    # status
    # ------------------------------------------------------------------

    def status(self, hai_url: str) -> HaiStatusResult:
        """Check registration/verification status of the current agent.

        Calls ``GET /api/v1/agents/{jacs_id}/verify``.

        Args:
            hai_url: Base URL of the HAI server.

        Returns:
            HaiStatusResult with verification details.
        """
        jacs_id = self._get_jacs_id()
        url = self._make_url(hai_url, f"/api/v1/agents/{jacs_id}/verify")
        headers = self._build_auth_headers()

        last_error: Optional[Exception] = None
        for attempt in range(self._max_retries):
            try:
                resp = httpx.get(url, headers=headers, timeout=self._timeout)

                if resp.status_code == 200:
                    data = resp.json()
                    # VerifyAgentResponse fields:
                    # jacs_id, registered, registrations, dns_verified, registered_at
                    registrations = data.get("registrations", [])
                    return HaiStatusResult(
                        registered=data.get("registered", True),
                        agent_id=data.get("jacs_id", jacs_id),
                        registration_id="",
                        registered_at=data.get("registered_at", ""),
                        hai_signatures=[
                            r.get("algorithm", "") for r in registrations
                        ],
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

                last_error = HaiError(
                    f"Status check failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                )

            except (httpx.ConnectError, httpx.TimeoutException) as exc:
                last_error = HaiConnectionError(f"Connection failed: {exc}")

            except HaiError:
                raise

            except Exception as exc:
                last_error = HaiError(f"Unexpected error: {exc}")

            if attempt < self._max_retries - 1:
                time.sleep(2**attempt)

        raise last_error or HaiError("Status check failed after all retries")

    # ------------------------------------------------------------------
    # get_agent_attestation
    # ------------------------------------------------------------------

    def get_agent_attestation(
        self,
        hai_url: str,
        agent_id: str,
    ) -> HaiStatusResult:
        """Get HAI attestation status for any agent by ID.

        Unlike ``status()`` which checks the current agent, this queries any
        agent by its JACS ID.

        Args:
            hai_url: Base URL of the HAI server.
            agent_id: JACS agent ID to check.

        Returns:
            HaiStatusResult with registration details.
        """
        url = self._make_url(hai_url, f"/api/v1/agents/{agent_id}/verify")
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)

            if resp.status_code == 200:
                data = resp.json()
                registrations = data.get("registrations", [])
                return HaiStatusResult(
                    registered=data.get("registered", True),
                    agent_id=data.get("jacs_id", agent_id),
                    registration_id="",
                    registered_at=data.get("registered_at", ""),
                    hai_signatures=[
                        r.get("algorithm", "") for r in registrations
                    ],
                    raw_response=data,
                )

            if resp.status_code == 404:
                return HaiStatusResult(
                    registered=False,
                    agent_id=agent_id,
                    raw_response=resp.json() if resp.text else {},
                )

            raise HaiApiError(
                f"Attestation check failed: HTTP {resp.status_code}",
                status_code=resp.status_code,
                body=resp.text,
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Failed to get attestation: {exc}")

    # ------------------------------------------------------------------
    # check_username / claim_username
    # ------------------------------------------------------------------

    def check_username(self, hai_url: str, username: str) -> dict[str, Any]:
        """Check if a username is available for @hai.ai email.

        ``GET /api/v1/agents/username/check?username={name}``

        Args:
            hai_url: Base URL of the HAI server.
            username: Desired username to check.

        Returns:
            Dict with ``available`` (bool), ``username`` (str), and
            optional ``reason`` (str).
        """
        url = self._make_url(hai_url, "/api/v1/agents/username/check")

        try:
            resp = httpx.get(
                url,
                params={"username": username},
                timeout=self._timeout,
            )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Username check failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            return resp.json()

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username check failed: {exc}")

    def claim_username(
        self, hai_url: str, agent_id: str, username: str
    ) -> dict[str, Any]:
        """Claim a username for an agent, getting ``{username}@hai.ai`` email.

        ``POST /api/v1/agents/{agent_id}/username`` with body
        ``{username}``.  Requires JACS auth.

        Args:
            hai_url: Base URL of the HAI server.
            agent_id: Agent ID to claim the username for.
            username: Desired username.

        Returns:
            Dict with ``username``, ``email``, and ``agent_id``.
        """
        url = self._make_url(hai_url, f"/api/v1/agents/{agent_id}/username")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        try:
            resp = httpx.post(
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

            return resp.json()

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username claim failed: {exc}")

    # ------------------------------------------------------------------
    # benchmark
    # ------------------------------------------------------------------

    def benchmark(
        self,
        hai_url: str,
        name: str = "mediator",
        tier: str = "free",
        timeout: Optional[float] = None,
    ) -> BenchmarkResult:
        """Run a benchmark via HAI.

        Sends ``POST /api/benchmark/run`` with ``{name, tier}``.

        Args:
            hai_url: Base URL of the HAI server.
            name: Benchmark scenario name (default: "mediator").
            tier: Benchmark tier: "free", "dns_certified", or "fully_certified".
            timeout: Optional timeout override for benchmark execution.

        Returns:
            BenchmarkResult with scores and detailed test results.
        """
        url = self._make_url(hai_url, "/api/benchmark/run")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload = {"name": name, "tier": tier}
        request_timeout = timeout or max(self._timeout, 120.0)

        try:
            resp = httpx.post(
                url, json=payload, headers=headers, timeout=request_timeout,
            )

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

            # Async job: poll for result
            job_id = data.get("job_id") or data.get("jobId")
            if job_id:
                return self._poll_benchmark_result(hai_url, job_id, request_timeout)

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

    def _poll_benchmark_result(
        self,
        hai_url: str,
        job_id: str,
        timeout: float,
    ) -> BenchmarkResult:
        """Poll for an async benchmark result."""
        url = self._make_url(hai_url, f"/api/benchmark/jobs/{job_id}")
        headers = self._build_auth_headers()

        start_time = time.time()
        poll_interval = 2.0

        while (time.time() - start_time) < timeout:
            try:
                resp = httpx.get(url, headers=headers, timeout=30.0)

                if resp.status_code != 200:
                    raise BenchmarkError(
                        f"Poll failed: HTTP {resp.status_code}",
                        status_code=resp.status_code,
                    )

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
                    raise BenchmarkError(
                        data.get("error", "Benchmark job failed"),
                        response_data=data,
                    )

                time.sleep(poll_interval)
                poll_interval = min(poll_interval * 1.5, 10.0)

            except HaiError:
                raise
            except Exception as exc:
                raise BenchmarkError(f"Failed to poll benchmark status: {exc}")

        raise BenchmarkError(f"Benchmark timed out after {timeout}s")

    # ------------------------------------------------------------------
    # free_run
    # ------------------------------------------------------------------

    def free_run(
        self,
        hai_url: str,
        transport: str = "sse",
    ) -> FreeChaoticResult:
        """Run a free benchmark.

        Connects to HAI and runs the canonical scenario with a cheap model.
        No judge evaluation, no scoring.  Returns the raw conversation
        transcript with structural annotations.

        Rate limited to 3 runs per JACS keypair per 24 hours.

        Args:
            hai_url: Base URL of the HAI server.
            transport: Transport protocol: "sse" (default) or "ws".

        Returns:
            FreeChaoticResult with transcript and annotations.
        """
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
            resp = httpx.post(
                url, json=payload, headers=headers, timeout=max(self._timeout, 120.0),
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

    # ------------------------------------------------------------------
    # dns_certified_run
    # ------------------------------------------------------------------

    def dns_certified_run(
        self,
        hai_url: str,
        transport: str = "sse",
        open_browser: bool = True,
        payment_poll_interval: float = 2.0,
        payment_poll_timeout: float = 300.0,
    ) -> BaselineRunResult:
        """Run a $5 DNS-certified benchmark.

        Flow:
        1. Creates a Stripe Checkout session via the API.
        2. Opens the checkout URL in the user's browser.
        3. Polls for payment confirmation.
        4. Runs the benchmark with quality models and judge evaluation.
        5. Returns the single aggregate score.

        Args:
            hai_url: Base URL of the HAI server.
            transport: Transport protocol: "sse" or "ws".
            open_browser: Whether to auto-open Stripe checkout.
            payment_poll_interval: Seconds between payment status checks.
            payment_poll_timeout: Max seconds to wait for payment.

        Returns:
            BaselineRunResult with score and transcript.
        """
        jacs_id = self._get_jacs_id()

        # Step 1: Create Stripe Checkout session
        purchase_url = self._make_url(hai_url, "/api/benchmark/purchase")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        purchase_payload = {"tier": "dns_certified", "agent_id": jacs_id}

        try:
            resp = httpx.post(
                purchase_url,
                json=purchase_payload,
                headers=headers,
                timeout=self._timeout,
            )
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Authentication failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise BenchmarkError(
                    f"Failed to create payment: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                )

            purchase_data = resp.json()
            checkout_url = purchase_data.get("checkout_url", "")
            payment_id = purchase_data.get("payment_id", "")

            if not checkout_url:
                raise BenchmarkError("No checkout URL returned from API")

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise BenchmarkError(f"Failed to create payment: {exc}")

        # Step 2: Open browser for payment
        if open_browser:
            webbrowser.open(checkout_url)

        # Step 3: Poll for payment confirmation
        payment_status_url = self._make_url(
            hai_url, f"/api/benchmark/payments/{payment_id}/status"
        )
        start_time = time.time()

        while (time.time() - start_time) < payment_poll_timeout:
            try:
                status_resp = httpx.get(
                    payment_status_url, headers=headers, timeout=self._timeout,
                )
                if status_resp.status_code == 200:
                    status_data = status_resp.json()
                    payment_status = status_data.get("status", "")

                    if payment_status == "paid":
                        break
                    if payment_status in ("failed", "expired", "cancelled"):
                        raise BenchmarkError(
                            f"Payment {payment_status}: "
                            f"{status_data.get('message', '')}"
                        )
            except HaiError:
                raise
            except Exception as exc:
                logger.debug("Payment poll error: %s", exc)

            time.sleep(payment_poll_interval)
        else:
            raise BenchmarkError(
                f"Payment not confirmed within {payment_poll_timeout}s. "
                "Complete payment in your browser and retry."
            )

        # Step 4: Run the benchmark
        run_url = self._make_url(hai_url, "/api/benchmark/run")
        # Refresh auth headers with fresh timestamp
        run_headers = self._build_auth_headers()
        run_headers["Content-Type"] = "application/json"

        run_payload: dict[str, Any] = {
            "name": f"DNS Certified Run - {jacs_id[:8]}",
            "tier": "dns_certified",
            "payment_id": payment_id,
            "transport": transport,
        }

        try:
            run_resp = httpx.post(
                run_url,
                json=run_payload,
                headers=run_headers,
                timeout=max(self._timeout, 300.0),
            )

            if run_resp.status_code not in (200, 201):
                raise BenchmarkError(
                    f"Baseline run failed: HTTP {run_resp.status_code}",
                    status_code=run_resp.status_code,
                )

            data = run_resp.json()
            transcript = self._parse_transcript(data.get("transcript", []))
            score = float(data.get("score", 0.0))

            return BaselineRunResult(
                success=True,
                run_id=data.get("run_id", data.get("runId", "")),
                score=score,
                transcript=transcript,
                payment_id=payment_id,
                raw_response=data,
            )

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise BenchmarkError(f"Baseline run failed: {exc}")

    # ------------------------------------------------------------------
    # certified_run
    # ------------------------------------------------------------------

    def certified_run(self, **kwargs: Any) -> None:
        """Run a fully_certified tier benchmark.

        The fully_certified tier ($499/month) is coming soon.
        Contact support@hai.ai for early access.
        """
        raise NotImplementedError(
            "The fully_certified tier ($499/month) is coming soon. "
            "Contact support@hai.ai for early access."
        )

    # ------------------------------------------------------------------
    # submit_benchmark_response
    # ------------------------------------------------------------------

    def submit_benchmark_response(
        self,
        hai_url: str,
        job_id: str,
        message: str,
        metadata: Optional[dict[str, Any]] = None,
        processing_time_ms: int = 0,
    ) -> JobResponseResult:
        """Submit a benchmark job response.

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
        from jacs.hai.config import get_config, get_private_key

        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        response_body: dict[str, Any] = {"message": message}
        if metadata is not None:
            response_body["metadata"] = metadata
        response_body["processing_time_ms"] = processing_time_ms

        job_response_payload = {"response": response_body}

        # Always wrap as signed JACS document
        cfg = get_config()
        payload: dict[str, Any] = sign_response(
            job_response_payload, get_private_key(), cfg.jacs_id or "",
        )

        url = self._make_url(hai_url, f"/api/v1/agents/jobs/{job_id}/response")

        last_exc: Optional[Exception] = None
        for attempt in range(RETRY_MAX_ATTEMPTS):
            try:
                resp = httpx.post(url, json=payload, headers=headers, timeout=30.0)

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

                if should_retry(resp.status_code):
                    delay = backoff(attempt)
                    logger.warning(
                        "submit_benchmark_response got %d, retrying in %.1fs",
                        resp.status_code,
                        delay,
                    )
                    time.sleep(delay)
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

            except httpx.HTTPStatusError as exc:
                raise HaiApiError(
                    f"Failed to submit response: {exc.response.status_code}",
                    status_code=exc.response.status_code,
                    body=exc.response.text,
                ) from exc
            except (httpx.ConnectError, httpx.ReadTimeout) as exc:
                last_exc = exc
                delay = backoff(attempt)
                logger.warning(
                    "submit_benchmark_response connection error (%s), retrying",
                    exc,
                )
                time.sleep(delay)
                headers = self._build_auth_headers()
                headers["Content-Type"] = "application/json"
                continue

        raise HaiConnectionError(
            f"Failed to submit response after {RETRY_MAX_ATTEMPTS} attempts"
        ) from last_exc

    # ------------------------------------------------------------------
    # sign_benchmark_result
    # ------------------------------------------------------------------

    def sign_benchmark_result(
        self,
        run_id: str,
        score: Optional[float] = None,
        tier: str = "",
        transcript: Optional[list[dict[str, Any]]] = None,
        metadata: Optional[dict[str, Any]] = None,
    ) -> dict[str, str]:
        """Sign a benchmark result for independent verification.

        Creates a JACS-signed document containing the benchmark result.

        Args:
            run_id: The benchmark run ID from HAI.
            score: The benchmark score (0-100), if available.
            tier: Benchmark tier ("free", "dns_certified", "fully_certified").
            transcript: Optional transcript messages to include.
            metadata: Optional additional metadata.

        Returns:
            Dict with ``signed_document`` (JSON string) and ``agent_jacs_id``.
        """
        from jacs.hai.config import get_config, get_private_key

        cfg = get_config()
        payload: dict[str, Any] = {
            "type": "benchmark_result",
            "run_id": run_id,
            "tier": tier,
            "agent_id": cfg.jacs_id or "",
            "signed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }

        if score is not None:
            payload["score"] = score
        if transcript is not None:
            payload["transcript"] = transcript
        if metadata is not None:
            payload["metadata"] = metadata

        return sign_response(payload, get_private_key(), cfg.jacs_id or "")

    # ------------------------------------------------------------------
    # Email CRUD
    # ------------------------------------------------------------------

    def send_email(
        self,
        hai_url: str,
        to: str,
        subject: str,
        body: str,
        in_reply_to: Optional[str] = None,
    ) -> SendEmailResult:
        """Send an email from this agent's @hai.ai address.

        Args:
            hai_url: Base URL of the HAI server.
            to: Recipient address (must be @hai.ai for MVP).
            subject: Email subject line.
            body: Plain text email body.
            in_reply_to: Optional Message-ID for threading.

        Returns:
            SendEmailResult with message_id and status.
        """
        jacs_id = self._get_jacs_id()
        url = self._make_url(hai_url, f"/api/agents/{jacs_id}/email/send")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            payload["in_reply_to"] = in_reply_to

        try:
            resp = httpx.post(url, json=payload, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email send auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code == 429:
                raise HaiApiError(
                    f"Email rate limited: {resp.text}",
                    status_code=429,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email send failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            return SendEmailResult(
                message_id=data.get("message_id", ""),
                status=data.get("status", "sent"),
            )

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email send failed: {exc}")

    def list_messages(
        self,
        hai_url: str,
        limit: int = 20,
        offset: int = 0,
        folder: str = "inbox",
    ) -> list[EmailMessage]:
        """List email messages for this agent.

        Args:
            hai_url: Base URL of the HAI server.
            limit: Max messages to return.
            offset: Pagination offset.
            folder: Folder to list ("inbox", "sent").

        Returns:
            List of EmailMessage objects.
        """
        jacs_id = self._get_jacs_id()
        url = self._make_url(hai_url, f"/api/agents/{jacs_id}/email/messages")
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(
                url,
                params={"limit": limit, "offset": offset, "folder": folder},
                headers=headers,
                timeout=self._timeout,
            )

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email list auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email list failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            messages = data if isinstance(data, list) else data.get("messages", [])
            return [
                EmailMessage(
                    id=m.get("id", ""),
                    from_address=m.get("from_address", m.get("from", "")),
                    to_address=m.get("to_address", m.get("to", "")),
                    subject=m.get("subject", ""),
                    body=m.get("body", ""),
                    sent_at=m.get("sent_at", ""),
                    read_at=m.get("read_at"),
                    thread_id=m.get("thread_id"),
                )
                for m in messages
            ]

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email list failed: {exc}")

    def mark_read(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as read.

        Args:
            hai_url: Base URL of the HAI server.
            message_id: ID of the message to mark as read.

        Returns:
            True if successful.
        """
        jacs_id = self._get_jacs_id()
        url = self._make_url(
            hai_url,
            f"/api/agents/{jacs_id}/email/messages/{message_id}/read",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.post(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email mark_read auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(
                    f"Email mark_read failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return True

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email mark_read failed: {exc}")

    def get_email_status(self, hai_url: str) -> EmailStatus:
        """Get email rate-limit and reputation status.

        Args:
            hai_url: Base URL of the HAI server.

        Returns:
            EmailStatus with daily limits and tier info.
        """
        jacs_id = self._get_jacs_id()
        url = self._make_url(hai_url, f"/api/agents/{jacs_id}/email/status")
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email status auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email status failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            return EmailStatus(
                daily_limit=int(data.get("daily_limit", 0)),
                daily_used=int(data.get("daily_used", 0)),
                resets_at=data.get("resets_at", ""),
                reputation_tier=data.get("reputation_tier", ""),
                current_tier=data.get("current_tier", ""),
            )

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email status failed: {exc}")

    # ------------------------------------------------------------------
    # fetch_remote_key
    # ------------------------------------------------------------------

    def fetch_remote_key(
        self,
        hai_url: str,
        jacs_id: str,
        version: str = "latest",
    ) -> PublicKeyInfo:
        """Fetch another agent's public key from HAI.

        Args:
            hai_url: Base URL of the HAI server.
            jacs_id: The target agent's JACS ID.
            version: Key version ("latest" or a specific version string).

        Returns:
            PublicKeyInfo with the agent's public key and metadata.

        Raises:
            HaiApiError: If the agent or key is not found (404).
        """
        url = self._make_url(
            hai_url, f"/jacs/v1/agents/{jacs_id}/keys/{version}"
        )

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code == 404:
                raise HaiApiError(
                    f"No public key found for agent {jacs_id} version {version}",
                    status_code=404,
                    body=resp.text,
                )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Key lookup failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

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

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    # ------------------------------------------------------------------
    # connect (SSE + WS)
    # ------------------------------------------------------------------

    def connect(
        self,
        hai_url: str,
        *,
        transport: str = "sse",
    ) -> Iterator[HaiEvent]:
        """Connect to HAI and yield events.

        Args:
            hai_url: Base URL of the HAI server.
            transport: ``"sse"`` or ``"ws"``.

        Yields:
            HaiEvent instances.
        """
        if transport not in ("sse", "ws"):
            raise ValueError(f"transport must be 'sse' or 'ws', got '{transport}'")

        self._hai_url = hai_url
        self._should_disconnect = False
        self._connected = False

        if transport == "ws":
            yield from self._connect_ws(hai_url)
        else:
            yield from self._connect_sse(hai_url)

    def _connect_sse(self, hai_url: str) -> Iterator[HaiEvent]:
        """Stream events from ``GET /api/v1/agents/connect`` via SSE."""
        url = self._make_url(hai_url, "/api/v1/agents/connect")
        headers = self._build_auth_headers()
        headers["Accept"] = "text/event-stream"

        attempt = 0
        while not self._should_disconnect:
            try:
                if self._last_event_id:
                    headers["Last-Event-ID"] = self._last_event_id

                with httpx.stream(
                    "GET",
                    url,
                    headers=headers,
                    timeout=httpx.Timeout(
                        connect=10.0, read=90.0, write=10.0, pool=10.0
                    ),
                ) as response:
                    if response.status_code in (401, 403):
                        raise HaiAuthError(
                            f"Authentication failed: {response.status_code}",
                            status_code=response.status_code,
                        )
                    if (
                        should_retry(response.status_code)
                        and attempt < RETRY_MAX_ATTEMPTS
                    ):
                        delay = backoff(attempt)
                        logger.warning(
                            "SSE connect got %d, retrying in %.1fs",
                            response.status_code,
                            delay,
                        )
                        time.sleep(delay)
                        attempt += 1
                        continue
                    response.raise_for_status()

                    self._connected = True
                    self._sse_connection = response
                    attempt = 0

                    buf: list[str] = []
                    for raw_line in response.iter_lines():
                        if self._should_disconnect:
                            break
                        line = raw_line.rstrip("\n").rstrip("\r")

                        if line == "":
                            parsed = parse_sse_lines(buf)
                            buf = []
                            if parsed is None:
                                continue
                            event_type, data_str = parsed
                            event = self._make_event(event_type, data_str)
                            if event is not None:
                                yield event
                        else:
                            buf.append(line)

            except (
                httpx.ReadTimeout,
                httpx.RemoteProtocolError,
                httpx.ReadError,
            ) as exc:
                self._connected = False
                if self._should_disconnect:
                    break
                if not self._connected and attempt == 0:
                    raise HaiConnectionError(f"SSE connection failed: {exc}") from exc
                if attempt >= RETRY_MAX_ATTEMPTS:
                    raise HaiConnectionError(
                        f"SSE connection lost after {RETRY_MAX_ATTEMPTS} retries"
                    ) from exc
                delay = backoff(attempt)
                logger.warning(
                    "SSE connection lost (%s), reconnecting in %.1fs",
                    exc,
                    delay,
                )
                time.sleep(delay)
                attempt += 1
                headers = self._build_auth_headers()
                headers["Accept"] = "text/event-stream"
                continue
            except httpx.HTTPStatusError as exc:
                self._connected = False
                raise HaiApiError(
                    f"SSE connect failed: {exc.response.status_code}",
                    status_code=exc.response.status_code,
                    body=exc.response.text,
                ) from exc

            break

        self._connected = False

    def _connect_ws(self, hai_url: str) -> Iterator[HaiEvent]:
        """Stream events via ``/ws/agent/connect``."""
        import websockets.sync.client as ws_sync

        base = hai_url.rstrip("/")
        if base.startswith("https://"):
            ws_url = "wss://" + base[len("https://"):]
        elif base.startswith("http://"):
            ws_url = "ws://" + base[len("http://"):]
        else:
            ws_url = base
        ws_url += "/ws/agent/connect"

        headers = self._build_auth_headers()

        attempt = 0
        while not self._should_disconnect:
            try:
                with ws_sync.connect(
                    ws_url, additional_headers=headers, close_timeout=5,
                ) as ws:
                    self._ws = ws
                    self._connected = True
                    attempt = 0

                    for raw_msg in ws:
                        if self._should_disconnect:
                            break
                        if isinstance(raw_msg, bytes):
                            raw_msg = raw_msg.decode("utf-8", errors="replace")

                        try:
                            data = json.loads(raw_msg)
                        except json.JSONDecodeError:
                            logger.warning("Non-JSON WS message: %s", raw_msg[:200])
                            continue

                        event = self._make_event_from_ws(data)
                        if event is not None:
                            yield event

            except Exception as exc:
                self._connected = False
                if self._should_disconnect:
                    break
                if attempt >= RETRY_MAX_ATTEMPTS:
                    raise HaiConnectionError(
                        f"WebSocket lost after {RETRY_MAX_ATTEMPTS} retries: {exc}"
                    ) from exc
                delay = backoff(attempt)
                logger.warning(
                    "WebSocket lost (%s), reconnecting in %.1fs",
                    exc,
                    delay,
                )
                time.sleep(delay)
                attempt += 1
                headers = self._build_auth_headers()
                continue

            break

        self._connected = False

    # ------------------------------------------------------------------
    # Event construction helpers
    # ------------------------------------------------------------------

    def _unwrap_if_signed(self, data: dict[str, Any]) -> dict[str, Any]:
        """Unwrap JACS-signed envelope if present, optionally verifying."""
        if is_signed_event(data):
            payload, verified = unwrap_signed_event(
                data,
                hai_url=self._hai_url,
                verify=self._verify_server_signatures,
            )
            if self._verify_server_signatures and not verified:
                logger.warning("Server signature verification failed")
            return payload
        return data

    def _make_event(
        self, event_type: str, data_str: str
    ) -> Optional[HaiEvent]:
        """Build HaiEvent from an SSE (event_type, data_string) pair."""
        try:
            data: Any = json.loads(data_str)
        except json.JSONDecodeError:
            data = data_str

        if isinstance(data, dict):
            data = self._unwrap_if_signed(data)

        if event_type == "benchmark_job" and isinstance(data, dict):
            data = flatten_benchmark_job(data)

        return HaiEvent(event_type=event_type, data=data, raw=data_str)

    def _make_event_from_ws(self, data: dict[str, Any]) -> Optional[HaiEvent]:
        """Build HaiEvent from a parsed WebSocket JSON message."""
        data = self._unwrap_if_signed(data)
        event_type = data.get("type", "unknown")

        if event_type == "benchmark_job":
            return HaiEvent(
                event_type="benchmark_job",
                data=flatten_benchmark_job(data),
            )
        if event_type == "disconnect":
            return HaiEvent(
                event_type="disconnect",
                data=data.get("reason", ""),
            )

        return HaiEvent(event_type=event_type, data=data)

    # ------------------------------------------------------------------
    # disconnect
    # ------------------------------------------------------------------

    def disconnect(self) -> None:
        """Disconnect from the HAI server."""
        self._should_disconnect = True
        self._connected = False

        if self._sse_connection is not None:
            try:
                self._sse_connection.close()
            except Exception:
                pass
            self._sse_connection = None

        if self._ws is not None:
            try:
                self._ws.close()
            except Exception:
                pass
            self._ws = None

    @property
    def is_connected(self) -> bool:
        """Return True if currently connected."""
        return self._connected


# ---------------------------------------------------------------------------
# Module-level convenience functions
# ---------------------------------------------------------------------------

_client: Optional[HaiClient] = None


def _get_client() -> HaiClient:
    """Get or create the global HaiClient singleton."""
    global _client
    if _client is None:
        _client = HaiClient()
    return _client


def testconnection(hai_url: str) -> bool:
    """Test connectivity to the HAI server."""
    return _get_client().testconnection(hai_url)


def hello_world(hai_url: str, include_test: bool = False) -> HelloWorldResult:
    """Perform a hello world exchange with HAI."""
    return _get_client().hello_world(hai_url, include_test)


def register(
    hai_url: str,
    preview: bool = False,
    owner_email: Optional[str] = None,
) -> Union[HaiRegistrationResult, HaiRegistrationPreview]:
    """Register the loaded JACS agent with HAI."""
    return _get_client().register(hai_url, preview=preview, owner_email=owner_email)


def status(hai_url: str) -> HaiStatusResult:
    """Check registration status of the current agent."""
    return _get_client().status(hai_url)


def check_username(hai_url: str, username: str) -> dict[str, Any]:
    """Check if a username is available for @hai.ai email."""
    return _get_client().check_username(hai_url, username)


def claim_username(hai_url: str, agent_id: str, username: str) -> dict[str, Any]:
    """Claim a username for an agent."""
    return _get_client().claim_username(hai_url, agent_id, username)


def benchmark(
    hai_url: str,
    name: str = "mediator",
    tier: str = "free",
) -> BenchmarkResult:
    """Run a benchmark via HAI."""
    return _get_client().benchmark(hai_url, name=name, tier=tier)


def free_run(
    hai_url: str, transport: str = "sse"
) -> FreeChaoticResult:
    """Run a free benchmark."""
    return _get_client().free_run(hai_url, transport)


def dns_certified_run(
    hai_url: str, transport: str = "sse", open_browser: bool = True
) -> BaselineRunResult:
    """Run a $5 DNS-certified benchmark."""
    return _get_client().dns_certified_run(hai_url, transport, open_browser)


def certified_run(**kwargs: Any) -> None:
    """Run a fully_certified tier benchmark.

    The fully_certified tier ($499/month) is coming soon.
    Contact support@hai.ai for early access.
    """
    raise NotImplementedError(
        "The fully_certified tier ($499/month) is coming soon. "
        "Contact support@hai.ai for early access."
    )


def submit_benchmark_response(
    hai_url: str,
    job_id: str,
    message: str,
    metadata: Optional[dict[str, Any]] = None,
    processing_time_ms: int = 0,
) -> JobResponseResult:
    """Submit a benchmark job response."""
    return _get_client().submit_benchmark_response(
        hai_url, job_id, message, metadata, processing_time_ms,
    )


def sign_benchmark_result(
    run_id: str,
    score: Optional[float] = None,
    tier: str = "",
    transcript: Optional[list[dict[str, Any]]] = None,
    metadata: Optional[dict[str, Any]] = None,
) -> dict[str, str]:
    """Sign a benchmark result for independent verification."""
    return _get_client().sign_benchmark_result(
        run_id, score, tier, transcript, metadata,
    )


def connect(
    hai_url: str,
    *,
    transport: str = "sse",
) -> Iterator[HaiEvent]:
    """Connect to HAI event stream."""
    return _get_client().connect(hai_url, transport=transport)


def disconnect() -> None:
    """Disconnect from the HAI event stream."""
    _get_client().disconnect()


def send_email(
    hai_url: str,
    to: str,
    subject: str,
    body: str,
    in_reply_to: Optional[str] = None,
) -> SendEmailResult:
    """Send an email from this agent's @hai.ai address."""
    return _get_client().send_email(hai_url, to, subject, body, in_reply_to)


def list_messages(
    hai_url: str,
    limit: int = 20,
    offset: int = 0,
    folder: str = "inbox",
) -> list[EmailMessage]:
    """List email messages for this agent."""
    return _get_client().list_messages(hai_url, limit, offset, folder)


def mark_read(hai_url: str, message_id: str) -> bool:
    """Mark an email message as read."""
    return _get_client().mark_read(hai_url, message_id)


def get_email_status(hai_url: str) -> EmailStatus:
    """Get email rate-limit and reputation status."""
    return _get_client().get_email_status(hai_url)


def fetch_remote_key(
    hai_url: str,
    jacs_id: str,
    version: str = "latest",
) -> PublicKeyInfo:
    """Fetch another agent's public key from HAI."""
    return _get_client().fetch_remote_key(hai_url, jacs_id, version)


# ---------------------------------------------------------------------------
# generate_verify_link
# ---------------------------------------------------------------------------


def generate_verify_link(
    document: str,
    base_url: str = "https://hai.ai",
    hosted: Optional[bool] = None,
) -> str:
    """Build a verification URL for a signed JACS document.

    Supports two modes:

    - **Inline** (default): Encodes the full document in the URL as base64:
      ``{base_url}/jacs/verify?s={base64url(document)}``
    - **Hosted** (opt-in): Uses the document ID to reference a server-stored
      copy: ``{base_url}/verify/{document_id}``

    Args:
        document: The full signed JACS document string (JSON).
        base_url: Base URL of the verifier (no trailing slash).
            Default ``"https://hai.ai"``.
        hosted: Force hosted mode (``True``) or inline mode (``False``).
            ``None`` defaults to inline mode.

    Returns:
        Full verification URL.

    Raises:
        ValueError: If inline mode is used but the document exceeds URL
            limits, or if hosted mode is used but no document ID is found.
    """
    base = base_url.rstrip("/")

    if hosted is None:
        hosted = False

    if not hosted:
        encoded = base64.urlsafe_b64encode(
            document.encode("utf-8")
        ).rstrip(b"=").decode("ascii")
        path_and_query = f"/jacs/verify?s={encoded}"
        full_url = f"{base}{path_and_query}"
        if len(full_url) > MAX_VERIFY_URL_LEN:
            raise ValueError(
                f"Verify URL would exceed max length ({MAX_VERIFY_URL_LEN}). "
                f"Document must be at most {MAX_VERIFY_DOCUMENT_BYTES} UTF-8 bytes. "
                f"Use hosted=True for large documents (e.g. post-quantum signatures)."
            )
        return full_url
    else:
        try:
            doc_data = json.loads(document)
            doc_id = (
                doc_data.get("jacsDocumentId")
                or doc_data.get("document_id")
                or doc_data.get("id")
                or ""
            )
        except (json.JSONDecodeError, TypeError):
            doc_id = ""

        if not doc_id:
            raise ValueError(
                "Cannot generate hosted verify link: no document ID found in document. "
                "Document must contain 'jacsDocumentId', 'document_id', or 'id' field."
            )

        return f"{base}/verify/{doc_id}"


# ---------------------------------------------------------------------------
# register_new_agent (standalone bootstrapper)
# ---------------------------------------------------------------------------


def register_new_agent(
    name: str,
    owner_email: str,
    version: str = "1.0.0",
    hai_url: str = "https://hai.ai",
    key_dir: Optional[str] = None,
    config_path: str = "./jacs.config.json",
    domain: Optional[str] = None,
    description: Optional[str] = None,
    quiet: bool = False,
) -> RegistrationResult:
    """Generate a keypair, self-sign, register with HAI, and save config.

    This is the one-call setup for a new agent.  It:
    1. Generates an Ed25519 keypair and writes PEM files to *key_dir*.
    2. Creates a self-signed JACS agent document.
    3. POSTs the document to ``/api/v1/agents/register``.
    4. Saves ``jacs.config.json`` with the returned ``jacsId``.
    5. Loads the config so the SDK is immediately usable.

    Args:
        name: Agent display name (ASCII-only).
        owner_email: Owner's email for linking agent to a HAI user account.
        version: Agent version string.
        hai_url: HAI server base URL.
        key_dir: Directory to write key files into. Defaults to ``~/.jacs/keys``.
        config_path: Path for the generated ``jacs.config.json``.
        domain: Optional domain for DNS verification.
        description: Optional agent description.
        quiet: Suppress post-registration messaging.

    Returns:
        RegistrationResult with ``agent_id``, ``jacs_id``.

    Raises:
        ValueError: If *owner_email* is empty.
    """
    if not owner_email:
        raise ValueError(
            "owner_email is required -- agents must be associated with a verified HAI user"
        )
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import (
        Encoding,
        NoEncryption,
        PrivateFormat,
        PublicFormat,
    )

    from jacs.hai import config as hai_config

    # 1. Generate keypair
    private_key = Ed25519PrivateKey.generate()
    public_key = private_key.public_key()

    kd = Path(key_dir).expanduser() if key_dir else (Path.home() / ".jacs" / "keys")
    kd.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        kd.chmod(0o700)
    except OSError:
        pass

    private_key_path = kd / "agent_private_key.pem"
    private_key_path.write_bytes(
        private_key.private_bytes(Encoding.PEM, PrivateFormat.PKCS8, NoEncryption())
    )
    # Keep private key permissions restrictive even if umask is permissive.
    try:
        private_key_path.chmod(0o600)
    except OSError:
        pass
    public_pem = public_key.public_bytes(
        Encoding.PEM, PublicFormat.SubjectPublicKeyInfo
    ).decode()
    (kd / "agent_public_key.pem").write_text(public_pem)

    # 2. Self-sign agent document
    agent_doc = create_agent_document(
        name=name,
        version=version,
        public_key_pem=public_pem,
        private_key=private_key,
    )
    agent_doc["description"] = description or "Agent registered via Python SDK"
    if domain:
        agent_doc["domain"] = domain
    agent_json_str = json.dumps(agent_doc, indent=2)

    # 3. Register with HAI (no API key -- the self-signed doc is the auth)
    url = f"{hai_url.rstrip('/')}/api/v1/agents/register"
    payload = {
        "agent_json": agent_json_str,
        "public_key": base64.b64encode(public_pem.encode("utf-8")).decode("utf-8"),
        "owner_email": owner_email,
    }
    if domain:
        payload["domain"] = domain

    resp = httpx.post(
        url, json=payload, headers={"Content-Type": "application/json"}, timeout=30.0,
    )
    if resp.status_code in (401, 403):
        raise HaiAuthError(
            f"Registration auth failed: {resp.status_code}",
            status_code=resp.status_code,
            body=resp.text,
        )
    resp.raise_for_status()

    data = resp.json()
    agent_id = str(data.get("agent_id", ""))
    jacs_id = str(data.get("jacs_id", agent_doc.get("jacsId", "")))

    # 4. Save config and load it
    config_data = {
        "jacsAgentName": name,
        "jacsAgentVersion": version,
        "jacsKeyDir": str(kd.resolve()),
        "jacsId": jacs_id,
    }
    p = Path(config_path)
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "w") as f:
        json.dump(config_data, f, indent=2)
        f.write("\n")

    # 5. Load into module state and reset singleton
    hai_config.load(config_path)
    global _client
    _client = None

    # 6. Print next-step messaging
    if not quiet:
        print(f"\nAgent created and submitted for registration!")
        print(f"  -> Check your email ({owner_email}) for a verification link")
        print(f"  -> Click the link and log into hai.ai to complete registration")
        print(f"  -> After verification, claim a @hai.ai username with:")
        print(f"     client.claim_username('my-agent')")
        print(f"  -> Config saved to {config_path}")
        print(f"  -> Keys saved to {kd}")

        if domain:
            key_hash = _compute_public_key_hash(public_pem)
            print(f"\n--- DNS Setup Instructions ---")
            print(f"Add this TXT record to your domain '{domain}':")
            print(f"  Name:  _jacs.{domain}")
            print(f"  Type:  TXT")
            print(f"  Value: {key_hash}")
            print(f"DNS verification enables the dns_certified tier.\n")
        else:
            print()

    return RegistrationResult(agent_id=agent_id, jacs_id=jacs_id)


def _compute_public_key_hash(pem: str) -> str:
    """Compute SHA-256 hash of a PEM public key, matching Rust API format."""
    import hashlib
    digest = hashlib.sha256(pem.encode("utf-8")).hexdigest()
    return f"sha256:{digest}"


def _verify_dns(domain: str, public_key_pem: str) -> tuple[bool, str]:
    """Verify DNS TXT record for Level 2 domain verification.

    Returns:
        (valid, message) tuple.
    """
    try:
        import dns.resolver
    except ImportError:
        return False, "dnspython not installed (pip install jacs[dns])"

    expected_hash = _compute_public_key_hash(public_key_pem)
    record_name = f"_jacs.{domain}"

    try:
        answers = dns.resolver.resolve(record_name, "TXT")
        for rdata in answers:
            txt_value = rdata.to_text().strip('"')
            if txt_value == expected_hash:
                return True, f"DNS TXT record matches at {record_name}"
        return False, f"DNS TXT record found at {record_name} but no matching hash"
    except dns.resolver.NXDOMAIN:
        return False, f"No DNS record found at {record_name}"
    except dns.resolver.NoAnswer:
        return False, f"No TXT record at {record_name}"
    except dns.exception.Timeout:
        return False, f"DNS lookup timed out for {record_name}"
    except Exception as e:
        return False, f"DNS lookup failed: {e}"


# ---------------------------------------------------------------------------
# verify_agent (standalone)
# ---------------------------------------------------------------------------


def verify_agent(
    agent_document: Union[str, dict],
    min_level: int = 1,
    require_domain: Optional[str] = None,
    hai_url: str = "https://hai.ai",
) -> AgentVerificationResult:
    """Verify another agent's trust level.

    Verification Levels:
        - Level 1 (basic): JACS self-signature valid.
        - Level 2 (domain): DNS TXT record verification passed.
        - Level 3 (attested): HAI has registered and signed the agent.

    Args:
        agent_document: The agent's JACS document (JSON string or dict).
        min_level: Minimum required verification level (1, 2, or 3).
        require_domain: If specified, require agent to be verified for this domain.
        hai_url: HAI server URL.

    Returns:
        AgentVerificationResult with verification status at all levels.
    """
    from jacs.hai.crypt import verify_string as _verify_string

    errors: list[str] = []
    agent_id = ""
    jacs_valid = False
    dns_valid = False
    hai_attested = False
    domain = ""
    hai_signatures: list[str] = []
    raw_response: dict[str, Any] = {}

    # Convert to dict
    if isinstance(agent_document, str):
        try:
            doc = json.loads(agent_document)
        except json.JSONDecodeError as exc:
            errors.append(f"Invalid JSON: {exc}")
            doc = {}
    else:
        doc = agent_document

    # Level 1: JACS signature verification (local cryptographic check)
    agent_id = doc.get("jacsId", "")
    jacs_sig = doc.get("jacsSignature")
    pub_key_pem = doc.get("jacsPublicKey", "")

    # Extract the base64 signature from either structured or bare format
    if isinstance(jacs_sig, dict):
        sig_b64 = jacs_sig.get("signature", "")
    elif isinstance(jacs_sig, str):
        sig_b64 = jacs_sig
    else:
        sig_b64 = ""

    if sig_b64 and pub_key_pem:
        try:
            from cryptography.hazmat.primitives.serialization import (
                load_pem_public_key,
            )

            pub_key = load_pem_public_key(pub_key_pem.encode("utf-8"))

            # Reconstruct canonical form: include jacsSignature minus .signature
            import copy
            signing_doc = copy.deepcopy(doc)
            if isinstance(signing_doc.get("jacsSignature"), dict):
                signing_doc["jacsSignature"].pop("signature", None)
            else:
                del signing_doc["jacsSignature"]
            canonical = canonicalize_json(signing_doc)
            jacs_valid = _verify_string(pub_key, canonical, sig_b64)  # type: ignore[arg-type]
            if not jacs_valid:
                errors.append("JACS signature invalid")
        except Exception as exc:
            errors.append(f"JACS verification error: {exc}")
    else:
        errors.append("Missing jacsSignature or jacsPublicKey")

    # Level 2: DNS verification
    domain = doc.get("jacsDomain", "") or require_domain or ""
    if jacs_valid and domain and pub_key_pem:
        dns_valid, dns_msg = _verify_dns(domain, pub_key_pem)
        if not dns_valid:
            errors.append(dns_msg)

    # Level 3: HAI attestation (requires network)
    if jacs_valid and agent_id:
        try:
            client = _get_client()
            attestation = client.get_agent_attestation(hai_url, agent_id)
            hai_attested = (
                attestation.registered and len(attestation.hai_signatures) > 0
            )
            if hai_attested:
                hai_signatures = attestation.hai_signatures
            raw_response = attestation.raw_response
        except Exception as exc:
            errors.append(f"HAI verification error: {exc}")

    # Compute level
    if hai_attested and jacs_valid:
        level = 3
        level_name = "attested"
    elif dns_valid and jacs_valid:
        level = 2
        level_name = "domain"
    elif jacs_valid:
        level = 1
        level_name = "basic"
    else:
        level = 0
        level_name = "none"

    valid = level >= min_level

    if require_domain and domain != require_domain:
        valid = False
        errors.append(f"Domain mismatch: expected {require_domain}, got {domain}")

    return AgentVerificationResult(
        valid=valid,
        level=level,
        level_name=level_name,
        agent_id=agent_id,
        jacs_valid=jacs_valid,
        dns_valid=dns_valid,
        hai_attested=hai_attested,
        domain=domain,
        hai_signatures=hai_signatures,
        errors=errors,
        raw_response=raw_response,
    )
