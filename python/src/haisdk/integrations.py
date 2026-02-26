"""Framework integration helpers for HAI SDK Step 2 flows.

These helpers are thin wrappers around canonical JACS adapters to avoid
duplicate framework-specific logic in this repository.
"""

from __future__ import annotations

import importlib
import inspect
from functools import wraps
from typing import Any, Callable, TypeVar

TCallable = TypeVar("TCallable", bound=Callable[..., Any])


def _load_optional(module_name: str, *, feature: str, install_hint: str) -> Any:
    try:
        return importlib.import_module(module_name)
    except ImportError as exc:
        raise ImportError(
            f"{feature} requires optional dependency '{module_name}'. {install_hint}"
        ) from exc


def _resolve_base_adapter(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.base",
        feature="Agent SDK signing wrapper",
        install_hint='Install with: pip install "haisdk[agentsdk]"',
    )
    adapter_cls = getattr(module, "BaseJacsAdapter", None)
    if adapter_cls is None:
        raise ImportError(
            "jacs.adapters.base is available but missing BaseJacsAdapter. "
            "Install/upgrade JACS: pip install -U jacs"
        )
    return adapter_cls(client=client, config_path=config_path, strict=strict)


# ---------------------------------------------------------------------------
# LangChain / LangGraph (delegates to jacs.adapters.langchain)
# ---------------------------------------------------------------------------


def langchain_signing_middleware(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.langchain",
        feature="LangChain/LangGraph integration",
        install_hint='Install with: pip install "haisdk[langchain,langgraph]"',
    )
    return module.jacs_signing_middleware(
        client=client,
        config_path=config_path,
        strict=strict,
    )


def langgraph_wrap_tool_call(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Callable[..., Any]:
    module = _load_optional(
        "jacs.adapters.langchain",
        feature="LangGraph wrap_tool_call integration",
        install_hint='Install with: pip install "haisdk[langgraph]"',
    )
    return module.jacs_wrap_tool_call(
        client=client,
        config_path=config_path,
        strict=strict,
    )


def langgraph_awrap_tool_call(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Callable[..., Any]:
    module = _load_optional(
        "jacs.adapters.langchain",
        feature="LangGraph async wrap_tool_call integration",
        install_hint='Install with: pip install "haisdk[langgraph]"',
    )
    return module.jacs_awrap_tool_call(
        client=client,
        config_path=config_path,
        strict=strict,
    )


# ---------------------------------------------------------------------------
# CrewAI (delegates to jacs.adapters.crewai)
# ---------------------------------------------------------------------------


def crewai_guardrail(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Callable[..., Any]:
    module = _load_optional(
        "jacs.adapters.crewai",
        feature="CrewAI integration",
        install_hint='Install with: pip install "haisdk[crewai]"',
    )
    return module.jacs_guardrail(
        client=client,
        config_path=config_path,
        strict=strict,
    )


def crewai_signed_task(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
    **task_kwargs: Any,
) -> Callable[..., Any]:
    module = _load_optional(
        "jacs.adapters.crewai",
        feature="CrewAI signed task integration",
        install_hint='Install with: pip install "haisdk[crewai]"',
    )
    return module.signed_task(
        client=client,
        config_path=config_path,
        strict=strict,
        **task_kwargs,
    )


def crewai_signed_tool(
    inner_tool: Any,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.crewai",
        feature="CrewAI signed tool integration",
        install_hint='Install with: pip install "haisdk[crewai]"',
    )
    cls = getattr(module, "JacsSignedTool")
    return cls(
        inner_tool,
        client=client,
        config_path=config_path,
        strict=strict,
    )


def crewai_verified_input(
    inner_tool: Any,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.crewai",
        feature="CrewAI verified input integration",
        install_hint='Install with: pip install "haisdk[crewai]"',
    )
    cls = getattr(module, "JacsVerifiedInput")
    return cls(
        inner_tool,
        client=client,
        config_path=config_path,
        strict=strict,
    )


# ---------------------------------------------------------------------------
# MCP (delegates to jacs.mcp)
# ---------------------------------------------------------------------------


def create_mcp_server(name: str, config_path: str | None = None) -> Any:
    module = _load_optional(
        "jacs.mcp",
        feature="MCP server integration",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.create_jacs_mcp_server(name, config_path=config_path)


def mcp_tool(func: Callable[..., Any]) -> Callable[..., Any]:
    module = _load_optional(
        "jacs.mcp",
        feature="MCP tool decorator",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.jacs_tool(func)


def sign_mcp_message(message: dict[str, Any]) -> str:
    module = _load_optional(
        "jacs.mcp",
        feature="MCP message signing",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.sign_mcp_message(message)


def verify_mcp_message(signed_json: str) -> dict[str, Any]:
    module = _load_optional(
        "jacs.mcp",
        feature="MCP message verification",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.verify_mcp_message(signed_json)


def register_jacs_tools(
    mcp_server: Any,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
    *,
    tools: list[str] | None = None,
) -> Any:
    module = _load_optional(
        "jacs.adapters.mcp",
        feature="MCP JACS tool registration",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.register_jacs_tools(
        mcp_server,
        client=client,
        config_path=config_path,
        strict=strict,
        tools=tools,
    )


def register_a2a_tools(
    mcp_server: Any,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.mcp",
        feature="MCP A2A tool registration",
        install_hint='Install with: pip install "haisdk[mcp,a2a]"',
    )
    return module.register_a2a_tools(
        mcp_server,
        client=client,
        config_path=config_path,
        strict=strict,
    )


def register_trust_tools(
    mcp_server: Any,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    module = _load_optional(
        "jacs.adapters.mcp",
        feature="MCP trust tool registration",
        install_hint='Install with: pip install "haisdk[mcp]"',
    )
    return module.register_trust_tools(
        mcp_server,
        client=client,
        config_path=config_path,
        strict=strict,
    )


# ---------------------------------------------------------------------------
# Agent SDK (framework-neutral wrappers around tool callables)
# ---------------------------------------------------------------------------


def agentsdk_tool_wrapper(
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
    default_tool_name: str | None = None,
) -> Callable[[TCallable], TCallable]:
    """Wrap Agent SDK tool functions so outputs are signed by JACS.

    Works for both sync and async callables. The wrapped tool returns a
    signed JACS JSON string. If strict=False, signing failures return
    the original payload unchanged.
    """

    adapter = _resolve_base_adapter(
        client=client,
        config_path=config_path,
        strict=strict,
    )

    def decorator(tool: TCallable) -> TCallable:
        tool_name = default_tool_name or getattr(tool, "__name__", "tool")

        if inspect.iscoroutinefunction(tool):

            @wraps(tool)
            async def async_wrapped(*args: Any, **kwargs: Any) -> str:
                result = await tool(*args, **kwargs)
                payload = {"tool": tool_name, "result": result}
                return adapter.sign_output_or_passthrough(payload)

            return async_wrapped  # type: ignore[return-value]

        @wraps(tool)
        def wrapped(*args: Any, **kwargs: Any) -> str:
            result = tool(*args, **kwargs)
            payload = {"tool": tool_name, "result": result}
            return adapter.sign_output_or_passthrough(payload)

        return wrapped  # type: ignore[return-value]

    return decorator


def agentsdk_verify_payload(
    signed_payload: str,
    client: Any = None,
    config_path: str | None = None,
    strict: bool = False,
) -> Any:
    """Verify Agent SDK payloads previously signed with JACS."""

    adapter = _resolve_base_adapter(
        client=client,
        config_path=config_path,
        strict=strict,
    )
    return adapter.verify_input_or_passthrough(signed_payload)


__all__ = [
    "langchain_signing_middleware",
    "langgraph_wrap_tool_call",
    "langgraph_awrap_tool_call",
    "crewai_guardrail",
    "crewai_signed_task",
    "crewai_signed_tool",
    "crewai_verified_input",
    "create_mcp_server",
    "mcp_tool",
    "sign_mcp_message",
    "verify_mcp_message",
    "register_jacs_tools",
    "register_a2a_tools",
    "register_trust_tools",
    "agentsdk_tool_wrapper",
    "agentsdk_verify_payload",
]
