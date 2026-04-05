"""Tests for the `haiai` Python import namespace."""

from __future__ import annotations


def test_haiai_top_level_reexports_core_symbols() -> None:
    import haiai
    from haiai import HaiClient

    assert haiai.HaiClient is HaiClient
    assert hasattr(haiai, "AsyncHaiClient")
    assert hasattr(haiai, "config")


def test_haiai_submodule_imports_work() -> None:
    from haiai.async_client import AsyncHaiClient
    from haiai.client import HaiClient
    from haiai.config import load

    assert HaiClient.__name__ == "HaiClient"
    assert AsyncHaiClient.__name__ == "AsyncHaiClient"
    assert callable(load)


def test_haiai_signing_and_crypt_are_consistent(loaded_config: None) -> None:
    from haiai.crypt import canonicalize_json as public_canonicalize_json
    from haiai.signing import sign_response as public_sign_response
    from haiai.signing import canonicalize_json as signing_canonicalize_json
    from haiai.signing import sign_response as signing_sign_response

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


def test_haiai_exports_all_platform_convenience_functions() -> None:
    """Every platform operation convenience function must be importable from haiai."""
    from haiai import (
        get_message,
        delete_message,
        mark_unread,
        search_messages,
        get_unread_count,
        reply,
        forward,
        archive,
        unarchive,
        contacts,
        send_signed_email,
        rotate_keys,
        update_labels,
        verify_email,
    )

    for fn in [
        get_message,
        delete_message,
        mark_unread,
        search_messages,
        get_unread_count,
        reply,
        forward,
        archive,
        unarchive,
        contacts,
        send_signed_email,
        rotate_keys,
        update_labels,
        verify_email,
    ]:
        assert callable(fn), f"{fn.__name__} should be callable"


def test_haiai_all_includes_new_exports() -> None:
    """All new exports should be in __all__ for `from haiai import *`."""
    import haiai

    expected = [
        "get_message",
        "delete_message",
        "mark_unread",
        "search_messages",
        "get_unread_count",
        "reply",
        "forward",
        "archive",
        "unarchive",
        "contacts",
        "send_signed_email",
        "rotate_keys",
        "update_labels",
        "verify_email",
    ]
    for name in expected:
        assert name in haiai.__all__, f"{name} missing from haiai.__all__"
