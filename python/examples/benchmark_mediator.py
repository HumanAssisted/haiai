#!/usr/bin/env python3
"""Serve private 3.1 benchmark jobs through an admitted haiai identity.

HAI requests, authentication and response signing stay in the Rust SDK.
The response journal prevents reconnects from buying the same completion twice.
"""

from __future__ import annotations

import argparse
import importlib
import json
import os
import sqlite3
import time
from pathlib import Path

CONTRACT = "hai.benchmark.mediator/v1"


def open_journal(path: str) -> sqlite3.Connection:
    target = Path(path)
    if target.is_symlink():
        raise ValueError("The mediator journal must not be a symlink")
    fd = os.open(target, os.O_CREAT | os.O_RDWR, 0o600)
    os.close(fd)
    os.chmod(target, 0o600)
    db = sqlite3.connect(target)
    db.execute(
        "CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, digest TEXT NOT NULL, response TEXT, submitted INTEGER NOT NULL DEFAULT 0)"
    )
    return db


def handle_job(client, data: dict, complete, journal: sqlite3.Connection) -> None:
    config = data.get("config", data)
    metadata = config.get("metadata", {})
    request = metadata.get("benchmark_mediator", {})
    if request.get("schema") != CONTRACT or request.get("protocol_id") != "v3.1":
        raise ValueError("This mediator accepts only benchmark 3.1 completion jobs")
    job_id = data.get("job_id")
    digest = metadata.get("request_sha256")
    if not job_id or not isinstance(digest, str) or len(digest) != 64:
        raise ValueError("Missing job identity or exact-request digest")
    previous = journal.execute(
        "SELECT digest,response FROM jobs WHERE id=?", (job_id,)
    ).fetchone()
    if previous:
        if previous[0] != digest:
            raise ValueError("Job identity was reused for different input")
        if previous[1] is None:
            raise RuntimeError(
                "Prior completion outcome is unknown; reconcile before retrying"
            )
        response = json.loads(previous[1])
    else:
        # Commit before a provider call. A crash leaves an explicit unknown.
        with journal:
            journal.execute(
                "INSERT INTO jobs(id,digest) VALUES (?,?)", (job_id, digest)
            )
        started = time.monotonic()
        result = complete(request)
        if (
            result["model"] != request["model"]
            or result["provider"] != request["provider"]
        ):
            raise ValueError("Provider response differs from the requested model/route")
        response = {
            "message": result["content"],
            "metadata": {
                "benchmark_mediator": {
                    "schema": CONTRACT,
                    "request_sha256": digest,
                    "model": result["model"],
                    "provider": result["provider"],
                    "usage": result["usage"],
                }
            },
            "processing_time_ms": round((time.monotonic() - started) * 1000),
        }
        with journal:
            journal.execute(
                "UPDATE jobs SET response=? WHERE id=?", (json.dumps(response), job_id)
            )
    # Yield and end are responses too; never leave the server waiting for them.
    client.submit_benchmark_response(job_id=job_id, **response)
    with journal:
        journal.execute("UPDATE jobs SET submitted=1 WHERE id=?", (job_id,))


def resubmit_pending(client, journal: sqlite3.Connection) -> None:
    """Replay saved replies after reconnect, never the provider call."""
    pending = journal.execute(
        "SELECT id,response FROM jobs WHERE submitted=0 AND response IS NOT NULL"
    ).fetchall()
    for job_id, response in pending:
        client.submit_benchmark_response(job_id=job_id, **json.loads(response))
        with journal:
            journal.execute("UPDATE jobs SET submitted=1 WHERE id=?", (job_id,))


def complete_openai(request: dict) -> dict:
    """Reference callback for direct OpenAI. Other providers supply a callback."""
    if request["provider"] != "openai":
        raise ValueError("The reference callback supports direct OpenAI only")
    from openai import OpenAI

    client = OpenAI(base_url="https://api.openai.com/v1", max_retries=0, timeout=150)
    body = {"model": request["model"], "messages": request["messages"]}
    if request["reasoning_effort"] is not None:
        body.update(
            reasoning_effort=request["reasoning_effort"],
            max_completion_tokens=request["max_output_tokens"],
        )
    else:
        body.update(
            temperature=request["temperature"], max_tokens=request["max_output_tokens"]
        )
    result = client.chat.completions.create(**body)
    if result.usage is None:
        raise RuntimeError("Provider returned no usage; inspect the provider record")
    cached = (
        getattr(
            getattr(result.usage, "prompt_tokens_details", None), "cached_tokens", 0
        )
        or 0
    )
    return {
        "content": result.choices[0].message.content or "",
        "model": result.model,
        "provider": "openai",
        "usage": {
            "prompt_tokens": result.usage.prompt_tokens - cached,
            "completion_tokens": result.usage.completion_tokens,
            "total_tokens": result.usage.total_tokens,
            "cache_read_input_tokens": cached,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--url", default="https://hai.ai")
    parser.add_argument("--transport", choices=["sse", "ws"], default="sse")
    parser.add_argument(
        "--journal", required=True, help="Durable local SQLite response journal"
    )
    parser.add_argument("--complete", help="Optional module:function provider callback")
    args = parser.parse_args()
    complete = complete_openai
    if args.complete:
        module, name = args.complete.split(":", 1)
        complete = getattr(importlib.import_module(module), name)
    from haiai import HaiClient, config

    config.load(args.config)
    client = HaiClient(verify_server_signatures=True)
    with open_journal(args.journal) as journal:
        try:
            for event in client.connect(args.url, transport=args.transport):
                if event.event_type == "connected":
                    resubmit_pending(client, journal)
                elif event.event_type == "benchmark_job":
                    handle_job(client, event.data, complete, journal)
        finally:
            client.disconnect()


if __name__ == "__main__":
    main()
