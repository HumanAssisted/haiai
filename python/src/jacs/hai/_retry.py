"""Retry helpers and backoff logic for HAI SDK HTTP operations."""

from __future__ import annotations

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
