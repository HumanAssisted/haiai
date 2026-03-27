"""MCP tool contract parity tests -- verify the Python FFI adapter can
support every tool category defined in the shared MCP tool contract.

The Rust MCP server (hai-mcp) is the canonical MCP implementation. Python
and Node do not have their own MCP servers (deleted per CLI_PARITY_AUDIT.md).
However, the FFI adapter must expose the underlying methods needed to serve
each MCP tool category, so that any future MCP reimplementation is backed
by the same FFI layer.

This test loads ``fixtures/mcp_tool_contract.json`` and validates that:
1. The fixture is structurally valid (has required fields, counts match).
2. Every MCP tool category maps to at least one FFI adapter method.
3. The total_tool_count field matches the actual number of tools.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

from haiai._ffi_adapter import FFIAdapter

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

FIXTURES_DIR = Path(__file__).resolve().parents[2] / "fixtures"

# Mapping from MCP tool name prefixes/categories to the FFI adapter methods
# that would back them.  Each MCP tool maps to one or more FFI methods.
MCP_TOOL_TO_FFI_METHODS: dict[str, list[str]] = {
    "hai_hello": ["hello"],
    "hai_check_username": ["check_username"],
    "hai_claim_username": ["claim_username"],
    "hai_register_agent": ["register"],
    "hai_agent_status": ["verify_status"],
    "hai_verify_status": ["verify_status"],
    "hai_generate_verify_link": [],  # client-side only, no FFI method needed
    "hai_send_email": ["send_email"],
    "hai_list_messages": ["list_messages"],
    "hai_get_message": ["get_message"],
    "hai_delete_message": ["delete_message"],
    "hai_mark_read": ["mark_read"],
    "hai_mark_unread": ["mark_unread"],
    "hai_search_messages": ["search_messages"],
    "hai_get_unread_count": ["get_unread_count"],
    "hai_get_email_status": ["get_email_status"],
    "hai_reply_email": ["reply_with_options"],
    "hai_forward_email": ["forward"],
    "hai_archive_message": ["archive"],
    "hai_unarchive_message": ["unarchive"],
    "hai_list_contacts": ["contacts"],
    "hai_self_knowledge": [],  # embedded docs, no FFI method needed
    "hai_create_email_template": ["create_email_template"],
    "hai_list_email_templates": ["list_email_templates"],
    "hai_search_email_templates": ["list_email_templates"],
    "hai_get_email_template": ["get_email_template"],
    "hai_update_email_template": ["update_email_template"],
    "hai_delete_email_template": ["delete_email_template"],
}


def _load_mcp_contract() -> dict[str, Any]:
    path = FIXTURES_DIR / "mcp_tool_contract.json"
    return json.loads(path.read_text())


def _get_adapter_methods() -> set[str]:
    """Return public method names from FFIAdapter."""
    import inspect

    return {
        name
        for name, _ in inspect.getmembers(FFIAdapter, predicate=inspect.isfunction)
        if not name.startswith("_")
    }


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestMCPToolContractParity:
    """Verify MCP tool contract is structurally valid and backed by FFI."""

    def test_fixture_is_valid_json_with_required_fields(self) -> None:
        """mcp_tool_contract.json must have required_tools, total_tool_count, version."""
        contract = _load_mcp_contract()
        assert "required_tools" in contract, "missing required_tools"
        assert "total_tool_count" in contract, "missing total_tool_count"
        assert "version" in contract, "missing version"

    def test_total_tool_count_matches_actual(self) -> None:
        """total_tool_count must equal the number of entries in required_tools."""
        contract = _load_mcp_contract()
        declared = contract["total_tool_count"]
        actual = len(contract["required_tools"])
        assert declared == actual, (
            f"total_tool_count ({declared}) != len(required_tools) ({actual})"
        )

    def test_every_tool_has_name_properties_required(self) -> None:
        """Each tool entry must have name, properties, and required fields."""
        contract = _load_mcp_contract()
        for tool in contract["required_tools"]:
            assert "name" in tool, f"tool entry missing 'name': {tool}"
            assert "properties" in tool, f"tool {tool['name']} missing 'properties'"
            assert "required" in tool, f"tool {tool['name']} missing 'required'"

    def test_ffi_adapter_covers_mcp_tool_categories(self) -> None:
        """FFIAdapter must have methods to back every MCP tool that needs FFI.

        Tools that are client-side only (e.g. generate_verify_link,
        self_knowledge) are excluded from FFI coverage checks.
        """
        contract = _load_mcp_contract()
        adapter_methods = _get_adapter_methods()
        missing: list[str] = []

        for tool in contract["required_tools"]:
            tool_name = tool["name"]
            ffi_methods = MCP_TOOL_TO_FFI_METHODS.get(tool_name)
            if ffi_methods is None:
                # Unknown tool in fixture -- mapping needs update
                missing.append(
                    f"{tool_name}: no mapping in MCP_TOOL_TO_FFI_METHODS"
                )
                continue
            for method in ffi_methods:
                if method not in adapter_methods:
                    missing.append(
                        f"{tool_name} -> FFI method '{method}' not in FFIAdapter"
                    )

        assert not missing, (
            "MCP tools missing FFI adapter backing:\n"
            + "\n".join(f"  - {m}" for m in missing)
        )

    def test_mapping_covers_all_fixture_tools(self) -> None:
        """MCP_TOOL_TO_FFI_METHODS must have an entry for every fixture tool.

        This fails if the fixture adds a new tool and nobody updates
        the mapping, ensuring the parity check stays exhaustive.
        """
        contract = _load_mcp_contract()
        fixture_names = {t["name"] for t in contract["required_tools"]}
        mapping_names = set(MCP_TOOL_TO_FFI_METHODS.keys())

        unmapped = fixture_names - mapping_names
        assert not unmapped, (
            f"Fixture tools without MCP_TOOL_TO_FFI_METHODS mapping: {sorted(unmapped)}"
        )
