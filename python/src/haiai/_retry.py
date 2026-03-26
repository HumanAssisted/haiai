"""Retry helpers -- DEPRECATED.

Retry logic is now handled inside the Rust FFI binding (haiipy).
These symbols are kept only for backward compatibility with SSE/WS
streaming code that still uses native httpx.
"""

from __future__ import annotations

# Kept for SSE/WS streaming code that still uses native httpx (Phase 2 migration)
RETRY_BACKOFF_BASE = 1.0  # seconds
RETRY_BACKOFF_MAX = 30.0  # seconds
RETRY_MAX_ATTEMPTS = 5
RETRYABLE_STATUS_CODES = frozenset({429, 500, 502, 503, 504})


def should_retry(status_code: int) -> bool:
    """Return True if the status code is retryable."""
    return status_code in RETRYABLE_STATUS_CODES


def backoff(attempt: int) -> float:
    """Return exponential backoff delay (capped) for the given attempt number."""
    return min(RETRY_BACKOFF_BASE * (2 ** attempt), RETRY_BACKOFF_MAX)
