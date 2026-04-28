/*
 * haiigo.h -- C header for the haiigo Rust cdylib.
 *
 * Manually maintained because cbindgen cannot expand Rust macros.
 * The authoritative list of exported symbols matches the CGo extern
 * declarations in go/ffi/ffi.go.
 *
 * All methods return a JSON error envelope:
 *   Success: {"ok": <result>}
 *   Error:   {"error": {"kind": "...", "message": "..."}}
 *
 * Every returned char* MUST be freed with hai_free_string().
 */

#ifndef HAIIGO_H
#define HAIIGO_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque client handle */
typedef const void *HaiClientHandle;

/* --------------------------------------------------------------------------
 * Handle management
 * -------------------------------------------------------------------------- */

/**
 * Retrieve the last error from `hai_client_new` as a JSON string.
 * Returns null if no error is stored. Caller must free the returned string
 * with `hai_free_string`.
 */
char *hai_last_error(void);

/**
 * Create a new HAI client from a config JSON string.
 * Returns an opaque handle. Caller must call `hai_client_free` when done.
 * On failure, returns null. Call `hai_last_error()` to get error details.
 */
HaiClientHandle hai_client_new(const char *config_json);

/** Free a HAI client handle. */
void hai_client_free(HaiClientHandle handle);

/** Free a string returned by any hai_* function. */
void hai_free_string(char *s);

/* --------------------------------------------------------------------------
 * Registration & Identity
 * -------------------------------------------------------------------------- */

char *hai_hello(HaiClientHandle handle, bool include_test);
char *hai_register(HaiClientHandle handle, const char *options_json);
char *hai_rotate_keys(HaiClientHandle handle, const char *options_json);
char *hai_update_agent(HaiClientHandle handle, const char *agent_data);
char *hai_submit_response(HaiClientHandle handle, const char *params_json);
char *hai_verify_status(HaiClientHandle handle, const char *agent_id);

/* --------------------------------------------------------------------------
 * Username
 * -------------------------------------------------------------------------- */

char *hai_update_username(HaiClientHandle handle, const char *agent_id, const char *username);
char *hai_delete_username(HaiClientHandle handle, const char *agent_id);

/* --------------------------------------------------------------------------
 * Email Core
 * -------------------------------------------------------------------------- */

char *hai_send_email(HaiClientHandle handle, const char *options_json);
char *hai_send_signed_email(HaiClientHandle handle, const char *options_json);
char *hai_list_messages(HaiClientHandle handle, const char *options_json);
char *hai_update_labels(HaiClientHandle handle, const char *params_json);
char *hai_get_email_status(HaiClientHandle handle);
char *hai_get_message(HaiClientHandle handle, const char *message_id);
char *hai_get_raw_email(HaiClientHandle handle, const char *message_id);
char *hai_get_unread_count(HaiClientHandle handle);

/* --------------------------------------------------------------------------
 * Email Actions
 * -------------------------------------------------------------------------- */

char *hai_mark_read(HaiClientHandle handle, const char *message_id);
char *hai_mark_unread(HaiClientHandle handle, const char *message_id);
char *hai_delete_message(HaiClientHandle handle, const char *message_id);
char *hai_archive(HaiClientHandle handle, const char *message_id);
char *hai_unarchive(HaiClientHandle handle, const char *message_id);
char *hai_reply_with_options(HaiClientHandle handle, const char *params_json);
char *hai_forward(HaiClientHandle handle, const char *params_json);

/* --------------------------------------------------------------------------
 * Search & Contacts
 * -------------------------------------------------------------------------- */

char *hai_search_messages(HaiClientHandle handle, const char *options_json);
char *hai_contacts(HaiClientHandle handle);

/* --------------------------------------------------------------------------
 * Key Operations
 * -------------------------------------------------------------------------- */

char *hai_fetch_remote_key(HaiClientHandle handle, const char *jacs_id, const char *version);
char *hai_fetch_key_by_hash(HaiClientHandle handle, const char *hash);
char *hai_fetch_key_by_email(HaiClientHandle handle, const char *email);
char *hai_fetch_key_by_domain(HaiClientHandle handle, const char *domain);
char *hai_fetch_all_keys(HaiClientHandle handle, const char *jacs_id);

/* --------------------------------------------------------------------------
 * Verification
 * -------------------------------------------------------------------------- */

char *hai_verify_document(HaiClientHandle handle, const char *document);
char *hai_get_verification(HaiClientHandle handle, const char *agent_id);
char *hai_verify_agent_document(HaiClientHandle handle, const char *request_json);

/* --------------------------------------------------------------------------
 * Benchmarks
 * -------------------------------------------------------------------------- */

char *hai_benchmark(HaiClientHandle handle, const char *name, const char *tier);
char *hai_free_run(HaiClientHandle handle, const char *transport);
char *hai_pro_run(HaiClientHandle handle, const char *options_json);
char *hai_enterprise_run(HaiClientHandle handle);

/* --------------------------------------------------------------------------
 * JACS Delegation
 * -------------------------------------------------------------------------- */

char *hai_sign_message(HaiClientHandle handle, const char *message);
char *hai_canonical_json(HaiClientHandle handle, const char *value_json);
char *hai_verify_a2a_artifact(HaiClientHandle handle, const char *wrapped_json);
char *hai_build_auth_header(HaiClientHandle handle);
char *hai_export_agent_json(HaiClientHandle handle);

/* --------------------------------------------------------------------------
 * Client State (Read)
 * -------------------------------------------------------------------------- */

/**
 * Get the JACS ID of the client.
 * Returns a JSON envelope: {"ok":"<jacs_id>"} or {"error":...}.
 */
char *hai_jacs_id(HaiClientHandle handle);

/* --------------------------------------------------------------------------
 * Client State (Mutating)
 * -------------------------------------------------------------------------- */

char *hai_set_hai_agent_id(HaiClientHandle handle, const char *id);
char *hai_set_agent_email(HaiClientHandle handle, const char *email);

#ifdef __cplusplus
}
#endif

#endif  /* HAIIGO_H */
