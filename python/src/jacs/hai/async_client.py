"""Async HAI client using ``httpx.AsyncClient``.

Provides the same API as ``HaiClient`` but with async methods suitable
for use in FastAPI, LangChain, CrewAI, AutoGen, and other async frameworks.

Usage::

    from haisdk import config
    from haisdk.async_client import AsyncHaiClient

    config.load("./jacs.config.json")

    async with AsyncHaiClient() as client:
        result = await client.hello_world("https://hai.ai")
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
)
from jacs.hai.models import (
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
    SendEmailResult,
    TranscriptMessage,
)
from jacs.hai.signing import is_signed_event, sign_response, unwrap_signed_event

logger = logging.getLogger("jacs.hai.async_client")


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
        from jacs.hai.config import get_config
        cfg = get_config()
        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required in config for JACS authentication")
        return cfg.jacs_id

    def _build_jacs_auth_header(self) -> str:
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
        from jacs.hai.config import is_loaded, get_config
        if not (is_loaded() and get_config().jacs_id):
            raise HaiAuthError(
                "No JACS authentication available. "
                "Call jacs.hai.config.load() with a config containing jacsId."
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
        return HelloWorldResult(
            success=True,
            timestamp=data.get("timestamp", ""),
            client_ip=data.get("client_ip", ""),
            hai_public_key_fingerprint=data.get("hai_public_key_fingerprint", ""),
            message=data.get("message", ""),
            hello_id=data.get("hello_id", ""),
            test_scenario=data.get("test_scenario"),
            raw_response=data,
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
        from jacs.hai.config import get_config

        http = await self._get_http()
        cfg = get_config()

        if agent_json is None:
            from jacs.hai.config import get_private_key
            from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

            priv_key = get_private_key()
            pub_pem = priv_key.public_key().public_bytes(
                Encoding.PEM, PublicFormat.SubjectPublicKeyInfo
            ).decode()
            agent_doc = create_agent_document(
                name=cfg.name, version=cfg.version,
                public_key_pem=pub_pem, private_key=priv_key,
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
                    return HaiRegistrationResult(
                        success=True,
                        agent_id=data.get("agent_id", ""),
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
    ) -> SendEmailResult:
        """Send an email from this agent's @hai.ai address."""
        http = await self._get_http()
        jacs_id = self._get_jacs_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/send")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        payload: dict[str, Any] = {"to": to, "subject": subject, "body": body}
        if in_reply_to is not None:
            payload["in_reply_to"] = in_reply_to

        try:
            resp = await http.post(url, json=payload, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email send auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code == 429:
                raise HaiApiError(f"Email rate limited: {resp.text}", status_code=429, body=resp.text)
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

    async def list_messages(
        self, hai_url: str, limit: int = 20, offset: int = 0, folder: str = "inbox",
    ) -> list[EmailMessage]:
        """List email messages for this agent."""
        http = await self._get_http()
        jacs_id = self._get_jacs_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/messages")
        headers = self._build_auth_headers()

        try:
            resp = await http.get(url, params={"limit": limit, "offset": offset, "folder": folder}, headers=headers)
            if resp.status_code in (401, 403):
                raise HaiAuthError("Email list auth failed", status_code=resp.status_code, body=resp.text)
            if resp.status_code not in (200, 201):
                raise HaiApiError(f"Email list failed: HTTP {resp.status_code}", status_code=resp.status_code, body=resp.text)
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
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email list failed: {exc}")

    async def mark_read(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as read."""
        http = await self._get_http()
        jacs_id = self._get_jacs_id()
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
        jacs_id = self._get_jacs_id()
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
            return EmailStatus(
                daily_limit=int(data.get("daily_limit", 0)),
                daily_used=int(data.get("daily_used", 0)),
                resets_at=data.get("resets_at", ""),
                reputation_tier=data.get("reputation_tier", ""),
                current_tier=data.get("current_tier", ""),
            )
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email status failed: {exc}")

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
