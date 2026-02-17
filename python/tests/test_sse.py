"""Tests for jacs.hai._sse module."""

from __future__ import annotations

from jacs.hai._sse import flatten_benchmark_job, parse_sse_lines


class TestParseSSELines:
    def test_simple_data_event(self) -> None:
        lines = ['data: {"foo": 1}']
        result = parse_sse_lines(lines)
        assert result is not None
        event_type, data = result
        assert event_type == "message"
        assert data == '{"foo": 1}'

    def test_named_event(self) -> None:
        lines = ["event: benchmark_job", 'data: {"id": 42}']
        result = parse_sse_lines(lines)
        assert result is not None
        event_type, data = result
        assert event_type == "benchmark_job"
        assert data == '{"id": 42}'

    def test_multi_line_data(self) -> None:
        lines = ["data: line1", "data: line2"]
        result = parse_sse_lines(lines)
        assert result is not None
        _, data = result
        assert data == "line1\nline2"

    def test_empty_lines_no_data(self) -> None:
        lines = ["event: heartbeat"]
        result = parse_sse_lines(lines)
        assert result is None

    def test_comment_lines_ignored(self) -> None:
        lines = [": this is a comment", "data: hello"]
        result = parse_sse_lines(lines)
        assert result is not None
        _, data = result
        assert data == "hello"

    def test_empty_input(self) -> None:
        result = parse_sse_lines([])
        assert result is None

    def test_whitespace_stripped(self) -> None:
        lines = ["event:  connected ", "data:  ok "]
        result = parse_sse_lines(lines)
        assert result is not None
        event_type, data = result
        assert event_type == "connected"
        assert data == "ok"


class TestFlattenBenchmarkJob:
    def test_basic_flatten(self) -> None:
        raw = {
            "type": "benchmark_job",
            "job_id": "j-123",
            "scenario_id": "scenario-1",
            "config": {
                "run_id": "r-456",
                "conversation": [{"role": "a", "content": "hi"}],
                "scenario_name": "mediation-basics",
                "max_turns": 10,
                "metadata": {"tier": "free"},
            },
        }
        flat = flatten_benchmark_job(raw)
        assert flat["job_id"] == "j-123"
        assert flat["run_id"] == "r-456"
        assert flat["scenario_context"] == "mediation-basics"
        assert flat["max_turns"] == 10
        assert len(flat["conversation"]) == 1

    def test_missing_config(self) -> None:
        raw = {"job_id": "j1"}
        flat = flatten_benchmark_job(raw)
        assert flat["job_id"] == "j1"
        assert flat["run_id"] == ""
        assert flat["conversation"] == []
        assert flat["max_turns"] == 30

    def test_scenario_fallback(self) -> None:
        raw = {"scenario_id": "s-99", "config": {}}
        flat = flatten_benchmark_job(raw)
        assert flat["scenario_context"] == "s-99"
