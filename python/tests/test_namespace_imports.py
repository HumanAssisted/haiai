"""Compatibility tests for the `haiai` Python import namespace."""

from __future__ import annotations


def test_haiai_top_level_reexports_core_symbols() -> None:
    import haiai
    from jacs.hai import HaiClient as LegacyHaiClient

    assert haiai.HaiClient is LegacyHaiClient
    assert hasattr(haiai, "AsyncHaiClient")
    assert hasattr(haiai, "config")


def test_haiai_submodule_imports_work() -> None:
    from haiai.async_client import AsyncHaiClient
    from haiai.client import HaiClient
    from haiai.config import load

    assert HaiClient.__name__ == "HaiClient"
    assert AsyncHaiClient.__name__ == "AsyncHaiClient"
    assert callable(load)



def test_haiai_library_passthrough_maps_to_legacy_functions() -> None:
    from haiai.crypt import canonicalize_json as public_canonicalize_json
    from haiai.signing import sign_response as public_sign_response
    from jacs.hai.signing import canonicalize_json as signing_canonicalize_json
    from jacs.hai.signing import sign_response as signing_sign_response

    payload = {"z": 1, "a": {"b": 2}}
    assert public_sign_response is signing_sign_response
    assert public_canonicalize_json(payload) == signing_canonicalize_json(payload)


def test_haiai_step2_modules_import() -> None:
    import haiai
    from haiai import a2a
    from haiai.agentsdk import agentsdk_tool_wrapper
    from haiai.crewai import crewai_guardrail
    from haiai.integrations import (
        create_mcp_server,
        register_a2a_tools,
        register_jacs_tools,
        register_trust_tools,
    )
    from haiai.langgraph import langgraph_wrap_tool_call
    from haiai.mcp import mcp_tool

    assert hasattr(haiai, "integrations")
    assert hasattr(haiai, "a2a")
    assert callable(a2a.get_a2a_integration)
    assert callable(agentsdk_tool_wrapper)
    assert callable(crewai_guardrail)
    assert callable(create_mcp_server)
    assert callable(register_jacs_tools)
    assert callable(register_a2a_tools)
    assert callable(register_trust_tools)
    assert callable(langgraph_wrap_tool_call)
    assert callable(mcp_tool)
