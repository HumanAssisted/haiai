from __future__ import annotations

import json
import base64
from pathlib import Path

import pytest

from haiai.client import HaiClient
from haiai.async_client import AsyncHaiClient
from haiai.signing import canonicalize_json


def _load_fixture() -> dict[str, object]:
    fixture_path = (
        Path(__file__).resolve().parents[2] / "fixtures" / "cross_lang_test.json"
    )
    return json.loads(fixture_path.read_text())


def test_cross_lang_canonical_json_cases(loaded_config: None) -> None:
    fixture = _load_fixture()
    for case in fixture["canonical_json_cases"]:
        assert canonicalize_json(case["input"]) == case["expected"]


@pytest.mark.parametrize("client_type", [HaiClient, AsyncHaiClient])
def test_no_context_auth_is_actionably_rejected(client_type) -> None:
    from haiai.errors import HaiError

    with pytest.raises(HaiError, match="build_request_auth_header"):
        client_type()._build_jacs_auth_header()


def test_cross_lang_request_auth_delegates_exact_bytes(loaded_config: None) -> None:
    example = _load_fixture()["request_auth"]["example"]
    client = HaiClient()
    ffi = client._get_ffi()
    ffi.responses["build_request_auth_header"] = example["stub_header"]
    body = base64.b64decode(example["body_base64"])
    assert (
        client.build_request_auth_header(example["method"], example["url"], body)
        == example["stub_header"]
    )
    name, args, _kwargs = ffi.calls[-1]
    assert name == "build_request_auth_header"
    assert json.loads(args[0]) == {
        key: example[key] for key in ("method", "url", "body_base64")
    }


@pytest.mark.asyncio
async def test_async_cross_lang_request_auth_delegates_exact_bytes(
    loaded_config: None,
) -> None:
    example = _load_fixture()["request_auth"]["example"]
    client = AsyncHaiClient()
    ffi = client._get_ffi()
    ffi.responses["build_request_auth_header"] = example["stub_header"]
    body = base64.b64decode(example["body_base64"])
    assert (
        await client.build_request_auth_header(example["method"], example["url"], body)
        == example["stub_header"]
    )
    assert json.loads(ffi.calls[-1][1][0]) == {
        key: example[key] for key in ("method", "url", "body_base64")
    }


def test_request_auth_requires_explicit_bytes() -> None:
    with pytest.raises(TypeError, match="exact transmitted"):
        HaiClient().build_request_auth_header("POST", "https://hai.ai/", "not bytes")


def test_request_auth_empty_body_is_encoded_explicitly(loaded_config: None) -> None:
    client = HaiClient()
    ffi = client._get_ffi()
    ffi.responses["build_request_auth_header"] = "JACS v2.fixture"
    client.build_request_auth_header("GET", "https://hai.ai/")
    assert json.loads(ffi.calls[-1][1][0])["body_base64"] == ""


def test_request_auth_audience_is_client_configuration() -> None:
    from haiai.client import _build_ffi_config

    assert json.loads(_build_ffi_config())["request_auth_audience"] == "hai.ai"
    assert (
        json.loads(_build_ffi_config(request_auth_audience="staging.hai"))[
            "request_auth_audience"
        ]
        == "staging.hai"
    )
