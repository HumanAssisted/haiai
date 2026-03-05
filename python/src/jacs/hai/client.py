"""HaiClient -- full-featured client for the HAI benchmark platform.

Ports every public method from the JACS monolith (jacs.hai) with:
  - JACS-only authentication (no API key / Bearer fallback)
  - All signing via JACS binding-core (zero local crypto)
  - SSE and WebSocket transports
  - Retry with exponential backoff
"""

from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import time
import webbrowser
from pathlib import Path
from typing import Any, Generator, Iterator, Optional, Union
from urllib.parse import quote

import httpx

from jacs.hai._retry import RETRY_MAX_ATTEMPTS, backoff, should_retry
from jacs.hai._sse import flatten_benchmark_job, parse_sse_lines
from jacs.hai.signing import canonicalize_json, create_agent_document
from jacs.hai.errors import (
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
from jacs.hai.models import (
    AgentConfig,
    AgentVerificationResult,
    AttestationResult,
    AttestationVerifyResult,
    BaselineRunResult,
    BenchmarkResult,
    ChainEntry,
    EmailMessage,
    EmailStatus,
    EmailVerificationResultV2,
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
    RegistrationResult,
    RotationResult,
    SendEmailResult,
    TranscriptMessage,
)
from jacs.hai.signing import is_signed_event, sign_response, unwrap_signed_event

logger = logging.getLogger("jacs.hai.client")

# Verify link constants (HAI / public verification URLs)
MAX_VERIFY_URL_LEN = 2048
MAX_VERIFY_DOCUMENT_BYTES = 1515


def _read_public_key_pem(cfg: "AgentConfig") -> str:
    """Read the agent's public key PEM from the key directory."""
    key_dir = Path(cfg.key_dir)
    candidates = [
        key_dir / "agent_public_key.pem",
        key_dir / f"{cfg.name}.public.pem",
        key_dir / "public_key.pem",
    ]
    for p in candidates:
        if p.is_file():
            return p.read_text(encoding="utf-8")
    raise FileNotFoundError(
        f"Public key not found. Searched: {', '.join(str(p) for p in candidates)}"
    )


# ---------------------------------------------------------------------------
# HaiClient
# ---------------------------------------------------------------------------


class HaiClient:
    """Client for the HAI benchmark platform.

    Handles JACS-signed authentication and event streaming over SSE or
    WebSocket.  All operations require a loaded JACS config (via
    ``jacs.hai.config.load()``).  There is **no API-key fallback**.

    Example::

        from haisdk import config, HaiClient

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
        self._hai_agent_id: Optional[str] = None
        self._agent_email: Optional[str] = None
        # Agent key cache: maps cache_key -> (PublicKeyInfo, cached_at_monotonic)
        self._key_cache: dict[str, tuple[Any, float]] = {}
        self._KEY_CACHE_TTL: float = 300.0  # 5 minutes

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------

    @property
    def agent_email(self) -> Optional[str]:
        """The agent's ``@hai.ai`` email address, set after ``claim_username``."""
        return self._agent_email

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _make_url(base_url: str, path: str) -> str:
        """Construct a full URL from base and path."""
        base = base_url.rstrip("/")
        path = "/" + path.lstrip("/")
        return base + path

    @staticmethod
    def _escape_path_segment(value: str) -> str:
        """Escape a user-controlled URL path segment."""
        return quote(value, safe="")

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
        from jacs.hai.config import get_config

        cfg = get_config()
        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required in config for JACS authentication")
        return cfg.jacs_id

    def _get_hai_agent_id(self) -> str:
        """Return the HAI-assigned agent UUID for email URL paths.

        Falls back to the JACS ID if not set (e.g. before registration).
        """
        return self._hai_agent_id or self._get_jacs_id()

    def _build_jacs_auth_header(self) -> str:
        """Build ``Authorization: JACS {jacsId}:{timestamp}:{signature}``.

        The signed message is ``"{jacsId}:{timestamp}"`` matching the Rust
        ``extract_jacs_credentials`` parser.
        Signing delegates to JACS binding-core via the loaded JacsAgent.
        """
        from jacs.hai.config import get_config, get_agent

        cfg = get_config()
        agent = get_agent()

        if cfg.jacs_id is None:
            raise HaiAuthError("jacsId is required for JACS authentication")

        timestamp = int(time.time())
        message = f"{cfg.jacs_id}:{timestamp}"
        signature = agent.sign_string(message)
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
        timestamp = int(time.time())
        message = f"{jacs_id}:{version}:{timestamp}"
        signature = agent.sign_string(message)
        return f"JACS {jacs_id}:{version}:{timestamp}:{signature}"

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

        Verification delegates to JACS binding-core.

        Args:
            message: The message string that was signed.
            signature: Base64-encoded signature.
            hai_public_key: HAI's public key (PEM, base64 raw, or key ID).
            hai_url: HAI server URL (for key ID lookup).

        Returns:
            True if signature is valid.
        """
        if not signature or not message:
            return False

        if not hai_public_key:
            return False

        from jacs.hai.signing import verify_string as _verify_string

        try:
            if hai_public_key.startswith("-----"):
                # PEM-encoded public key
                return _verify_string(message, signature, hai_public_key)
            else:
                # Try base64 raw key first, then treat as key ID
                try:
                    base64.b64decode(hai_public_key)
                    # Wrap raw base64 as PEM
                    pem_key = (
                        "-----BEGIN PUBLIC KEY-----\n"
                        + hai_public_key
                        + "\n-----END PUBLIC KEY-----\n"
                    )
                    return _verify_string(message, signature, pem_key)
                except Exception:
                    # Treat as key ID/fingerprint -- look up from server
                    if not hai_url:
                        return False
                    from jacs.hai.signing import fetch_server_keys

                    keys = fetch_server_keys(hai_url)
                    match = next((k for k in keys if k.key_id == hai_public_key), None)
                    if match is None:
                        return False
                    return _verify_string(message, signature, match.public_key_pem)
        except Exception as exc:
            logger.debug("Signature verification failed: %s", exc)
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
            from jacs.hai.config import get_agent

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
    # key rotation
    # ------------------------------------------------------------------

    def rotate_keys(
        self,
        hai_url: Optional[str] = None,
        register_with_hai: bool = True,
        config_path: Optional[str] = None,
        algorithm: str = "ring-Ed25519",
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
                ``"ring-Ed25519"``). Pass ``"pq2025"`` for post-quantum.

        Returns:
            RotationResult with old/new versions, public key hash, and
            whether re-registration succeeded.

        Raises:
            HaiAuthError: If no agent is currently loaded.
            RegistrationError: Only if re-registration fails and
                ``register_with_hai=True``, but the local rotation is
                still preserved.
        """
        import hashlib
        import shutil
        import tempfile
        import uuid

        from jacs.hai import config as config_mod
        from jacs.hai.signing import create_agent_document

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
        # Look for the private key file (same search order as config.load)
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

        # Find matching public key
        pub_path = key_dir / priv_path.name.replace("private", "public")
        if not pub_path.is_file():
            # Try common alternatives
            for name in ["agent_public_key.pem", "jacs.public.pem", f"{cfg.name}.public.pem", "public_key.pem"]:
                alt = key_dir / name
                if alt.is_file():
                    pub_path = alt
                    break

        archive_pub = pub_path.with_suffix(f".{old_version}.pem") if pub_path.is_file() else None

        # 2. Archive old keys
        logger.info("Archiving old private key: %s -> %s", priv_path, archive_priv)
        shutil.move(str(priv_path), str(archive_priv))

        if pub_path.is_file() and archive_pub is not None:
            logger.info("Archiving old public key: %s -> %s", pub_path, archive_pub)
            shutil.move(str(pub_path), str(archive_pub))

        # 3. Generate new keypair via JACS SimpleAgent.create_agent()
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

                # Generate new agent with keys via JACS
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

                # Copy new key files to original locations
                new_priv_src = Path(new_info.get("private_key_path", ""))
                new_pub_src = Path(new_info.get("public_key_path", ""))

                if new_priv_src.is_file():
                    shutil.copy2(str(new_priv_src), str(priv_path))
                    os.chmod(str(priv_path), 0o600)
                if new_pub_src.is_file():
                    shutil.copy2(str(new_pub_src), str(pub_path))
                    os.chmod(str(pub_path), 0o644)

        except Exception as exc:
            # Rollback: restore archived keys
            logger.error("Key generation failed, rolling back: %s", exc)
            shutil.move(str(archive_priv), str(priv_path))
            if archive_pub is not None and archive_pub.is_file():
                shutil.move(str(archive_pub), str(pub_path))
            raise HaiAuthError(f"Key generation failed: {exc}") from exc

        # 4. Use the newly-created agent directly for signing
        new_version = str(uuid.uuid4())

        cfg_path = config_path or os.environ.get(
            "JACS_CONFIG_PATH", "./jacs.config.json"
        )

        # Wrap the new agent for JacsAgent API compatibility
        try:
            from jacs.simple import _EphemeralAgentAdapter
            new_agent = _EphemeralAgentAdapter(_new_agent)
        except ImportError:
            new_agent = _new_agent

        # Update module state with new agent and config
        config_mod._config = AgentConfig(
            name=cfg.name,
            version=new_version,
            key_dir=cfg.key_dir,
            jacs_id=jacs_id,
        )
        config_mod._agent = new_agent
        config_mod.save(cfg_path)

        # 5. Build new agent document signed by the new agent
        agent_doc = create_agent_document(
            agent=new_agent,
            name=cfg.name,
            version=new_version,
            jacs_id=jacs_id,
            extra_fields={"jacsPreviousVersion": old_version},
        )
        signed_agent_json = json.dumps(agent_doc, indent=2)

        # 6. Compute new public key hash
        # Read raw bytes from the JACS key file for public key hash
        if pub_path.is_file():
            pub_key_raw = pub_path.read_bytes()
            new_public_key_hash = hashlib.sha256(pub_key_raw).hexdigest()
        else:
            new_public_key_hash = ""

        logger.info(
            "Key rotation complete: %s -> %s (agent=%s)",
            old_version, new_version, jacs_id,
        )

        # 7. Optionally re-register with HAI using the OLD agent for auth
        # (chain of trust: old agent vouches for new key)
        registered = False
        if register_with_hai:
            if hai_url is None:
                logger.warning(
                    "register_with_hai=True but no hai_url provided; "
                    "skipping registration"
                )
            else:
                try:
                    # Build 4-part auth header signed by the OLD agent
                    old_auth = self._build_jacs_auth_header_with_key(
                        jacs_id, old_version, old_agent,
                    )
                    url = self._make_url(hai_url, "/api/v1/agents/register")
                    payload: dict[str, Any] = {
                        "agent_json": signed_agent_json,
                    }
                    if pub_pem_str:
                        payload["public_key"] = base64.b64encode(
                            pub_pem_str.encode("utf-8")
                        ).decode("utf-8")
                    headers = {
                        "Authorization": old_auth,
                        "Content-Type": "application/json",
                    }
                    resp = httpx.post(
                        url, json=payload, headers=headers, timeout=self._timeout,
                    )
                    if resp.status_code in (200, 201):
                        registered = True
                        logger.info("Re-registered with HAI after rotation")
                    else:
                        logger.warning(
                            "HAI re-registration returned HTTP %d (local rotation preserved)",
                            resp.status_code,
                        )
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

    def status(self, hai_url: str) -> HaiStatusResult:
        """Check registration/verification status of the current agent.

        Calls ``GET /api/v1/agents/{jacs_id}/verify``.

        Args:
            hai_url: Base URL of the HAI server.

        Returns:
            HaiStatusResult with verification details.
        """
        jacs_id = self._get_jacs_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_jacs_id}/verify")
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
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/verify")
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
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
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

            data = resp.json()
            self._agent_email = data.get("email")
            return data

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username claim failed: {exc}")

    def update_username(
        self, hai_url: str, agent_id: str, username: str
    ) -> dict[str, Any]:
        """Update (rename) a claimed username for an agent.

        ``PUT /api/v1/agents/{agent_id}/username`` with body ``{username}``.
        Requires JACS auth.
        """
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
        headers = self._build_auth_headers()

        try:
            resp = httpx.put(
                url,
                headers=headers,
                json={"username": username},
                timeout=self._timeout,
            )

            if resp.status_code == 401:
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
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username update failed: {exc}")

    def delete_username(self, hai_url: str, agent_id: str) -> dict[str, Any]:
        """Delete a claimed username for an agent.

        ``DELETE /api/v1/agents/{agent_id}/username``.
        Requires JACS auth.
        """
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/username")
        headers = self._build_auth_headers()

        try:
            resp = httpx.delete(
                url,
                headers=headers,
                timeout=self._timeout,
            )

            if resp.status_code == 401:
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
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Username delete failed: {exc}")

    def verify_document(
        self,
        hai_url: str,
        document: Union[str, dict[str, Any]],
    ) -> dict[str, Any]:
        """Verify a signed JACS document via HAI's public verify endpoint.

        ``POST /api/jacs/verify`` with body ``{"document": "<json string>"}``.
        This endpoint is public and does not require authentication.
        """
        url = self._make_url(hai_url, "/api/jacs/verify")
        raw_document = document if isinstance(document, str) else json.dumps(document)

        try:
            resp = httpx.post(
                url,
                json={"document": raw_document},
                timeout=self._timeout,
            )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Document verification failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Document verification failed: {exc}")

    def get_verification(
        self,
        hai_url: str,
        agent_id: str,
    ) -> dict[str, Any]:
        """Get advanced 3-level verification status for an agent.

        ``GET /api/v1/agents/{agent_id}/verification``.
        This endpoint is public and does not require authentication.
        """
        safe_agent_id = self._escape_path_segment(agent_id)
        url = self._make_url(hai_url, f"/api/v1/agents/{safe_agent_id}/verification")

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Advanced verification failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Advanced verification failed: {exc}")

    def verify_agent_document(
        self,
        hai_url: str,
        agent_json: Union[str, dict[str, Any]],
        *,
        public_key: Optional[str] = None,
        domain: Optional[str] = None,
    ) -> dict[str, Any]:
        """Verify an agent document via HAI's advanced verification endpoint.

        ``POST /api/v1/agents/verify`` with ``{agent_json, public_key?, domain?}``.
        This endpoint is public and does not require authentication.
        """
        url = self._make_url(hai_url, "/api/v1/agents/verify")
        payload: dict[str, Any] = {
            "agent_json": agent_json if isinstance(agent_json, str) else json.dumps(agent_json),
        }
        if public_key is not None:
            payload["public_key"] = public_key
        if domain is not None:
            payload["domain"] = domain

        try:
            resp = httpx.post(
                url,
                json=payload,
                timeout=self._timeout,
            )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Agent document verification failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return resp.json()
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Agent document verification failed: {exc}")

    # ------------------------------------------------------------------
    # attestation
    # ------------------------------------------------------------------

    def create_attestation(
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
            claims: Array of claim objects (name, value, confidence, etc).
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

        try:
            resp = httpx.post(
                url, json=payload, headers=headers, timeout=self._timeout,
            )
            if resp.status_code == 404:
                raise HaiError(f"Agent '{agent_id}' not registered with HAI")
            if resp.status_code in (401, 403):
                raise HaiAuthError(f"Authentication failed: {resp.text}")
            resp.raise_for_status()
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Failed to create attestation: {exc}")

    def list_attestations(
        self,
        hai_url: str,
        agent_id: str,
        limit: int = 20,
        offset: int = 0,
    ) -> dict:
        """List attestations for a registered agent.

        Args:
            hai_url: Base URL of the HAI server.
            agent_id: The agent's JACS ID.
            limit: Max number of results (default 20).
            offset: Pagination offset.

        Returns:
            Dict with attestations array and total count.
        """
        escaped = self._escape_path_segment(agent_id)
        url = self._make_url(
            hai_url,
            f"/api/v1/agents/{escaped}/attestations?limit={limit}&offset={offset}",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)
            resp.raise_for_status()
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Failed to list attestations: {exc}")

    def get_attestation(
        self,
        hai_url: str,
        agent_id: str,
        doc_id: str,
    ) -> dict:
        """Get a specific attestation document.

        Args:
            hai_url: Base URL of the HAI server.
            agent_id: The agent's JACS ID.
            doc_id: The attestation document ID.

        Returns:
            Dict with attestation, hai_signature, and verification.
        """
        escaped_agent = self._escape_path_segment(agent_id)
        escaped_doc = self._escape_path_segment(doc_id)
        url = self._make_url(
            hai_url,
            f"/api/v1/agents/{escaped_agent}/attestations/{escaped_doc}",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)
            if resp.status_code == 404:
                raise HaiError(
                    f"Attestation '{doc_id}' not found for agent '{agent_id}'"
                )
            resp.raise_for_status()
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Failed to get attestation: {exc}")

    def verify_attestation(
        self,
        hai_url: str,
        document: str,
    ) -> dict:
        """Verify an attestation document via HAI.

        Checks cryptographic validity and whether HAI has co-signed the
        attestation.

        Args:
            hai_url: Base URL of the HAI server.
            document: The attestation JSON document as a string.

        Returns:
            Dict with crypto_valid, evidence_valid, hai_signed, badge_level.
        """
        url = self._make_url(hai_url, "/api/v1/attestations/verify")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        try:
            resp = httpx.post(
                url,
                json={"document": document},
                headers=headers,
                timeout=self._timeout,
            )
            resp.raise_for_status()
            return resp.json()
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Failed to verify attestation: {exc}")

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
        safe_job_id = self._escape_path_segment(job_id)
        url = self._make_url(hai_url, f"/api/benchmark/jobs/{safe_job_id}")
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
        safe_payment_id = self._escape_path_segment(payment_id)
        payment_status_url = self._make_url(
            hai_url, f"/api/benchmark/payments/{safe_payment_id}/status"
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
        from jacs.hai.config import get_config, get_agent

        headers = self._build_auth_headers()
        headers["Content-Type"] = "application/json"

        response_body: dict[str, Any] = {"message": message}
        if metadata is not None:
            response_body["metadata"] = metadata
        response_body["processing_time_ms"] = processing_time_ms

        job_response_payload = {"response": response_body}

        # Always wrap as signed JACS document (signing via JACS binding-core)
        cfg = get_config()
        payload: dict[str, Any] = sign_response(
            job_response_payload, get_agent(), cfg.jacs_id or "",
        )

        safe_job_id = self._escape_path_segment(job_id)
        url = self._make_url(hai_url, f"/api/v1/agents/jobs/{safe_job_id}/response")

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
        from jacs.hai.config import get_config, get_agent

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
        hai_url: str,
        to: str,
        subject: str,
        body: str,
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
    ) -> SendEmailResult:
        """Send an email from this agent's @hai.ai address.

        Args:
            hai_url: Base URL of the HAI server.
            to: Recipient address (must be @hai.ai for MVP).
            subject: Email subject line.
            body: Plain text email body.
            in_reply_to: Optional Message-ID for threading.
            attachments: Optional list of attachment dicts, each with keys
                ``filename`` (str), ``content_type`` (str), and ``data``
                (bytes).  Included in the content hash and sent as
                base64-encoded payloads.

        Returns:
            SendEmailResult with message_id and status.
        """
        if self._agent_email is None:
            raise HaiError("agent email not set -- call claim_username first")

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

        try:
            resp = httpx.post(url, json=payload, headers=headers, timeout=self._timeout)

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
                    status_code=403,
                    body=resp.text,
                )
            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email send auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code == 400 and (err_code == "RECIPIENT_NOT_FOUND" or "Invalid recipient" in resp.text):
                raise RecipientNotFound(
                    err_data.get("message", "Recipient not found"),
                    status_code=400,
                    body=resp.text,
                )
            if resp.status_code == 400 and err_code == "SUBJECT_TOO_LONG":
                raise SubjectTooLong(
                    err_data.get("message", "Subject too long"),
                    status_code=400,
                    body=resp.text,
                )
            if resp.status_code == 400 and err_code == "BODY_TOO_LARGE":
                raise BodyTooLarge(
                    err_data.get("message", "Body too large"),
                    status_code=400,
                    body=resp.text,
                )
            if resp.status_code == 429:
                raise RateLimited(
                    err_data.get("message", "Rate limited"),
                    status_code=429,
                    body=resp.text,
                    resets_at=err_data.get("resets_at", ""),
                )
            if resp.status_code == 400:
                body_lower = resp.text.lower()
                if "recipient" in body_lower:
                    raise RecipientNotFound(
                        f"Recipient not found: {resp.text}",
                        status_code=400,
                        body=resp.text,
                    )
                if "subject" in body_lower:
                    raise SubjectTooLong(
                        f"Subject too long: {resp.text}",
                        status_code=400,
                        body=resp.text,
                    )
                if "body" in body_lower:
                    raise BodyTooLarge(
                        f"Body too large: {resp.text}",
                        status_code=400,
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

    def sign_email(self, hai_url: str, raw_email: bytes) -> bytes:
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

        url = self._make_url(hai_url, "/api/v1/email/sign")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "message/rfc822"

        try:
            resp = httpx.post(url, content=raw_email, headers=headers, timeout=self._timeout)
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

    def verify_email(self, hai_url: str, raw_email: bytes) -> EmailVerificationResultV2:
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

        url = self._make_url(hai_url, "/api/v1/email/verify")
        headers = self._build_auth_headers()
        headers["Content-Type"] = "message/rfc822"

        try:
            resp = httpx.post(url, content=raw_email, headers=headers, timeout=self._timeout)
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
            )
        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email verify failed: {exc}")

    def list_messages(
        self,
        hai_url: str,
        limit: int = 20,
        offset: int = 0,
        direction: Optional[str] = None,
    ) -> list[EmailMessage]:
        """List email messages for this agent.

        Args:
            hai_url: Base URL of the HAI server.
            limit: Max messages to return.
            offset: Pagination offset.
            direction: Filter by direction ("inbound" or "outbound").

        Returns:
            List of EmailMessage objects.
        """
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/messages")
        headers = self._build_auth_headers()

        params: dict[str, Any] = {"limit": limit, "offset": offset}
        if direction is not None:
            params["direction"] = direction

        try:
            resp = httpx.get(
                url,
                params=params,
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
                    body_text=m.get("body_text", ""),
                    created_at=m.get("created_at", ""),
                    direction=m.get("direction", ""),
                    message_id=m.get("message_id", ""),
                    in_reply_to=m.get("in_reply_to"),
                    is_read=m.get("is_read", False),
                    delivery_status=m.get("delivery_status", ""),
                    read_at=m.get("read_at"),
                    jacs_verified=m.get("jacs_verified"),
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
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/read",
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
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/status")
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
                email=data.get("email", ""),
                status=data.get("status", ""),
                tier=data.get("tier", ""),
                billing_tier=data.get("billing_tier", ""),
                messages_sent_24h=int(data.get("messages_sent_24h", 0)),
                daily_limit=int(data.get("daily_limit", 0)),
                daily_used=int(data.get("daily_used", 0)),
                resets_at=data.get("resets_at", ""),
                messages_sent_total=int(data.get("messages_sent_total", 0)),
            )

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email status failed: {exc}")

    def get_message(self, hai_url: str, message_id: str) -> EmailMessage:
        """Get a single email message by ID.

        Args:
            hai_url: Base URL of the HAI server.
            message_id: ID of the message to retrieve.

        Returns:
            EmailMessage object.
        """
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email get_message auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code == 404:
                raise HaiApiError(
                    f"Message not found: {message_id}",
                    status_code=404,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email get_message failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            m = resp.json()
            return EmailMessage(
                id=m.get("id", ""),
                from_address=m.get("from_address", m.get("from", "")),
                to_address=m.get("to_address", m.get("to", "")),
                subject=m.get("subject", ""),
                body_text=m.get("body_text", ""),
                created_at=m.get("created_at", ""),
                direction=m.get("direction", ""),
                message_id=m.get("message_id", ""),
                in_reply_to=m.get("in_reply_to"),
                is_read=m.get("is_read", False),
                delivery_status=m.get("delivery_status", ""),
                read_at=m.get("read_at"),
                jacs_verified=m.get("jacs_verified"),
            )

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email get_message failed: {exc}")

    def delete_message(self, hai_url: str, message_id: str) -> bool:
        """Delete an email message.

        Args:
            hai_url: Base URL of the HAI server.
            message_id: ID of the message to delete.

        Returns:
            True if successful (204).
        """
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.delete(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email delete_message auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code == 404:
                raise HaiApiError(
                    f"Message not found: {message_id}",
                    status_code=404,
                    body=resp.text,
                )
            if resp.status_code not in (200, 204):
                raise HaiApiError(
                    f"Email delete_message failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return True

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email delete_message failed: {exc}")

    def mark_unread(self, hai_url: str, message_id: str) -> bool:
        """Mark an email message as unread.

        Args:
            hai_url: Base URL of the HAI server.
            message_id: ID of the message to mark as unread.

        Returns:
            True if successful.
        """
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_message_id = self._escape_path_segment(message_id)
        url = self._make_url(
            hai_url,
            f"/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/unread",
        )
        headers = self._build_auth_headers()

        try:
            resp = httpx.post(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email mark_unread auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201, 204):
                raise HaiApiError(
                    f"Email mark_unread failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            return True

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email mark_unread failed: {exc}")

    def search_messages(
        self,
        hai_url: str,
        q: Optional[str] = None,
        direction: Optional[str] = None,
        from_address: Optional[str] = None,
        to_address: Optional[str] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
        limit: int = 20,
        offset: int = 0,
    ) -> list[EmailMessage]:
        """Search email messages.

        Args:
            hai_url: Base URL of the HAI server.
            q: Free-text search query.
            direction: Filter by direction ("inbound" or "outbound").
            from_address: Filter by sender address.
            to_address: Filter by recipient address.
            since: ISO 8601 start date filter.
            until: ISO 8601 end date filter.
            limit: Max messages to return.
            offset: Pagination offset.

        Returns:
            List of matching EmailMessage objects.
        """
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

        try:
            resp = httpx.get(
                url, params=params, headers=headers, timeout=self._timeout,
            )

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email search auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email search failed: HTTP {resp.status_code}",
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
                    body_text=m.get("body_text", ""),
                    created_at=m.get("created_at", ""),
                    direction=m.get("direction", ""),
                    message_id=m.get("message_id", ""),
                    in_reply_to=m.get("in_reply_to"),
                    is_read=m.get("is_read", False),
                    delivery_status=m.get("delivery_status", ""),
                    read_at=m.get("read_at"),
                    jacs_verified=m.get("jacs_verified"),
                )
                for m in messages
            ]

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email search failed: {exc}")

    def get_unread_count(self, hai_url: str) -> int:
        """Get the number of unread email messages.

        Args:
            hai_url: Base URL of the HAI server.

        Returns:
            Number of unread messages.
        """
        jacs_id = self._get_hai_agent_id()
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(hai_url, f"/api/agents/{safe_jacs_id}/email/unread-count")
        headers = self._build_auth_headers()

        try:
            resp = httpx.get(url, headers=headers, timeout=self._timeout)

            if resp.status_code in (401, 403):
                raise HaiAuthError(
                    "Email unread_count auth failed",
                    status_code=resp.status_code,
                    body=resp.text,
                )
            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Email unread_count failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            return int(data.get("count", 0))

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Email unread_count failed: {exc}")

    def reply(
        self,
        hai_url: str,
        message_id: str,
        body: str,
        subject: Optional[str] = None,
    ) -> SendEmailResult:
        """Reply to an email message.

        Fetches the original message, then sends a reply with appropriate
        threading headers.

        Args:
            hai_url: Base URL of the HAI server.
            message_id: ID of the message to reply to.
            body: Reply body text.
            subject: Override subject (defaults to "Re: <original subject>").

        Returns:
            SendEmailResult with message_id and status.
        """
        original = self.get_message(hai_url, message_id)
        reply_subject = subject if subject is not None else f"Re: {original.subject}"
        return self.send_email(
            hai_url,
            to=original.from_address,
            subject=reply_subject,
            body=body,
            in_reply_to=original.message_id,
        )

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
        cache_key = f"remote:{jacs_id}:{version}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        safe_jacs_id = self._escape_path_segment(jacs_id)
        safe_version = self._escape_path_segment(version)
        url = self._make_url(
            hai_url, f"/jacs/v1/agents/{safe_jacs_id}/keys/{safe_version}"
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
            result = PublicKeyInfo(
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
            self._set_cached_key(cache_key, result)
            return result

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    # ------------------------------------------------------------------
    # fetch_key_by_hash / fetch_key_by_email / fetch_key_by_domain / fetch_all_keys
    # ------------------------------------------------------------------

    def fetch_key_by_hash(
        self,
        hai_url: str,
        public_key_hash: str,
    ) -> PublicKeyInfo:
        """Fetch an agent's public key by its SHA-256 hash.

        Args:
            hai_url: Base URL of the HAI server.
            public_key_hash: SHA-256 hash in ``sha256:<hex>`` format.

        Returns:
            PublicKeyInfo with the agent's public key and metadata.

        Raises:
            HaiApiError: If no key is found for the hash (404).
        """
        cache_key = f"hash:{public_key_hash}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        safe_hash = self._escape_path_segment(public_key_hash)
        url = self._make_url(hai_url, f"/jacs/v1/keys/by-hash/{safe_hash}")

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code == 404:
                raise HaiApiError(
                    f"No key found for hash: {public_key_hash}",
                    status_code=404,
                    body=resp.text,
                )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Key lookup failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            result = PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""),
                version=data.get("version", ""),
                public_key=data.get("public_key", ""),
                public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""),
                public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""),
                dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
            self._set_cached_key(cache_key, result)
            return result

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    def fetch_key_by_email(
        self,
        hai_url: str,
        email: str,
    ) -> PublicKeyInfo:
        """Fetch an agent's public key by their ``@hai.ai`` email address.

        Args:
            hai_url: Base URL of the HAI server.
            email: The agent's email address (e.g., ``alice@hai.ai``).

        Returns:
            PublicKeyInfo with the agent's public key and metadata.

        Raises:
            HaiApiError: If no agent is found for the email (404).
        """
        cache_key = f"email:{email}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        safe_email = self._escape_path_segment(email)
        url = self._make_url(hai_url, f"/api/agents/keys/{safe_email}")

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code == 404:
                raise HaiApiError(
                    f"No key found for email: {email}",
                    status_code=404,
                    body=resp.text,
                )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Key lookup failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            result = PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""),
                version=data.get("version", ""),
                public_key=data.get("public_key", ""),
                public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""),
                public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""),
                dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
            self._set_cached_key(cache_key, result)
            return result

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    def fetch_key_by_domain(
        self,
        hai_url: str,
        domain: str,
    ) -> PublicKeyInfo:
        """Fetch the latest DNS-verified agent key for a domain.

        Args:
            hai_url: Base URL of the HAI server.
            domain: DNS domain (e.g., ``example.com``).

        Returns:
            PublicKeyInfo with the agent's public key and metadata.

        Raises:
            HaiApiError: If no DNS-verified agent is found for the domain (404).
        """
        cache_key = f"domain:{domain}"
        cached = self._get_cached_key(cache_key)
        if cached is not None:
            return cached

        safe_domain = self._escape_path_segment(domain)
        url = self._make_url(
            hai_url, f"/jacs/v1/agents/by-domain/{safe_domain}"
        )

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code == 404:
                raise HaiApiError(
                    f"No verified agent for domain: {domain}",
                    status_code=404,
                    body=resp.text,
                )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Key lookup failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            data = resp.json()
            result = PublicKeyInfo(
                jacs_id=data.get("jacs_id", ""),
                version=data.get("version", ""),
                public_key=data.get("public_key", ""),
                public_key_raw_b64=data.get("public_key_raw_b64", ""),
                algorithm=data.get("algorithm", ""),
                public_key_hash=data.get("public_key_hash", ""),
                status=data.get("status", ""),
                dns_verified=data.get("dns_verified", False),
                created_at=data.get("created_at", ""),
            )
            self._set_cached_key(cache_key, result)
            return result

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key lookup failed: {exc}")

    def fetch_all_keys(
        self,
        hai_url: str,
        jacs_id: str,
    ) -> dict:
        """Fetch all key versions for an agent.

        Args:
            hai_url: Base URL of the HAI server.
            jacs_id: The target agent's JACS ID.

        Returns:
            Dict with ``jacs_id``, ``keys`` (list of key entries), and ``total``.

        Raises:
            HaiApiError: If the agent is not found (404).
        """
        safe_jacs_id = self._escape_path_segment(jacs_id)
        url = self._make_url(
            hai_url, f"/jacs/v1/agents/{safe_jacs_id}/keys"
        )

        try:
            resp = httpx.get(url, timeout=self._timeout)

            if resp.status_code == 404:
                raise HaiApiError(
                    f"Agent not found: {jacs_id}",
                    status_code=404,
                    body=resp.text,
                )

            if resp.status_code not in (200, 201):
                raise HaiApiError(
                    f"Key history lookup failed: HTTP {resp.status_code}",
                    status_code=resp.status_code,
                    body=resp.text,
                )

            return resp.json()

        except (httpx.ConnectError, httpx.TimeoutException) as exc:
            raise HaiConnectionError(f"Connection failed: {exc}")
        except HaiError:
            raise
        except Exception as exc:
            raise HaiError(f"Key history lookup failed: {exc}")

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


def update_username(hai_url: str, agent_id: str, username: str) -> dict[str, Any]:
    """Update (rename) a claimed username for an agent."""
    return _get_client().update_username(hai_url, agent_id, username)


def delete_username(hai_url: str, agent_id: str) -> dict[str, Any]:
    """Delete a claimed username for an agent."""
    return _get_client().delete_username(hai_url, agent_id)


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
    attachments: Optional[list[dict[str, Any]]] = None,
) -> SendEmailResult:
    """Send an email from this agent's @hai.ai address."""
    return _get_client().send_email(
        hai_url, to, subject, body, in_reply_to, attachments=attachments,
    )


def sign_email(hai_url: str, raw_email: bytes) -> bytes:
    """Sign a raw RFC 5322 email with a JACS attachment via the HAI API."""
    return _get_client().sign_email(hai_url, raw_email)


def verify_email(hai_url: str, raw_email: bytes) -> EmailVerificationResultV2:
    """Verify a JACS-signed email via the HAI API."""
    return _get_client().verify_email(hai_url, raw_email)


def list_messages(
    hai_url: str,
    limit: int = 20,
    offset: int = 0,
    direction: Optional[str] = None,
) -> list[EmailMessage]:
    """List email messages for this agent."""
    return _get_client().list_messages(hai_url, limit, offset, direction)


def mark_read(hai_url: str, message_id: str) -> bool:
    """Mark an email message as read."""
    return _get_client().mark_read(hai_url, message_id)


def get_email_status(hai_url: str) -> EmailStatus:
    """Get email rate-limit and reputation status."""
    return _get_client().get_email_status(hai_url)


def get_message(hai_url: str, message_id: str) -> EmailMessage:
    """Get a single email message by ID."""
    return _get_client().get_message(hai_url, message_id)


def delete_message(hai_url: str, message_id: str) -> bool:
    """Delete an email message."""
    return _get_client().delete_message(hai_url, message_id)


def mark_unread(hai_url: str, message_id: str) -> bool:
    """Mark an email message as unread."""
    return _get_client().mark_unread(hai_url, message_id)


def search_messages(
    hai_url: str,
    q: Optional[str] = None,
    direction: Optional[str] = None,
    from_address: Optional[str] = None,
    to_address: Optional[str] = None,
    since: Optional[str] = None,
    until: Optional[str] = None,
    limit: int = 20,
    offset: int = 0,
) -> list[EmailMessage]:
    """Search email messages."""
    return _get_client().search_messages(
        hai_url, q=q, direction=direction, from_address=from_address,
        to_address=to_address, since=since, until=until, limit=limit, offset=offset,
    )


def get_unread_count(hai_url: str) -> int:
    """Get the number of unread email messages."""
    return _get_client().get_unread_count(hai_url)


def reply(
    hai_url: str,
    message_id: str,
    body: str,
    subject: Optional[str] = None,
) -> SendEmailResult:
    """Reply to an email message."""
    return _get_client().reply(hai_url, message_id, body, subject)


def fetch_remote_key(
    hai_url: str,
    jacs_id: str,
    version: str = "latest",
) -> PublicKeyInfo:
    """Fetch another agent's public key from HAI."""
    return _get_client().fetch_remote_key(hai_url, jacs_id, version)


def verify_document(
    hai_url: str,
    document: Union[str, dict[str, Any]],
) -> dict[str, Any]:
    """Verify a signed JACS document via HAI's public verify endpoint."""
    return _get_client().verify_document(hai_url, document)


def get_verification(hai_url: str, agent_id: str) -> dict[str, Any]:
    """Get advanced 3-level verification status for an agent."""
    return _get_client().get_verification(hai_url, agent_id)


def verify_agent_document(
    hai_url: str,
    agent_json: Union[str, dict[str, Any]],
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
    algorithm: str = "ring-Ed25519",
) -> RegistrationResult:
    """Generate a keypair, self-sign, register with HAI, and save config.

    This is the one-call setup for a new agent.  It:
    1. Generates a keypair via JACS and writes key files to *key_dir*.
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
        algorithm: Signing algorithm (default ``"ring-Ed25519"``).
            Pass ``"pq2025"`` for post-quantum.

    Returns:
        RegistrationResult with ``agent_id``, ``jacs_id``.

    Raises:
        ValueError: If *owner_email* is empty.
    """
    if not owner_email:
        raise ValueError(
            "owner_email is required -- agents must be associated with a verified HAI user"
        )
    import shutil

    from jacs.hai import config as hai_config

    try:
        from jacs import SimpleAgent as _SimpleAgent
    except ImportError:
        from jacs.jacs import SimpleAgent as _SimpleAgent  # type: ignore[no-redef]

    private_key_password = hai_config.load_private_key_password()
    password_str = private_key_password.decode("utf-8")

    kd = Path(key_dir).expanduser() if key_dir else (Path.home() / ".jacs" / "keys")
    kd.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        kd.chmod(0o700)
    except OSError:
        pass

    # 1. Generate keypair + agent via JACS SimpleAgent.create_agent()
    data_dir = kd.parent / "data"
    data_dir.mkdir(parents=True, exist_ok=True)

    _new_agent, new_info = _SimpleAgent.create_agent(
        name=name,
        password=password_str,
        algorithm=algorithm,
        data_directory=str(data_dir),
        key_directory=str(kd),
        config_path=str(Path(config_path).resolve()),
        description=description or "Agent registered via Python SDK",
        domain=domain or "",
        default_storage="fs",
    )

    # Copy JACS-generated key files to standard names expected by the SDK
    private_key_path = kd / "agent_private_key.pem"
    if not private_key_path.is_file():
        priv_src = Path(new_info.get("private_key_path", ""))
        if priv_src.is_file():
            shutil.copy2(str(priv_src), str(private_key_path))
    try:
        private_key_path.chmod(0o600)
    except OSError:
        pass

    # Get public key PEM from the agent
    # JACS may store keys as raw byte files, so get_public_key_pem()
    # may fail. Fall back to reading the raw key and constructing PEM manually.
    public_pem = ""
    try:
        public_pem = _new_agent.get_public_key_pem()
    except Exception:
        pub_src = Path(new_info.get("public_key_path", ""))
        if pub_src.is_file():
            raw_key = pub_src.read_bytes()
            if len(raw_key) == 32:
                # Wrap raw key in ASN.1 SubjectPublicKeyInfo (Ed25519 format)
                # Prefix: 30 2a 30 05 06 03 2b 65 70 03 21 00
                asn1_prefix = bytes([
                    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03,
                    0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
                ])
                der = asn1_prefix + raw_key
                b64 = base64.b64encode(der).decode("ascii")
                public_pem = f"-----BEGIN PUBLIC KEY-----\n{b64}\n-----END PUBLIC KEY-----\n"

    # Also write the public key in PEM format at the expected path
    pub_key_path = kd / "agent_public_key.pem"
    if not pub_key_path.is_file() and public_pem:
        pub_key_path.write_text(public_pem, encoding="utf-8")

    logger.debug(
        "register_new_agent: key_dir=%s, private_key exists=%s, "
        "public_key exists=%s, public_pem len=%d",
        kd, private_key_path.is_file(), pub_key_path.is_file(), len(public_pem),
    )

    # 2. Set up module state directly from the created agent
    # (Avoids JacsAgent.load() which needs pre-existing agent data files)
    try:
        from jacs.simple import _EphemeralAgentAdapter
        wrapped_agent = _EphemeralAgentAdapter(_new_agent)
    except ImportError:
        wrapped_agent = _new_agent  # Fallback if adapter not available

    hai_config._config = hai_config.AgentConfig(
        name=name,
        version=version,
        key_dir=str(kd.resolve()),
        jacs_id=None,  # Will be set after registration
    )
    hai_config._agent = wrapped_agent
    agent = wrapped_agent

    extra_fields: dict = {"description": description or "Agent registered via Python SDK"}
    if domain:
        extra_fields["domain"] = domain
    agent_doc = create_agent_document(
        agent=agent,
        name=name,
        version=version,
        extra_fields=extra_fields,
    )
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

    # 4. Update config with returned jacsId and reload
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

    # 5. Update module state with jacsId (agent is already loaded)
    hai_config._config = hai_config.AgentConfig(
        name=name,
        version=version,
        key_dir=str(kd.resolve()),
        jacs_id=jacs_id,
    )
    # _agent was already set above; keep it
    global _client
    _client = None

    # 6. Print next-step messaging
    if not quiet:
        print(f"\nAgent created and submitted for registration!")
        print(f"  -> Check your email ({owner_email}) for a verification link")
        print(f"  -> Click the link and log into hai.ai to complete registration")
        print(f"  -> After verification, claim a @hai.ai username with:")
        print(f"     client.claim_username('{hai_url}', '{agent_id}', 'my-agent')")
        print(f"  -> Config saved to {config_path}")
        print(f"  -> Keys saved to {kd}")
        print(
            "  -> Private key encrypted using JACS_PASSWORD_FILE/JACS_PRIVATE_KEY_PASSWORD"
        )

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
    from jacs.hai.signing import verify_string as _verify_string

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
            # Reconstruct canonical form: include jacsSignature minus .signature
            import copy
            signing_doc = copy.deepcopy(doc)
            if isinstance(signing_doc.get("jacsSignature"), dict):
                signing_doc["jacsSignature"].pop("signature", None)
            else:
                del signing_doc["jacsSignature"]
            canonical = canonicalize_json(signing_doc)
            # Delegate verification to JACS binding-core
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
