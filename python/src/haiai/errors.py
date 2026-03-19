"""Error classes for the HAI SDK.

Hierarchy::

    HaiError (base)
    +-- HaiApiError
    |   +-- HaiAuthError
    |   +-- HaiConnectionError
    +-- RegistrationError
    +-- BenchmarkError
    +-- SSEError
    +-- WebSocketError

Aliases for backward compatibility with the JACS monolith:
    AuthenticationError = HaiAuthError
    HaiConnectionError  (already matches)
"""

from __future__ import annotations

from typing import Any, Optional


class HaiError(Exception):
    """Base exception for all HAI SDK errors.

    Attributes:
        message: Human-readable error description.
        status_code: HTTP status code if available.
        response_data: Raw response data from the API if available.
        code: Structured error code (e.g. ``JACS_NOT_LOADED``).
        action: Developer-facing hint describing how to fix the issue.
    """

    def __init__(
        self,
        message: str,
        status_code: Optional[int] = None,
        response_data: Optional[dict[str, Any]] = None,
        *,
        code: str = "",
        action: str = "",
    ) -> None:
        full_msg = f"{message}. {action}" if action else message
        super().__init__(full_msg)
        self.message = message
        self.status_code = status_code
        self.response_data = response_data or {}
        self.code = code
        self.action = action
        self.error_code = ""  # populated from API error_code field when available

    def __str__(self) -> str:
        if self.status_code:
            return f"{self.message} (HTTP {self.status_code})"
        return self.message

    @classmethod
    def from_response(
        cls, response: Any, default_message: str = "HAI API error"
    ) -> "HaiError":
        """Create an error from an HTTP response object."""
        try:
            data = response.json()
            message = data.get("error", data.get("message", default_message))
        except (ValueError, AttributeError):
            message = default_message
            data = {}

        status_code = getattr(response, "status_code", None)
        err = cls(message, status_code, data)
        err.error_code = data.get("error_code", "")
        return err


class HaiApiError(HaiError):
    """Error returned by the HAI server (non-2xx response)."""

    def __init__(
        self, message: str, status_code: int = 0, body: str = ""
    ) -> None:
        super().__init__(message, status_code=status_code)
        self.body = body


class HaiAuthError(HaiApiError):
    """Authentication or authorisation error (401/403)."""


class HaiConnectionError(HaiError):
    """Could not connect or connection was lost."""


class RegistrationError(HaiError):
    """Error during agent registration."""


class BenchmarkError(HaiError):
    """Error during benchmark execution."""


class SSEError(HaiError):
    """Error with Server-Sent Events stream."""


class WebSocketError(HaiError):
    """Error with WebSocket connection."""


class EmailNotActive(HaiApiError):
    """Agent email is allocated but not yet active (403)."""


class RecipientNotFound(HaiApiError):
    """Recipient address does not exist (400)."""


class RateLimited(HaiApiError):
    """Too many requests (429).

    Attributes:
        resets_at: ISO 8601 timestamp when the rate limit resets.
    """

    def __init__(
        self, message: str, status_code: int = 429, body: str = "",
        resets_at: str = "",
    ) -> None:
        super().__init__(message, status_code=status_code, body=body)
        self.resets_at = resets_at


class SubjectTooLong(HaiApiError):
    """Email subject exceeds the maximum length (400)."""


class BodyTooLarge(HaiApiError):
    """Email body exceeds the maximum size (400)."""


# Backward-compatibility aliases matching the JACS monolith names
AuthenticationError = HaiAuthError
