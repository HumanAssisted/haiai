"""HAI Python SDK -- agent identity, JACS signing, and benchmark client.

Usage (preferred)::

    from haiai import config, HaiClient

    # Required for encrypted private keys (configure exactly one source):
    # export JACS_PRIVATE_KEY_PASSWORD=dev-password
    # or: export JACS_PASSWORD_FILE=/secure/path/jacs-password.txt
    config.load("./jacs.config.json")

    client = HaiClient()
    if client.testconnection("https://beta.hai.ai"):
        result = client.hello_world("https://beta.hai.ai")
        print(result.message)

Zero-config quickstart::

    from haiai import register_new_agent

    result = register_new_agent(name="My Agent", owner_email="user@example.com")
    print(f"Registered: {result.jacs_id}")
"""

try:
    from importlib.metadata import version as _pkg_version

    __version__ = _pkg_version("haiai")
except Exception:
    __version__ = "0.0.0"  # fallback when package metadata unavailable

from haiai import config
from haiai.async_client import AsyncHaiClient
from haiai.client import (
    DEFAULT_BASE_URL,
    MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
    HaiClient,
    archive,
    benchmark,
    connect,
    contacts,
    certified_run,
    delete_message,
    delete_username,
    disconnect,
    dns_certified_run,
    enterprise_run,
    fetch_all_keys,
    fetch_key_by_domain,
    fetch_key_by_email,
    fetch_key_by_hash,
    fetch_remote_key,
    forward,
    free_run,
    generate_verify_link,
    get_email_status,
    get_message,
    get_unread_count,
    get_verification,
    hello_world,
    list_messages,
    mark_read,
    mark_unread,
    pro_run,
    register,
    register_new_agent,
    reply,
    rotate_keys,
    search_messages,
    send_email,
    send_signed_email,
    sign_benchmark_result,
    sign_email,
    status,
    submit_benchmark_response,
    testconnection,
    unarchive,
    update_labels,
    update_username,
    verify_agent,
    verify_agent_document,
    verify_document,
    verify_email,
)
from haiai.errors import (
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
from haiai.hash import compute_content_hash
from haiai.models import (
    AgentConfig,
    AgentVerificationResult,
    BaselineRunResult,
    BenchmarkResult,
    ChainEntry,
    EmailMessage,
    EmailStatus,
    EmailVerificationResultV2,
    ExtractMediaSignatureResult,
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
    RawEmailResult,
    RegistrationResult,
    RotationResult,
    SendEmailResult,
    SignImageResult,
    SignTextResult,
    TranscriptMessage,
    VerifyImageResult,
    VerifyTextResult,
    VerifyTextSignature,
)
from haiai import a2a
from haiai import integrations
from haiai.agent import Agent

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
    "ExtractMediaSignatureResult",
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
    "RawEmailResult",
    "RegistrationResult",
    "RotationResult",
    "SendEmailResult",
    "SignImageResult",
    "SignTextResult",
    "TranscriptMessage",
    "VerifyImageResult",
    "VerifyTextResult",
    "VerifyTextSignature",
    # Hash functions
    "compute_content_hash",
    # Constants
    "DEFAULT_BASE_URL",
    "MAX_VERIFY_URL_LEN",
    "MAX_VERIFY_DOCUMENT_BYTES",
    # Convenience functions
    "archive",
    "benchmark",
    "certified_run",
    "connect",
    "contacts",
    "delete_message",
    "delete_username",
    "disconnect",
    "dns_certified_run",
    "enterprise_run",
    "fetch_all_keys",
    "fetch_key_by_domain",
    "fetch_key_by_email",
    "fetch_key_by_hash",
    "fetch_remote_key",
    "forward",
    "free_run",
    "generate_verify_link",
    "get_email_status",
    "get_message",
    "get_unread_count",
    "get_verification",
    "hello_world",
    "list_messages",
    "mark_read",
    "mark_unread",
    "pro_run",
    "register",
    "register_new_agent",
    "reply",
    "rotate_keys",
    "search_messages",
    "send_email",
    "send_signed_email",
    "sign_benchmark_result",
    "sign_email",
    "status",
    "submit_benchmark_response",
    "testconnection",
    "unarchive",
    "update_labels",
    "update_username",
    "verify_agent",
    "verify_agent_document",
    "verify_document",
    "verify_email",
    # Submodules
    "integrations",
    "a2a",
    "Agent",
]
