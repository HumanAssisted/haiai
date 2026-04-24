"""HaiClient -- full-featured client for the HAI benchmark platform.

All HTTP-based API calls delegate to the FFI adapter (haiipy binding-core).
SSE/WS streaming, key rotation, and local signing remain native Python.
"""

from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import time
from pathlib import Path
from typing import Any, Generator, Iterator, Optional, Union

from haiai import _client_shared
from haiai._ffi_adapter import FFIAdapter, map_ffi_error
from haiai._sse import flatten_benchmark_job, parse_sse_lines
from haiai.signing import canonicalize_json, create_agent_document
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
    SSEError,
    SubjectTooLong,
    WebSocketError,
)
from haiai.models import (
    AgentConfig,
    AgentVerificationResult,
    AttestationResult,
    AttestationVerifyResult,
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
    RawEmailResult,
    RegistrationResult,
    RotationResult,
    SendEmailResult,
    TranscriptMessage,
)
from haiai.signing import is_signed_event, sign_response, unwrap_signed_event

logger = logging.getLogger("haiai.client")

# Default HAI API base URL. Override with HAI_URL or HAI_API_URL env vars.
DEFAULT_BASE_URL = "https://beta.hai.ai"

# Verify link constants (HAI / public verification URLs)
MAX_VERIFY_URL_LEN = 2048
MAX_VERIFY_DOCUMENT_BYTES = 1515


def _armor_key_bytes(raw: bytes, block_type: str) -> str:
    encoded = base64.b64encode(raw).decode("ascii")
    lines = [encoded[i:i + 64] for i in range(0, len(encoded), 64)]
    return (
        f"-----BEGIN {block_type}-----\n"
        + "\n".join(lines)
        + f"\n-----END {block_type}-----\n"
    )


def _normalize_public_key_pem(raw: bytes) -> str:
    try:
        text = raw.decode("utf-8").strip()
    except UnicodeDecodeError:
        text = ""

    if "BEGIN PUBLIC KEY" in text:
        return text if text.endswith("\n") else text + "\n"
    return _armor_key_bytes(raw, "PUBLIC KEY")


def _read_public_key_pem(cfg: "AgentConfig") -> str:
    """Read the agent's public key PEM from the key directory."""
    key_dir = Path(cfg.key_dir)
    candidates = [
        key_dir / "agent_public_key.pem",
        key_dir / f"{cfg.name}.public.pem",
        key_dir / "public_key.pem",
        key_dir / "jacs.public.pem",
    ]
    for p in candidates:
        if p.is_file():
            return _normalize_public_key_pem(p.read_bytes())
    raise FileNotFoundError(
        f"Public key not found. Searched: {', '.join(str(p) for p in candidates)}"
    )


def _verify_hai_message_impl(
    message: str,
    signature: str,
    hai_public_key: str = "",
    hai_url: Optional[str] = None,
) -> bool:
    """Verify a HAI-signed message using PEM, base64 key material, or key lookup."""
    if not signature or not message:
        return False

    if not hai_public_key:
        return False

    from haiai.signing import verify_string as _verify_string

    try:
        if hai_public_key.startswith("-----"):
            return _verify_string(message, signature, hai_public_key)

        try:
            base64.b64decode(hai_public_key)
            pem_key = (
                "-----BEGIN PUBLIC KEY-----\n"
                + hai_public_key
                + "\n-----END PUBLIC KEY-----\n"
            )
            return _verify_string(message, signature, pem_key)
        except Exception:
            if not hai_url:
                return False
            from haiai.signing import fetch_server_keys

            keys = fetch_server_keys(hai_url)
            match = next((k for k in keys if k.key_id == hai_public_key), None)
            if match is None:
                return False
            return _verify_string(message, signature, match.public_key_pem)
    except Exception as exc:
        logger.debug("Signature verification failed: %s", exc)
        return False


def _build_ffi_config() -> str:
    """Build the JSON config string for the FFI adapter from loaded JACS config."""
    from haiai.config import get_config, is_loaded

    config: dict[str, Any] = {}

    if is_loaded():
        cfg = get_config()
        if cfg.jacs_id:
            config["jacs_id"] = cfg.jacs_id
        config["agent_name"] = cfg.name
        config["agent_version"] = cfg.version
        config["key_dir"] = cfg.key_dir

    # Pick up base URL from env
    base_url = os.environ.get("HAI_URL") or os.environ.get("HAI_API_URL") or DEFAULT_BASE_URL
    config["base_url"] = base_url

    # Pick up config path from env
    config_path = os.environ.get("JACS_CONFIG_PATH", "./jacs.config.json")
    config["jacs_config_path"] = config_path

    return json.dumps(config)


# ---------------------------------------------------------------------------
# HaiClient
# ---------------------------------------------------------------------------


class HaiClient:
    """Client for the HAI benchmark platform.

    Handles JACS-signed authentication and event streaming over SSE or
    WebSocket.  All operations require a loaded JACS config (via
    ``haiai.config.load()``).  There is **no API-key fallback**.

    Example::

        from haiai import config, HaiClient

        config.load("./jacs.config.json")
        client = HaiClient()
        if client.testconnection("https://beta.hai.ai"):
            result = client.hello_world("https://beta.hai.ai")
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
        self._hai_agent_id: Optional[str] = None
        self._agent_email: Optional[str] = None
        # Agent key cache: maps cache_key -> (PublicKeyInfo, cached_at_monotonic)
        self._key_cache: dict[str, tuple[Any, float]] = {}
        self._KEY_CACHE_TTL: float = 300.0  # 5 minutes
        self._ffi: Optional[FFIAdapter] = None

    def _get_ffi(self) -> FFIAdapter:
        """Lazily create the FFI adapter."""
        if self._ffi is None:
            self._ffi = FFIAdapter(_build_ffi_config())
        return self._ffi

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------

    @property
    def agent_email(self) -> Optional[str]:
        """The agent's ``@hai.ai`` email address, set after registration."""
        return self._agent_email

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _make_url(base_url: str, path: str) -> str:
        """Construct a full URL from base and path.

        Raises:
            ValueError: If base_url does not start with http:// or https://.
        """
        return _client_shared.make_url(base_url, path, validate_scheme=True)

    @staticmethod
    def _escape_path_segment(value: str) -> str:
        """Escape a user-controlled URL path segment."""
        return _client_shared.escape_path_segment(value)

    def _get_cached_key(self, cache_key: str) -> Optional[Any]:
        """Return a cached key if it exists and hasn't expired, else None."""
        entry = self._key_cache.get(cache_key)
        if entry is None:
            return None
        value, cached_at = entry
        if time.monotonic() - cached_at >= self._KEY_CACHE_TTL:
            del self._key_cache[cache_key]
            return None
        return value

    def _set_cached_key(self, cache_key: str, value: Any) -> None:
        """Store a key in the cache with the current timestamp."""
        self._key_cache[cache_key] = (value, time.monotonic())

    def invalidate_key_cache(self) -> None:
        """Clear the agent key cache, forcing subsequent fetches to hit the API."""
        self._key_cache.clear()

    def _get_jacs_id(self) -> str:
        """Return the loaded JACS ID, raising if not available."""
        return _client_shared.get_jacs_id()

    def _get_hai_agent_id(self) -> str:
        """Return the HAI-assigned agent UUID for email URL paths.

        Falls back to the JACS ID if not set (e.g. before registration).
        """
        return _client_shared.get_hai_agent_id(self._hai_agent_id)

    def _build_jacs_auth_header(self) -> str:
        """Build ``Authorization: JACS {jacsId}:{timestamp}:{signature}``.

        Delegates to JACS binding-core ``build_auth_header`` when available.
        Otherwise constructs the header locally using JACS ``sign_string``.
        Both paths require a loaded JACS agent.
        """
        return _client_shared.build_jacs_auth_header()

    def _build_auth_headers(self) -> dict[str, str]:
        """Return auth headers using JACS signature authentication."""
        return _client_shared.build_auth_headers()

    @staticmethod
    def _build_jacs_auth_header_with_key(
        jacs_id: str,
        version: str,
        agent: Any,
    ) -> str:
        """Build a 4-part JACS auth header signed by an explicit agent.

        Returns ``JACS {jacsId}:{version}:{timestamp}:{signature}``.
        Used during key rotation to authenticate re-registration with
        the OLD agent's key (chain of trust).
        Signing delegates to JACS binding-core.
        """
        return _client_shared.build_jacs_auth_header_with_key(
            jacs_id, version, agent
        )

    @staticmethod
    def _parse_transcript(
        raw_messages: list[dict[str, Any]],
    ) -> list[TranscriptMessage]:
        """Parse raw transcript messages from API response."""
        return _client_shared.parse_transcript(raw_messages)

    @staticmethod
    def _parse_public_key_info(data: dict[str, Any], **defaults: Any) -> PublicKeyInfo:
        """Parse a PublicKeyInfo from an FFI response dict."""
        return _client_shared.parse_public_key_info(data, **defaults)

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

    # ------------------------------------------------------------------
    # testconnection
    # ------------------------------------------------------------------

    def testconnection(self, hai_url: Optional[str] = None) -> bool:
        """Test connectivity to the HAI server.

        Uses the FFI-backed hello() method as a single authenticated health
        check.  Returns True on success, False on any error.

        Args:
            hai_url: Base URL of the HAI server (optional, unused by FFI).

        Returns:
            True if the server is reachable.
        """
        try:
            ffi = self._get_ffi()
            ffi.hello(False)
            return True
        except Exception:
            return False

    # ------------------------------------------------------------------
    # hello_world
    # ------------------------------------------------------------------

    def hello_world(
        self,
        hai_url: Optional[str] = None,
        include_test: bool = False,
    ) -> HelloWorldResult:
        """Send a JACS-signed hello request to HAI and get a signed ACK.

        Args:
            hai_url: Base URL of the HAI server (defaults to DEFAULT_BASE_URL).
            include_test: If True, include a test scenario preview.

        Returns:
            HelloWorldResult with HAI's signed acknowledgment.

        Raises:
            HaiAuthError: If JACS config is not loaded.
            HaiApiError: On any non-2xx response.
        """
        hai_url = hai_url or DEFAULT_BASE_URL
        ffi = self._get_ffi()
        data = ffi.hello(include_test)

        # Verify HAI's signature on the ACK
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

        Verification delegates to JACS binding-core.

        Args:
            message: The message string that was signed.
            signature: Base64-encoded signature.
            hai_public_key: HAI's public key (PEM, base64 raw, or key ID).
            hai_url: HAI server URL (for key ID lookup).

        Returns:
            True if signature is valid.
        """
        return _verify_hai_message_impl(
            message=message,
            signature=signature,
            hai_public_key=hai_public_key,
            hai_url=hai_url,
        )

    # ------------------------------------------------------------------
    # register (existing agent)
    # ------------------------------------------------------------------

    def register(
        self,
        hai_url: Optional[str] = None,
        agent_json: Optional[str] = None,
        public_key: Optional[str] = None,
        preview: bool = False,
        owner_email: Optional[str] = None,
    ) -> Union[HaiRegistrationResult, HaiRegistrationPreview]:
        """Register a JACS agent with HAI.

        Args:
            hai_url: Base URL of the HAI server (defaults to DEFAULT_BASE_URL).
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
        hai_url = hai_url or DEFAULT_BASE_URL
        from haiai.config import get_config

        cfg = get_config()

        # Build agent_json from config if not provided
        if agent_json is None:
            from haiai.config import get_agent

            agent = get_agent()
            # Get public key PEM from the agent's key files
            pub_pem = _read_public_key_pem(cfg)
            agent_doc = create_agent_document(
                agent=agent,
                name=cfg.name,
                version=cfg.version,
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
        data = ffi.register(payload)

        agent_id = data.get("agent_id", "")
        if agent_id:
            self._hai_agent_id = agent_id
        return HaiRegistrationResult(
            success=True,
            agent_id=agent_id,
            hai_signature="",
            registration_id="",
            registered_at=data.get("registered_at", ""),
            capabilities=[],
            raw_response=data,
        )

    # ------------------------------------------------------------------
    # key rotation
    # ------------------------------------------------------------------

    def rotate_keys(
        self,
        hai_url: Optional[str] = None,
        register_with_hai: bool = True,
        config_path: Optional[str] = None,
        algorithm: str = "pq2025",
    ) -> RotationResult:
        """Rotate the agent's cryptographic keys.

        This generates a new keypair via JACS, archives the old keys
        (with a version suffix), builds a new self-signed agent document,
        updates the config, and optionally re-registers with HAI.

        The old keys are preserved on disk so that previously signed
        documents can still be verified.

        Args:
            hai_url: Base URL of the HAI server (required if
                ``register_with_hai=True``).
            register_with_hai: If True (default), re-register the agent
                with HAI after local rotation. A network failure here
                does NOT rollback the local rotation.
            config_path: Path to jacs.config.json. Defaults to the path
                used by ``config.load()`` (or ``./jacs.config.json``).
            algorithm: Signing algorithm for the new key (default
                ``"pq2025"``). Pass ``"ring-Ed25519"`` for Ed25519.

        Returns:
            RotationResult with old/new versions, public key hash, and
            whether re-registration succeeded.

        Raises:
            HaiAuthError: If no agent is currently loaded.
            RegistrationError: Only if re-registration fails and
                ``register_with_hai=True``, but the local rotation is
                still preserved.
        """
        # rotate_keys stays native -- it involves file I/O, JACS agent creation,
        # and key archival that are inherently local operations.
        import hashlib
        import shutil
        import tempfile
        import uuid

        from haiai import config as config_mod
        from haiai.signing import create_agent_document

        cfg = config_mod.get_config()
        old_agent = config_mod.get_agent()

        if cfg.jacs_id is None:
            raise HaiAuthError(
                "Cannot rotate keys: no jacsId in config. "
                "Register an agent first."
            )

        old_version = cfg.version
        jacs_id = cfg.jacs_id
        key_dir = Path(cfg.key_dir)

        # 1. Determine archive paths
        priv_candidates = [
            key_dir / "agent_private_key.pem",
            key_dir / "jacs.private.pem.enc",
            key_dir / f"{cfg.name}.private.pem",
            key_dir / "private_key.pem",
        ]
        priv_path: Optional[Path] = None
        for p in priv_candidates:
            if p.is_file():
                priv_path = p
                break

        if priv_path is None:
            raise HaiAuthError(
                "Cannot rotate keys: private key file not found. "
                f"Searched: {', '.join(str(p) for p in priv_candidates)}"
            )

        archive_priv = priv_path.with_suffix(f".{old_version}.pem")

        pub_path = key_dir / priv_path.name.replace("private", "public")
        if not pub_path.is_file():
            for name in ["agent_public_key.pem", "jacs.public.pem", f"{cfg.name}.public.pem", "public_key.pem"]:
                alt = key_dir / name
                if alt.is_file():
                    pub_path = alt
                    break

        archive_pub = pub_path.with_suffix(f".{old_version}.pem") if pub_path.is_file() else None

        # 2. Pre-sign auth header with old agent BEFORE archiving keys
        old_auth_header = None
        if register_with_hai and hai_url is not None:
            try:
                old_auth_header = self._build_jacs_auth_header_with_key(
                    jacs_id, old_version, old_agent,
                )
            except Exception as exc:
                logger.warning(
                    "Failed to pre-sign rotation auth header: %s", exc
                )

        # 3. Archive old keys (after pre-signing)
        logger.info("Archiving old private key: %s -> %s", priv_path, archive_priv)
        shutil.move(str(priv_path), str(archive_priv))

        if pub_path.is_file() and archive_pub is not None:
            logger.info("Archiving old public key: %s -> %s", pub_path, archive_pub)
            shutil.move(str(pub_path), str(archive_pub))

        # 4. Generate new keypair via JACS SimpleAgent.create_agent()
        try:
            from jacs import SimpleAgent as _SimpleAgent
        except ImportError:
            from jacs.jacs import SimpleAgent as _SimpleAgent  # type: ignore[no-redef]

        password_bytes = config_mod.load_private_key_password()
        password_str = password_bytes.decode("utf-8")

        try:
            with tempfile.TemporaryDirectory() as tmp_dir:
                tmp_path = Path(tmp_dir)
                tmp_key_dir = tmp_path / "keys"
                tmp_key_dir.mkdir()
                tmp_data_dir = tmp_path / "data"
                tmp_data_dir.mkdir()
                tmp_config = tmp_path / "jacs.config.json"

                _new_agent, new_info = _SimpleAgent.create_agent(
                    name=cfg.name,
                    password=password_str,
                    algorithm=algorithm,
                    data_directory=str(tmp_data_dir),
                    key_directory=str(tmp_key_dir),
                    config_path=str(tmp_config),
                    description="",
                    domain="",
                    default_storage="fs",
                )

                new_priv_src = Path(new_info.get("private_key_path", ""))
                new_pub_src = Path(new_info.get("public_key_path", ""))

                if new_priv_src.is_file():
                    shutil.copy2(str(new_priv_src), str(priv_path))
                    os.chmod(str(priv_path), 0o600)
                if new_pub_src.is_file():
                    shutil.copy2(str(new_pub_src), str(pub_path))
                    os.chmod(str(pub_path), 0o644)

        except Exception as exc:
            logger.error("Key generation failed, rolling back: %s", exc)
            shutil.move(str(archive_priv), str(priv_path))
            if archive_pub is not None and archive_pub.is_file():
                shutil.move(str(archive_pub), str(pub_path))
            raise HaiAuthError(f"Key generation failed: {exc}") from exc

        # 5. Use the newly-created agent directly for signing
        new_version = str(uuid.uuid4())

        cfg_path = config_path or os.environ.get(
            "JACS_CONFIG_PATH", "./jacs.config.json"
        )

        try:
            from jacs.simple import _EphemeralAgentAdapter
            new_agent = _EphemeralAgentAdapter(_new_agent)
        except ImportError:
            new_agent = _new_agent

        config_mod._config = AgentConfig(
            name=cfg.name,
            version=new_version,
            key_dir=cfg.key_dir,
            jacs_id=jacs_id,
        )
        config_mod._agent = new_agent
        config_mod.save(cfg_path)

        # 6. Build new agent document signed by the new agent
        agent_doc = create_agent_document(
            agent=new_agent,
            name=cfg.name,
            version=new_version,
            jacs_id=jacs_id,
            extra_fields={"jacsPreviousVersion": old_version},
        )
        signed_agent_json = json.dumps(agent_doc, indent=2)

        # 7. Compute new public key hash and read PEM for re-registration
        pub_pem_str = ""
        if pub_path.is_file():
            pub_key_raw = pub_path.read_bytes()
            new_public_key_hash = hashlib.sha256(pub_key_raw).hexdigest()
            pub_pem_str = _normalize_public_key_pem(pub_key_raw)
        else:
            new_public_key_hash = ""

        logger.info(
            "Key rotation complete: %s -> %s (agent=%s)",
            old_version, new_version, jacs_id,
        )

        # 8. Optionally re-register with HAI using FFI
        registered = False
        if register_with_hai:
            if hai_url is None:
                logger.warning(
                    "register_with_hai=True but no hai_url; skipping registration"
                )
            else:
                try:
                    # Reset FFI adapter to pick up new keys
                    self._ffi = None
                    ffi = self._get_ffi()
                    reg_payload: dict[str, Any] = {
                        "agent_json": signed_agent_json,
                    }
                    if pub_pem_str:
                        reg_payload["public_key"] = base64.b64encode(
                            pub_pem_str.encode("utf-8")
                        ).decode("utf-8")
                    ffi.register(reg_payload)
                    registered = True
                    logger.info("Re-registered with HAI after rotation")
                except Exception as exc:
                    logger.warning(
                        "HAI re-registration failed (local rotation preserved): %s",
                        exc,
                    )

        return RotationResult(
            jacs_id=jacs_id,
            old_version=old_version,
            new_version=new_version,
            new_public_key_hash=new_public_key_hash,
            registered_with_hai=registered,
            signed_agent_json=signed_agent_json,
        )

    # ------------------------------------------------------------------
    # status
    # ------------------------------------------------------------------

    def status(self, hai_url: Optional[str] = None) -> HaiStatusResult:
        """Check registration/verification status of the current agent.

        Args:
            hai_url: Base URL of the HAI server (optional, unused by FFI).

        Returns:
            HaiStatusResult with verification details.
        """
        ffi = self._get_ffi()
        jacs_id = self._get_jacs_id()
        data = ffi.verify_status(jacs_id)

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
            registration_id="",
            registered_at=data.get("registered_at", ""),
            hai_signatures=[
                r.get("algorithm", "") for r in registrations
            ],
            raw_response=data,
        )

    # ------------------------------------------------------------------
    # get_agent_attestation
    # ------------------------------------------------------------------

    def get_agent_attestation(
        self,
        hai_url: Optional[str] = None,
        agent_id: str = "",
    ) -> HaiStatusResult:
        """Get HAI attestation status for any agent by ID.

        Args:
            hai_url: Base URL of the HAI server (optional, unused by FFI).
            agent_id: JACS agent ID to check.

        Returns:
            HaiStatusResult with registration details.
        """
        ffi = self._get_ffi()
        data = ffi.get_verification(agent_id)

        if not data.get("registered", True) and not data.get("jacs_id"):
            return HaiStatusResult(
                registered=False,
                agent_id=agent_id,
                raw_response=data,
            )

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

    # ------------------------------------------------------------------
    # Username management
    # ------------------------------------------------------------------

    def update_username(
        self, hai_url: Optional[str] = None, agent_id: str = "", username: str = ""
    ) -> dict[str, Any]:
        """Update (rename) a claimed username for an agent."""
        if not agent_id:
            raise ValueError("'agent_id' is required")
        if not username:
            raise ValueError("'username' is required")
        ffi = self._get_ffi()
        return ffi.update_username(agent_id, username)

    def delete_username(self, hai_url: Optional[str] = None, agent_id: str = "") -> dict[str, Any]:
        """Delete a claimed username for an agent."""
        if not agent_id:
            raise ValueError("'agent_id' is required")
        ffi = self._get_ffi()
        return ffi.delete_username(agent_id)

    def verify_document(
        self,
        hai_url: Optional[str] = None,
        document: Union[str, dict[str, Any]] = "",
    ) -> dict[str, Any]:
        """Verify a signed JACS document via HAI's public verify endpoint."""
        ffi = self._get_ffi()
        raw_document = document if isinstance(document, str) else json.dumps(document)
        return ffi.verify_document(raw_document)

    def get_verification(
        self,
        hai_url: Optional[str] = None,
        agent_id: str = "",
    ) -> dict[str, Any]:
        """Get advanced 3-level verification status for an agent."""
        if not agent_id:
            raise ValueError("'agent_id' is required")
        ffi = self._get_ffi()
        return ffi.get_verification(agent_id)

    def verify_agent_document(
        self,
        hai_url: Optional[str] = None,
        agent_json: Union[str, dict[str, Any]] = "",
        *,
        public_key: Optional[str] = None,
        domain: Optional[str] = None,
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
        return ffi.verify_agent_document(json.dumps(request))

    # ------------------------------------------------------------------
    # attestation
    # ------------------------------------------------------------------

    def create_attestation(
        self,
        hai_url: Optional[str] = None,
        agent_id: str = "",
        subject: Optional[dict] = None,
        claims: Optional[list] = None,
        evidence: list | None = None,
    ) -> dict:
        """Create a signed attestation document for a registered agent."""
        if not agent_id:
            raise ValueError("'agent_id' is required")
        ffi = self._get_ffi()
        params = {
            "agent_id": agent_id,
            "subject": subject or {},
            "claims": claims or [],
            "evidence": evidence or [],
        }
        return ffi.create_attestation(params)

    def list_attestations(
        self,
        hai_url: Optional[str] = None,
        agent_id: str = "",
        limit: int = 20,
        offset: int = 0,
    ) -> dict:
        """List attestations for a registered agent."""
        ffi = self._get_ffi()
        params = {"agent_id": agent_id, "limit": limit, "offset": offset}
        return ffi.list_attestations(params)

    def get_attestation(
        self,
        hai_url: Optional[str] = None,
        agent_id: str = "",
        doc_id: str = "",
    ) -> dict:
        """Get a specific attestation document."""
        ffi = self._get_ffi()
        return ffi.get_attestation(agent_id, doc_id)

    def verify_attestation(
        self,
        hai_url: Optional[str] = None,
        document: str = "",
    ) -> dict:
        """Verify an attestation document via HAI."""
        ffi = self._get_ffi()
        return ffi.verify_attestation(document)

    # ------------------------------------------------------------------
    # benchmark
    # ------------------------------------------------------------------

    def benchmark(
        self,
        hai_url: Optional[str] = None,
        name: str = "mediator",
        tier: str = "free",
        timeout: Optional[float] = None,
    ) -> BenchmarkResult:
        """Run a benchmark via HAI.

        Args:
            hai_url: Base URL of the HAI server.
            name: Benchmark scenario name (default: "mediator").
            tier: Benchmark tier: "free", "pro", or "enterprise".
            timeout: Optional timeout override for benchmark execution.

        Returns:
            BenchmarkResult with scores and detailed test results.
        """
        ffi = self._get_ffi()
        data = ffi.benchmark(name, tier)

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

    # ------------------------------------------------------------------
    # free_run
    # ------------------------------------------------------------------

    def free_run(
        self,
        hai_url: Optional[str] = None,
        transport: str = "sse",
    ) -> FreeChaoticResult:
        """Run a free benchmark.

        Args:
            hai_url: Base URL of the HAI server (optional, unused by FFI).
            transport: Transport protocol: "sse" (default) or "ws".

        Returns:
            FreeChaoticResult with transcript and annotations.
        """
        ffi = self._get_ffi()
        data = ffi.free_run(transport)

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

    # ------------------------------------------------------------------
    # pro_run
    # ------------------------------------------------------------------

    def pro_run(
        self,
        hai_url: Optional[str] = None,
        transport: str = "sse",
    ) -> BaselineRunResult:
        """Run a pro tier benchmark ($20/month).

        The entire payment + benchmark flow is handled by the Rust FFI layer,
        matching the Node and Go SDK patterns.

        Args:
            hai_url: Base URL of the HAI server (optional, unused by FFI).
            transport: Transport type for the benchmark run (default: "sse").

        Returns:
            BaselineRunResult with benchmark results.
        """
        ffi = self._get_ffi()
        data = ffi.pro_run({
            "transport": transport,
            "poll_interval_ms": 2000,
            "poll_timeout_secs": 300,
        })

        transcript = self._parse_transcript(data.get("transcript", []))
        score = float(data.get("score", 0.0))

        return BaselineRunResult(
            success=True,
            run_id=data.get("run_id", data.get("runId", "")),
            score=score,
            transcript=transcript,
            payment_id=data.get("payment_id", ""),
            raw_response=data,
        )

    # ------------------------------------------------------------------
    # enterprise_run
    # ------------------------------------------------------------------

    def enterprise_run(self, **kwargs: Any) -> None:
        """Run an enterprise tier benchmark.

        The enterprise tier is coming soon.
        Contact support@hai.ai for early access.
        """
        raise NotImplementedError(
            "The enterprise tier is coming soon. "
            "Contact support@hai.ai for early access."
        )

    # Deprecated aliases for backward compatibility
    dns_certified_run = pro_run
    certified_run = enterprise_run

    # ------------------------------------------------------------------
    # submit_benchmark_response
    # ------------------------------------------------------------------

    def submit_benchmark_response(
        self,
        hai_url: Optional[str] = None,
        job_id: str = "",
        message: str = "",
        metadata: Optional[dict[str, Any]] = None,
        processing_time_ms: int = 0,
    ) -> JobResponseResult:
        """Submit a benchmark job response.

        The response is wrapped as a JACS-signed document.
        """
        if not job_id:
            raise ValueError("'job_id' is required")
        ffi = self._get_ffi()
        response_body: dict[str, Any] = {"message": message}
        if metadata is not None:
            response_body["metadata"] = metadata
        response_body["processing_time_ms"] = processing_time_ms

        data = ffi.submit_response({
            "job_id": job_id,
            "response": response_body,
        })

        return JobResponseResult(
            success=data.get("success", True),
            job_id=data.get("job_id", data.get("jobId", job_id)),
            message=data.get("message", "Response accepted"),
            raw_response=data,
        )

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
        This stays native as it uses local JACS signing.
        """
        from haiai.config import get_config, get_agent

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

        return sign_response(payload, get_agent(), cfg.jacs_id or "")

    # ------------------------------------------------------------------
    # Email CRUD
    # ------------------------------------------------------------------

    def send_email(
        self,
        hai_url: Optional[str] = None,
        to: str = "",
        subject: str = "",
        body: str = "",
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
        cc: Optional[list[str]] = None,
        bcc: Optional[list[str]] = None,
        labels: Optional[list[str]] = None,
    ) -> SendEmailResult:
        """Send an email from this agent's @hai.ai address."""
        if not to:
            raise ValueError("'to' is required")
        if not subject:
            raise ValueError("'subject' is required")
        if not body:
            raise ValueError("'body' is required")
        if self._agent_email is None:
            raise HaiError("agent email not set -- register with a username first")

        ffi = self._get_ffi()
        options: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            options["in_reply_to"] = in_reply_to
        if cc:
            options["cc"] = cc
        if bcc:
            options["bcc"] = bcc
        if labels:
            options["labels"] = labels
        if attachments:
            options["attachments"] = [
                {
                    "filename": a["filename"],
                    "content_type": a["content_type"],
                    "data_base64": base64.b64encode(a["data"]).decode(),
                }
                for a in attachments
            ]

        data = ffi.send_email(options)
        return SendEmailResult(
            message_id=data.get("message_id", ""),
            status=data.get("status", "sent"),
        )

    def sign_email(self, hai_url: Optional[str] = None, raw_email: bytes = b"") -> bytes:
        """Sign a raw RFC 5822 email via the HAI server."""
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()
        if not raw_email:
            raise ValueError("'raw_email' is required")

        ffi = self._get_ffi()
        b64_input = base64.b64encode(raw_email).decode("ascii")
        b64_result = ffi.sign_email_raw(b64_input)
        return base64.b64decode(b64_result)

    def send_signed_email(
        self,
        hai_url: Optional[str] = None,
        to: str = "",
        subject: str = "",
        body: str = "",
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
        cc: Optional[list[str]] = None,
        bcc: Optional[list[str]] = None,
        labels: Optional[list[str]] = None,
    ) -> SendEmailResult:
        """Send an agent-signed email.

        Builds RFC 5322 MIME, signs with the agent's JACS key via the Rust
        FFI layer, and submits to the HAI API. The server validates the
        signature, countersigns, and delivers.
        """
        if not to:
            raise ValueError("'to' is required")
        if not subject:
            raise ValueError("'subject' is required")
        if not body:
            raise ValueError("'body' is required")
        if self._agent_email is None:
            raise HaiError("agent email not set -- register with a username first")

        ffi = self._get_ffi()
        options: dict[str, Any] = {
            "to": to,
            "subject": subject,
            "body": body,
        }
        if in_reply_to is not None:
            options["in_reply_to"] = in_reply_to
        if cc:
            options["cc"] = cc
        if bcc:
            options["bcc"] = bcc
        if labels:
            options["labels"] = labels
        if attachments:
            options["attachments"] = [
                {
                    "filename": a["filename"],
                    "content_type": a["content_type"],
                    "data_base64": base64.b64encode(a["data"]).decode(),
                }
                for a in attachments
            ]

        data = ffi.send_signed_email(options)
        return SendEmailResult(
            message_id=data.get("message_id", ""),
            status=data.get("status", "sent"),
        )

    def verify_email(self, hai_url: Optional[str] = None, raw_email: bytes = b"") -> EmailVerificationResultV2:
        """Verify a JACS-signed email via the HAI API."""
        import email.message
        if isinstance(raw_email, email.message.EmailMessage):
            raw_email = raw_email.as_bytes()
        if not raw_email:
            raise ValueError("'raw_email' is required")

        ffi = self._get_ffi()
        b64_input = base64.b64encode(raw_email).decode("ascii")
        data = ffi.verify_email_raw(b64_input)
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

    def list_messages(
        self,
        hai_url: Optional[str] = None,
        limit: int = 20,
        offset: int = 0,
        direction: Optional[str] = None,
        is_read: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
        has_attachments: Optional[bool] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
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
        if has_attachments is not None:
            options["has_attachments"] = has_attachments
        if since is not None:
            options["since"] = since
        if until is not None:
            options["until"] = until

        items = ffi.list_messages(options)
        messages = items if isinstance(items, list) else items.get("messages", [])
        return [EmailMessage.from_dict(m) for m in messages]

    def mark_read(self, hai_url: Optional[str] = None, message_id: str = "") -> bool:
        """Mark an email message as read."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        ffi.mark_read(message_id)
        return True

    def get_email_status(self, hai_url: Optional[str] = None) -> EmailStatus:
        """Get email rate-limit and reputation status."""
        ffi = self._get_ffi()
        data = ffi.get_email_status()
        return self._parse_email_status(data)

    def get_message(self, hai_url: Optional[str] = None, message_id: str = "") -> EmailMessage:
        """Get a single email message by ID."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        m = ffi.get_message(message_id)
        return EmailMessage.from_dict(m)

    def get_raw_email(
        self, hai_url: Optional[str] = None, message_id: str = ""
    ) -> RawEmailResult:
        """Fetch the raw RFC 5322 bytes of a message for local JACS verification.

        Returns :class:`RawEmailResult` with ``raw_email`` as ``bytes`` on the
        happy path, or ``None`` when ``available`` is False (see
        ``omitted_reason``). Pair with :meth:`verify_email` to verify offline.

        Byte-fidelity (PRD R2): ``raw_email`` is byte-identical to what JACS
        signed — no trimming, no line-ending normalization.
        """
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        data = ffi.get_raw_email(message_id)
        return RawEmailResult.from_dict(data)

    def delete_message(self, hai_url: Optional[str] = None, message_id: str = "") -> bool:
        """Delete an email message."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        ffi.delete_message(message_id)
        return True

    def mark_unread(self, hai_url: Optional[str] = None, message_id: str = "") -> bool:
        """Mark an email message as unread."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        ffi.mark_unread(message_id)
        return True

    def search_messages(
        self,
        hai_url: Optional[str] = None,
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
        has_attachments: Optional[bool] = None,
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
        if has_attachments is not None:
            options["has_attachments"] = has_attachments

        items = ffi.search_messages(options)
        messages = items if isinstance(items, list) else items.get("messages", [])
        return [EmailMessage.from_dict(m) for m in messages]

    def get_unread_count(self, hai_url: Optional[str] = None) -> int:
        """Get the number of unread email messages."""
        ffi = self._get_ffi()
        return ffi.get_unread_count()

    def reply(
        self,
        hai_url: Optional[str] = None,
        message_id: str = "",
        body: str = "",
        subject: Optional[str] = None,
    ) -> SendEmailResult:
        """Reply to an email message. Always JACS-signed."""
        if not message_id:
            raise ValueError("'message_id' is required")
        if not body:
            raise ValueError("'body' is required")
        original = self.get_message(hai_url, message_id)
        # Sanitize: strip CR/LF that may be present from email header folding.
        clean_subject = (original.subject or "").replace("\r", "").replace("\n", "")
        reply_subject = subject if subject is not None else f"Re: {clean_subject}"
        return self.send_signed_email(
            hai_url,
            to=original.from_address,
            subject=reply_subject,
            body=body,
            in_reply_to=original.message_id,
        )

    def forward(
        self,
        hai_url: Optional[str] = None,
        message_id: str = "",
        to: str = "",
        comment: Optional[str] = None,
    ) -> SendEmailResult:
        """Forward an email message to another recipient."""
        if not message_id:
            raise ValueError("'message_id' is required")
        if not to:
            raise ValueError("'to' is required")
        ffi = self._get_ffi()
        params: dict[str, Any] = {
            "message_id": message_id,
            "to": to,
        }
        if comment is not None:
            params["comment"] = comment

        data = ffi.forward(params)
        return SendEmailResult(
            message_id=data.get("message_id", ""),
            status=data.get("status", ""),
        )

    def archive(self, hai_url: Optional[str] = None, message_id: str = "") -> bool:
        """Archive an email message."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        ffi.archive(message_id)
        return True

    def unarchive(self, hai_url: Optional[str] = None, message_id: str = "") -> bool:
        """Unarchive an email message."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        ffi.unarchive(message_id)
        return True

    def update_labels(
        self,
        hai_url: Optional[str] = None,
        message_id: str = "",
        add: Optional[list[str]] = None,
        remove: Optional[list[str]] = None,
    ) -> list[str]:
        """Update labels on an email message."""
        if not message_id:
            raise ValueError("'message_id' is required")
        ffi = self._get_ffi()
        data = ffi.update_labels({
            "message_id": message_id,
            "add": add or [],
            "remove": remove or [],
        })
        return data.get("labels", [])

    def _ensure_agent_email(self, hai_url: Optional[str] = None) -> None:
        """Auto-discover agent_email from email status if not already set.

        Mirrors the MCP server's ``prepare_email_client`` pattern:
        call ``get_email_status`` to learn the agent's email, then
        set it on the FFI client so ``contacts()`` and other
        email-dependent calls succeed.
        """
        ffi = self._get_ffi()
        if self._agent_email is not None:
            return
        try:
            status = self.get_email_status(hai_url)
            if status.email:
                ffi.set_agent_email(status.email)
                self._agent_email = status.email
        except Exception:
            pass

    def contacts(self, hai_url: Optional[str] = None) -> list["Contact"]:
        """List contacts derived from email message history."""
        self._ensure_agent_email(hai_url)
        ffi = self._get_ffi()
        items = ffi.contacts()
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

    def create_email_template(
        self,
        hai_url: Optional[str] = None,
        name: str = "",
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
        return ffi.create_email_template(options)

    def list_email_templates(
        self,
        hai_url: Optional[str] = None,
        limit: int = 20,
        offset: int = 0,
        q: Optional[str] = None,
    ) -> dict:
        """List or search email templates."""
        ffi = self._get_ffi()
        options: dict[str, Any] = {"limit": limit, "offset": offset}
        if q is not None:
            options["q"] = q
        return ffi.list_email_templates(options)

    def get_email_template(self, hai_url: Optional[str] = None, template_id: str = "") -> dict:
        """Get a single email template by ID."""
        ffi = self._get_ffi()
        return ffi.get_email_template(template_id)

    def update_email_template(
        self,
        hai_url: Optional[str] = None,
        template_id: str = "",
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
        return ffi.update_email_template(template_id, options)

    def delete_email_template(self, hai_url: Optional[str] = None, template_id: str = "") -> None:
        """Delete an email template."""
        ffi = self._get_ffi()
        ffi.delete_email_template(template_id)

    # ------------------------------------------------------------------
    # fetch_remote_key
    # ------------------------------------------------------------------

    def fetch_remote_key(
        self,
        hai_url: Optional[str] = None,
        jacs_id: str = "",
        version: str = "latest",
    ) -> PublicKeyInfo:
        """Fetch another agent's public key from HAI."""
        if not jacs_id:
            raise ValueError("'jacs_id' is required")
        cache_key = f"remote:{jacs_id}:{version}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        ffi = self._get_ffi()
        data = ffi.fetch_remote_key(jacs_id, version)
        result = self._parse_public_key_info(data, jacs_id=jacs_id, version=version)
        self._set_cached_key(cache_key, result)
        return result

    def fetch_key_by_hash(
        self,
        hai_url: Optional[str] = None,
        public_key_hash: str = "",
    ) -> PublicKeyInfo:
        """Fetch an agent's public key by its SHA-256 hash."""
        if not public_key_hash:
            raise ValueError("'public_key_hash' is required")
        cache_key = f"hash:{public_key_hash}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        ffi = self._get_ffi()
        data = ffi.fetch_key_by_hash(public_key_hash)
        result = self._parse_public_key_info(data)
        self._set_cached_key(cache_key, result)
        return result

    def fetch_key_by_email(
        self,
        hai_url: Optional[str] = None,
        email: str = "",
    ) -> PublicKeyInfo:
        """Fetch an agent's public key by their ``@hai.ai`` email address."""
        if not email:
            raise ValueError("'email' is required")
        cache_key = f"email:{email}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        ffi = self._get_ffi()
        data = ffi.fetch_key_by_email(email)
        result = self._parse_public_key_info(data)
        self._set_cached_key(cache_key, result)
        return result

    def fetch_key_by_domain(
        self,
        hai_url: Optional[str] = None,
        domain: str = "",
    ) -> PublicKeyInfo:
        """Fetch the latest DNS-verified agent key for a domain."""
        if not domain:
            raise ValueError("'domain' is required")
        cache_key = f"domain:{domain}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        ffi = self._get_ffi()
        data = ffi.fetch_key_by_domain(domain)
        result = self._parse_public_key_info(data)
        self._set_cached_key(cache_key, result)
        return result

    def fetch_all_keys(
        self,
        hai_url: Optional[str] = None,
        jacs_id: str = "",
    ) -> dict:
        """Fetch all key versions for an agent."""
        if not jacs_id:
            raise ValueError("'jacs_id' is required")
        ffi = self._get_ffi()
        return ffi.fetch_all_keys(jacs_id)

    # ------------------------------------------------------------------
    # connect (SSE + WS) -- via FFI opaque handles
    # ------------------------------------------------------------------

    def connect(
        self,
        hai_url: Optional[str] = None,
        *,
        transport: str = "sse",
    ) -> Iterator[HaiEvent]:
        """Connect to HAI and yield events.

        Args:
            hai_url: Base URL of the HAI server (defaults to DEFAULT_BASE_URL).
            transport: ``"sse"`` or ``"ws"``.

        Yields:
            HaiEvent instances.
        """
        hai_url = hai_url or DEFAULT_BASE_URL
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
        """Stream events via SSE using FFI opaque handles."""
        from haiai._retry import RETRY_MAX_ATTEMPTS, backoff

        ffi = self._get_ffi()
        attempt = 0
        while not self._should_disconnect:
            handle = None
            try:
                handle = ffi.connect_sse()
                self._connected = True
                attempt = 0  # Reset on successful connect

                while not self._should_disconnect:
                    event_data = ffi.sse_next_event(handle)
                    if event_data is None:
                        # Connection closed
                        break

                    event = _client_shared.make_ffi_event(event_data)
                    if event.id:
                        self._last_event_id = event.id
                    yield event

            except HaiAuthError:
                raise
            except Exception as exc:
                if attempt >= RETRY_MAX_ATTEMPTS:
                    raise HaiConnectionError(
                        f"SSE connection failed after {attempt} attempts: {exc}"
                    )
                delay = backoff(attempt)
                logger.warning(
                    "SSE connection lost, retrying in %.1fs: %s", delay, exc
                )
                time.sleep(delay)
                attempt += 1
            finally:
                self._connected = False
                if handle is not None:
                    try:
                        ffi.sse_close(handle)
                    except Exception:
                        pass

    def _connect_ws(self, hai_url: str) -> Iterator[HaiEvent]:
        """Stream events via WebSocket using FFI opaque handles."""
        from haiai._retry import RETRY_MAX_ATTEMPTS, backoff

        ffi = self._get_ffi()
        attempt = 0
        while not self._should_disconnect:
            handle = None
            try:
                handle = ffi.connect_ws()
                self._connected = True
                attempt = 0

                while not self._should_disconnect:
                    event_data = ffi.ws_next_event(handle)
                    if event_data is None:
                        break

                    event = _client_shared.make_ffi_event(event_data)
                    yield event

            except HaiAuthError:
                raise
            except Exception as exc:
                if attempt >= RETRY_MAX_ATTEMPTS:
                    raise HaiConnectionError(
                        f"WS connection failed after {attempt} attempts: {exc}"
                    )
                delay = backoff(attempt)
                logger.warning(
                    "WS connection lost, retrying in %.1fs: %s", delay, exc
                )
                time.sleep(delay)
                attempt += 1
            finally:
                self._connected = False
                if handle is not None:
                    try:
                        ffi.ws_close(handle)
                    except Exception:
                        pass

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


def testconnection(hai_url: Optional[str] = None) -> bool:
    """Test connectivity to the HAI server."""
    return _get_client().testconnection(hai_url)


def hello_world(hai_url: Optional[str] = None, include_test: bool = False) -> HelloWorldResult:
    """Perform a hello world exchange with HAI."""
    return _get_client().hello_world(hai_url, include_test)


def register(
    hai_url: Optional[str] = None,
    preview: bool = False,
    owner_email: Optional[str] = None,
) -> Union[HaiRegistrationResult, HaiRegistrationPreview]:
    """Register the loaded JACS agent with HAI."""
    return _get_client().register(hai_url, preview=preview, owner_email=owner_email)


def status(hai_url: Optional[str] = None) -> HaiStatusResult:
    """Check registration status of the current agent."""
    return _get_client().status(hai_url)


def update_username(hai_url: Optional[str] = None, agent_id: str = "", username: str = "") -> dict[str, Any]:
    """Update (rename) a claimed username for an agent."""
    return _get_client().update_username(hai_url, agent_id, username)


def delete_username(hai_url: Optional[str] = None, agent_id: str = "") -> dict[str, Any]:
    """Delete a claimed username for an agent."""
    return _get_client().delete_username(hai_url, agent_id)


def benchmark(
    hai_url: Optional[str] = None,
    name: str = "mediator",
    tier: str = "free",
) -> BenchmarkResult:
    """Run a benchmark via HAI."""
    return _get_client().benchmark(hai_url, name=name, tier=tier)


def free_run(
    hai_url: Optional[str] = None, transport: str = "sse"
) -> FreeChaoticResult:
    """Run a free benchmark."""
    return _get_client().free_run(hai_url, transport)


def pro_run(
    hai_url: Optional[str] = None, transport: str = "sse",
) -> BaselineRunResult:
    """Run a pro tier benchmark ($20/month)."""
    return _get_client().pro_run(hai_url, transport)


# Deprecated alias for backward compatibility
dns_certified_run = pro_run


def enterprise_run(**kwargs: Any) -> None:
    """Run an enterprise tier benchmark.

    The enterprise tier is coming soon.
    Contact support@hai.ai for early access.
    """
    raise NotImplementedError(
        "The enterprise tier is coming soon. "
        "Contact support@hai.ai for early access."
    )


# Deprecated alias for backward compatibility
certified_run = enterprise_run


def submit_benchmark_response(
    hai_url: Optional[str] = None,
    job_id: str = "",
    message: str = "",
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
    hai_url: Optional[str] = None,
    *,
    transport: str = "sse",
) -> Iterator[HaiEvent]:
    """Connect to HAI event stream."""
    return _get_client().connect(hai_url, transport=transport)


def disconnect() -> None:
    """Disconnect from the HAI event stream."""
    _get_client().disconnect()


def send_email(
    hai_url: Optional[str] = None,
    to: str = "",
    subject: str = "",
    body: str = "",
    in_reply_to: Optional[str] = None,
    attachments: Optional[list[dict[str, Any]]] = None,
    cc: Optional[list[str]] = None,
    bcc: Optional[list[str]] = None,
    labels: Optional[list[str]] = None,
) -> SendEmailResult:
    """Send an email from this agent's @hai.ai address."""
    return _get_client().send_email(
        hai_url, to, subject, body, in_reply_to,
        attachments=attachments, cc=cc, bcc=bcc, labels=labels,
    )


def sign_email(hai_url: Optional[str] = None, raw_email: bytes = b"") -> bytes:
    """Sign a raw RFC 5322 email with a JACS attachment via the HAI API."""
    return _get_client().sign_email(hai_url, raw_email)


def send_signed_email(
    hai_url: Optional[str] = None,
    to: str = "",
    subject: str = "",
    body: str = "",
    in_reply_to: Optional[str] = None,
    attachments: Optional[list[dict[str, Any]]] = None,
    cc: Optional[list[str]] = None,
    bcc: Optional[list[str]] = None,
    labels: Optional[list[str]] = None,
) -> SendEmailResult:
    """Send an agent-signed email (builds MIME, signs, and sends)."""
    return _get_client().send_signed_email(
        hai_url, to, subject, body, in_reply_to,
        attachments=attachments, cc=cc, bcc=bcc, labels=labels,
    )


def verify_email(hai_url: Optional[str] = None, raw_email: bytes = b"") -> EmailVerificationResultV2:
    """Verify a JACS-signed email via the HAI API."""
    return _get_client().verify_email(hai_url, raw_email)


def list_messages(
    hai_url: Optional[str] = None,
    limit: int = 20,
    offset: int = 0,
    direction: Optional[str] = None,
    is_read: Optional[bool] = None,
    folder: Optional[str] = None,
    label: Optional[str] = None,
    has_attachments: Optional[bool] = None,
    since: Optional[str] = None,
    until: Optional[str] = None,
) -> list[EmailMessage]:
    """List email messages for this agent."""
    return _get_client().list_messages(
        hai_url, limit, offset, direction,
        is_read=is_read, folder=folder, label=label,
        has_attachments=has_attachments, since=since, until=until,
    )


def mark_read(hai_url: Optional[str] = None, message_id: str = "") -> bool:
    """Mark an email message as read."""
    return _get_client().mark_read(hai_url, message_id)


def get_email_status(hai_url: Optional[str] = None) -> EmailStatus:
    """Get email rate-limit and reputation status."""
    return _get_client().get_email_status(hai_url)


def get_message(hai_url: Optional[str] = None, message_id: str = "") -> EmailMessage:
    """Get a single email message by ID."""
    return _get_client().get_message(hai_url, message_id)


def delete_message(hai_url: Optional[str] = None, message_id: str = "") -> bool:
    """Delete an email message."""
    return _get_client().delete_message(hai_url, message_id)


def mark_unread(hai_url: Optional[str] = None, message_id: str = "") -> bool:
    """Mark an email message as unread."""
    return _get_client().mark_unread(hai_url, message_id)


def search_messages(
    hai_url: Optional[str] = None,
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
    has_attachments: Optional[bool] = None,
    limit: int = 20,
    offset: int = 0,
) -> list[EmailMessage]:
    """Search email messages."""
    return _get_client().search_messages(
        hai_url, q=q, direction=direction, from_address=from_address,
        to_address=to_address, since=since, until=until,
        is_read=is_read, jacs_verified=jacs_verified,
        folder=folder, label=label, has_attachments=has_attachments,
        limit=limit, offset=offset,
    )


def get_unread_count(hai_url: Optional[str] = None) -> int:
    """Get the number of unread email messages."""
    return _get_client().get_unread_count(hai_url)


def reply(
    hai_url: Optional[str] = None,
    message_id: str = "",
    body: str = "",
    subject: Optional[str] = None,
) -> SendEmailResult:
    """Reply to an email message."""
    return _get_client().reply(hai_url, message_id, body, subject)


def forward(
    hai_url: Optional[str] = None,
    message_id: str = "",
    to: str = "",
    comment: Optional[str] = None,
) -> SendEmailResult:
    """Forward an email message to another recipient."""
    return _get_client().forward(hai_url, message_id, to, comment)


def archive(hai_url: Optional[str] = None, message_id: str = "") -> bool:
    """Archive an email message."""
    return _get_client().archive(hai_url, message_id)


def unarchive(hai_url: Optional[str] = None, message_id: str = "") -> bool:
    """Unarchive an email message."""
    return _get_client().unarchive(hai_url, message_id)


def contacts(hai_url: Optional[str] = None) -> list:
    """List contacts derived from email history."""
    return _get_client().contacts(hai_url)


def update_labels(
    hai_url: Optional[str] = None,
    message_id: str = "",
    add: Optional[list[str]] = None,
    remove: Optional[list[str]] = None,
) -> list[str]:
    """Update labels on an email message."""
    return _get_client().update_labels(hai_url, message_id, add=add, remove=remove)


def rotate_keys(
    hai_url: Optional[str] = None,
    register_with_hai: bool = True,
    config_path: Optional[str] = None,
    algorithm: str = "pq2025",
) -> RotationResult:
    """Rotate the agent's cryptographic keys."""
    return _get_client().rotate_keys(
        hai_url, register_with_hai=register_with_hai,
        config_path=config_path, algorithm=algorithm,
    )


def fetch_remote_key(
    hai_url: Optional[str] = None,
    jacs_id: str = "",
    version: str = "latest",
) -> PublicKeyInfo:
    """Fetch another agent's public key from HAI."""
    return _get_client().fetch_remote_key(hai_url, jacs_id, version)


def fetch_key_by_hash(hai_url: Optional[str] = None, public_key_hash: str = "") -> PublicKeyInfo:
    """Fetch an agent's public key by its SHA-256 hash."""
    return _get_client().fetch_key_by_hash(hai_url, public_key_hash)


def fetch_key_by_email(hai_url: Optional[str] = None, email: str = "") -> PublicKeyInfo:
    """Fetch an agent's public key by their ``@hai.ai`` email address."""
    return _get_client().fetch_key_by_email(hai_url, email)


def fetch_key_by_domain(hai_url: Optional[str] = None, domain: str = "") -> PublicKeyInfo:
    """Fetch the latest DNS-verified agent key for a domain."""
    return _get_client().fetch_key_by_domain(hai_url, domain)


def fetch_all_keys(hai_url: Optional[str] = None, jacs_id: str = "") -> dict:
    """Fetch all key versions for an agent."""
    return _get_client().fetch_all_keys(hai_url, jacs_id)


def verify_document(
    hai_url: Optional[str] = None,
    document: Union[str, dict[str, Any]] = "",
) -> dict[str, Any]:
    """Verify a signed JACS document via HAI's public verify endpoint."""
    return _get_client().verify_document(hai_url, document)


def get_verification(hai_url: Optional[str] = None, agent_id: str = "") -> dict[str, Any]:
    """Get advanced 3-level verification status for an agent."""
    return _get_client().get_verification(hai_url, agent_id)


def verify_agent_document(
    hai_url: Optional[str] = None,
    agent_json: Union[str, dict[str, Any]] = "",
    *,
    public_key: Optional[str] = None,
    domain: Optional[str] = None,
) -> dict[str, Any]:
    """Verify an agent document via HAI's advanced verification endpoint."""
    return _get_client().verify_agent_document(
        hai_url,
        agent_json,
        public_key=public_key,
        domain=domain,
    )


# ---------------------------------------------------------------------------
# generate_verify_link
# ---------------------------------------------------------------------------


def _encode_verify_payload(document: str) -> str:
    """URL-safe base64 encoding for verification payloads."""
    from haiai.config import is_loaded, get_agent

    if not is_loaded():
        raise HaiError(
            "encode_verify_payload requires a loaded JACS agent",
            code="JACS_NOT_LOADED",
            action="Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
        )

    agent = get_agent()
    if hasattr(agent, "encode_verify_payload"):
        return agent.encode_verify_payload(document)

    return base64.urlsafe_b64encode(
        document.encode("utf-8")
    ).rstrip(b"=").decode("ascii")


def generate_verify_link(
    document: str,
    base_url: str = DEFAULT_BASE_URL,
    hosted: Optional[bool] = None,
) -> str:
    """Build a verification URL for a signed JACS document.

    TODO: This link cannot be embedded in the email it verifies — the signed body would need to
    contain its own base64 encoding (chicken-and-egg), and hosting the content behind a token
    creates a public access path to private messages. Per-message verification is therefore
    recipient-initiated: paste the raw email at /verify.
    """
    base = base_url.rstrip("/")

    if hosted is None:
        hosted = False

    if not hosted:
        encoded = _encode_verify_payload(document)
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
    hai_url: str = DEFAULT_BASE_URL,
    key_dir: Optional[str] = None,
    config_path: str = "./jacs.config.json",
    domain: Optional[str] = None,
    description: Optional[str] = None,
    quiet: bool = False,
    algorithm: str = "pq2025",
    registration_key: Optional[str] = None,
) -> RegistrationResult:
    """Generate a keypair, self-sign, register with HAI, and save config.

    Delegates entirely to the Rust FFI binding which handles keygen, doc
    creation, signing, and the HTTP registration call.
    """
    if not owner_email:
        raise ValueError(
            "owner_email is required -- agents must be associated with a verified HAI user"
        )

    from haiai import config as hai_config

    private_key_password = hai_config.load_private_key_password()
    password_str = private_key_password.decode("utf-8")

    kd = Path(key_dir).expanduser() if key_dir else (Path.home() / ".jacs" / "keys")
    data_dir = kd.parent / "data"

    # Build FFI options -- Rust handles keygen, doc creation, signing, and HTTP
    options: dict[str, Any] = {
        "agent_name": name,
        "password": password_str,
        "algorithm": algorithm,
        "owner_email": owner_email,
        "base_url": hai_url,
        "key_directory": str(kd),
        "data_directory": str(data_dir),
        "config_path": str(Path(config_path).resolve()),
        "description": description or "Agent registered via Python SDK",
    }
    if domain:
        options["domain"] = domain
    if registration_key:
        options["registration_key"] = registration_key

    # Create a temporary FFI adapter with minimal config (just base_url)
    ffi_config = json.dumps({"base_url": hai_url})
    ffi = FFIAdapter(ffi_config)
    data = ffi.register_new_agent(options)

    agent_id = str(data.get("agent_id", ""))
    jacs_id = str(data.get("jacs_id", ""))
    key_directory = data.get("key_directory", str(kd))

    # Update module state with returned identifiers
    hai_config._config = hai_config.AgentConfig(
        name=name,
        version=version,
        key_dir=str(Path(key_directory).resolve()),
        jacs_id=jacs_id,
    )
    global _client
    _client = None

    # Print next-step messaging
    if not quiet:
        print(f"\nAgent created and submitted for registration!")
        print(f"  -> Check your email ({owner_email}) for a verification link")
        print(f"  -> Your agent is registered with username from your reservation")
        print(f"  -> Config saved to {config_path}")
        print(f"  -> Keys saved to {key_directory}")
        print(
            "  -> Private key encrypted using JACS_PASSWORD_FILE/JACS_PRIVATE_KEY_PASSWORD"
        )

        if domain:
            # Read public key PEM for DNS hash if available
            pub_key_path = Path(data.get("public_key_path", ""))
            if pub_key_path.is_file():
                public_pem = pub_key_path.read_text(encoding="utf-8")
                key_hash = _compute_public_key_hash(public_pem)
                print(f"\n--- DNS Setup Instructions ---")
                print(f"Add this TXT record to your domain '{domain}':")
                print(f"  Name:  _jacs.{domain}")
                print(f"  Type:  TXT")
                print(f"  Value: {key_hash}")
                print(f"DNS verification enables the pro tier.\n")
            else:
                dns_record = data.get("dns_record", "")
                if dns_record:
                    print(f"\n--- DNS Setup Instructions ---")
                    print(f"Add this TXT record to your domain '{domain}':")
                    print(f"  Name:  _jacs.{domain}")
                    print(f"  Type:  TXT")
                    print(f"  Value: {dns_record}")
                    print(f"DNS verification enables the pro tier.\n")
                else:
                    print()
        else:
            print()

    return RegistrationResult(agent_id=agent_id, jacs_id=jacs_id)


def _compute_public_key_hash(pem: str) -> str:
    """Compute SHA-256 hash of a PEM public key, matching Rust API format."""
    import hashlib
    digest = hashlib.sha256(pem.encode("utf-8")).hexdigest()
    return f"sha256:{digest}"


def _verify_dns(domain: str, public_key_pem: str) -> tuple[bool, str]:
    """Verify DNS TXT record for Level 2 domain verification."""
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
    hai_url: str = DEFAULT_BASE_URL,
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
    from haiai.signing import verify_string as _verify_string

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

    if isinstance(jacs_sig, dict):
        sig_b64 = jacs_sig.get("signature", "")
    elif isinstance(jacs_sig, str):
        sig_b64 = jacs_sig
    else:
        sig_b64 = ""

    if sig_b64 and pub_key_pem:
        try:
            import copy
            signing_doc = copy.deepcopy(doc)
            if isinstance(signing_doc.get("jacsSignature"), dict):
                signing_doc["jacsSignature"].pop("signature", None)
            else:
                del signing_doc["jacsSignature"]
            canonical = canonicalize_json(signing_doc)
            jacs_valid = _verify_string(canonical, sig_b64, pub_key_pem)
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
