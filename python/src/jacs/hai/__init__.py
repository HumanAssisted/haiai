"""HAI Python SDK -- agent identity, JACS signing, and benchmark client.

Usage::

    from jacs.hai import config, HaiClient

    config.load("./jacs.config.json")

    client = HaiClient()
    if client.testconnection("https://hai.ai"):
        result = client.hello_world("https://hai.ai")
        print(result.message)

Zero-config quickstart::

    from jacs.hai import register_new_agent

    result = register_new_agent(name="My Agent", owner_email="user@example.com")
    print(f"Registered: {result.jacs_id}")
"""

__version__ = "0.1.0"

from jacs.hai import config
from jacs.hai.async_client import AsyncHaiClient
from jacs.hai.client import (
    HaiClient,
    benchmark,
    connect,
    disconnect,
    dns_certified_run,
    fetch_remote_key,
    free_run,
    get_email_status,
    hello_world,
    list_messages,
    mark_read,
    register,
    register_new_agent,
    send_email,
    sign_benchmark_result,
    status,
    submit_benchmark_response,
    testconnection,
    verify_agent,
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
    "EmailMessage",
    "EmailStatus",
    "FreeChaoticResult",
    "HaiEvent",
    "HaiRegistrationPreview",
    "HaiRegistrationResult",
    "HaiStatusResult",
    "HelloWorldResult",
    "JobResponseResult",
    "PublicKeyInfo",
    "RegistrationResult",
    "SendEmailResult",
    "TranscriptMessage",
    # Convenience functions
    "testconnection",
    "hello_world",
    "register",
    "register_new_agent",
    "verify_agent",
    "status",
    "benchmark",
    "free_run",
    "dns_certified_run",
    "submit_benchmark_response",
    "sign_benchmark_result",
    "send_email",
    "list_messages",
    "mark_read",
    "get_email_status",
    "fetch_remote_key",
    "connect",
    "disconnect",
]
