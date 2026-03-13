"""HAI Python SDK -- agent identity, JACS signing, and benchmark client.

Usage (preferred)::

    from haiai import config, HaiClient

    # Required for encrypted private keys (configure exactly one source):
    # export JACS_PRIVATE_KEY_PASSWORD=dev-password
    # or: export JACS_PASSWORD_FILE=/secure/path/jacs-password.txt
    config.load("./jacs.config.json")

    client = HaiClient()
    if client.testconnection("https://hai.ai"):
        result = client.hello_world("https://hai.ai")
        print(result.message)

Zero-config quickstart::

    from haiai import register_new_agent

    result = register_new_agent(name="My Agent", owner_email="user@example.com")
    print(f"Registered: {result.jacs_id}")
"""

__version__ = "0.1.0"

from jacs.hai import config
from jacs.hai.async_client import AsyncHaiClient
from jacs.hai.client import (
    MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
    HaiClient,
    benchmark,
    delete_username,
    connect,
    disconnect,
    pro_run,
    dns_certified_run,
    enterprise_run,
    certified_run,
    fetch_all_keys,
    fetch_key_by_domain,
    fetch_key_by_email,
    fetch_key_by_hash,
    fetch_remote_key,
    free_run,
    generate_verify_link,
    get_verification,
    get_email_status,
    hello_world,
    list_messages,
    mark_read,
    register,
    register_new_agent,
    send_email,
    sign_email,
    verify_email,
    sign_benchmark_result,
    status,
    submit_benchmark_response,
    testconnection,
    update_username,
    verify_agent,
    verify_agent_document,
    verify_document,
)
from jacs.hai.errors import (
    AuthenticationError,
    BenchmarkError,
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RegistrationError,
    SSEError,
    WebSocketError,
)
from jacs.hai.hash import compute_content_hash
from jacs.hai.models import (
    AgentConfig,
    AgentVerificationResult,
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
    KeyRegistryResponse,
    PublicKeyInfo,
    RegistrationResult,
    RotationResult,
    SendEmailResult,
    TranscriptMessage,
)

__all__ = [
    # Config
    "config",
    # Client classes
    "HaiClient",
    "AsyncHaiClient",
    # Error types
    "HaiError",
    "HaiApiError",
    "HaiAuthError",
    "HaiConnectionError",
    "RegistrationError",
    "BenchmarkError",
    "SSEError",
    "WebSocketError",
    "AuthenticationError",
    # Data types
    "AgentConfig",
    "AgentVerificationResult",
    "BaselineRunResult",
    "BenchmarkResult",
    "ChainEntry",
    "EmailMessage",
    "EmailStatus",
    "EmailVerificationResultV2",
    "FieldResult",
    "FieldStatus",
    "FreeChaoticResult",
    "HaiEvent",
    "HaiRegistrationPreview",
    "HaiRegistrationResult",
    "HaiStatusResult",
    "HelloWorldResult",
    "JobResponseResult",
    "KeyRegistryResponse",
    "PublicKeyInfo",
    "RegistrationResult",
    "RotationResult",
    "SendEmailResult",
    "TranscriptMessage",
    # Hash functions
    "compute_content_hash",
    # Constants
    "MAX_VERIFY_URL_LEN",
    "MAX_VERIFY_DOCUMENT_BYTES",
    # Convenience functions
    "generate_verify_link",
    "testconnection",
    "hello_world",
    "register",
    "register_new_agent",
    "verify_agent",
    "verify_document",
    "get_verification",
    "verify_agent_document",
    "update_username",
    "delete_username",
    "status",
    "benchmark",
    "free_run",
    "pro_run",
    "dns_certified_run",
    "enterprise_run",
    "certified_run",
    "submit_benchmark_response",
    "sign_benchmark_result",
    "send_email",
    "sign_email",
    "verify_email",
    "list_messages",
    "mark_read",
    "get_email_status",
    "fetch_all_keys",
    "fetch_key_by_domain",
    "fetch_key_by_email",
    "fetch_key_by_hash",
    "fetch_remote_key",
    "connect",
    "disconnect",
]
