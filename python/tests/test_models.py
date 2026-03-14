"""Tests for jacs.hai.models module."""

from __future__ import annotations

from jacs.hai.models import (
    AgentConfig,
    AgentVerificationResult,
    BaselineRunResult,
    BenchmarkResult,
    EmailMessage,
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


class TestAgentConfig:
    def test_required_fields(self) -> None:
        cfg = AgentConfig(name="bot", version="1.0", key_dir="./keys")
        assert cfg.name == "bot"
        assert cfg.jacs_id is None

    def test_with_jacs_id(self) -> None:
        cfg = AgentConfig(name="a", version="1", key_dir="k", jacs_id="j1")
        assert cfg.jacs_id == "j1"


class TestHaiEvent:
    def test_minimal(self) -> None:
        evt = HaiEvent(event_type="heartbeat", data={})
        assert evt.event_type == "heartbeat"
        assert evt.id is None
        assert evt.retry is None
        assert evt.raw == ""

    def test_with_all_fields(self) -> None:
        evt = HaiEvent(event_type="job", data={"a": 1}, id="42", retry=5000, raw="raw")
        assert evt.id == "42"
        assert evt.retry == 5000


class TestRegistrationResult:
    def test_defaults(self) -> None:
        r = RegistrationResult(agent_id="a1", jacs_id="j1")
        assert r.agent_id == "a1"
        assert r.jacs_id == "j1"

    def test_no_api_key_field(self) -> None:
        """Verify api_key was removed (JACS-only auth)."""
        r = RegistrationResult(agent_id="a", jacs_id="j")
        assert not hasattr(r, "api_key")


class TestHaiRegistrationResult:
    def test_defaults(self) -> None:
        r = HaiRegistrationResult(success=True, agent_id="a1")
        assert r.hai_signature == ""
        assert r.capabilities == []
        assert r.raw_response == {}


class TestHaiRegistrationPreview:
    def test_fields(self) -> None:
        p = HaiRegistrationPreview(
            agent_id="a", agent_name="n", payload_json="{}", endpoint="/r",
            headers={"Content-Type": "application/json"},
        )
        assert p.endpoint == "/r"


class TestHaiStatusResult:
    def test_not_registered(self) -> None:
        s = HaiStatusResult(registered=False, agent_id="x")
        assert not s.registered
        assert s.hai_signatures == []

    def test_registered(self) -> None:
        s = HaiStatusResult(registered=True, agent_id="x", hai_signatures=["ed25519"])
        assert s.hai_signatures == ["ed25519"]


class TestHelloWorldResult:
    def test_defaults(self) -> None:
        h = HelloWorldResult(success=True)
        assert h.hai_signature_valid is False
        assert h.raw_response == {}


class TestTranscriptMessage:
    def test_defaults(self) -> None:
        m = TranscriptMessage(role="mediator", content="hello")
        assert m.timestamp == ""
        assert m.annotations == []

    def test_with_annotations(self) -> None:
        m = TranscriptMessage(role="a", content="b", annotations=["escalated"])
        assert m.annotations == ["escalated"]


class TestFreeChaoticResult:
    def test_success(self) -> None:
        r = FreeChaoticResult(success=True, run_id="r1")
        assert r.transcript == []
        assert r.upsell_message == ""


class TestBaselineRunResult:
    def test_fields(self) -> None:
        r = BaselineRunResult(success=True, score=85.5, payment_id="pay_123")
        assert r.score == 85.5
        assert r.payment_id == "pay_123"


class TestBenchmarkResult:
    def test_fields(self) -> None:
        r = BenchmarkResult(success=True, suite="mediator", score=72.0, passed=8, total=10)
        assert r.failed == 0
        assert r.duration_ms == 0


class TestJobResponseResult:
    def test_fields(self) -> None:
        r = JobResponseResult(success=True, job_id="j1", message="ok")
        assert r.raw_response == {}


class TestAgentVerificationResult:
    def test_level_1(self) -> None:
        v = AgentVerificationResult(
            valid=True, level=1, level_name="basic", agent_id="a1",
            jacs_valid=True,
        )
        assert v.valid
        assert not v.dns_valid
        assert not v.hai_attested

    def test_level_3(self) -> None:
        v = AgentVerificationResult(
            valid=True, level=3, level_name="attested", agent_id="a2",
            jacs_valid=True, dns_valid=True, hai_attested=True,
            hai_signatures=["ed25519"],
        )
        assert v.hai_signatures == ["ed25519"]


class TestEmailMessageFromDict:
    """Verify EmailMessage.from_dict() parses all fields including new ones."""

    def test_from_dict_minimal(self) -> None:
        """Minimal dict produces valid EmailMessage with None for optional fields."""
        m = EmailMessage.from_dict({
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Hi", "body_text": "Body", "created_at": "2026-01-01T00:00:00Z",
        })
        assert m.id == "m1"
        assert m.body_text_clean is None
        assert m.quoted_text is None
        assert m.thread is None
        assert m.cc_addresses == []
        assert m.labels == []
        assert m.folder == "inbox"

    def test_from_dict_with_reply_text(self) -> None:
        """from_dict parses body_text_clean and quoted_text."""
        m = EmailMessage.from_dict({
            "id": "m1", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Re: Hi", "body_text": "New\n\n> Old",
            "created_at": "2026-01-01T00:00:00Z",
            "body_text_clean": "New",
            "quoted_text": "Old",
        })
        assert m.body_text_clean == "New"
        assert m.quoted_text == "Old"

    def test_from_dict_with_recursive_thread(self) -> None:
        """from_dict recursively parses thread entries as EmailMessage."""
        m = EmailMessage.from_dict({
            "id": "m2", "from_address": "a@hai.ai", "to_address": "b@hai.ai",
            "subject": "Re: Hi", "body_text": "Reply",
            "created_at": "2026-01-01T01:00:00Z",
            "body_text_clean": "Reply",
            "thread": [
                {
                    "id": "m1", "from_address": "b@hai.ai", "to_address": "a@hai.ai",
                    "subject": "Hi", "body_text": "Original",
                    "created_at": "2026-01-01T00:00:00Z",
                    "body_text_clean": "Original",
                },
            ],
        })
        assert m.thread is not None
        assert len(m.thread) == 1
        assert isinstance(m.thread[0], EmailMessage)
        assert m.thread[0].id == "m1"
        assert m.thread[0].body_text_clean == "Original"
        assert m.thread[0].thread is None

    def test_from_dict_from_fallback(self) -> None:
        """from_dict falls back to 'from' key when 'from_address' is missing."""
        m = EmailMessage.from_dict({
            "id": "m1", "from": "sender@hai.ai", "to": "rcpt@hai.ai",
            "subject": "X", "body_text": "Y", "created_at": "2026-01-01T00:00:00Z",
        })
        assert m.from_address == "sender@hai.ai"
