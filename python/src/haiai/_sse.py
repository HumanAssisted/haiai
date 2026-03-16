"""SSE (Server-Sent Events) line parser for HAI SDK."""

from __future__ import annotations

from typing import Any, Optional


def parse_sse_lines(lines: list[str]) -> Optional[tuple[str, str]]:
    """Parse buffered SSE lines into (event_type, data).

    Returns None when the lines do not form a complete event.
    """
    event_type = "message"
    data_parts: list[str] = []

    for line in lines:
        if line.startswith("event:"):
            event_type = line[len("event:"):].strip()
        elif line.startswith("data:"):
            data_parts.append(line[len("data:"):].strip())
        # Comments (":") and other fields are ignored

    if not data_parts:
        return None

    return event_type, "\n".join(data_parts)


def flatten_benchmark_job(raw: dict[str, Any]) -> dict[str, Any]:
    """Flatten the server's nested BenchmarkJob event for the harness.

    Server sends::

        {"type": "benchmark_job", "job_id": "...", "scenario_id": "...",
         "config": {"run_id": "...", "conversation": [...], ...}}

    The harness expects top-level ``job_id``, ``run_id``, ``conversation``,
    ``scenario_context``, etc.
    """
    config = raw.get("config", {})
    return {
        "job_id": str(raw.get("job_id", "")),
        "run_id": str(config.get("run_id", "")),
        "scenario_context": config.get(
            "scenario_name", raw.get("scenario_id", "")
        ),
        "conversation": config.get("conversation", []),
        "max_turns": config.get("max_turns", 30),
        "metadata": config.get("metadata", {}),
    }
