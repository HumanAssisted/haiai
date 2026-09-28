"""Keep optional robust extraction consistent across sync and async facades."""

import pytest


@pytest.mark.parametrize("robust", [False, True])
def test_extract_options_reach_ffi(ffi_client, robust):
    client, ffi = ffi_client
    client.extract_media_signature("stripped.png", raw_payload=True, robust=robust)
    assert ffi.calls[-1] == (
        "extract_media_signature",
        ("stripped.png", {"raw_payload": True, "robust": robust}),
        {},
    )


@pytest.mark.asyncio
@pytest.mark.parametrize("robust", [False, True])
async def test_async_extract_options_reach_ffi(async_ffi_client, robust):
    client, ffi = async_ffi_client
    await client.extract_media_signature("stripped.png", raw_payload=True, robust=robust)
    assert ffi.calls[-1] == (
        "extract_media_signature",
        ("stripped.png", {"raw_payload": True, "robust": robust}),
        {},
    )
