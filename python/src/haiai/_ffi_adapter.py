"""FFI Adapter -- wraps haiipy native binding for Python SDK.

All HTTP calls delegate to the Rust hai-binding-core via PyO3.
This module handles:
- JSON serialization/deserialization across FFI boundary
- Error mapping from FFI error strings to Python exception classes
- Sync and async variants for all methods

The haiipy binding provides:
- Async methods via ``pyo3_async_runtimes::tokio::future_into_py``
- Sync methods with ``_sync`` suffix via ``py.allow_threads(|| RT.block_on(...))``
"""

from __future__ import annotations

import json
import re
from typing import Any, Optional

from haiai.errors import (
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    EmailNotActive,
    RateLimited,
    RecipientNotFound,
)


# =============================================================================
# Error Mapping
# =============================================================================


def map_ffi_error(err: Exception) -> HaiError:
    """Map an FFI error (raised by PyO3) to the appropriate Python exception.

    FFI errors have the format: "{ErrorKind}: {message}"
    e.g. "AuthFailed: JACS signature rejected"
    """
    message = str(err)

    if message.startswith("AuthFailed:"):
        return HaiAuthError(message[len("AuthFailed:"):].strip(), status_code=401)
    if message.startswith("RateLimited:"):
        return RateLimited(message[len("RateLimited:"):].strip(), status_code=429)
    if message.startswith("NotFound:"):
        msg = message[len("NotFound:"):].strip()
        if "email not active" in msg.lower():
            return EmailNotActive(msg, status_code=403)
        if "recipient" in msg.lower():
            return RecipientNotFound(msg, status_code=400)
        return HaiApiError(msg, status_code=404)
    if message.startswith("NetworkFailed:"):
        return HaiConnectionError(message[len("NetworkFailed:"):].strip())
    if message.startswith("ApiError:"):
        msg = message[len("ApiError:"):].strip()
        # Try to extract status code
        match = re.search(r"status (\d+)", msg)
        status = int(match.group(1)) if match else None
        if "email not active" in msg.lower():
            return EmailNotActive(msg, status_code=status or 403)
        if "recipient" in msg.lower():
            return RecipientNotFound(msg, status_code=status or 400)
        return HaiApiError(msg, status_code=status or 0)
    if message.startswith("ConfigFailed:"):
        return HaiError(message[len("ConfigFailed:"):].strip())
    if message.startswith("SerializationFailed:"):
        return HaiError(message[len("SerializationFailed:"):].strip())
    if message.startswith("InvalidArgument:"):
        return HaiError(message[len("InvalidArgument:"):].strip())
    if message.startswith("ProviderError:"):
        return HaiAuthError(message[len("ProviderError:"):].strip())

    return HaiError(message)


# =============================================================================
# Sync FFI Adapter
# =============================================================================


class FFIAdapter:
    """Synchronous FFI adapter wrapping haiipy.HaiClient.

    Every method:
    1. Serializes arguments to JSON where needed
    2. Calls the native ``_sync`` FFI method (releases GIL)
    3. Parses the JSON response
    4. Catches FFI errors and maps them to Python exceptions
    """

    def __init__(self, config_json: str) -> None:
        try:
            import haiipy  # type: ignore[import-untyped]
        except ImportError as e:
            raise HaiError(
                "Failed to import haiipy native binding. "
                "Ensure the haiipy package is installed."
            ) from e
        self._native = haiipy.HaiClient(config_json)

    # --- Registration & Identity ---

    def hello(self, include_test: bool = False) -> dict[str, Any]:
        try:
            raw = self._native.hello_sync(include_test)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def check_username(self, username: str) -> dict[str, Any]:
        try:
            raw = self._native.check_username_sync(username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def register(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.register_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def register_new_agent(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.register_new_agent_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def rotate_keys(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.rotate_keys_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def update_agent(self, agent_data: str) -> dict[str, Any]:
        try:
            raw = self._native.update_agent_sync(agent_data)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def submit_response(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.submit_response_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def verify_status(self, agent_id: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = self._native.verify_status_sync(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Username ---

    def claim_username(self, agent_id: str, username: str) -> dict[str, Any]:
        try:
            raw = self._native.claim_username_sync(agent_id, username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def update_username(self, agent_id: str, username: str) -> dict[str, Any]:
        try:
            raw = self._native.update_username_sync(agent_id, username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def delete_username(self, agent_id: str) -> dict[str, Any]:
        try:
            raw = self._native.delete_username_sync(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Core ---

    def send_email(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.send_email_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def send_signed_email(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.send_signed_email_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def list_messages(self, options: dict[str, Any]) -> list[Any]:
        try:
            raw = self._native.list_messages_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def update_labels(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.update_labels_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_email_status(self) -> dict[str, Any]:
        try:
            raw = self._native.get_email_status_sync()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_message(self, message_id: str) -> dict[str, Any]:
        try:
            raw = self._native.get_message_sync(message_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_unread_count(self) -> int:
        try:
            raw = self._native.get_unread_count_sync()
            data = json.loads(raw)
            # binding-core serializes the u64 return directly, so JSON is a bare number
            if isinstance(data, int):
                return data
            # Fallback: if the shape is {"count": N} (future API change)
            if isinstance(data, dict):
                return data.get("count", 0)
            return 0
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Actions ---

    def mark_read(self, message_id: str) -> None:
        try:
            self._native.mark_read_sync(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def mark_unread(self, message_id: str) -> None:
        try:
            self._native.mark_unread_sync(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def delete_message(self, message_id: str) -> None:
        try:
            self._native.delete_message_sync(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def archive(self, message_id: str) -> None:
        try:
            self._native.archive_sync(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def unarchive(self, message_id: str) -> None:
        try:
            self._native.unarchive_sync(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def reply_with_options(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.reply_with_options_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def forward(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.forward_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Search & Contacts ---

    def search_messages(self, options: dict[str, Any]) -> list[Any]:
        try:
            raw = self._native.search_messages_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def contacts(self) -> list[Any]:
        try:
            raw = self._native.contacts_sync()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Server Keys ---

    def fetch_server_keys(self) -> dict[str, Any]:
        try:
            raw = self._native.fetch_server_keys_sync()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Sign/Verify (raw) ---

    def sign_email_raw(self, raw_email_b64: str) -> str:
        try:
            return self._native.sign_email_raw_sync(raw_email_b64)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def verify_email_raw(self, raw_email_b64: str) -> dict[str, Any]:
        try:
            raw = self._native.verify_email_raw_sync(raw_email_b64)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Attestations ---

    def create_attestation(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.create_attestation_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def list_attestations(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.list_attestations_sync(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_attestation(self, agent_id: str, doc_id: str) -> dict[str, Any]:
        try:
            raw = self._native.get_attestation_sync(agent_id, doc_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def verify_attestation(self, document: str) -> dict[str, Any]:
        try:
            raw = self._native.verify_attestation_sync(document)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Templates ---

    def create_email_template(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.create_email_template_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def list_email_templates(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.list_email_templates_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_email_template(self, template_id: str) -> dict[str, Any]:
        try:
            raw = self._native.get_email_template_sync(template_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def update_email_template(self, template_id: str, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.update_email_template_sync(template_id, json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def delete_email_template(self, template_id: str) -> None:
        try:
            self._native.delete_email_template_sync(template_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Key Operations ---

    def fetch_remote_key(self, jacs_id: str, version: str = "latest") -> dict[str, Any]:
        try:
            raw = self._native.fetch_remote_key_sync(jacs_id, version)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def fetch_key_by_hash(self, hash_val: str) -> dict[str, Any]:
        try:
            raw = self._native.fetch_key_by_hash_sync(hash_val)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def fetch_key_by_email(self, email: str) -> dict[str, Any]:
        try:
            raw = self._native.fetch_key_by_email_sync(email)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def fetch_key_by_domain(self, domain: str) -> dict[str, Any]:
        try:
            raw = self._native.fetch_key_by_domain_sync(domain)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def fetch_all_keys(self, jacs_id: str) -> dict[str, Any]:
        try:
            raw = self._native.fetch_all_keys_sync(jacs_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Verification ---

    def verify_document(self, document: str) -> dict[str, Any]:
        try:
            raw = self._native.verify_document_sync(document)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def get_verification(self, agent_id: str) -> dict[str, Any]:
        try:
            raw = self._native.get_verification_sync(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def verify_agent_document(self, request_json: str) -> dict[str, Any]:
        try:
            raw = self._native.verify_agent_document_sync(request_json)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Benchmarks ---

    def benchmark(self, name: Optional[str] = None, tier: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = self._native.benchmark_sync(name, tier)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def free_run(self, transport: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = self._native.free_run_sync(transport)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def pro_run(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = self._native.pro_run_sync(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def enterprise_run(self) -> None:
        try:
            self._native.enterprise_run_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- JACS Delegation ---

    def build_auth_header(self) -> str:
        try:
            return self._native.build_auth_header_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def sign_message(self, message: str) -> str:
        try:
            return self._native.sign_message_sync(message)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def canonical_json(self, value_json: str) -> str:
        try:
            return self._native.canonical_json_sync(value_json)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def verify_a2a_artifact(self, wrapped_json: str) -> dict[str, Any]:
        try:
            raw = self._native.verify_a2a_artifact_sync(wrapped_json)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def export_agent_json(self) -> dict[str, Any]:
        try:
            raw = self._native.export_agent_json_sync()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Client State ---

    def jacs_id(self) -> str:
        try:
            return self._native.jacs_id_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def base_url(self) -> str:
        try:
            return self._native.base_url_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def hai_agent_id(self) -> str:
        try:
            return self._native.hai_agent_id_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def agent_email(self) -> Optional[str]:
        try:
            return self._native.agent_email_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def set_hai_agent_id(self, agent_id: str) -> None:
        try:
            self._native.set_hai_agent_id_sync(agent_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def set_agent_email(self, email: str) -> None:
        try:
            self._native.set_agent_email_sync(email)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- SSE Streaming ---

    def connect_sse(self) -> int:
        try:
            return self._native.connect_sse_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def sse_next_event(self, handle: int) -> Optional[dict[str, Any]]:
        try:
            raw = self._native.sse_next_event_sync(handle)
            if raw is None:
                return None
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def sse_close(self, handle: int) -> None:
        try:
            self._native.sse_close_sync(handle)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- WS Streaming ---

    def connect_ws(self) -> int:
        try:
            return self._native.connect_ws_sync()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def ws_next_event(self, handle: int) -> Optional[dict[str, Any]]:
        try:
            raw = self._native.ws_next_event_sync(handle)
            if raw is None:
                return None
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    def ws_close(self, handle: int) -> None:
        try:
            self._native.ws_close_sync(handle)
        except RuntimeError as err:
            raise map_ffi_error(err) from err


# =============================================================================
# Async FFI Adapter
# =============================================================================


class AsyncFFIAdapter:
    """Async FFI adapter wrapping haiipy.HaiClient.

    Every method:
    1. Serializes arguments to JSON where needed
    2. Calls the native async FFI method (returns Python coroutine)
    3. Parses the JSON response
    4. Catches FFI errors and maps them to Python exceptions
    """

    def __init__(self, config_json: str) -> None:
        try:
            import haiipy  # type: ignore[import-untyped]
        except ImportError as e:
            raise HaiError(
                "Failed to import haiipy native binding. "
                "Ensure the haiipy package is installed."
            ) from e
        self._native = haiipy.HaiClient(config_json)

    # --- Registration & Identity ---

    async def hello(self, include_test: bool = False) -> dict[str, Any]:
        try:
            raw = await self._native.hello(include_test)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def check_username(self, username: str) -> dict[str, Any]:
        try:
            raw = await self._native.check_username(username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def register(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.register(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def register_new_agent(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.register_new_agent(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def rotate_keys(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.rotate_keys(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def update_agent(self, agent_data: str) -> dict[str, Any]:
        try:
            raw = await self._native.update_agent(agent_data)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def submit_response(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.submit_response(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def verify_status(self, agent_id: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = await self._native.verify_status(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Username ---

    async def claim_username(self, agent_id: str, username: str) -> dict[str, Any]:
        try:
            raw = await self._native.claim_username(agent_id, username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def update_username(self, agent_id: str, username: str) -> dict[str, Any]:
        try:
            raw = await self._native.update_username(agent_id, username)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def delete_username(self, agent_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.delete_username(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Core ---

    async def send_email(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.send_email(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def send_signed_email(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.send_signed_email(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def list_messages(self, options: dict[str, Any]) -> list[Any]:
        try:
            raw = await self._native.list_messages(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def update_labels(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.update_labels(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_email_status(self) -> dict[str, Any]:
        try:
            raw = await self._native.get_email_status()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_message(self, message_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.get_message(message_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_unread_count(self) -> int:
        try:
            raw = await self._native.get_unread_count()
            data = json.loads(raw)
            # binding-core serializes the u64 return directly, so JSON is a bare number
            if isinstance(data, int):
                return data
            # Fallback: if the shape is {"count": N} (future API change)
            if isinstance(data, dict):
                return data.get("count", 0)
            return 0
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Actions ---

    async def mark_read(self, message_id: str) -> None:
        try:
            await self._native.mark_read(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def mark_unread(self, message_id: str) -> None:
        try:
            await self._native.mark_unread(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def delete_message(self, message_id: str) -> None:
        try:
            await self._native.delete_message(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def archive(self, message_id: str) -> None:
        try:
            await self._native.archive(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def unarchive(self, message_id: str) -> None:
        try:
            await self._native.unarchive(message_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def reply_with_options(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.reply_with_options(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def forward(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.forward(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Search & Contacts ---

    async def search_messages(self, options: dict[str, Any]) -> list[Any]:
        try:
            raw = await self._native.search_messages(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def contacts(self) -> list[Any]:
        try:
            raw = await self._native.contacts()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Server Keys ---

    async def fetch_server_keys(self) -> dict[str, Any]:
        try:
            raw = await self._native.fetch_server_keys()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Sign/Verify (raw) ---

    async def sign_email_raw(self, raw_email_b64: str) -> str:
        try:
            return await self._native.sign_email_raw(raw_email_b64)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def verify_email_raw(self, raw_email_b64: str) -> dict[str, Any]:
        try:
            raw = await self._native.verify_email_raw(raw_email_b64)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Attestations ---

    async def create_attestation(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.create_attestation(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def list_attestations(self, params: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.list_attestations(json.dumps(params))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_attestation(self, agent_id: str, doc_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.get_attestation(agent_id, doc_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def verify_attestation(self, document: str) -> dict[str, Any]:
        try:
            raw = await self._native.verify_attestation(document)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Email Templates ---

    async def create_email_template(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.create_email_template(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def list_email_templates(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.list_email_templates(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_email_template(self, template_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.get_email_template(template_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def update_email_template(self, template_id: str, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.update_email_template(template_id, json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def delete_email_template(self, template_id: str) -> None:
        try:
            await self._native.delete_email_template(template_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Key Operations ---

    async def fetch_remote_key(self, jacs_id: str, version: str = "latest") -> dict[str, Any]:
        try:
            raw = await self._native.fetch_remote_key(jacs_id, version)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def fetch_key_by_hash(self, hash_val: str) -> dict[str, Any]:
        try:
            raw = await self._native.fetch_key_by_hash(hash_val)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def fetch_key_by_email(self, email: str) -> dict[str, Any]:
        try:
            raw = await self._native.fetch_key_by_email(email)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def fetch_key_by_domain(self, domain: str) -> dict[str, Any]:
        try:
            raw = await self._native.fetch_key_by_domain(domain)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def fetch_all_keys(self, jacs_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.fetch_all_keys(jacs_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Verification ---

    async def verify_document(self, document: str) -> dict[str, Any]:
        try:
            raw = await self._native.verify_document(document)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def get_verification(self, agent_id: str) -> dict[str, Any]:
        try:
            raw = await self._native.get_verification(agent_id)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def verify_agent_document(self, request_json: str) -> dict[str, Any]:
        try:
            raw = await self._native.verify_agent_document(request_json)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Benchmarks ---

    async def benchmark(self, name: Optional[str] = None, tier: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = await self._native.benchmark(name, tier)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def free_run(self, transport: Optional[str] = None) -> dict[str, Any]:
        try:
            raw = await self._native.free_run(transport)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def pro_run(self, options: dict[str, Any]) -> dict[str, Any]:
        try:
            raw = await self._native.pro_run(json.dumps(options))
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def enterprise_run(self) -> None:
        try:
            await self._native.enterprise_run()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- JACS Delegation ---

    async def build_auth_header(self) -> str:
        try:
            return await self._native.build_auth_header()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def sign_message(self, message: str) -> str:
        try:
            return await self._native.sign_message(message)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def canonical_json(self, value_json: str) -> str:
        try:
            return await self._native.canonical_json(value_json)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def verify_a2a_artifact(self, wrapped_json: str) -> dict[str, Any]:
        try:
            raw = await self._native.verify_a2a_artifact(wrapped_json)
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def export_agent_json(self) -> dict[str, Any]:
        try:
            raw = await self._native.export_agent_json()
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- Client State ---

    async def jacs_id(self) -> str:
        try:
            return await self._native.jacs_id()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def base_url(self) -> str:
        try:
            return await self._native.base_url()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def hai_agent_id(self) -> str:
        try:
            return await self._native.hai_agent_id()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def agent_email(self) -> Optional[str]:
        try:
            return await self._native.agent_email()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def set_hai_agent_id(self, agent_id: str) -> None:
        try:
            await self._native.set_hai_agent_id(agent_id)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def set_agent_email(self, email: str) -> None:
        try:
            await self._native.set_agent_email(email)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- SSE Streaming ---

    async def connect_sse(self) -> int:
        try:
            return await self._native.connect_sse()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def sse_next_event(self, handle: int) -> Optional[dict[str, Any]]:
        try:
            raw = await self._native.sse_next_event(handle)
            if raw is None:
                return None
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def sse_close(self, handle: int) -> None:
        try:
            await self._native.sse_close(handle)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    # --- WS Streaming ---

    async def connect_ws(self) -> int:
        try:
            return await self._native.connect_ws()
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def ws_next_event(self, handle: int) -> Optional[dict[str, Any]]:
        try:
            raw = await self._native.ws_next_event(handle)
            if raw is None:
                return None
            return json.loads(raw)
        except RuntimeError as err:
            raise map_ffi_error(err) from err

    async def ws_close(self, handle: int) -> None:
        try:
            await self._native.ws_close(handle)
        except RuntimeError as err:
            raise map_ffi_error(err) from err
