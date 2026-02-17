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

    result = register_new_agent(name="My Agent")
    print(f"Registered: {result.jacs_id}")
"""

__version__ = "0.1.0"

from jacs.hai import config
from jacs.hai.client import (
    HaiClient,
    baseline_run,
    benchmark,
    connect,
    disconnect,
    free_chaotic_run,
    hello_world,
    register,
    register_new_agent,
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
    FreeChaoticResult,
    HaiEvent,
    HaiRegistrationPreview,
    HaiRegistrationResult,
    HaiStatusResult,
    HelloWorldResult,
    JobResponseResult,
    RegistrationResult,
    TranscriptMessage,
)

__all__ = [
    # Config
    "config",
    # Client class
    "HaiClient",
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
    "FreeChaoticResult",
    "HaiEvent",
    "HaiRegistrationPreview",
    "HaiRegistrationResult",
    "HaiStatusResult",
    "HelloWorldResult",
    "JobResponseResult",
    "RegistrationResult",
    "TranscriptMessage",
    # Convenience functions
    "testconnection",
    "hello_world",
    "register",
    "register_new_agent",
    "verify_agent",
    "status",
    "benchmark",
    "free_chaotic_run",
    "baseline_run",
    "submit_benchmark_response",
    "sign_benchmark_result",
    "connect",
    "disconnect",
]
