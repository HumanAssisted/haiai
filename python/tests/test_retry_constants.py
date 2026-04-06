"""Tests for M13: Python retry constants should match Rust values.

The Rust core uses DEFAULT_MAX_RECONNECT_ATTEMPTS = 10.
The Python _retry.py module must use the same value to ensure
consistent reconnection behavior across SDKs.
"""

from __future__ import annotations


def test_retry_max_attempts_matches_rust():
    """RETRY_MAX_ATTEMPTS must be 10, matching Rust DEFAULT_MAX_RECONNECT_ATTEMPTS."""
    from haiai._retry import RETRY_MAX_ATTEMPTS
    assert RETRY_MAX_ATTEMPTS == 10, (
        f"RETRY_MAX_ATTEMPTS is {RETRY_MAX_ATTEMPTS}, expected 10 "
        f"(must match Rust DEFAULT_MAX_RECONNECT_ATTEMPTS)"
    )


def test_backoff_returns_float():
    """backoff() must return a float delay."""
    from haiai._retry import backoff
    delay = backoff(0)
    assert isinstance(delay, float)
    assert delay > 0


def test_backoff_is_capped():
    """backoff() delay must not exceed RETRY_BACKOFF_MAX."""
    from haiai._retry import RETRY_BACKOFF_MAX, backoff
    delay = backoff(100)  # Very high attempt number
    assert delay <= RETRY_BACKOFF_MAX
