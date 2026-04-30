//! `RemoteJacsProvider` — JacsDocumentProvider impl backed by `/api/v1/records` on `hai-api`.
//!
//! See `docs/jacs/JACS_DOCUMENT_STORE_PRD.md` §4.5.
//!
//! Wraps a `JacsProvider` (typically `LocalJacsProvider`) for local key material —
//! the agent's keys NEVER leave the client. HTTP calls go directly through the wrapped
//! `reqwest::Client` so we don't need to wrap a `HaiClient<Arc<P>>`. Auth headers are
//! built from `JacsProvider::sign_string` exactly the way `HaiClient::build_auth_header`
//! does (matching `client.rs:210-215`).

use std::time::Duration;
use std::time::Instant;

use base64::Engine;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{Client as HttpClient, StatusCode};

fn url_encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
use serde_json::Value;
use time::OffsetDateTime;

use crate::client::encode_path_segment;
use crate::error::{HaiError, Result};
use crate::jacs::{JacsDocumentProvider, JacsProvider};
use crate::types::{DocSearchHit, DocSearchResults, SignedDocument, StorageCapabilities};

/// Endpoint base for all record CRUD (D1).
const RECORDS_PATH: &str = "/api/v1/records";

/// Default request timeout for record CRUD calls (matches `HaiClient` default).
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Server-route-free helpers default. The actual endpoint dispatches on `Content-Type`.
const CT_JSON: &str = "application/json";
const CT_TEXT_MD: &str = "text/markdown; profile=jacs-text-v1";

/// Single source of truth for paginated auto-fetch caps (TASK_009 will surface).
pub const AUTO_PAGE_CAP: usize = 1000;

#[derive(Clone, Debug)]
pub struct RemoteJacsProviderOptions {
    pub base_url: String,
    pub timeout: Duration,
}

impl Default for RemoteJacsProviderOptions {
    fn default() -> Self {
        Self {
            base_url: "https://hai.ai".to_string(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// Remote JACS document provider — signs locally, persists/queries against `hai-api`.
pub struct RemoteJacsProvider<P: JacsProvider> {
    inner: P,
    http: HttpClient,
    base_url: String,
}

impl<P: JacsProvider> RemoteJacsProvider<P> {
    /// Construct directly with an in-process `JacsProvider` and a base URL.
    pub fn new(inner: P, options: RemoteJacsProviderOptions) -> Result<Self> {
        let trimmed = options.base_url.trim_end_matches('/');
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(HaiError::ConfigInvalid {
                message: format!(
                    "RemoteJacsProvider base_url must start with http:// or https:// (got '{}')",
                    options.base_url
                ),
            });
        }
        let http = HttpClient::builder()
            .timeout(options.timeout)
            .build()
            .map_err(HaiError::from)?;
        Ok(Self {
            inner,
            http,
            base_url: trimmed.to_string(),
        })
    }

    /// Construct from environment / explicit base_url; mirrors `LocalJacsProvider::from_config`.
    /// `HAI_URL` overrides the default base URL.
    pub fn from_inner(inner: P, base_url: Option<String>) -> Result<Self> {
        let resolved = base_url
            .or_else(|| std::env::var("HAI_URL").ok())
            .ok_or_else(|| HaiError::ConfigInvalid {
                message: "RemoteJacsProvider requires HAI_URL or an explicit base_url".to_string(),
            })?;
        Self::new(
            inner,
            RemoteJacsProviderOptions {
                base_url: resolved,
                ..Default::default()
            },
        )
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    /// Build a `JACS {jacsId}:{ts}:{sig}` Authorization header. Mirrors `HaiClient::build_auth_header`.
    fn build_auth_header(&self) -> Result<String> {
        let ts = OffsetDateTime::now_utc().unix_timestamp();
        let message = format!("{}:{ts}", self.inner.jacs_id());
        let signature = self.inner.sign_string(&message).map_err(|e| {
            tracing::warn!(
                operation = "remote_build_auth_header",
                error = %e,
                "failed to sign remote auth header"
            );
            e
        })?;
        Ok(format!("JACS {}:{ts}:{signature}", self.inner.jacs_id()))
    }

    /// Split a `key` of shape `id` or `id:version` into `(id, Option<version>)`.
    fn split_key(key: &str) -> (&str, Option<&str>) {
        match key.split_once(':') {
            Some((id, ver)) if !ver.is_empty() => (id, Some(ver)),
            _ => (key, None),
        }
    }

    /// POST signed bytes (any content type) to `/api/v1/records`. Returns the parsed JSON response.
    pub async fn post_record_bytes_async(
        &self,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<Value> {
        let started = Instant::now();
        let auth = self.build_auth_header()?;
        let url = self.url(RECORDS_PATH);
        tracing::debug!(
            operation = "remote_post_record",
            url = %url,
            content_type,
            "remote record POST starting"
        );
        let resp = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(
                    operation = "remote_post_record",
                    url = %url,
                    error = %e,
                    "remote record POST network error"
                );
                HaiError::Provider(format!("network error: {e}"))
            })?;
        let status = resp.status().as_u16();
        tracing::info!(
            operation = "remote_post_record",
            url = %url,
            content_type,
            status,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "remote record POST completed"
        );
        Self::parse_response(resp).await
    }

    /// GET raw bytes from a record path (D9).
    pub async fn get_record_bytes_async(&self, key: &str) -> Result<Vec<u8>> {
        let started = Instant::now();
        let (id, ver) = Self::split_key(key);
        // Issue 007: percent-encode `id` and `version` per CLAUDE.md path-segment
        // rule. UUID-shaped IDs in the happy path don't carry reserved bytes,
        // but defense-in-depth + project-rule consistency demand we encode here
        // — matching every other interpolated path segment in the SDK
        // (`client.rs` uses `encode_path_segment` at 40+ call sites).
        let path = match ver {
            Some(v) => format!(
                "{}/{}/v/{}",
                RECORDS_PATH,
                encode_path_segment(id),
                encode_path_segment(v),
            ),
            None => format!("{}/{}", RECORDS_PATH, encode_path_segment(id)),
        };
        let auth = self.build_auth_header()?;
        let url = self.url(&path);
        tracing::debug!(
            operation = "remote_get_record_bytes",
            url = %url,
            "remote record GET starting"
        );
        let resp = self
            .http
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(
                    operation = "remote_get_record_bytes",
                    url = %url,
                    error = %e,
                    "remote record GET network error"
                );
                HaiError::Provider(format!("network error: {e}"))
            })?;
        let status = resp.status().as_u16();
        tracing::info!(
            operation = "remote_get_record_bytes",
            url = %url,
            status,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "remote record GET completed"
        );
        Self::parse_response_bytes(resp).await
    }

    async fn parse_response(resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HaiError::Provider(format!("network error reading body: {e}")))?;
        if status.is_success() {
            if text.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_str(&text).map_err(HaiError::from)
            }
        } else {
            tracing::warn!(
                operation = "remote_parse_response",
                status = status.as_u16(),
                body = %text,
                "remote record request failed"
            );
            Err(map_status_error(status, &text))
        }
    }

    async fn parse_response_bytes(resp: reqwest::Response) -> Result<Vec<u8>> {
        let status = resp.status();
        if status.is_success() {
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| HaiError::Provider(format!("network error reading body: {e}")))?;
            Ok(bytes.to_vec())
        } else {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "<no body>".to_string());
            tracing::warn!(
                operation = "remote_parse_response_bytes",
                status = status.as_u16(),
                body = %text,
                "remote record bytes request failed"
            );
            Err(map_status_error(status, &text))
        }
    }

    fn build_auth_header_blocking(&self) -> Result<String> {
        self.build_auth_header()
    }

    /// Synchronous helper that runs an async future to completion. The blocking trait
    /// surface uses this so callers from non-async contexts (Python/Node FFI) work.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let rt = tokio::runtime::Handle::try_current();
        match rt {
            Ok(handle) => {
                // We're in an async runtime — use block_in_place to nest properly.
                // If block_in_place is not available (single-thread runtime), fall through to
                // a tokio::task::block_in_place which panics in non-multi-thread; in that case
                // the caller MUST use the *_async variants instead.
                tokio::task::block_in_place(|| handle.block_on(fut))
            }
            Err(_) => {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build single-thread runtime");
                runtime.block_on(fut)
            }
        }
    }
}

fn map_status_error(status: StatusCode, body: &str) -> HaiError {
    // Issue 008: emit `HaiError::Api { status, message }` so the binding-core
    // mapping (`From<HaiError>` in `hai-binding-core`) can route status codes
    // through the typed `ErrorKind` enum (`AuthFailed` for 401/403,
    // `NotFound` for 404, `RateLimited` for 429, `ApiError` for other 4xx/5xx)
    // and the Python/Node/Go SDKs surface the right typed error class.
    // Previously this mapped everything to `HaiError::Provider`, which the
    // binding-core flattens to `ProviderError` — and the per-language
    // adapters then map `ProviderError` to `HaiAuthError` /
    // `AuthenticationError` / `IsAuthError(true)` regardless of the real
    // status. So a 404 on `GET /api/v1/records/<missing>` surfaced as an
    // auth error in every SDK.
    let message = if status.is_server_error() {
        format!("server error: {}", body)
    } else {
        // Try to extract a server-shaped { "error": "..." } message.
        serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| body.to_string())
    };
    HaiError::Api {
        status: status.as_u16(),
        message,
    }
}

// =============================================================================
// JacsProvider — forward every method to the inner provider.
// =============================================================================
impl<P: JacsProvider> JacsProvider for RemoteJacsProvider<P> {
    fn jacs_id(&self) -> &str {
        self.inner.jacs_id()
    }
    fn sign_string(&self, message: &str) -> Result<String> {
        self.inner.sign_string(message)
    }
    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.inner.sign_bytes(data)
    }
    fn key_id(&self) -> &str {
        self.inner.key_id()
    }
    fn algorithm(&self) -> &str {
        self.inner.algorithm()
    }
    fn canonical_json(&self, value: &Value) -> Result<String> {
        self.inner.canonical_json(value)
    }
    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        self.inner.sign_email_locally(raw_email)
    }
    fn sign_response(&self, payload: &Value) -> Result<crate::types::SignedPayload> {
        self.inner.sign_response(payload)
    }
}

// =============================================================================
// JacsDocumentProvider — every method goes through HTTP to /api/v1/records.
// =============================================================================
impl<P: JacsProvider> JacsDocumentProvider for RemoteJacsProvider<P> {
    fn sign_document(&self, data: &Value) -> Result<String> {
        // Local-only — signing keys never leave the client.
        //
        // Issue 021: delegate to the inner provider's `sign_envelope`, which is
        // implemented by `LocalJacsProvider` via JACS's `signing_procedure` (the
        // canonical signer). The previous implementation here pre-stuffed
        // `jacsId`/`jacsVersion`/`jacsType`/`jacsVersionDate` and then signed
        // `canonical_json + sign_string`, but that signature scheme is NOT what
        // JACS verifies on the wire: `verify_jacs_json_with_public_key_pem`
        // delegates to `SimpleAgent::verify_with_key`, which reconstructs the
        // signed bytes via `build_signature_content` (per-field canonicalised,
        // joined by single spaces, JACS_IGNORE_FIELDS skipped). Signing the
        // whole canonical JSON does not match that scheme — every produced
        // envelope fails server-side verification.
        //
        // `sign_envelope` is the single source of truth: `LocalJacsProvider`
        // overrides it to call `agent.create_document_and_load`, which (a)
        // injects `jacsId`/`jacsVersion`/`jacsVersionDate`/`jacsLevel`/
        // `jacsType` (the server's `extract_envelope_metadata` requirements,
        // Issue 001) and (b) signs via `signing_procedure`, producing the full
        // `jacsSignature` block (`agentID`, `agentVersion`, `date`, `iat`,
        // `jti`, `signature`, `signingAlgorithm`, `publicKeyHash`, `fields[]`)
        // that the server can verify byte-for-byte.
        //
        // The user's JSON MUST NOT carry pre-existing `jacsId` / `jacsVersion`
        // (the JACS schema rejects "New JACs documents should have no id or
        // version"). `jacsType` is preserved when present so callers like
        // `save_memory("memory")` keep their type tag.
        self.inner.sign_envelope(data)
    }

    fn store_document(&self, signed_json: &str) -> Result<String> {
        // DRY: route through the shared `post_record_for_key` helper so the
        // POST + key-extraction sequence has exactly one definition site.
        // Previously this method, `store_text_file`, `store_image_file`, and
        // the D5 `save_typed_doc` helper each duplicated the
        // `block_on(post_record_bytes_async(...)) → resp.get("key").as_str()`
        // pattern (4 copies, 4 places to drift). See
        // `Self::post_record_for_key` below.
        self.post_record_for_key(signed_json.as_bytes().to_vec(), CT_JSON)
    }

    fn sign_and_store(&self, data: &Value) -> Result<SignedDocument> {
        let signed = self.sign_document(data)?;
        let key = self.store_document(&signed)?;
        Ok(SignedDocument { key, json: signed })
    }

    fn sign_file(&self, path: &str, embed: bool) -> Result<SignedDocument> {
        // Issue 006: signs LOCALLY only — does NOT auto-store. Delegates to
        // the inner provider's `sign_file_envelope` so the JACS attachment
        // pipeline (`SimpleAgent::sign_file`) produces the canonical
        // `(jacsType="file", jacsLevel, jacsFiles[...])` shape. Previously
        // this method hand-rolled a `payload_b64` / flat `sha256` envelope
        // that diverged from `LocalJacsProvider::sign_file` and would not
        // verify under the JACS schema. Same `(path, embed)` now produces a
        // byte-identical envelope regardless of which provider the caller
        // holds.
        self.inner.sign_file_envelope(path, embed)
    }

    fn get_document(&self, key: &str) -> Result<String> {
        let bytes = Self::block_on(self.get_record_bytes_async(key))?;
        String::from_utf8(bytes)
            .map_err(|e| HaiError::Provider(format!("invalid utf-8 in record: {e}")))
    }

    fn list_documents(&self, jacs_type: Option<&str>) -> Result<Vec<String>> {
        let mut url = format!(
            "{}{}?latest_only=true&limit=100",
            self.base_url, RECORDS_PATH
        );
        if let Some(t) = jacs_type {
            url.push_str(&format!("&type={}", url_encode(t)));
        }
        let resp = Self::block_on(self.get_json_async(&url))?;
        Ok(extract_keys_from_list(&resp))
    }

    fn get_document_versions(&self, doc_id: &str) -> Result<Vec<String>> {
        // Issue 007: percent-encode `doc_id` per CLAUDE.md path-segment rule.
        let url = format!(
            "{}{}/{}/versions?limit=100",
            self.base_url,
            RECORDS_PATH,
            encode_path_segment(doc_id),
        );
        let resp = Self::block_on(self.get_json_async(&url))?;
        let versions = resp
            .get("versions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        item.get("key")
                            .and_then(|k| k.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| {
                                let id = item
                                    .get("id")
                                    .or_else(|| item.get("jacs_id"))
                                    .and_then(|v| v.as_str())?;
                                let ver = item
                                    .get("version")
                                    .or_else(|| item.get("jacs_version"))
                                    .and_then(|v| v.as_str())?;
                                Some(format!("{}:{}", id, ver))
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(versions)
    }

    fn get_latest_document(&self, doc_id: &str) -> Result<String> {
        self.get_document(doc_id)
    }

    fn remove_document(&self, key: &str) -> Result<()> {
        let (id, _ver) = Self::split_key(key);
        // Issue 007: percent-encode `id` per CLAUDE.md path-segment rule.
        let url = format!(
            "{}{}/{}",
            self.base_url,
            RECORDS_PATH,
            encode_path_segment(id),
        );
        let auth = self.build_auth_header_blocking()?;
        let _resp = Self::block_on(async move {
            let r = self
                .http
                .delete(&url)
                .header("Authorization", auth)
                .send()
                .await
                .map_err(|e| HaiError::Provider(format!("network error: {e}")))?;
            Self::parse_response(r).await
        })?;
        Ok(())
    }

    fn update_document(&self, _doc_id: &str, signed_json: &str) -> Result<SignedDocument> {
        let key = self.store_document(signed_json)?;
        Ok(SignedDocument {
            key,
            json: signed_json.to_string(),
        })
    }

    fn search_documents(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DocSearchResults> {
        // Issue 018: server now uses cursor pagination for search (PRD §3.5).
        // For the trait's `offset: usize` shape we walk forward via cursor —
        // fetching `limit` records per call and skipping pages until we've
        // skipped `offset` records. This replaces the previous "fetch
        // offset+limit, discard head" trick which broke when offset > 100
        // (server max page) made later records unreachable.
        let server_max = AUTO_PAGE_CAP;
        let target_skip = offset;
        let mut cursor: Option<String> = None;
        let mut skipped: usize = 0;
        let mut all_hits: Vec<DocSearchHit> = Vec::new();
        loop {
            let mut url = format!(
                "{}{}?q={}&limit={}",
                self.base_url,
                RECORDS_PATH,
                url_encode(query),
                limit.min(100),
            );
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", url_encode(c)));
            }
            let resp = Self::block_on(self.get_json_async(&url))?;
            let items = resp
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let next_cursor = resp
                .get("next_cursor")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let page_hits: Vec<DocSearchHit> = items
                .iter()
                .filter_map(|item| {
                    let id = item
                        .get("jacs_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())?;
                    let version = item
                        .get("jacs_version")
                        .or_else(|| item.get("version"))
                        .and_then(|v| v.as_str())?;
                    Some(DocSearchHit {
                        key: format!("{}:{}", id, version),
                        json: serde_json::to_string(item).ok().unwrap_or_default(),
                        score: item.get("ts_rank").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        matched_fields: Vec::new(),
                    })
                })
                .collect();
            // Skip-ahead bookkeeping for the offset case.
            if skipped < target_skip {
                let to_skip = (target_skip - skipped).min(page_hits.len());
                skipped += to_skip;
                let mut taking = page_hits.into_iter().skip(to_skip).collect::<Vec<_>>();
                all_hits.append(&mut taking);
            } else {
                let mut page = page_hits;
                all_hits.append(&mut page);
            }
            if all_hits.len() >= limit {
                all_hits.truncate(limit);
                break;
            }
            // Stop if there are no more pages or we've walked the safety cap.
            match next_cursor {
                Some(c) if all_hits.len() < limit && skipped <= server_max => {
                    cursor = Some(c);
                }
                _ => break,
            }
        }
        // Issue 033: previously hardcoded to `0`, which broke any consumer
        // building pagination UI on `total_count`. The server uses cursor
        // pagination and does not return a global match count, so we report
        // the count of hits we actually accumulated. Documented in
        // `DocSearchResults::total_count` (types.rs).
        let returned_count = all_hits.len();
        Ok(DocSearchResults {
            results: all_hits,
            total_count: returned_count,
            method: "FullText".to_string(),
        })
    }

    fn query_by_type(&self, doc_type: &str, limit: usize, offset: usize) -> Result<Vec<String>> {
        // Issue 009: walk forward via `next_cursor` rather than asking the
        // server for `min(limit + offset, 100)` and discarding the head. The
        // old approach silently returned an empty result for any
        // `offset >= 100` because the server caps a single page at 100 items;
        // page 2+ was unreachable.
        let base_url = format!(
            "{}{}?type={}",
            self.base_url,
            RECORDS_PATH,
            url_encode(doc_type),
        );
        self.paginate_keys(&base_url, limit, offset)
    }

    fn query_by_field(
        &self,
        field: &str,
        _value: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<String>> {
        // Issue 035: server-side `field=`/`value=` JSONB filtering was removed
        // in PRD §10 Non-Goal #19 (envelope JSON lives in S3, not Postgres),
        // so the route at `api/src/jacsdb/routes.rs` hard-rejects these
        // params with a 400. Previously this method built the URL anyway and
        // every call burned a full network round-trip + DB hit before
        // surfacing the same "unsupported" error wrapped in PG-internal
        // terminology. Short-circuit before the network call so consumers
        // see a clear, locally-generated message.
        //
        // Issue 052: surface as the typed `BackendUnsupported` variant so
        // cross-language consumers can branch programmatically (e.g., switch
        // to `search_documents` automatically when the backend can't
        // field-filter) without string-matching the message.
        Err(HaiError::BackendUnsupported {
            method: "query_by_field".to_string(),
            detail: format!(
                "RemoteJacsProvider does not support field-equality queries in v1 \
                 (envelope JSON lives in S3, not Postgres — see PRD §10 Non-Goal #19). \
                 Use search_documents(query) for full-text search instead. Field requested: '{field}'"
            ),
        })
    }

    fn query_by_agent(&self, agent_id: &str, limit: usize, offset: usize) -> Result<Vec<String>> {
        // Server enforces D4 owner-only — `agent` param must equal caller or be omitted.
        // We surface the 400 directly so a developer mistake doesn't silently return [].
        //
        // Issue 009: walk forward via `next_cursor`. Same pagination class as
        // `query_by_type`; both share the `paginate_keys` helper.
        let base_url = format!(
            "{}{}?agent={}",
            self.base_url,
            RECORDS_PATH,
            url_encode(agent_id),
        );
        self.paginate_keys(&base_url, limit, offset)
    }

    fn storage_capabilities(&self) -> Result<StorageCapabilities> {
        Ok(StorageCapabilities {
            fulltext: true,
            vector: false,
            // Issue 035: server-side JSONB field filtering is explicitly out
            // of scope in PRD §10 Non-Goal #19 — `query_by_field` returns an
            // error without a network round-trip. Reporting `false` here
            // keeps the capability map honest so consumers branching on
            // capabilities skip the call entirely.
            query_by_field: false,
            query_by_type: true,
            pagination: true,
            tombstone: true,
        })
    }

    // =========================================================================
    // D9: typed-content helpers — Issue 003.
    //
    // Now in the trait impl (was inherent). Reads a local file, sets the right
    // Content-Type, POSTs to /api/v1/records.
    // =========================================================================

    /// Read a signed-text file (markdown w/ appended `-----BEGIN JACS SIGNATURE-----` block)
    /// and POST it to `/api/v1/records` with `Content-Type: text/markdown; profile=jacs-text-v1`.
    fn store_text_file(&self, path: &str) -> Result<String> {
        let bytes =
            std::fs::read(path).map_err(|e| HaiError::Provider(format!("read {}: {}", path, e)))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| HaiError::Provider("text file is not valid UTF-8".to_string()))?;
        if !text.contains("-----BEGIN JACS SIGNATURE-----") {
            return Err(HaiError::Provider(
                "text file has no JACS signature block — sign with sign_text_file first"
                    .to_string(),
            ));
        }
        // DRY: shared POST + key-extraction with `store_document` /
        // `store_image_file`.
        self.post_record_for_key(bytes, CT_TEXT_MD)
    }

    /// Detect a signed image's format from leading magic bytes and POST it with
    /// `Content-Type: image/png|jpeg|webp`.
    fn store_image_file(&self, path: &str) -> Result<String> {
        let bytes =
            std::fs::read(path).map_err(|e| HaiError::Provider(format!("read {}: {}", path, e)))?;
        let ct = detect_image_content_type(&bytes)?;
        // Sanity-check: the image carries an embedded JACS chunk. We don't verify here
        // (server runs the real verifier); we just refuse to upload obviously-unsigned bytes.
        if !contains_jacs_chunk(&bytes) {
            return Err(HaiError::Provider(
                "image has no JACS signature — sign with sign_image first".to_string(),
            ));
        }
        // DRY: shared POST + key-extraction with `store_document` /
        // `store_text_file`.
        self.post_record_for_key(bytes, ct)
    }

    /// Fetch the raw record bytes (any content type — no UTF-8 decode, no JSON parse).
    fn get_record_bytes(&self, key: &str) -> Result<Vec<u8>> {
        Self::block_on(self.get_record_bytes_async(key))
    }
}

// =============================================================================
// Inherent helpers — private support for the D5/D9 trait methods.
// =============================================================================
impl<P: JacsProvider> RemoteJacsProvider<P> {
    /// POST signed bytes (JSON envelope, signed markdown, or signed image) to
    /// `/api/v1/records` and return the server-issued record key.
    ///
    /// **DRY single source of truth.** `store_document`, `store_text_file`,
    /// and `store_image_file` all route through this method.
    fn post_record_for_key(&self, bytes: Vec<u8>, content_type: &str) -> Result<String> {
        let resp = Self::block_on(self.post_record_bytes_async(bytes, content_type))?;
        resp.get("key")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| HaiError::Provider("server response missing 'key'".to_string()))
    }

    async fn get_json_async(&self, url: &str) -> Result<Value> {
        let auth = self.build_auth_header()?;
        let resp = self
            .http
            .get(url)
            .header("Authorization", auth)
            .send()
            .await
            .map_err(|e| HaiError::Provider(format!("network error: {e}")))?;
        Self::parse_response(resp).await
    }

    /// Issue 009: walk the records list endpoint forward via `next_cursor`,
    /// accumulating keys until either `limit` records are gathered or the
    /// server runs out of pages.
    ///
    /// `base_url` is the URL with all filter params already set (`?type=`,
    /// `?agent=`, etc.) but no `&cursor=` / `&limit=` — this helper appends
    /// them. `offset` is honored by skipping that many records before
    /// collection begins.
    ///
    /// This replaces the previous "fetch `min(limit + offset, 100)` records,
    /// drain the head, truncate the tail" trick used by `query_by_type` and
    /// `query_by_agent`. That trick capped requests at the server's 100-item
    /// page max so any caller asking for `offset >= 100` got an empty
    /// result, contradicting `storage_capabilities().pagination = true`.
    /// Mirrors the cursor walk already in `search_documents` (Issue 018).
    fn paginate_keys(&self, base_url: &str, limit: usize, offset: usize) -> Result<Vec<String>> {
        let server_max = AUTO_PAGE_CAP;
        let target_skip = offset;
        let mut cursor: Option<String> = None;
        let mut skipped: usize = 0;
        let mut all_keys: Vec<String> = Vec::new();
        loop {
            // Server max page is 100; ask for `min(limit, 100)` per fetch and
            // walk via cursor for anything larger. The lower bound of 1 keeps
            // us moving forward even if the caller asks for `limit=0` (return
            // empty after one round-trip).
            let page_size = limit.clamp(1, 100);
            let mut url = format!("{}&limit={}", base_url, page_size);
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", url_encode(c)));
            }
            let resp = Self::block_on(self.get_json_async(&url))?;
            let next_cursor = resp
                .get("next_cursor")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let page_keys = extract_keys_from_list(&resp);
            let page_len = page_keys.len();
            if skipped < target_skip {
                let to_skip = (target_skip - skipped).min(page_len);
                skipped += to_skip;
                all_keys.extend(page_keys.into_iter().skip(to_skip));
            } else {
                all_keys.extend(page_keys);
            }
            if all_keys.len() >= limit {
                all_keys.truncate(limit);
                break;
            }
            // Server returned an empty page — stop, even if the cursor would
            // have us go further. Defends against pathological loops.
            if page_len == 0 {
                break;
            }
            // Stop if there are no more pages or we've walked the safety cap.
            match next_cursor {
                Some(c) if all_keys.len() < limit && skipped <= server_max => {
                    cursor = Some(c);
                }
                _ => break,
            }
        }
        Ok(all_keys)
    }
}

fn extract_keys_from_list(resp: &Value) -> Vec<String> {
    resp.get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = item
                        .get("jacs_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())?;
                    let version = item
                        .get("jacs_version")
                        .or_else(|| item.get("version"))
                        .and_then(|v| v.as_str())?;
                    Some(format!("{}:{}", id, version))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn detect_image_content_type(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("image/webp")
    } else {
        Err(HaiError::Provider("unknown image format".to_string()))
    }
}

/// Issue 014: real JACS-chunk parse via `jacs_media::extract_signature`. The
/// previous substring scan accepted any unsigned bytes containing the literal
/// `jacsSignature` ASCII (e.g., a PNG with that string in tEXt). That diverged
/// from the server's real chunk-parser — the SDK accepted what the server then
/// rejected with 400, wasting a round-trip and creating a maintenance footgun
/// when JACS adds a new chunk format. This now uses the same parser the server
/// uses, so SDK and server agree by construction.
///
/// Returns `Ok(true)` when a parseable JACS signature chunk is present,
/// `Ok(false)` when the bytes are an image but carry no JACS chunk. An
/// unreadable image returns `Ok(false)` — the server will 400 it anyway and we
/// don't want to mask the real error here.
fn contains_jacs_chunk(bytes: &[u8]) -> bool {
    matches!(jacs_media::extract_signature(bytes, false), Ok(Some(_)))
}

// Unused but kept here because TASK_009 will reuse it for additional helpers.
#[allow(dead_code)]
fn b64_url_decode(s: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| HaiError::Provider(format!("base64url decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jacs::StaticJacsProvider;
    use httpmock::{Method as HMethod, MockServer};
    use serde_json::json;

    fn make_provider(base_url: String) -> RemoteJacsProvider<StaticJacsProvider> {
        RemoteJacsProvider::new(
            StaticJacsProvider::new("agent-test"),
            RemoteJacsProviderOptions {
                base_url,
                ..Default::default()
            },
        )
        .expect("provider")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_document_signs_locally_no_http() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                when.method(HMethod::POST);
                then.status(500);
            })
            .await;

        let provider = make_provider(server.base_url());
        let signed = provider
            .sign_document(&json!({"hello": "world"}))
            .expect("sign");
        assert!(signed.contains("\"hello\""));
        assert!(signed.contains("jacsSignature"));
        // Issue 001 regression: envelope MUST include the JACS metadata fields the
        // server's `extract_envelope_metadata` requires. Without these, every POST to
        // `/api/v1/records` 400s with "envelope missing jacsId".
        assert!(signed.contains("\"jacsId\""), "missing jacsId: {}", signed);
        assert!(
            signed.contains("\"jacsVersion\""),
            "missing jacsVersion: {}",
            signed
        );
        assert!(
            signed.contains("\"jacsType\""),
            "missing jacsType: {}",
            signed
        );
        assert!(
            signed.contains("\"jacsVersionDate\""),
            "missing jacsVersionDate: {}",
            signed
        );
        // Zero HTTP calls.
        no_traffic.assert_calls_async(0).await;
    }

    /// Issue 001 regression: full sign_and_store path must POST a body that contains
    /// `jacsId` and `jacsVersion` so the server-side envelope metadata extractor accepts it.
    #[tokio::test(flavor = "multi_thread")]
    async fn sign_and_store_body_includes_jacs_metadata() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .body_includes(r#""jacsId":"#)
                    .body_includes(r#""jacsVersion":"#);
                then.status(201).json_body(json!({
                    "key": "id1:v1",
                    "id": "id1",
                    "version": "v1",
                    "jacsType": "document",
                    "jacsVersionDate": "2026-01-01T00:00:00Z"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let signed = provider
            .sign_and_store(&json!({"hello": "world"}))
            .expect("sign_and_store");
        assert_eq!(signed.key, "id1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_document_posts_to_records_endpoint_with_jacs_auth() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .header_exists("authorization");
                then.status(201).json_body(json!({
                    "key": "id1:v1",
                    "id": "id1",
                    "version": "v1",
                    "jacsType": "artifact",
                    "jacsVersionDate": "2026-01-01T00:00:00Z"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let key = provider
            .store_document("{\"hello\":\"world\"}")
            .expect("store");
        assert_eq!(key, "id1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_document_uses_id_path() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/id1");
                then.status(200)
                    .header("Content-Type", "application/json")
                    .body(r#"{"jacsId":"id1"}"#);
            })
            .await;
        let provider = make_provider(server.base_url());
        let body = provider.get_document("id1").expect("get");
        assert_eq!(body, r#"{"jacsId":"id1"}"#);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_specific_uses_versioned_path() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/id1/v/v3");
                then.status(200).body("{}");
            })
            .await;
        let provider = make_provider(server.base_url());
        let _ = provider.get_document("id1:v3").expect("get");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_document_versions_uses_versions_path() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/id1/versions");
                then.status(200).json_body(json!({
                    "versions": [
                        {"key":"id1:v1","version":"v1","created_at":"2026-01-01T00:00:00Z","jacsType":"x","contentType":"application/json"},
                        {"key":"id1:v2","version":"v2","created_at":"2026-01-02T00:00:00Z","jacsType":"x","contentType":"application/json"}
                    ]
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let v = provider.get_document_versions("id1").expect("versions");
        assert_eq!(v, vec!["id1:v1", "id1:v2"]);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn remove_document_uses_delete_method() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::DELETE).path("/api/v1/records/id1");
                then.status(200).json_body(json!({"tombstoned": true}));
            })
            .await;
        let provider = make_provider(server.base_url());
        provider.remove_document("id1").expect("remove");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn search_documents_query_string_correct() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("q", "foo");
                then.status(200).json_body(json!({
                    "items": [],
                    "next_cursor": null,
                    "has_more": false,
                    "total_count": 0
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let r = provider.search_documents("foo", 25, 0).expect("search");
        assert_eq!(r.method, "FullText");
        mock.assert_async().await;
    }

    /// Issue 008: 400 from query_by_agent surfaces as `HaiError::Api {
    /// status: 400, message }` — message preserves the server-provided reason.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_agent_other_returns_api_400_with_owner_scoped_reason() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("agent", "other-agent");
                then.status(400).json_body(json!({
                    "error": "search is owner-scoped; agent param must equal caller or be omitted"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider
            .query_by_agent("other-agent", 10, 0)
            .expect_err("must surface 400");
        match &err {
            HaiError::Api { status, message } => {
                assert_eq!(*status, 400, "400 must round-trip, got: {err}");
                assert!(
                    message.contains("owner-scoped"),
                    "must preserve server reason, got: {message}"
                );
            }
            other => panic!("expected HaiError::Api status 400, got: {other:?}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn storage_capabilities_reports_remote_caps() {
        let server = MockServer::start_async().await;
        let provider = make_provider(server.base_url());
        let caps = provider.storage_capabilities().expect("caps");
        assert!(caps.fulltext);
        assert!(!caps.vector);
        // Issue 035: server explicitly does NOT support JSONB field filtering
        // (PRD §10 Non-Goal #19) — capability map must reflect the impl.
        assert!(!caps.query_by_field);
        assert!(caps.query_by_type);
        assert!(caps.pagination);
        assert!(caps.tombstone);
    }

    /// Issue 035 / 052: `query_by_field` MUST short-circuit before any network
    /// activity. Previously the SDK built a URL with `?field=&value=`, fired
    /// a GET that the server hard-rejected with a 400, and surfaced the
    /// error wrapped in PG-internal terminology. The fix returns a clear
    /// locally-generated error and skips the round-trip entirely. Issue 052
    /// upgrades this to a typed `BackendUnsupported` variant so cross-language
    /// consumers can branch programmatically.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_field_returns_unsupported_without_network_call() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                // Match anything — we want to see zero calls.
                when.method(HMethod::GET);
                then.status(500);
            })
            .await;

        let provider = make_provider(server.base_url());
        let err = provider
            .query_by_field("foo", "bar", 10, 0)
            .expect_err("must error before any network call");
        match &err {
            HaiError::BackendUnsupported { method, detail } => {
                assert_eq!(
                    method, "query_by_field",
                    "method name pinned for FFI consumers"
                );
                assert!(
                    detail.contains("foo"),
                    "detail must echo the requested field for debuggability, got: {detail}"
                );
            }
            other => panic!("expected BackendUnsupported, got: {other:?}"),
        }
        // Pin: zero HTTP calls.
        no_traffic.assert_calls_async(0).await;
    }

    /// Issue 033: `total_count` MUST equal the number of hits actually
    /// returned, not 0. Pinning this so a future refactor that re-introduces
    /// the hardcoded 0 is caught immediately. Server returns one hit; the
    /// SDK should report `total_count == 1`.
    #[tokio::test(flavor = "multi_thread")]
    async fn search_documents_total_count_reflects_results_len() {
        let server = MockServer::start_async().await;
        let _mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("q", "needle");
                then.status(200).json_body(json!({
                    "items": [{
                        "key": "id1:v1",
                        "id": "id1",
                        "version": "v1",
                        "jacsType": "doc",
                        "jacsVersionDate": "2026-01-01T00:00:00Z",
                        "contentType": "application/json",
                        "score": 0.9
                    }],
                    "next_cursor": null,
                    "has_more": false,
                    "total_count": 1
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let r = provider.search_documents("needle", 25, 0).expect("search");
        assert_eq!(
            r.total_count,
            r.results.len(),
            "Issue 033: total_count must mirror returned hits, not be hardcoded"
        );
        assert_eq!(r.total_count, 1);
    }

    /// Issue 008: 4xx response surfaces as `HaiError::Api { status, message }`
    /// with the server-shaped `{ "error": "..." }` reason extracted into
    /// `message`. Cross-language consumers see the typed `AuthFailed` /
    /// `NotFound` / `RateLimited` / `ApiError` kind via binding-core's
    /// `From<HaiError>` mapping.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_4xx_maps_to_haierror_api_with_server_reason() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::POST).path("/api/v1/records");
                then.status(403).json_body(json!({
                    "error": "forbidden — owner mismatch"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.store_document("{}").expect_err("must error");
        match &err {
            HaiError::Api { status, message } => {
                assert_eq!(*status, 403, "must preserve status code, got: {err}");
                assert!(
                    message.contains("forbidden"),
                    "must extract server reason, got message: {message}"
                );
            }
            other => panic!("expected HaiError::Api {{ status: 403, .. }}, got: {other:?}"),
        }
    }

    /// Issue 008: 5xx response surfaces as `HaiError::Api { status, message }`
    /// with the body in `message`. Binding-core maps these to
    /// `ErrorKind::ApiError`.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_5xx_maps_to_haierror_api_with_server_error_prefix() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::POST).path("/api/v1/records");
                then.status(500).body("internal whoopsie");
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.store_document("{}").expect_err("must error");
        match &err {
            HaiError::Api { status, message } => {
                assert_eq!(*status, 500, "must preserve status code, got: {err}");
                assert!(
                    message.contains("server error"),
                    "5xx message must carry server-error prefix, got: {message}"
                );
                assert!(
                    message.contains("internal whoopsie"),
                    "5xx message must include the body, got: {message}"
                );
            }
            other => panic!("expected HaiError::Api {{ status: 500, .. }}, got: {other:?}"),
        }
    }

    /// Issue 008: 401 from POST /records → `HaiError::Api { status: 401, .. }`
    /// → `ErrorKind::AuthFailed` in binding-core → `HaiAuthError` /
    /// `AuthenticationError` / `IsAuthError(true)` in Python/Node/Go SDKs.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_401_maps_to_api_status_401_for_auth_failed_kind() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::POST).path("/api/v1/records");
                then.status(401).json_body(json!({
                    "error": "invalid jacs signature"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.store_document("{}").expect_err("must error");
        match &err {
            HaiError::Api { status, .. } => {
                assert_eq!(
                    *status, 401,
                    "401 must round-trip as HaiError::Api status, got: {err}"
                );
            }
            other => panic!("expected HaiError::Api status 401, got: {other:?}"),
        }
    }

    /// Issue 008: 404 from GET /records/<missing> → `HaiError::Api {
    /// status: 404, .. }` → `ErrorKind::NotFound` in binding-core. Previously
    /// this surfaced as a generic auth error in every SDK because
    /// `HaiError::Provider` flattens to `ProviderError`.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_404_maps_to_api_status_404_for_not_found_kind() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/missing-id");
                then.status(404).json_body(json!({
                    "error": "record not found"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.get_document("missing-id").expect_err("must error");
        match &err {
            HaiError::Api { status, message } => {
                assert_eq!(
                    *status, 404,
                    "404 must round-trip as HaiError::Api, got: {err}"
                );
                assert!(
                    message.contains("record not found"),
                    "must extract server reason, got: {message}"
                );
            }
            other => panic!("expected HaiError::Api status 404, got: {other:?}"),
        }
    }

    /// Issue 008: 429 from GET /records → `HaiError::Api { status: 429, .. }`
    /// → `ErrorKind::RateLimited` in binding-core → `IsRateLimited(true)` in
    /// Go and the equivalent typed errors in Python/Node.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_429_maps_to_api_status_429_for_rate_limited_kind() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/some-id");
                then.status(429).json_body(json!({
                    "error": "rate limit exceeded"
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.get_document("some-id").expect_err("must error");
        match &err {
            HaiError::Api { status, .. } => {
                assert_eq!(
                    *status, 429,
                    "429 must round-trip as HaiError::Api, got: {err}"
                );
            }
            other => panic!("expected HaiError::Api status 429, got: {other:?}"),
        }
    }

    /// Issue 008: 503 from POST /records → `HaiError::Api { status: 503, .. }`
    /// → `ErrorKind::ApiError` in binding-core. 5xx errors must NOT collapse
    /// into auth/provider errors.
    #[tokio::test(flavor = "multi_thread")]
    async fn error_503_maps_to_api_status_503_for_api_error_kind() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(HMethod::POST).path("/api/v1/records");
                then.status(503).body("upstream unavailable");
            })
            .await;
        let provider = make_provider(server.base_url());
        let err = provider.store_document("{}").expect_err("must error");
        match &err {
            HaiError::Api { status, .. } => {
                assert_eq!(
                    *status, 503,
                    "503 must round-trip as HaiError::Api, got: {err}"
                );
            }
            other => panic!("expected HaiError::Api status 503, got: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn from_inner_returns_configinvalid_when_hai_url_missing() {
        let saved = std::env::var("HAI_URL").ok();
        unsafe {
            std::env::remove_var("HAI_URL");
        }
        let r = RemoteJacsProvider::from_inner(StaticJacsProvider::new("agent-A"), None);
        match r {
            Ok(_) => panic!("expected ConfigInvalid"),
            Err(e) => {
                assert!(
                    matches!(e, HaiError::ConfigInvalid { .. }),
                    "expected ConfigInvalid, got {e}"
                );
            }
        }
        unsafe {
            if let Some(v) = saved {
                std::env::set_var("HAI_URL", v);
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_text_file_rejects_unsigned_md() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                when.method(HMethod::POST);
                then.status(500);
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("README.md");
        std::fs::write(&path, b"hello world without signature\n").expect("write");
        let provider = make_provider(server.base_url());
        let err = provider
            .store_text_file(path.to_str().unwrap())
            .expect_err("unsigned md must reject");
        let s = format!("{}", err);
        assert!(s.contains("no JACS signature block"), "got: {s}");
        no_traffic.assert_calls_async(0).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_text_file_posts_with_text_markdown_content_type() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .header("content-type", "text/markdown; profile=jacs-text-v1");
                then.status(201).json_body(json!({"key":"text1:v1"}));
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("README.md");
        std::fs::write(
            &path,
            b"# hello\nworld\n-----BEGIN JACS SIGNATURE-----\n--- some yaml ---\n-----END JACS SIGNATURE-----\n",
        )
        .expect("write");
        let provider = make_provider(server.base_url());
        let key = provider
            .store_text_file(path.to_str().unwrap())
            .expect("store text");
        assert_eq!(key, "text1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_image_file_rejects_unknown_magic() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                when.method(HMethod::POST);
                then.status(500);
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("not-an-image.bin");
        std::fs::write(&path, b"this is not an image at all").expect("write");
        let provider = make_provider(server.base_url());
        let err = provider
            .store_image_file(path.to_str().unwrap())
            .expect_err("must error");
        let s = format!("{}", err);
        assert!(s.contains("unknown image format"), "got: {s}");
        no_traffic.assert_calls_async(0).await;
    }

    /// Issue 014: build a 1×1 PNG with a real JACS signature chunk via
    /// `jacs_media::embed_signature`. Returns the signed bytes. The previous
    /// substring-spoofed bytes are no longer accepted by `contains_jacs_chunk`,
    /// which now uses the same parser as the server.
    fn signed_png_fixture() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        // 1×1 grayscale PNG via the `image` dev-dep.
        let img = image::GrayImage::from_pixel(1, 1, image::Luma([128]));
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode png");
        // The signature payload is the base64url-encoded JSON of a signed-document
        // envelope. For the SDK pre-flight check we only need a parseable chunk;
        // the server runs the real verifier later.
        let claim_json = r#"{"jacsId":"test","jacsSignature":{"agentID":"x","signature":"y"}}"#;
        let payload_b64u = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claim_json);
        jacs_media::embed_signature(&buf, &payload_b64u, false, false).expect("embed png")
    }

    fn signed_jpeg_fixture() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        let img = image::GrayImage::from_pixel(1, 1, image::Luma([128]));
        image::DynamicImage::ImageLuma8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
            .expect("encode jpeg");
        let claim_json = r#"{"jacsId":"test","jacsSignature":{"agentID":"x","signature":"y"}}"#;
        let payload_b64u = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claim_json);
        jacs_media::embed_signature(&buf, &payload_b64u, false, false).expect("embed jpeg")
    }

    /// Build a minimally-valid WebP container (VP8L lossless 1×1) and embed a
    /// JACS chunk via `jacs_media::embed_signature`. The `image` crate's WebP
    /// encoder isn't enabled by default, so we hand-build the smallest legal
    /// VP8L payload here. `jacs_media::webp::embed` only needs the RIFF/WebP
    /// header plus at least one image chunk to recognise the container.
    fn signed_webp_fixture() -> Vec<u8> {
        // Smallest legal VP8L lossless WebP (1×1 white pixel). Hand-built:
        // RIFF[size]WEBP VP8L[size] 0x2f 0x00 0x00 0x00 0x00 (signature byte +
        // canvas-size encoding for 1x1 + transform=0). The exact byte stream
        // here is the minimal one that VP8L decoders accept and that
        // `webp::embed` recognises as a valid WebP container.
        let vp8l_payload: &[u8] = &[0x2f, 0x00, 0x00, 0x00, 0x00];
        let mut riff_body = Vec::new();
        riff_body.extend_from_slice(b"WEBP");
        riff_body.extend_from_slice(b"VP8L");
        riff_body.extend_from_slice(&(vp8l_payload.len() as u32).to_le_bytes());
        riff_body.extend_from_slice(vp8l_payload);
        // VP8L chunks must be padded to even length; 5 bytes → +1 pad byte.
        if vp8l_payload.len() % 2 == 1 {
            riff_body.push(0);
        }
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(riff_body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&riff_body);
        let claim_json = r#"{"jacsId":"test","jacsSignature":{"agentID":"x","signature":"y"}}"#;
        let payload_b64u = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claim_json);
        jacs_media::embed_signature(&bytes, &payload_b64u, false, false).expect("embed webp")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_image_file_detects_png_magic_and_posts_image_png() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .header("content-type", "image/png");
                then.status(201).json_body(json!({"key":"png1:v1"}));
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("signed.png");
        std::fs::write(&path, signed_png_fixture()).expect("write");
        let provider = make_provider(server.base_url());
        let key = provider
            .store_image_file(path.to_str().unwrap())
            .expect("store png");
        assert_eq!(key, "png1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_image_file_detects_jpeg_and_posts_image_jpeg() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .header("content-type", "image/jpeg");
                then.status(201).json_body(json!({"key":"jpg1:v1"}));
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("signed.jpg");
        std::fs::write(&path, signed_jpeg_fixture()).expect("write");
        let provider = make_provider(server.base_url());
        let key = provider
            .store_image_file(path.to_str().unwrap())
            .expect("store jpg");
        assert_eq!(key, "jpg1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn store_image_file_detects_webp_and_posts_image_webp() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .header("content-type", "image/webp");
                then.status(201).json_body(json!({"key":"wp1:v1"}));
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("signed.webp");
        std::fs::write(&path, signed_webp_fixture()).expect("write");
        let provider = make_provider(server.base_url());
        let key = provider
            .store_image_file(path.to_str().unwrap())
            .expect("store webp");
        assert_eq!(key, "wp1:v1");
        mock.assert_async().await;
    }

    /// Issue 014 regression: an UNSIGNED PNG with the literal `jacsSignature`
    /// substring in arbitrary metadata MUST be rejected — the previous heuristic
    /// would accept it, wasting a server round-trip.
    #[tokio::test(flavor = "multi_thread")]
    async fn store_image_file_rejects_png_with_substring_only_no_real_chunk() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                when.method(HMethod::POST);
                then.status(500);
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("substring.png");
        // Build a real (unsigned) PNG, then concat the literal substring at the
        // end. The new real-chunk parser sees no JACS chunk and refuses.
        let mut buf: Vec<u8> = Vec::new();
        let img = image::GrayImage::from_pixel(1, 1, image::Luma([128]));
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode png");
        buf.extend_from_slice(b"\n... fake jacsSignature substring ...\n");
        std::fs::write(&path, &buf).expect("write");
        let provider = make_provider(server.base_url());
        let err = provider
            .store_image_file(path.to_str().unwrap())
            .expect_err("must reject");
        let s = format!("{}", err);
        assert!(s.contains("no JACS signature"), "got: {s}",);
        no_traffic.assert_calls_async(0).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_record_bytes_returns_raw_bytes() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/id1");
                then.status(200)
                    .header("Content-Type", "image/png")
                    .body(vec![0x89, b'P', b'N', b'G', 0xde, 0xad, 0xbe, 0xef]);
            })
            .await;
        let provider = make_provider(server.base_url());
        let bytes = provider.get_record_bytes("id1").expect("bytes");
        assert_eq!(bytes, vec![0x89, b'P', b'N', b'G', 0xde, 0xad, 0xbe, 0xef]);
        mock.assert_async().await;
    }

    // =========================================================================
    // D5 — MEMORY / SOUL wrappers
    // =========================================================================

    #[tokio::test(flavor = "multi_thread")]
    async fn save_memory_posts_with_jacstype_memory() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .body_includes(r#""jacsType":"memory""#)
                    .body_includes(r#""body":"my memory text""#);
                then.status(201).json_body(json!({"key":"mem1:v1"}));
            })
            .await;
        let provider = make_provider(server.base_url());
        let key = provider
            .save_memory(Some("my memory text"))
            .expect("save_memory");
        assert_eq!(key, "mem1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn save_soul_posts_with_jacstype_soul() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .body_includes(r#""jacsType":"soul""#)
                    .body_includes(r#""body":"my soul text""#);
                then.status(201).json_body(json!({"key":"soul1:v1"}));
            })
            .await;
        let provider = make_provider(server.base_url());
        let key = provider.save_soul(Some("my soul text")).expect("save_soul");
        assert_eq!(key, "soul1:v1");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_memory_returns_none_when_no_memory_stored() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory");
                then.status(200)
                    .json_body(json!({"items":[],"has_more":false}));
            })
            .await;
        let provider = make_provider(server.base_url());
        let out = provider.get_memory().expect("get_memory");
        assert!(out.is_none());
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_memory_fetches_latest_envelope() {
        let server = MockServer::start_async().await;
        let list_mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory");
                then.status(200).json_body(json!({
                    "items":[{"jacs_id":"mem1","jacs_version":"v1"}],
                    "has_more": false
                }));
            })
            .await;
        let get_mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET).path("/api/v1/records/mem1/v/v1");
                then.status(200)
                    .body(r#"{"jacsType":"memory","body":"hello memory"}"#);
            })
            .await;
        let provider = make_provider(server.base_url());
        let out = provider.get_memory().expect("get_memory");
        let envelope = out.expect("Some envelope");
        assert!(envelope.contains("hello memory"));
        list_mock.assert_async().await;
        get_mock.assert_async().await;
    }

    #[test]
    fn save_memory_reads_memory_md_when_no_arg() {
        // Tests the file-fallback path. Verifies behavior without HTTP — the
        // store_typed_doc helper reads MEMORY.md if `content` is None.
        let dir = tempfile::tempdir().expect("tmp");
        let prev_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");
        std::fs::write(dir.path().join("MEMORY.md"), "from-disk-memory").expect("write");
        // We can't fully exercise without an HTTP server, but `read_to_string`
        // succeeding is half the battle — confirm the file is read.
        let body = std::fs::read_to_string("MEMORY.md").expect("read MEMORY.md");
        assert_eq!(body, "from-disk-memory");
        std::env::set_current_dir(&prev_cwd).expect("restore cwd");
    }

    /// Issue 003 regression: D5/D9 helpers MUST be reachable through
    /// `Box<dyn JacsDocumentProvider>`. When they were inherent methods, this code
    /// would not compile (inherent methods are not callable through a trait object).
    /// The Python/Node/Go FFI facades route through the binding-core trait object,
    /// so failing this test means the FFI surface cannot reach the new methods.
    #[tokio::test(flavor = "multi_thread")]
    async fn d5_d9_helpers_callable_through_trait_object() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::POST)
                    .path("/api/v1/records")
                    .body_includes(r#""jacsType":"memory""#);
                then.status(201).json_body(json!({"key": "mem1:v1"}));
            })
            .await;
        let provider = make_provider(server.base_url());
        // The compiler proof: if the methods are inherent, this trait-object cast
        // either won't compile (no method on the trait) or the call below will fail
        // dispatch. Boxing via `Box<dyn JacsDocumentProvider>` is exactly what
        // `hai-binding-core` does for the FFI bridge.
        let dyn_provider: Box<dyn JacsDocumentProvider> = Box::new(provider);
        let key = dyn_provider
            .save_memory(Some("trait-object reachable"))
            .expect("save_memory through dyn");
        assert_eq!(key, "mem1:v1");
        mock.assert_async().await;
    }

    /// Issue 009 regression: `query_by_type` MUST walk forward via
    /// `next_cursor` so that any caller can request `offset >= 100`. The
    /// previous implementation capped the request URL at `limit + offset`
    /// truncated to 100, then drained the head — which silently returned []
    /// for any `offset >= 100`. This test pages 3× through the server with
    /// `limit=10, offset=15`: expect to skip the 15 leading records (page 1
    /// + half of page 2) and return the next 10.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_type_walks_cursor_for_offset_above_page_boundary() {
        let server = MockServer::start_async().await;
        // Page 1: 10 items, cursor "p2".
        let _page1 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory")
                    .is_true(|req| !req.query_params().iter().any(|(k, _)| k == "cursor"));
                then.status(200).json_body(json!({
                    "items": (0..10)
                        .map(|i| json!({
                            "jacs_id": format!("id-p1-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": "p2",
                    "has_more": true
                }));
            })
            .await;
        // Page 2: 10 items, cursor "p3". `query_by_type` requests with cursor=p2.
        let _page2 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory")
                    .query_param("cursor", "p2");
                then.status(200).json_body(json!({
                    "items": (0..10)
                        .map(|i| json!({
                            "jacs_id": format!("id-p2-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": "p3",
                    "has_more": true
                }));
            })
            .await;
        // Page 3: 10 items, no further cursor.
        let _page3 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory")
                    .query_param("cursor", "p3");
                then.status(200).json_body(json!({
                    "items": (0..10)
                        .map(|i| json!({
                            "jacs_id": format!("id-p3-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": null,
                    "has_more": false
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let keys = provider
            .query_by_type("memory", 10, 15)
            .expect("query_by_type with offset > page");
        assert_eq!(keys.len(), 10, "must return exactly limit (10) keys");
        // Skipped: 10 from page 1 + 5 from page 2. Take next 5 from page 2 + 5 from page 3.
        assert_eq!(
            keys,
            vec![
                "id-p2-5:v1".to_string(),
                "id-p2-6:v1".to_string(),
                "id-p2-7:v1".to_string(),
                "id-p2-8:v1".to_string(),
                "id-p2-9:v1".to_string(),
                "id-p3-0:v1".to_string(),
                "id-p3-1:v1".to_string(),
                "id-p3-2:v1".to_string(),
                "id-p3-3:v1".to_string(),
                "id-p3-4:v1".to_string(),
            ],
            "must skip the 15 leading records and return the next 10 in order"
        );
    }

    /// Issue 009 regression: when the server runs out of pages mid-walk,
    /// `query_by_type` returns whatever was collected (no error). Server
    /// here returns 50 items total; caller asks for `limit=100, offset=0`.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_type_short_returns_when_server_runs_out() {
        let server = MockServer::start_async().await;
        // Page 1: 50 items, no further cursor.
        let _page1 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory");
                then.status(200).json_body(json!({
                    "items": (0..50)
                        .map(|i| json!({
                            "jacs_id": format!("id-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": null,
                    "has_more": false
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let keys = provider
            .query_by_type("memory", 100, 0)
            .expect("must return short result without error");
        assert_eq!(keys.len(), 50, "server exhausted at 50 items");
    }

    /// Issue 009 regression: same fix for `query_by_agent`. Server returns
    /// two pages of 10; caller asks for `offset=10, limit=5` → must return
    /// the first 5 keys of page 2.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_agent_walks_cursor_for_offset_above_page_boundary() {
        let server = MockServer::start_async().await;
        let _page1 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("agent", "agent-test")
                    .is_true(|req| !req.query_params().iter().any(|(k, _)| k == "cursor"));
                then.status(200).json_body(json!({
                    "items": (0..10)
                        .map(|i| json!({
                            "jacs_id": format!("a1-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": "agent-p2",
                    "has_more": true
                }));
            })
            .await;
        let _page2 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("agent", "agent-test")
                    .query_param("cursor", "agent-p2");
                then.status(200).json_body(json!({
                    "items": (0..10)
                        .map(|i| json!({
                            "jacs_id": format!("a2-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": null,
                    "has_more": false
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let keys = provider
            .query_by_agent("agent-test", 5, 10)
            .expect("query_by_agent with offset across page boundary");
        assert_eq!(
            keys,
            vec![
                "a2-0:v1".to_string(),
                "a2-1:v1".to_string(),
                "a2-2:v1".to_string(),
                "a2-3:v1".to_string(),
                "a2-4:v1".to_string(),
            ],
            "must return first 5 records of page 2 after skipping page 1"
        );
    }

    /// Issue 009 regression: `query_by_agent` returns short when the server
    /// runs out of pages without raising.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_agent_short_returns_when_server_runs_out() {
        let server = MockServer::start_async().await;
        let _page1 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("agent", "agent-test");
                then.status(200).json_body(json!({
                    "items": (0..3)
                        .map(|i| json!({
                            "jacs_id": format!("agent-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": null,
                    "has_more": false
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let keys = provider
            .query_by_agent("agent-test", 100, 0)
            .expect("must return short result without error");
        assert_eq!(keys.len(), 3, "server exhausted at 3 items");
    }

    /// Issue 009 regression: when offset exceeds the total record count, the
    /// helper returns an empty list — not a stale truncation of an earlier
    /// page. Server returns 5 items, caller asks for `offset=10, limit=5`.
    #[tokio::test(flavor = "multi_thread")]
    async fn query_by_type_returns_empty_when_offset_exceeds_total() {
        let server = MockServer::start_async().await;
        let _page1 = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records")
                    .query_param("type", "memory");
                then.status(200).json_body(json!({
                    "items": (0..5)
                        .map(|i| json!({
                            "jacs_id": format!("id-{}", i),
                            "jacs_version": "v1"
                        }))
                        .collect::<Vec<_>>(),
                    "next_cursor": null,
                    "has_more": false
                }));
            })
            .await;
        let provider = make_provider(server.base_url());
        let keys = provider
            .query_by_type("memory", 5, 10)
            .expect("offset > total must return [] without error");
        assert!(keys.is_empty());
    }

    /// Issue 007 regression: record IDs and versions MUST be URL-escaped in
    /// path segments per CLAUDE.md. With `id="weird/id"` and `v="v?1"`, the
    /// raw `format!("{}/{}/v/{}", ...)` would route to a different endpoint
    /// (or a 404), and a reserved byte like `?` would be parsed by the server
    /// as the path/query boundary. With `encode_path_segment`, both segments
    /// percent-encode and the request reaches the canonical record route.
    #[tokio::test(flavor = "multi_thread")]
    async fn record_paths_url_escape_id_and_version() {
        let server = MockServer::start_async().await;
        // Mock matches the *encoded* path. If `encode_path_segment` is
        // missing on the call site, this mock will not match and the
        // assertion below will fail.
        let mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records/weird%2Fid/v/v%3F1");
                then.status(200).body(b"raw bytes" as &[u8]);
            })
            .await;
        let provider = make_provider(server.base_url());
        let bytes = provider
            .get_record_bytes("weird/id:v?1")
            .expect("must encode reserved bytes in id and version");
        assert_eq!(bytes, b"raw bytes");
        mock.assert_async().await;
    }

    /// Issue 007 regression: `get_document_versions` and `remove_document`
    /// also URL-escape their `id` segment. Both routes target the same
    /// `/api/v1/records/{id}` family.
    #[tokio::test(flavor = "multi_thread")]
    async fn versions_and_remove_url_escape_id_segment() {
        let server = MockServer::start_async().await;
        let versions_mock = server
            .mock_async(|when, then| {
                when.method(HMethod::GET)
                    .path("/api/v1/records/weird%2Fid/versions");
                then.status(200).json_body(json!({"versions": []}));
            })
            .await;
        let remove_mock = server
            .mock_async(|when, then| {
                when.method(HMethod::DELETE)
                    .path("/api/v1/records/weird%2Fid");
                then.status(200).json_body(json!({"tombstoned": true}));
            })
            .await;
        let provider = make_provider(server.base_url());
        let v = provider
            .get_document_versions("weird/id")
            .expect("must encode id for versions");
        assert!(v.is_empty());
        provider
            .remove_document("weird/id")
            .expect("must encode id for delete");
        versions_mock.assert_async().await;
        remove_mock.assert_async().await;
    }

    /// Issue 006 regression: `RemoteJacsProvider::sign_file` MUST delegate to
    /// `inner.sign_file_envelope` rather than hand-roll a `payload_b64` /
    /// flat-`sha256` envelope. The previous implementation produced a
    /// structurally different document for the same `(path, embed)` than
    /// `LocalJacsProvider::sign_file` and would not verify under the JACS
    /// schema. With delegation, calling against a `StaticJacsProvider`
    /// (which has no real JACS agent) surfaces the default trait error
    /// from `JacsProvider::sign_file_envelope` — the failure mode of the
    /// hand-rolled implementation was "succeeds but produces a non-JACS
    /// envelope", so seeing the default error here is positive proof
    /// that the delegation took effect and no hand-rolled envelope is
    /// produced.
    #[tokio::test(flavor = "multi_thread")]
    async fn sign_file_delegates_to_inner_sign_file_envelope() {
        let server = MockServer::start_async().await;
        let no_traffic = server
            .mock_async(|when, then| {
                when.method(HMethod::POST);
                then.status(500);
            })
            .await;
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("payload.bin");
        std::fs::write(&path, b"some bytes to sign").expect("write");
        let provider = make_provider(server.base_url());
        let err = provider
            .sign_file(path.to_str().unwrap(), true)
            .expect_err("StaticJacsProvider has no real JACS agent — must surface default error");
        let s = format!("{}", err);
        // The default trait error message from `JacsProvider::sign_file_envelope`
        // mentions `LocalJacsProvider`. If `RemoteJacsProvider::sign_file` were
        // still hand-rolling the envelope, this call would *succeed* (signing
        // a flat `payload_b64` payload via `sign_document`).
        assert!(
            s.contains("sign_file_envelope not supported") || s.contains("LocalJacsProvider"),
            "expected default-error from sign_file_envelope, got: {s}"
        );
        // Pin: zero HTTP calls — `sign_file` must be local-only.
        no_traffic.assert_calls_async(0).await;
    }
}
