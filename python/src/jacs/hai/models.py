"""Data models for the HAI SDK."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional


@dataclass
class AgentConfig:
    """JACS agent configuration loaded from jacs.config.json."""

    name: str
    version: str
    key_dir: str
    jacs_id: Optional[str] = None


@dataclass
class HaiEvent:
    """An event received from the HAI server over SSE or WebSocket.

    Attributes:
        event_type: Type of event (e.g., "benchmark_job", "heartbeat").
        data: Event payload (parsed JSON dict or raw string).
        id: Event ID if provided (for SSE resume).
        retry: Retry interval in ms if provided.
        raw: Raw event data string.
    """

    event_type: str
    data: Any
    id: Optional[str] = None
    retry: Optional[int] = None
    raw: str = ""


@dataclass
class RegistrationResult:
    """Result of registering an agent via ``register_new_agent()``."""

    agent_id: str
    jacs_id: str


@dataclass
class HaiRegistrationResult:
    """Result of registering an agent with HAI via ``HaiClient.register()``.

    Attributes:
        success: Whether registration was successful.
        agent_id: The registered agent's ID.
        hai_signature: HAI's signature on the registration.
        registration_id: Unique ID for this registration.
        registered_at: ISO 8601 timestamp of registration.
        capabilities: Capabilities recognized by HAI.
        raw_response: Full API response.
    """

    success: bool
    agent_id: str
    hai_signature: str = ""
    registration_id: str = ""
    registered_at: str = ""
    capabilities: list[str] = field(default_factory=list)
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class HaiRegistrationPreview:
    """Preview of what would be sent during registration.

    Attributes:
        agent_id: The agent's JACS ID.
        agent_name: Human-readable agent name.
        payload_json: The full JSON that would be sent (pretty-printed).
        endpoint: The API endpoint that would be called.
        headers: Headers that would be sent (API key masked).
    """

    agent_id: str
    agent_name: str
    payload_json: str
    endpoint: str
    headers: dict[str, str]


@dataclass
class HaiStatusResult:
    """Result of checking agent registration status.

    Attributes:
        registered: Whether the agent is registered with HAI.
        agent_id: The agent's JACS ID (if registered).
        registration_id: HAI registration ID (if registered).
        registered_at: When the agent was registered.
        hai_signatures: List of HAI signature IDs.
        raw_response: Full API response.
    """

    registered: bool
    agent_id: str = ""
    registration_id: str = ""
    registered_at: str = ""
    hai_signatures: list[str] = field(default_factory=list)
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class HelloWorldResult:
    """Result of a hello world exchange with HAI.

    Attributes:
        success: Whether the hello world exchange succeeded.
        timestamp: ISO 8601 timestamp from HAI's response.
        client_ip: The caller's IP address as seen by HAI.
        hai_public_key_fingerprint: HAI's public key fingerprint.
        message: Human-readable acknowledgment message from HAI.
        hai_signature_valid: Whether HAI's signature on the ACK was verified.
        raw_response: Full response from the API.
    """

    success: bool
    timestamp: str = ""
    client_ip: str = ""
    hai_public_key_fingerprint: str = ""
    message: str = ""
    hai_signature_valid: bool = False
    hello_id: str = ""
    test_scenario: Optional[str] = None
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class TranscriptMessage:
    """A single message in a benchmark transcript.

    Attributes:
        role: Speaker role ("party_a", "party_b", "mediator", "system").
        content: Message text content.
        timestamp: ISO 8601 timestamp of the message.
        annotations: Structural annotations (e.g., "Dispute escalated").
    """

    role: str
    content: str
    timestamp: str = ""
    annotations: list[str] = field(default_factory=list)


@dataclass
class FreeChaoticResult:
    """Result of a free chaotic benchmark run.

    Free tier: no score, no breakdown. Transcript + annotations only.
    Rate limited to 3 runs per keypair per 24 hours.

    Attributes:
        success: Whether the run completed.
        run_id: Unique ID for this benchmark run.
        transcript: List of transcript messages.
        upsell_message: CTA message for paid tiers.
        raw_response: Full response from the API.
    """

    success: bool
    run_id: str = ""
    transcript: list[TranscriptMessage] = field(default_factory=list)
    upsell_message: str = ""
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class BaselineRunResult:
    """Result of a $5 baseline benchmark run.

    Baseline tier: single aggregate score (0-100), no category breakdown.

    Attributes:
        success: Whether the run completed.
        run_id: Unique ID for this benchmark run.
        score: Single aggregate score (0-100).
        transcript: List of transcript messages.
        payment_id: ID of the Stripe payment used.
        raw_response: Full response from the API.
    """

    success: bool
    run_id: str = ""
    score: float = 0.0
    transcript: list[TranscriptMessage] = field(default_factory=list)
    payment_id: str = ""
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class BenchmarkResult:
    """Result of running a benchmark suite.

    Attributes:
        success: Whether the benchmark completed successfully.
        suite: Name of the benchmark suite.
        score: Overall benchmark score (0-100).
        passed: Number of tests passed.
        failed: Number of tests failed.
        total: Total number of tests.
        duration_ms: Total duration in milliseconds.
        results: Detailed results per test.
        raw_response: Full response from the API.
    """

    success: bool
    suite: str
    score: float = 0.0
    passed: int = 0
    failed: int = 0
    total: int = 0
    duration_ms: int = 0
    results: list[dict[str, Any]] = field(default_factory=list)
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class JobResponseResult:
    """Result of submitting a benchmark job response.

    Attributes:
        success: Whether the response was accepted.
        job_id: The job ID that was responded to.
        message: Acknowledgment message from HAI.
        raw_response: Full response from the API.
    """

    success: bool
    job_id: str = ""
    message: str = ""
    raw_response: dict[str, Any] = field(default_factory=dict)


@dataclass
class AgentVerificationResult:
    """Result of verifying an agent at all trust levels.

    Verification Levels:
        - Level 1 (basic): JACS self-signature valid (cryptographic proof).
        - Level 2 (domain): DNS TXT record verification passed.
        - Level 3 (attested): HAI has registered and signed the agent.

    Attributes:
        valid: Overall verification passed (meets min_level if specified).
        level: Highest verification level achieved (0, 1, 2, or 3).
        level_name: Human-readable level name.
        agent_id: The verified agent's JACS ID.
        jacs_valid: Level 1 -- JACS signature is cryptographically valid.
        dns_valid: Level 2 -- DNS verification passed.
        hai_attested: Level 3 -- Agent is registered with HAI signatures.
        domain: Verified domain (if Level 2+).
        hai_signatures: HAI signature algorithms (if Level 3).
        errors: List of verification errors encountered.
        raw_response: Full API response (if HAI verification performed).
    """

    valid: bool
    level: int
    level_name: str
    agent_id: str
    jacs_valid: bool = False
    dns_valid: bool = False
    hai_attested: bool = False
    domain: str = ""
    hai_signatures: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    raw_response: dict[str, Any] = field(default_factory=dict)
