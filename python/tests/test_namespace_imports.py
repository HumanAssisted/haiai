"""Compatibility tests for the `haisdk` Python import namespace."""

from __future__ import annotations


def test_haisdk_top_level_reexports_core_symbols() -> None:
    import haisdk
    from jacs.hai import HaiClient as LegacyHaiClient

    assert haisdk.HaiClient is LegacyHaiClient
    assert hasattr(haisdk, "AsyncHaiClient")
    assert hasattr(haisdk, "config")


def test_haisdk_submodule_imports_work() -> None:
    from haisdk.async_client import AsyncHaiClient
    from haisdk.client import HaiClient
    from haisdk.config import load

    assert HaiClient.__name__ == "HaiClient"
    assert AsyncHaiClient.__name__ == "AsyncHaiClient"
    assert callable(load)


def test_haisdk_cli_reexports_legacy_main() -> None:
    from haisdk.cli import main as public_main
    from jacs.hai.cli import main as legacy_main

    assert public_main is legacy_main


def test_haisdk_library_passthrough_maps_to_legacy_functions() -> None:
    from haisdk.crypt import canonicalize_json as public_canonicalize_json
    from haisdk.signing import sign_response as public_sign_response
    from jacs.hai.signing import canonicalize_json as signing_canonicalize_json
    from jacs.hai.signing import sign_response as signing_sign_response

    payload = {"z": 1, "a": {"b": 2}}
    assert public_sign_response is signing_sign_response
    assert public_canonicalize_json(payload) == signing_canonicalize_json(payload)


def test_haisdk_step2_modules_import() -> None:
    import haisdk
    from haisdk import a2a
    from haisdk.agentsdk import agentsdk_tool_wrapper
    from haisdk.crewai import crewai_guardrail
    from haisdk.integrations import (
        create_mcp_server,
        register_a2a_tools,
        register_jacs_tools,
        register_trust_tools,
    )
    from haisdk.langgraph import langgraph_wrap_tool_call
    from haisdk.mcp import mcp_tool

    assert hasattr(haisdk, "integrations")
    assert hasattr(haisdk, "a2a")
    assert callable(a2a.get_a2a_integration)
    assert callable(agentsdk_tool_wrapper)
    assert callable(crewai_guardrail)
    assert callable(create_mcp_server)
    assert callable(register_jacs_tools)
    assert callable(register_a2a_tools)
    assert callable(register_trust_tools)
    assert callable(langgraph_wrap_tool_call)
    assert callable(mcp_tool)
