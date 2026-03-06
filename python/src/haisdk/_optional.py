"""Shared helpers for optional dependency loading in `haisdk` wrappers."""

from __future__ import annotations

import importlib
from typing import Any


def load_optional_module(
    module_name: str,
    *,
    feature: str,
    install_hint: str,
) -> Any:
    try:
        return importlib.import_module(module_name)
    except ImportError as exc:
        raise ImportError(
            f"{feature} requires optional dependency '{module_name}'. {install_hint}"
        ) from exc


def require_attr(
    owner: Any,
    attr_name: str,
    *,
    owner_name: str,
    upgrade_hint: str,
) -> Any:
    value = getattr(owner, attr_name, None)
    if value is None:
        raise ImportError(
            f"{owner_name} is available but missing {attr_name}. {upgrade_hint}"
        )
    return value
