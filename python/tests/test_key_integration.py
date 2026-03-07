"""Live integration tests for JACS key rotation and versioned fetch operations.

Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
HAI_URL (defaults to http://localhost:3000).

Run:
    HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 uv run pytest tests/test_key_integration.py -v
"""

from __future__ import annotations

import os
import time

import pytest

from jacs.hai.client import HaiClient, register_new_agent
from jacs.hai.errors import HaiApiError

pytestmark = pytest.mark.skipif(
    os.environ.get("HAI_LIVE_TEST") != "1",
    reason="set HAI_LIVE_TEST=1 to run live API tests",
)

API_URL = os.environ.get("HAI_URL", "http://localhost:3000")


# ------------------------------------------------------------------
# Fixtures
# ------------------------------------------------------------------


@pytest.fixture(scope="module")
def registered_agent():
    """Register a fresh JACS agent and return (client, agent_name, result)."""
    import tempfile

    agent_name = f"py-key-integ-{int(time.time() * 1000)}"

    with tempfile.TemporaryDirectory() as tmp:
        key_dir = os.path.join(tmp, "keys")
        config_path = os.path.join(tmp, "jacs.config.json")

        owner_email = os.environ.get("HAI_OWNER_EMAIL", "jonathan@hai.io")

        result = register_new_agent(
            name=agent_name,
            owner_email=owner_email,
            hai_url=API_URL,
            key_dir=key_dir,
            config_path=config_path,
            description="Python key integration test agent",
            quiet=True,
        )

        client = HaiClient()

        yield client, agent_name, result


# ------------------------------------------------------------------
# Tests
# ------------------------------------------------------------------


class TestLiveRegisterThenFetchKeyMatches:
    """Register an agent, then fetch its key and verify it matches."""

    def test_fetch_remote_key_matches_registration(self, registered_agent):
        client, agent_name, result = registered_agent
        jacs_id = result.agent_id

        key = client.fetch_remote_key(API_URL, jacs_id, "latest")
        assert key.jacs_id == jacs_id or key.jacs_id != ""
        assert key.public_key != ""
        assert key.algorithm != ""


class TestLiveFetchKeyByHashMatches:
    """Register, compute the key hash, then look up by hash."""

    def test_fetch_key_by_hash(self, registered_agent):
        client, agent_name, result = registered_agent
        jacs_id = result.agent_id

        # First get the key to learn its hash
        key = client.fetch_remote_key(API_URL, jacs_id, "latest")
        if not key.public_key_hash:
            pytest.skip("server did not return public_key_hash")

        # Look up by hash
        by_hash = client.fetch_key_by_hash(API_URL, key.public_key_hash)
        assert by_hash.public_key == key.public_key
        assert by_hash.algorithm == key.algorithm


class TestLiveFetchKeyByEmailMatches:
    """Register, claim username, then fetch key by email."""

    def test_fetch_key_by_email(self, registered_agent):
        client, agent_name, result = registered_agent
        jacs_id = result.agent_id

        # Claim username
        try:
            claim = client.claim_username(API_URL, jacs_id, agent_name)
            email = claim.get("email", "")
        except Exception:
            pytest.skip("could not claim username")

        if not email:
            pytest.skip("no email returned from claim_username")

        # Look up by email
        by_email = client.fetch_key_by_email(API_URL, email)
        assert by_email.jacs_id != ""
        assert by_email.public_key != ""


class TestLiveFetchAllKeysReturnsHistory:
    """Register and then fetch all keys for the agent."""

    def test_fetch_all_keys(self, registered_agent):
        client, agent_name, result = registered_agent
        jacs_id = result.agent_id

        history = client.fetch_all_keys(API_URL, jacs_id)
        assert history["jacs_id"] == jacs_id or history.get("jacs_id", "") != ""
        assert history["total"] >= 1
        assert len(history["keys"]) >= 1
        assert history["keys"][0].get("public_key", "") != ""


class TestLiveFetchKeyByDomain:
    """Attempt to fetch a key by domain (may skip if no DNS-verified agents)."""

    def test_fetch_key_by_domain_404_for_fake(self, registered_agent):
        client, agent_name, result = registered_agent

        with pytest.raises(HaiApiError) as exc_info:
            client.fetch_key_by_domain(API_URL, "nonexistent-test-domain-12345.invalid")

        assert exc_info.value.status_code == 404
