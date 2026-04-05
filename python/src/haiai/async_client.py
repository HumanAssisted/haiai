"""Async HAI client using FFI adapter (haiipy binding-core).

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

from haiai._ffi_adapter import AsyncFFIAdapter
from haiai._sse import flatten_benchmark_job  # noqa: F401
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
    ``async`` and use the FFI adapter internally.
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
        self._hai_agent_id: Optional[str] = None
        self._agent_email: Optional[str] = None
        self._ffi: Optional[AsyncFFIAdapter] = None

    def _get_ffi(self) -> AsyncFFIAdapter:
        """Lazily create the async FFI adapter."""
        if self._ffi is None:
            from haiai.client import _build_ffi_config
            self._ffi = AsyncFFIAdapter(_build_ffi_config())
        return self._ffi

    @property
    def agent_email(self) -> Optional[str]:
        """Agent @hai.ai email, required for v2 email signing."""
        return self._agent_email

    def set_agent_email(self, email: str) -> None:
        """Set the agent @hai.ai email used in v2 email signing payloads."""
        self._agent_email = email

    async def close(self) -> None:
        """Close the client (no-op for FFI adapter)."""
        pass

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

    @staticmethod
    def _parse_public_key_info(data: dict[str, Any], **defaults: Any) -> PublicKeyInfo:
        return PublicKeyInfo(
            jacs_id=data.get("jacs_id", defaults.get("jacs_id", "")),
            version=data.get("version", defaults.get("version", "")),
            public_key=data.get("public_key", ""),
            public_key_raw_b64=data.get("public_key_raw_b64", ""),
            algorithm=data.get("algorithm", ""),
            public_key_hash=data.get("public_key_hash", ""),
            status=data.get("status", ""),
            dns_verified=data.get("dns_verified", False),
            created_at=data.get("created_at", ""),
        )

    # ------------------------------------------------------------------
    # testconnection
    # ------------------------------------------------------------------

    async def testconnection(self, hai_url: str) -> bool:
        """Test connectivity to the HAI server.

        Uses the FFI-backed hello() as a single authenticated health check.

        Args:
            hai_url: Base URL of the HAI server (kept for backward compat).
        """
        try:
            ffi = self._get_ffi()
            await ffi.hello(False)
            return True
        except Exception:
            return False

    # ------------------------------------------------------------------
    # hello_world
    # ------------------------------------------------------------------

    async def hello_world(
        self, hai_url: str, include_test: bool = False
    ) -> HelloWorldResult:
        """Send a JACS-signed hello request."""
        ffi = self._get_ffi()
        data = await ffi.hello(include_test)

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

        ffi = self._get_ffi()
        data = await ffi.register(payload)

        agent_id = data.get("agent_id", "")
        if agent_id:
            self._hai_agent_id = agent_id
        return HaiRegistrationResult(
            success=True,
            agent_id=agent_id,
            registered_at=data.get("registered_at", ""),
            raw_response=data,
        )

    # ------------------------------------------------------------------
    # status
    # ------------------------------------------------------------------

    async def status(self, hai_url: str) -> HaiStatusResult:
        """Check registration/verification status."""
        ffi = self._get_ffi()
        jacs_id = self._get_jacs_id()
        data = await ffi.verify_status(jacs_id)

        if not data.get("registered", True) and not data.get("jacs_id"):
            return HaiStatusResult(
                registered=False,
                agent_id=jacs_id,
                raw_response=data,
            )

        registrations = data.get("registrations", [])
        return HaiStatusResult(
            registered=data.get("registered", True),
            agent_id=data.get("jacs_id", jacs_id),
            registered_at=data.get("registered_at", ""),
            hai_signatures=[r.get("algorithm", "") for r in registrations],
            raw_response=data,
        )

    # ------------------------------------------------------------------
    # username APIs
    # ------------------------------------------------------------------

    async def update_username(
        self, hai_url: str, agent_id: str, username: str
    ) -> dict[str, Any]:
        """Rename an existing username for an agent."""
        ffi = self._get_ffi()
        return await ffi.update_username(agent_id, username)

    async def delete_username(self, hai_url: str, agent_id: str) -> dict[str, Any]:
        """Release a claimed username for an agent."""
        ffi = self._get_ffi()
        return await ffi.delete_username(agent_id)

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
        """Create a signed attestation document for a registered agent."""
        ffi = self._get_ffi()
        params = {
            "agent_id": agent_id,
            "subject": subject,
            "claims": claims,
            "evidence": evidence or [],
        }
        return await ffi.create_attestation(params)

    async def list_attestations(
        self,
        hai_url: str,
        agent_id: str,
        limit: int = 20,
        offset: int = 0,
    ) -> dict:
        """List attestations for a registered agent."""
        ffi = self._get_ffi()
        params = {"agent_id": agent_id, "limit": limit, "offset": offset}
        return await ffi.list_attestations(params)

    async def get_attestation(
        self,
        hai_url: str,
        agent_id: str,
        doc_id: str,
    ) -> dict:
        """Get a specific attestation document."""
        ffi = self._get_ffi()
        return await ffi.get_attestation(agent_id, doc_id)

    async def verify_attestation(
        self,
        hai_url: str,
        document: str,
    ) -> dict:
        """Verify an attestation document via HAI."""
        ffi = self._get_ffi()
        return await ffi.verify_attestation(document)

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
        ffi = self._get_ffi()
        data = await ffi.benchmark(name, tier)

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

    async def free_run(
        self,
        hai_url: str,
        transport: str = "sse",
    ) -> FreeChaoticResult:
        """Run a free benchmark (async)."""
        ffi = self._get_ffi()
        data = await ffi.free_run(transport)

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

    async def submit_benchmark_response(
        self,
        hai_url: str,
        job_id: str,
        message: str,
        metadata: Optional[dict[str, Any]] = None,
        processing_time_ms: int = 0,
    ) -> JobResponseResult:
        """Submit a benchmark job response (async)."""
        ffi = self._get_ffi()
        response_body: dict[str, Any] = {"message": message}
        if metadata is not None:
            response_body["metadata"] = metadata
        response_body["processing_time_ms"] = processing_time_ms

        data = await ffi.submit_response({
            "job_id": job_id,
            "response": response_body,
        })

        return JobResponseResult(
            success=data.get("success", True),
            job_id=data.get("job_id", data.get("jobId", job_id)),
            message=data.get("message", "Response accepted"),
            raw_response=data,
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
                "agent email not set -- register with a username first or call set_agent_email()"
            )

        ffi = self._get_ffi()
        options: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            options["in_reply_to"] = in_reply_to
        if attachments:
            options["attachments"] = [
                {
                    "filename": a["filename"],
                    "content_type": a["content_type"],
                    "data_base64": base64.b64encode(a["data"]).decode(),
                }
                for a in attachments
            ]
        if cc:
            options["cc"] = cc
        if bcc:
            options["bcc"] = bcc
        if labels:
            options["labels"] = labels

        data = await ffi.send_email(options)
        return SendEmailResult(message_id=data.get("message_id", ""), status=data.get("status", "sent"))

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

        Builds RFC 5322 MIME, signs with the agent's JACS key via the Rust
        FFI layer, and submits to the HAI API. The server validates the
        signature, countersigns, and delivers.
        """
        if self._agent_email is None:
            raise HaiError(
                "agent email not set -- register with a username first or call set_agent_email()"
            )

        ffi = self._get_ffi()
        options: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            options["in_reply_to"] = in_reply_to
        if attachments:
            options["attachments"] = [
                {
                    "filename": a["filename"],
                    "content_type": a["content_type"],
                    "data_base64": base64.b64encode(a["data"]).decode(),
                }
                for a in attachments
            ]
        if cc:
            options["cc"] = cc
        if bcc:
            options["bcc"] = bcc
        if labels:
            options["labels"] = labels

        data = await ffi.send_signed_email(options)
        return SendEmailResult(
            message_id=data.get("message_id", ""),
            status=data.get("status", "sent"),
        )

    async def sign_email(self, hai_url: str, raw_email: bytes) -> bytes:
        """Sign a raw RFC 5822 email via the HAI server."""
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()

        ffi = self._get_ffi()
        b64_input = base64.b64encode(raw_email).decode("ascii")
        b64_result = await ffi.sign_email_raw(b64_input)
        return base64.b64decode(b64_result)

    async def verify_email(self, hai_url: str, raw_email: bytes) -> EmailVerificationResultV2:
        """Verify a JACS-signed email via the HAI API."""
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()

        ffi = self._get_ffi()
        b64_input = base64.b64encode(raw_email).decode("ascii")
        data = await ffi.verify_email_raw(b64_input)
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
        ffi = self._get_ffi()
        options: dict[str, Any] = {"limit": limit, "offset": offset}
        if direction is not None:
            options["direction"] = direction
        if is_read is not None:
            options["is_read"] = is_read
        if folder is not None:
            options["folder"] = folder
        if label is not None:
            options["label"] = label

        items = await ffi.list_messages(options)
        messages = items if isinstance(items, list) else items.get("messages", [])
        return [EmailMessage.from_dict(m) for m in messages]

    async def mark_read(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as read."""
        ffi = self._get_ffi()
        await ffi.mark_read(message_id)
        return True

    async def get_email_status(self, hai_url: str) -> EmailStatus:
        """Get email rate-limit and reputation status."""
        ffi = self._get_ffi()
        data = await ffi.get_email_status()
        return self._parse_email_status(data)

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
        ffi = self._get_ffi()
        m = await ffi.get_message(message_id)
        return EmailMessage.from_dict(m)

    async def delete_message(self, hai_url: str, message_id: str) -> bool:
        """Delete an email message."""
        ffi = self._get_ffi()
        await ffi.delete_message(message_id)
        return True

    async def mark_unread(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as unread."""
        ffi = self._get_ffi()
        await ffi.mark_unread(message_id)
        return True

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
        ffi = self._get_ffi()
        options: dict[str, Any] = {"limit": limit, "offset": offset}
        if q is not None:
            options["q"] = q
        if direction is not None:
            options["direction"] = direction
        if from_address is not None:
            options["from_address"] = from_address
        if to_address is not None:
            options["to_address"] = to_address
        if since is not None:
            options["since"] = since
        if until is not None:
            options["until"] = until
        if is_read is not None:
            options["is_read"] = is_read
        if jacs_verified is not None:
            options["jacs_verified"] = jacs_verified
        if folder is not None:
            options["folder"] = folder
        if label is not None:
            options["label"] = label

        items = await ffi.search_messages(options)
        messages = items if isinstance(items, list) else items.get("messages", [])
        return [EmailMessage.from_dict(m) for m in messages]

    async def get_unread_count(self, hai_url: str) -> int:
        """Get the number of unread email messages."""
        ffi = self._get_ffi()
        return await ffi.get_unread_count()

    async def reply(
        self,
        hai_url: str,
        message_id: str,
        body: str,
        subject: Optional[str] = None,
    ) -> SendEmailResult:
        """Reply to an email message. Always JACS-signed."""
        original = await self.get_message(hai_url, message_id)
        # Sanitize: strip CR/LF that may be present from email header folding.
        clean_subject = (original.subject or "").replace("\r", "").replace("\n", "")
        reply_subject = subject if subject is not None else f"Re: {clean_subject}"
        return await self.send_signed_email(
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
        ffi = self._get_ffi()
        params: dict[str, Any] = {
            "message_id": message_id,
            "to": to,
        }
        if comment is not None:
            params["comment"] = comment

        data = await ffi.forward(params)
        return SendEmailResult(
            message_id=data.get("message_id", ""),
            status=data.get("status", "sent"),
        )

    async def archive(self, hai_url: str, message_id: str) -> bool:
        """Archive an email message."""
        ffi = self._get_ffi()
        await ffi.archive(message_id)
        return True

    async def unarchive(self, hai_url: str, message_id: str) -> bool:
        """Unarchive an email message."""
        ffi = self._get_ffi()
        await ffi.unarchive(message_id)
        return True

    async def update_labels(
        self,
        hai_url: str,
        message_id: str,
        add: list[str] | None = None,
        remove: list[str] | None = None,
    ) -> list[str]:
        """Update labels on an email message."""
        ffi = self._get_ffi()
        data = await ffi.update_labels({
            "message_id": message_id,
            "add": add or [],
            "remove": remove or [],
        })
        return data.get("labels", [])

    async def contacts(self, hai_url: str) -> list[Contact]:
        """List contacts derived from email history."""
        ffi = self._get_ffi()
        items = await ffi.contacts()
        result_items = items if isinstance(items, list) else items.get("contacts", [])
        return [
            Contact(
                email=c.get("email", ""),
                display_name=c.get("display_name"),
                last_contact=c.get("last_contact", ""),
                jacs_verified=c.get("jacs_verified", False),
                reputation_tier=c.get("reputation_tier"),
            )
            for c in result_items
        ]

    # ------------------------------------------------------------------
    # email templates
    # ------------------------------------------------------------------

    async def create_email_template(
        self,
        hai_url: str,
        name: str,
        how_to_send: Optional[str] = None,
        how_to_respond: Optional[str] = None,
        goal: Optional[str] = None,
        rules: Optional[str] = None,
    ) -> dict:
        """Create an email template."""
        ffi = self._get_ffi()
        options: dict[str, Any] = {"name": name}
        if how_to_send is not None:
            options["how_to_send"] = how_to_send
        if how_to_respond is not None:
            options["how_to_respond"] = how_to_respond
        if goal is not None:
            options["goal"] = goal
        if rules is not None:
            options["rules"] = rules
        return await ffi.create_email_template(options)

    async def list_email_templates(
        self,
        hai_url: str,
        limit: int = 20,
        offset: int = 0,
        q: Optional[str] = None,
    ) -> dict:
        """List or search email templates."""
        ffi = self._get_ffi()
        options: dict[str, Any] = {"limit": limit, "offset": offset}
        if q is not None:
            options["q"] = q
        return await ffi.list_email_templates(options)

    async def get_email_template(self, hai_url: str, template_id: str) -> dict:
        """Get a single email template by ID."""
        ffi = self._get_ffi()
        return await ffi.get_email_template(template_id)

    async def update_email_template(
        self,
        hai_url: str,
        template_id: str,
        name: Optional[str] = None,
        how_to_send: Optional[str] = None,
        how_to_respond: Optional[str] = None,
        goal: Optional[str] = None,
        rules: Optional[str] = None,
    ) -> dict:
        """Update an email template."""
        ffi = self._get_ffi()
        options: dict[str, Any] = {}
        if name is not None:
            options["name"] = name
        if how_to_send is not None:
            options["how_to_send"] = how_to_send
        if how_to_respond is not None:
            options["how_to_respond"] = how_to_respond
        if goal is not None:
            options["goal"] = goal
        if rules is not None:
            options["rules"] = rules
        return await ffi.update_email_template(template_id, options)

    async def delete_email_template(self, hai_url: str, template_id: str) -> None:
        """Delete an email template."""
        ffi = self._get_ffi()
        await ffi.delete_email_template(template_id)

    # ------------------------------------------------------------------
    # fetch_remote_key
    # ------------------------------------------------------------------

    async def fetch_remote_key(
        self, hai_url: str, jacs_id: str, version: str = "latest",
    ) -> PublicKeyInfo:
        """Fetch another agent's public key from HAI."""
        ffi = self._get_ffi()
        data = await ffi.fetch_remote_key(jacs_id, version)
        return self._parse_public_key_info(data, jacs_id=jacs_id, version=version)

    async def fetch_key_by_hash(self, hai_url: str, public_key_hash: str) -> PublicKeyInfo:
        """Fetch an agent's public key by its SHA-256 hash."""
        ffi = self._get_ffi()
        data = await ffi.fetch_key_by_hash(public_key_hash)
        return self._parse_public_key_info(data)

    async def fetch_key_by_email(self, hai_url: str, email: str) -> PublicKeyInfo:
        """Fetch an agent's public key by their @hai.ai email address."""
        ffi = self._get_ffi()
        data = await ffi.fetch_key_by_email(email)
        return self._parse_public_key_info(data)

    async def fetch_key_by_domain(self, hai_url: str, domain: str) -> PublicKeyInfo:
        """Fetch the latest DNS-verified agent key for a domain."""
        ffi = self._get_ffi()
        data = await ffi.fetch_key_by_domain(domain)
        return self._parse_public_key_info(data)

    async def fetch_all_keys(self, hai_url: str, jacs_id: str) -> dict:
        """Fetch all key versions for an agent."""
        ffi = self._get_ffi()
        return await ffi.fetch_all_keys(jacs_id)

    # ------------------------------------------------------------------
    # advanced verification endpoints
    # ------------------------------------------------------------------

    async def get_verification(
        self,
        hai_url: str,
        agent_id: str,
    ) -> dict[str, Any]:
        """Get advanced 3-level verification status for an agent."""
        ffi = self._get_ffi()
        return await ffi.get_verification(agent_id)

    async def verify_agent_document(
        self,
        hai_url: str,
        agent_json: str | dict[str, Any],
        *,
        public_key: str | None = None,
        domain: str | None = None,
    ) -> dict[str, Any]:
        """Verify an agent document via HAI's advanced verification endpoint."""
        ffi = self._get_ffi()
        request: dict[str, Any] = {
            "agent_json": agent_json if isinstance(agent_json, str) else json.dumps(agent_json),
        }
        if public_key is not None:
            request["public_key"] = public_key
        if domain is not None:
            request["domain"] = domain
        return await ffi.verify_agent_document(json.dumps(request))

    # ------------------------------------------------------------------
    # connect (SSE + WS async streaming) -- via FFI opaque handles
    # ------------------------------------------------------------------

    async def connect(
        self, hai_url: str, *, transport: str = "sse",
    ) -> AsyncIterator[HaiEvent]:
        """Connect to HAI and yield events asynchronously via FFI."""
        if transport not in ("sse", "ws"):
            raise ValueError(f"transport must be 'sse' or 'ws', got '{transport}'")

        self._hai_url = hai_url
        self._should_disconnect = False
        self._connected = False

        ffi = self._get_ffi()
        handle = None
        try:
            if transport == "ws":
                handle = await ffi.connect_ws()
            else:
                handle = await ffi.connect_sse()

            self._connected = True

            while not self._should_disconnect:
                if transport == "ws":
                    event_data = await ffi.ws_next_event(handle)
                else:
                    event_data = await ffi.sse_next_event(handle)

                if event_data is None:
                    break

                event = HaiEvent(
                    event_type=event_data.get("event_type", ""),
                    data=event_data.get("data", {}),
                    id=event_data.get("id"),
                    raw=event_data.get("raw", ""),
                )
                yield event

        finally:
            self._connected = False
            if handle is not None:
                try:
                    if transport == "ws":
                        await ffi.ws_close(handle)
                    else:
                        await ffi.sse_close(handle)
                except Exception:
                    pass

    def disconnect(self) -> None:
        """Signal the SSE loop to stop."""
        self._should_disconnect = True
        self._connected = False

    @property
    def is_connected(self) -> bool:
        return self._connected
