"""Tests for `haisdk.integrations` Step 2 helper wrappers."""

from __future__ import annotations

import sys
import types
from typing import Any

import pytest

from haisdk.integrations import (
    agentsdk_tool_wrapper,
    agentsdk_verify_payload,
    create_mcp_server,
    crewai_guardrail,
    crewai_signed_tool,
    crewai_verified_input,
    langchain_signing_middleware,
    langgraph_awrap_tool_call,
    langgraph_wrap_tool_call,
)


def _install_module(monkeypatch: pytest.MonkeyPatch, module_name: str, **attrs: Any) -> types.ModuleType:
    module = types.ModuleType(module_name)
    for key, value in attrs.items():
        setattr(module, key, value)
    monkeypatch.setitem(sys.modules, module_name, module)
    return module


def _install_package(monkeypatch: pytest.MonkeyPatch, package_name: str) -> types.ModuleType:
    module = types.ModuleType(package_name)
    module.__path__ = []  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, package_name, module)
    return module


def test_langgraph_helpers_delegate_to_jacs_langchain(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")
    _install_package(monkeypatch, "jacs.adapters")

    calls: dict[str, Any] = {}

    def fake_middleware(**kwargs: Any) -> str:
        calls["middleware"] = kwargs
        return "middleware-ok"

    def fake_wrap(**kwargs: Any) -> str:
        calls["wrap"] = kwargs
        return "wrap-ok"

    def fake_awrap(**kwargs: Any) -> str:
        calls["awrap"] = kwargs
        return "awrap-ok"

    _install_module(
        monkeypatch,
        "jacs.adapters.langchain",
        jacs_signing_middleware=fake_middleware,
        jacs_wrap_tool_call=fake_wrap,
        jacs_awrap_tool_call=fake_awrap,
    )

    assert (
        langchain_signing_middleware(client="c", config_path="cfg.json", strict=True)
        == "middleware-ok"
    )
    assert calls["middleware"] == {"client": "c", "config_path": "cfg.json", "strict": True}

    assert langgraph_wrap_tool_call(client="c2", config_path="cfg2", strict=False) == "wrap-ok"
    assert calls["wrap"] == {"client": "c2", "config_path": "cfg2", "strict": False}

    assert langgraph_awrap_tool_call(client="c3", config_path="cfg3", strict=True) == "awrap-ok"
    assert calls["awrap"] == {"client": "c3", "config_path": "cfg3", "strict": True}


def test_crewai_helpers_delegate_to_jacs_crewai(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")
    _install_package(monkeypatch, "jacs.adapters")

    calls: dict[str, Any] = {}

    def fake_guardrail(**kwargs: Any) -> str:
        calls["guardrail"] = kwargs
        return "guardrail-ok"

    class FakeSignedTool:
        def __init__(self, inner_tool: Any, **kwargs: Any) -> None:
            self.inner_tool = inner_tool
            self.kwargs = kwargs

    class FakeVerifiedInput:
        def __init__(self, inner_tool: Any, **kwargs: Any) -> None:
            self.inner_tool = inner_tool
            self.kwargs = kwargs

    _install_module(
        monkeypatch,
        "jacs.adapters.crewai",
        jacs_guardrail=fake_guardrail,
        JacsSignedTool=FakeSignedTool,
        JacsVerifiedInput=FakeVerifiedInput,
        signed_task=lambda **kwargs: kwargs,
    )

    assert crewai_guardrail(client="crew-client", strict=True) == "guardrail-ok"
    assert calls["guardrail"] == {
        "client": "crew-client",
        "config_path": None,
        "strict": True,
    }

    signed_tool = crewai_signed_tool("inner", client="s1", config_path="cfg", strict=True)
    assert isinstance(signed_tool, FakeSignedTool)
    assert signed_tool.inner_tool == "inner"
    assert signed_tool.kwargs == {"client": "s1", "config_path": "cfg", "strict": True}

    verified_tool = crewai_verified_input("inner2", client="s2", config_path="cfg2")
    assert isinstance(verified_tool, FakeVerifiedInput)
    assert verified_tool.inner_tool == "inner2"
    assert verified_tool.kwargs == {"client": "s2", "config_path": "cfg2", "strict": False}


def test_mcp_helper_delegates_to_jacs_mcp(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")

    calls: dict[str, Any] = {}

    def fake_create_server(name: str, config_path: str | None = None) -> dict[str, Any]:
        calls["server"] = {"name": name, "config_path": config_path}
        return {"name": name}

    _install_module(
        monkeypatch,
        "jacs.mcp",
        create_jacs_mcp_server=fake_create_server,
        jacs_tool=lambda fn: fn,
        sign_mcp_message=lambda msg: str(msg),
        verify_mcp_message=lambda signed: {"signed": signed},
    )

    assert create_mcp_server("demo", config_path="jacs.config.json") == {"name": "demo"}
    assert calls["server"] == {"name": "demo", "config_path": "jacs.config.json"}


def test_agentsdk_wrapper_and_verify_delegate_to_base_adapter(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _install_package(monkeypatch, "jacs")
    _install_package(monkeypatch, "jacs.adapters")

    instances: list[Any] = []

    class FakeBaseAdapter:
        def __init__(self, client: Any = None, config_path: str | None = None, strict: bool = False) -> None:
            self.client = client
            self.config_path = config_path
            self.strict = strict
            self.signed: Any = None
            instances.append(self)

        def sign_output_or_passthrough(self, payload: Any) -> str:
            self.signed = payload
            return f"signed::{payload['tool']}::{payload['result']}"

        def verify_input_or_passthrough(self, signed_payload: str) -> Any:
            return {"verified": signed_payload, "strict": self.strict}

    _install_module(monkeypatch, "jacs.adapters.base", BaseJacsAdapter=FakeBaseAdapter)

    decorator = agentsdk_tool_wrapper(client="client-1", config_path="cfg.json", strict=True)

    @decorator
    def add_one(value: int) -> int:
        return value + 1

    assert add_one(4) == "signed::add_one::5"
    assert instances[0].signed == {"tool": "add_one", "result": 5}

    verified = agentsdk_verify_payload("signed-json", strict=False)
    assert verified == {"verified": "signed-json", "strict": False}


@pytest.mark.asyncio
async def test_agentsdk_wrapper_supports_async_tools(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_package(monkeypatch, "jacs")
    _install_package(monkeypatch, "jacs.adapters")

    class FakeBaseAdapter:
        def __init__(self, **_: Any) -> None:
            pass

        def sign_output_or_passthrough(self, payload: Any) -> str:
            return f"async::{payload['tool']}::{payload['result']}"

        def verify_input_or_passthrough(self, signed_payload: str) -> Any:
            return signed_payload

    _install_module(monkeypatch, "jacs.adapters.base", BaseJacsAdapter=FakeBaseAdapter)

    decorator = agentsdk_tool_wrapper(default_tool_name="async_tool")

    @decorator
    async def async_tool(input_text: str) -> str:
        return input_text.upper()

    assert await async_tool("hello") == "async::async_tool::HELLO"


def test_missing_dependency_error_includes_install_hint() -> None:
    with pytest.raises(ImportError, match=r"haisdk\[langgraph\]"):
        langgraph_wrap_tool_call()
