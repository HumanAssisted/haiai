from __future__ import annotations

import inspect
import json
from pathlib import Path
from typing import Any

from haiai import mcp_server


def _load_fixture() -> dict[str, Any]:
    fixture_path = Path(__file__).resolve().parents[2] / "fixtures" / "mcp_tool_contract.json"
    return json.loads(fixture_path.read_text())


def _normalize_type(annotation: object, default: object) -> str:
    text = str(annotation)
    if "bool" in text or isinstance(default, bool):
        return "boolean"
    if "int" in text or (isinstance(default, int) and not isinstance(default, bool)):
        return "number"
    return "string"


def _tool_shape(name: str) -> dict[str, Any]:
    func = getattr(mcp_server, name)
    signature = inspect.signature(func)
    properties: dict[str, str] = {}
    required: list[str] = []

    for param in signature.parameters.values():
        properties[param.name] = _normalize_type(param.annotation, param.default)
        if param.default is inspect._empty:
            required.append(param.name)

    return {
        "name": name,
        "properties": properties,
        "required": sorted(required),
    }


def test_mcp_tool_contract_matches_shared_required_surface() -> None:
    fixture = _load_fixture()

    for tool in fixture["required_tools"]:
        actual = _tool_shape(tool["name"])
        assert actual["name"] == tool["name"]
        assert actual["required"] == sorted(tool["required"])
        for name, type_name in tool["properties"].items():
            assert actual["properties"][name] == type_name
