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
