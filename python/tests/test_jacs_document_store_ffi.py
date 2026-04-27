"""Issue 025 — Python FFI tests for the 7 D5/D9 JACS Document Store methods.

Exercises every D5 (save_memory / save_soul / get_memory / get_soul) and D9
(store_text_file / store_image_file / get_record_bytes) wrapper through the
binding-core layer (`FFIAdapter`/`AsyncFFIAdapter`) using the MockFFIAdapter
test infrastructure. The fixture file ``fixtures/ffi_method_parity.json``
declares these methods in the ``jacs_document_store`` group; this test file
pins their wire shape (argument names, return types, error mapping) at the
adapter boundary.

Mock-only: these tests do NOT spin up the haiipy native library. The full
HTTP round-trip is exercised by ``haisdk/rust/haiai/tests/jacs_remote_integration.rs``
(``--ignored`` against a live hosted stack).
"""

from __future__ import annotations

import json

import pytest

from tests.conftest import MockFFIAdapter

# ---------------------------------------------------------------------------
# Fixture
# ---------------------------------------------------------------------------


@pytest.fixture
def mock_ffi() -> MockFFIAdapter:
    return MockFFIAdapter()


# ---------------------------------------------------------------------------
# D5 — MEMORY / SOUL wrappers
# ---------------------------------------------------------------------------


class TestD5MemorySoulWrappers:
    """Each D5 method round-trips through MockFFIAdapter and records the
    expected call signature."""

    def test_save_memory_records_call_with_content(self, mock_ffi: MockFFIAdapter) -> None:
        mock_ffi.responses["save_memory"] = "mem-id:v1"
        key = mock_ffi.save_memory("# MEMORY.md\n\nproject: foo")
        assert key == "mem-id:v1"
        assert mock_ffi.calls[-1] == ("save_memory", ("# MEMORY.md\n\nproject: foo",), {})

    def test_save_memory_records_call_with_none_content(
        self, mock_ffi: MockFFIAdapter
    ) -> None:
        # Passing None signals "read MEMORY.md from CWD" — the SDK Rust
        # layer does the file read; the FFI surface just relays None.
        mock_ffi.responses["save_memory"] = "mem-id:v2"
        key = mock_ffi.save_memory(None)
        assert key == "mem-id:v2"
        assert mock_ffi.calls[-1] == ("save_memory", (None,), {})

    def test_save_soul_records_call_with_content(self, mock_ffi: MockFFIAdapter) -> None:
        mock_ffi.responses["save_soul"] = "soul-id:v1"
        key = mock_ffi.save_soul("# SOUL.md\n\nvoice: terse")
        assert key == "soul-id:v1"
        assert mock_ffi.calls[-1] == ("save_soul", ("# SOUL.md\n\nvoice: terse",), {})

    def test_get_memory_returns_envelope_json(self, mock_ffi: MockFFIAdapter) -> None:
        envelope = json.dumps({"jacsId": "mem-1", "jacsType": "memory", "body": "x"})
        mock_ffi.responses["get_memory"] = envelope
        out = mock_ffi.get_memory()
        assert out == envelope
        assert mock_ffi.calls[-1] == ("get_memory", (), {})

    def test_get_memory_returns_none_when_no_record_exists(
        self, mock_ffi: MockFFIAdapter
    ) -> None:
        # Issue 025: the real RemoteJacsProvider returns None when no
        # memory record exists. MockFFIAdapter's `_record` sentinel uses
        # `responses.get(...)` which returns None for missing keys but
        # then falls through to `{}` — wire a callable to honour the
        # documented None contract.
        mock_ffi.responses["get_memory"] = lambda: None
        out = mock_ffi.get_memory()
        assert out is None
        assert mock_ffi.calls[-1] == ("get_memory", (), {})

    def test_get_soul_returns_envelope_json(self, mock_ffi: MockFFIAdapter) -> None:
        envelope = json.dumps({"jacsId": "soul-1", "jacsType": "soul"})
        mock_ffi.responses["get_soul"] = envelope
        out = mock_ffi.get_soul()
        assert out == envelope
        assert mock_ffi.calls[-1] == ("get_soul", (), {})


# ---------------------------------------------------------------------------
# D9 — typed-content helpers
# ---------------------------------------------------------------------------


class TestD9TypedContentHelpers:
    """Each D9 method round-trips through MockFFIAdapter and records the
    expected call signature."""

    def test_store_text_file_records_path(self, mock_ffi: MockFFIAdapter) -> None:
        mock_ffi.responses["store_text_file"] = "txt-id:v1"
        key = mock_ffi.store_text_file("/tmp/signed.md")
        assert key == "txt-id:v1"
        assert mock_ffi.calls[-1] == ("store_text_file", ("/tmp/signed.md",), {})

    def test_store_image_file_records_path(self, mock_ffi: MockFFIAdapter) -> None:
        mock_ffi.responses["store_image_file"] = "png-id:v1"
        key = mock_ffi.store_image_file("/tmp/signed.png")
        assert key == "png-id:v1"
        assert mock_ffi.calls[-1] == ("store_image_file", ("/tmp/signed.png",), {})

    def test_get_record_bytes_returns_bytes(self, mock_ffi: MockFFIAdapter) -> None:
        png_magic = bytes(
            [0x89, ord("P"), ord("N"), ord("G"), 0x0D, 0x0A, 0x1A, 0x0A]
        )
        mock_ffi.responses["get_record_bytes"] = png_magic
        out = mock_ffi.get_record_bytes("png-id:v1")
        assert out == png_magic
        assert mock_ffi.calls[-1] == ("get_record_bytes", ("png-id:v1",), {})

    def test_get_record_bytes_default_returns_empty_bytes(
        self, mock_ffi: MockFFIAdapter
    ) -> None:
        # Sanity: when no response is staged, MockFFIAdapter falls back to b""
        # (instead of {} — bytes return type is honored even on the unset path).
        out = mock_ffi.get_record_bytes("missing")
        assert out == b""
        assert mock_ffi.calls[-1] == ("get_record_bytes", ("missing",), {})


# ---------------------------------------------------------------------------
# Generic JACS Document Store CRUD — also part of Issue 025's "20 methods"
# scope (the 13 generic + 4 D5 + 3 D9 = 20).
# ---------------------------------------------------------------------------


class TestJacsDocumentStoreCrud:
    """Pin the wire shape for the generic CRUD methods exposed in the
    parity fixture's ``jacs_document_store`` group."""

    def test_sign_and_store_passes_data_json(self, mock_ffi: MockFFIAdapter) -> None:
        payload_json = json.dumps({"hello": "world"})
        mock_ffi.responses["sign_and_store"] = {"key": "id1:v1", "json": "{}"}
        out = mock_ffi.sign_and_store(payload_json)
        assert out == {"key": "id1:v1", "json": "{}"}
        assert mock_ffi.calls[-1] == ("sign_and_store", (payload_json,), {})

    def test_search_documents_passes_query_limit_offset(
        self, mock_ffi: MockFFIAdapter
    ) -> None:
        mock_ffi.responses["search_documents"] = {"results": [], "total_count": 0}
        out = mock_ffi.search_documents("marker-xyz", 10, 0)
        assert out == {"results": [], "total_count": 0}
        assert mock_ffi.calls[-1] == ("search_documents", ("marker-xyz", 10, 0), {})

    def test_query_by_type_passes_three_args(self, mock_ffi: MockFFIAdapter) -> None:
        # Trait returns Vec<String> -> Python list[str] (PRD §4.5).
        mock_ffi.responses["query_by_type"] = ["k1", "k2"]
        out = mock_ffi.query_by_type("memory", 25, 0)
        assert out == ["k1", "k2"]
        assert mock_ffi.calls[-1] == ("query_by_type", ("memory", 25, 0), {})

    def test_list_documents_returns_list_of_strings(
        self, mock_ffi: MockFFIAdapter
    ) -> None:
        # Trait returns Vec<String>; the adapter MUST decode to list[str], not dict.
        mock_ffi.responses["list_documents"] = ["id1:v1", "id2:v1"]
        out = mock_ffi.list_documents()
        assert out == ["id1:v1", "id2:v1"]
        assert all(isinstance(k, str) for k in out)

    def test_storage_capabilities_takes_no_args(self, mock_ffi: MockFFIAdapter) -> None:
        mock_ffi.responses["storage_capabilities"] = {
            "fulltext": True,
            "vector": False,
        }
        out = mock_ffi.storage_capabilities()
        assert out["fulltext"] is True
        assert out["vector"] is False
        assert mock_ffi.calls[-1] == ("storage_capabilities", (), {})

    def test_remove_document_returns_none(self, mock_ffi: MockFFIAdapter) -> None:
        # Issue 025: tombstone returns void, not a key — pin that contract
        # so a future regression that starts returning a body fails fast.
        out = mock_ffi.remove_document("id1:v1")
        assert out is None
        assert mock_ffi.calls[-1] == ("remove_document", ("id1:v1",), {})


# ---------------------------------------------------------------------------
# FFI surface area — every D5/D9 method appears in the parity fixture.
# ---------------------------------------------------------------------------


class TestD5D9MethodsAreInParityFixture:
    """The 7 new methods MUST appear in ``fixtures/ffi_method_parity.json``.
    If a future PR adds (or renames) one of them, this test surfaces the gap
    immediately rather than silently skipping the parity matrix."""

    EXPECTED_D5_METHODS = ("save_memory", "save_soul", "get_memory", "get_soul")
    EXPECTED_D9_METHODS = ("store_text_file", "store_image_file", "get_record_bytes")

    def _load_parity_methods(self) -> set[str]:
        from pathlib import Path

        fixture_path = (
            Path(__file__).resolve().parents[2] / "fixtures" / "ffi_method_parity.json"
        )
        fixture = json.loads(fixture_path.read_text())
        names: set[str] = set()
        for group in fixture["methods"].values():
            for m in group:
                names.add(m["name"])
        return names

    def test_all_d5_methods_appear_in_parity(self) -> None:
        parity = self._load_parity_methods()
        for name in self.EXPECTED_D5_METHODS:
            assert (
                name in parity
            ), f"D5 method {name!r} missing from ffi_method_parity.json"

    def test_all_d9_methods_appear_in_parity(self) -> None:
        parity = self._load_parity_methods()
        for name in self.EXPECTED_D9_METHODS:
            assert (
                name in parity
            ), f"D9 method {name!r} missing from ffi_method_parity.json"
