"""Shared wire contract and crash/reconnect tests; completions are synthetic."""

import importlib.util
import json
from pathlib import Path
from unittest.mock import Mock

import pytest

from haiai import HaiClient
from haiai.async_client import AsyncHaiClient
from haiai._client_shared import make_ffi_event

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = json.loads((ROOT / "fixtures/benchmark_mediator_contract.json").read_text())
SPEC = importlib.util.spec_from_file_location(
    "benchmark_mediator_example", ROOT / "python/examples/benchmark_mediator.py"
)
example = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(example)


def test_native_event_keeps_exact_prompt_binding_and_usage_contract():
    event = make_ffi_event({"event_type": "benchmark_job", "data": FIXTURE["event"]})
    assert event.data == FIXTURE["event"]


def test_sync_submission_matches_rust_binding_schema(loaded_config):
    client = HaiClient()
    ffi = client._get_ffi()
    ffi.responses["submit_response"] = {"success": True}
    client.submit_benchmark_response(
        job_id=FIXTURE["event"]["job_id"], **FIXTURE["response"]
    )
    assert ffi.calls[-1][0] == "submit_response"
    assert ffi.calls[-1][1][0] == FIXTURE["ffi_submit_response"]


@pytest.mark.asyncio
async def test_async_submission_matches_rust_binding_schema(loaded_config):
    client = AsyncHaiClient()
    ffi = client._get_ffi()
    ffi.responses["submit_response"] = {"success": True}
    await client.submit_benchmark_response(
        "https://hai.ai", job_id=FIXTURE["event"]["job_id"], **FIXTURE["response"]
    )
    assert ffi.calls[-1][1][0] == FIXTURE["ffi_submit_response"]


def completion():
    response = FIXTURE["response"]
    return {
        "content": response["message"],
        **response["metadata"]["benchmark_mediator"],
    }


def test_journal_reuses_completion_when_submission_failed(tmp_path):
    client = Mock()
    client.submit_benchmark_response.side_effect = [
        RuntimeError("connection lost"),
        None,
    ]
    complete = Mock(return_value=completion())
    path = tmp_path / "mediator.sqlite"
    with example.open_journal(str(path)) as journal:
        with pytest.raises(RuntimeError, match="connection lost"):
            example.handle_job(client, FIXTURE["event"], complete, journal)
    with example.open_journal(str(path)) as journal:
        example.resubmit_pending(client, journal)
        example.resubmit_pending(client, journal)
    complete.assert_called_once_with(
        FIXTURE["event"]["config"]["metadata"]["benchmark_mediator"]
    )
    assert client.submit_benchmark_response.call_count == 2
    assert (
        client.submit_benchmark_response.call_args_list[0]
        == client.submit_benchmark_response.call_args_list[1]
    )
    assert path.stat().st_mode & 0o777 == 0o600


def test_unknown_provider_outcome_is_not_purchased_again(tmp_path):
    client = Mock()
    complete = Mock(side_effect=RuntimeError("provider disconnected"))
    with example.open_journal(str(tmp_path / "journal.sqlite")) as journal:
        with pytest.raises(RuntimeError, match="provider disconnected"):
            example.handle_job(client, FIXTURE["event"], complete, journal)
        with pytest.raises(RuntimeError, match="outcome is unknown"):
            example.handle_job(client, FIXTURE["event"], complete, journal)
    complete.assert_called_once()
    client.submit_benchmark_response.assert_not_called()


def test_changed_request_does_not_reuse_a_completion(tmp_path):
    client = Mock()
    complete = Mock(return_value=completion())
    changed = json.loads(json.dumps(FIXTURE["event"]))
    changed["config"]["metadata"]["request_sha256"] = "c" * 64
    with example.open_journal(str(tmp_path / "journal.sqlite")) as journal:
        example.handle_job(client, FIXTURE["event"], complete, journal)
        with pytest.raises(ValueError, match="different input"):
            example.handle_job(client, changed, complete, journal)
    complete.assert_called_once()


def test_wrong_protocol_does_not_call_the_provider(tmp_path):
    event = json.loads(json.dumps(FIXTURE["event"]))
    event["config"]["metadata"]["benchmark_mediator"]["protocol_id"] = "v3.0"
    complete = Mock()
    with example.open_journal(str(tmp_path / "journal.sqlite")) as journal:
        with pytest.raises(ValueError, match="only benchmark 3.1"):
            example.handle_job(Mock(), event, complete, journal)
    complete.assert_not_called()


def test_mediator_registration_flag_reaches_rust(loaded_config):
    client = HaiClient()
    ffi = client._get_ffi()
    ffi.responses["register"] = {"success": True}
    client.register(**FIXTURE["registration"])
    assert ffi.calls[-1][1][0] == FIXTURE["registration"]


@pytest.mark.asyncio
async def test_async_mediator_registration_flag_reaches_rust(loaded_config):
    client = AsyncHaiClient()
    ffi = client._get_ffi()
    ffi.responses["register"] = {"success": True}
    await client.register("https://hai.ai", **FIXTURE["registration"])
    assert ffi.calls[-1][1][0] == FIXTURE["registration"]
