use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jacs::agent::boilerplate::BoilerPlate;
use jacs::crypt::KeyManager;
use jacs::document::{DocumentService, service_from_agent};
use jacs::simple::{self, CreateAgentParams, SimpleAgent};
use serde_json::Value;

use crate::error::{HaiError, Result};
use crate::jacs::{
    JacsAgentLifecycle, JacsBatchProvider, JacsDocumentProvider, JacsEmailProvider, JacsProvider,
    JacsVerificationProvider,
};
#[cfg(feature = "agreements")]
use crate::jacs::JacsAgreementProvider;
#[cfg(feature = "attestation")]
use crate::jacs::JacsAttestationProvider;
use crate::key_format::normalize_public_key_pem;
#[cfg(feature = "jacs-crate")]
use crate::types::RotationResult;
use crate::types::{
    CreateAgentOptions, CreateAgentResult, DocSearchHit, DocSearchResults, DocVerificationResult,
    MigrateAgentResult, SignedDocument, SignedPayload, StorageCapabilities, UpdateAgentResult,
};

/// Local JACS-backed provider using the canonical Rust `jacs` crate.
///
/// This adapter loads the local agent configured by `jacs.config.json` and
/// delegates signing operations to JACS runtime methods.
pub struct LocalJacsProvider {
    agent: Mutex<jacs::agent::Agent>,
    jacs_id: String,
    algorithm: String,
    config_path: PathBuf,
    document_service: Option<Arc<dyn DocumentService>>,
}

impl LocalJacsProvider {
    pub fn from_config_path(config_path: Option<&Path>) -> Result<Self> {
        let config_path = resolve_jacs_config_path(config_path);
        let mut agent = jacs::get_empty_agent();
        agent
            .load_by_config(config_path.display().to_string())
            .map_err(|e| {
                HaiError::Provider(format!(
                    "failed to load JACS agent from {}: {e}",
                    config_path.display()
                ))
            })?;

        let jacs_id = agent
            .get_id()
            .map_err(|e| HaiError::Provider(format!("failed to resolve JACS agent id: {e}")))?;

        // Resolve algorithm from the loaded agent (PRD lines 86-98).
        // Fail fast if the algorithm cannot be determined — a silent default
        // would cause algorithm mismatches during verification (Issue 039).
        let algorithm = agent.get_key_algorithm().cloned().ok_or_else(|| {
            HaiError::Provider(
                "Cannot resolve signing algorithm from JACS agent. \
                 Ensure the agent was created with a valid key algorithm."
                    .to_string(),
            )
        })?;

        Ok(Self {
            agent: Mutex::new(agent),
            jacs_id,
            algorithm,
            config_path,
            document_service: None,
        })
    }

    /// Create a provider with a configured document storage backend.
    ///
    /// `storage_label` accepts routed `DocumentService` labels:
    /// `"fs"`, `"rusqlite"`, or `"sqlite"` (alias for `rusqlite`).
    pub fn from_config_path_with_storage(
        config_path: Option<&Path>,
        storage_label: &str,
    ) -> Result<Self> {
        let config_path = resolve_jacs_config_path(config_path);
        let mut agent = jacs::get_empty_agent();
        agent
            .load_by_config(config_path.display().to_string())
            .map_err(|e| {
                HaiError::Provider(format!(
                    "failed to load JACS agent from {}: {e}",
                    config_path.display()
                ))
            })?;

        let jacs_id = agent
            .get_id()
            .map_err(|e| HaiError::Provider(format!("failed to resolve JACS agent id: {e}")))?;

        let algorithm = agent.get_key_algorithm().cloned().ok_or_else(|| {
            HaiError::Provider(
                "Cannot resolve signing algorithm from JACS agent. \
                 Ensure the agent was created with a valid key algorithm."
                    .to_string(),
            )
        })?;

        // Validate the storage label
        let _label = resolve_storage_label(storage_label)?;

        // Create agent arc for DocumentService resolution
        let agent_arc = Arc::new(Mutex::new(agent));

        // Resolve the DocumentService from the agent's config
        let doc_service = service_from_agent(Arc::clone(&agent_arc)).map_err(|e| {
            HaiError::Provider(format!("failed to resolve document service for '{}': {e}", storage_label))
        })?;

        // Extract the agent back from the Arc (we're the only holder)
        let agent = Arc::try_unwrap(agent_arc)
            .map_err(|_| HaiError::Provider("failed to unwrap agent arc".to_string()))?;
        let agent = agent
            .into_inner()
            .map_err(|e| HaiError::Provider(format!("failed to extract agent: {e}")))?;

        Ok(Self {
            agent: Mutex::new(agent),
            jacs_id,
            algorithm,
            config_path,
            document_service: Some(doc_service),
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Whether this provider has a configured document service.
    pub fn has_document_service(&self) -> bool {
        self.document_service.is_some()
    }

    pub fn public_key_pem(&self) -> Result<String> {
        let agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
        let public_key = agent
            .get_public_key()
            .map_err(|e| HaiError::Provider(format!("failed to get JACS public key: {e}")))?;
        Ok(normalize_public_key_pem(&public_key))
    }

    pub fn create_agent(params: CreateAgentParams) -> Result<simple::AgentInfo> {
        SimpleAgent::create_with_params(params)
            .map(|(_, info)| info)
            .map_err(|e| HaiError::Provider(format!("failed to create JACS agent: {e}")))
    }

    pub fn create_agent_with_options(options: &CreateAgentOptions) -> Result<CreateAgentResult> {
        let mut params = CreateAgentParams {
            name: options.name.clone(),
            password: options.password.clone(),
            ..CreateAgentParams::default()
        };

        if let Some(v) = &options.algorithm {
            params.algorithm = v.clone();
        }
        if let Some(v) = &options.data_directory {
            params.data_directory = v.clone();
        }
        if let Some(v) = &options.key_directory {
            params.key_directory = v.clone();
        }
        if let Some(v) = &options.config_path {
            params.config_path = v.clone();
        }
        if let Some(v) = &options.agent_type {
            params.agent_type = v.clone();
        }
        if let Some(v) = &options.description {
            params.description = v.clone();
        }
        if let Some(v) = &options.domain {
            params.domain = v.clone();
        }
        if let Some(v) = &options.default_storage {
            params.default_storage = v.clone();
        }

        let info = Self::create_agent(params)?;
        Ok(map_agent_info(info))
    }

    /// Migrate a legacy agent whose document predates a schema change.
    /// This is a static method because the agent cannot be loaded before migration.
    pub fn migrate_agent(config_path: Option<&std::path::Path>) -> Result<MigrateAgentResult> {
        let path = resolve_jacs_config_path(config_path);
        let path_str = path.display().to_string();
        let result = simple::advanced::migrate_agent(Some(&path_str))
            .map_err(|e| HaiError::Provider(format!("agent migration failed: {e}")))?;

        Ok(MigrateAgentResult {
            jacs_id: result.jacs_id,
            old_version: result.old_version,
            new_version: result.new_version,
            patched_fields: result.patched_fields,
        })
    }

    fn update_config_version(&self, jacs_id: &str, new_version: &str) -> Result<()> {
        let config_str = std::fs::read_to_string(&self.config_path).map_err(|e| {
            HaiError::Provider(format!("failed to read config for version update: {e}"))
        })?;
        let mut config_value: Value = serde_json::from_str(&config_str)?;
        let new_lookup = format!("{}:{}", jacs_id, new_version);
        if let Some(obj) = config_value.as_object_mut() {
            obj.insert(
                "jacs_agent_id_and_version".to_string(),
                serde_json::json!(new_lookup),
            );
        }
        let updated_str = serde_json::to_string_pretty(&config_value)?;
        std::fs::write(&self.config_path, updated_str)
            .map_err(|e| HaiError::Provider(format!("failed to write updated config: {e}")))?;
        Ok(())
    }

    fn load_simple_agent(&self) -> Result<SimpleAgent> {
        SimpleAgent::load(Some(&self.config_path.display().to_string()), Some(false)).map_err(|e| {
            HaiError::Provider(format!(
                "failed to load SimpleAgent from {}: {e}",
                self.config_path.display()
            ))
        })
    }

    /// Get the document service, returning an error if not configured.
    fn require_document_service(&self) -> Result<&Arc<dyn DocumentService>> {
        self.document_service.as_ref().ok_or_else(|| {
            HaiError::Provider(
                "No document service configured. Pass a supported backend label to \
                 from_config_path_with_storage() or configure default_storage in jacs.config.json."
                    .to_string(),
            )
        })
    }
}

fn map_agent_info(info: simple::AgentInfo) -> CreateAgentResult {
    CreateAgentResult {
        agent_id: info.agent_id,
        name: info.name,
        public_key_path: info.public_key_path,
        config_path: info.config_path,
        version: info.version,
        algorithm: info.algorithm,
        private_key_path: info.private_key_path,
        data_directory: info.data_directory,
        key_directory: info.key_directory,
        domain: info.domain,
        dns_record: info.dns_record,
    }
}

/// Validate routed backend labels for DocumentService-backed operations.
fn resolve_storage_label(label: &str) -> Result<String> {
    match label {
        "fs" => Ok("fs".to_string()),
        "rusqlite" | "sqlite" => Ok("rusqlite".to_string()),
        other => Err(HaiError::Provider(format!(
            "Unsupported storage backend '{}'. Valid routed labels: fs, rusqlite, sqlite",
            other
        ))),
    }
}

// =============================================================================
// JacsProvider implementation
// =============================================================================

impl JacsProvider for LocalJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        agent
            .sign_string(message)
            .map_err(|e| HaiError::Provider(format!("JACS sign_string failed: {e}")))
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        #[cfg(feature = "jacs-crate")]
        {
            jacs::agent::Agent::sign_bytes(&mut agent, data)
                .map_err(|e| HaiError::Provider(format!("JACS sign_bytes failed: {e}")))
        }
        #[cfg(not(feature = "jacs-crate"))]
        {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            let sig_b64 = agent.sign_string(&encoded).map_err(|e| {
                HaiError::Provider(format!("JACS sign_bytes (via sign_string) failed: {e}"))
            })?;
            base64::engine::general_purpose::STANDARD
                .decode(&sig_b64)
                .map_err(|e| HaiError::Provider(format!("JACS sign_bytes decode failed: {e}")))
        }
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(jacs::protocol::canonicalize_json(value))
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        let result =
            jacs::a2a::provenance::verify_wrapped_artifact(&agent, &wrapped).map_err(|e| {
                HaiError::Provider(format!("JACS A2A artifact verification failed: {e}"))
            })?;

        serde_json::to_string(&result).map_err(|e| {
            HaiError::Provider(format!("failed to serialize verification result: {e}"))
        })
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        let envelope = jacs::protocol::sign_response(&mut agent, payload)
            .map_err(|e| HaiError::Provider(format!("JACS sign_response failed: {e}")))?;

        Ok(SignedPayload {
            signed_document: serde_json::to_string(&envelope)?,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        let simple = self.load_simple_agent()?;
        jacs::email::sign_email(raw_email, &simple)
            .map_err(|e| HaiError::Provider(format!("JACS email signing failed: {e}")))
    }

    fn export_agent_json(&self) -> Result<String> {
        let simple = self.load_simple_agent()?;
        simple
            .export_agent()
            .map_err(|e| HaiError::Provider(format!("failed to export JACS agent json: {e}")))
    }

    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        let old_version = {
            let agent = self
                .agent
                .lock()
                .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
            agent
                .get_value()
                .and_then(|v| v["jacsVersion"].as_str().map(String::from))
                .unwrap_or_default()
        };

        let updated_json = {
            let mut agent = self
                .agent
                .lock()
                .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
            agent
                .update_self(new_agent_data)
                .map_err(|e| HaiError::Provider(format!("failed to update agent: {e}")))?
        };

        let new_doc: Value = serde_json::from_str(&updated_json)?;
        let new_version = new_doc["jacsVersion"].as_str().unwrap_or("").to_string();

        {
            let agent = self
                .agent
                .lock()
                .map_err(|e| HaiError::Provider(format!("failed to lock agent for save: {e}")))?;
            agent
                .save()
                .map_err(|e| HaiError::Provider(format!("failed to save updated agent: {e}")))?;
        }

        self.update_config_version(&self.jacs_id, &new_version)?;

        Ok(UpdateAgentResult {
            jacs_id: self.jacs_id.clone(),
            old_version,
            new_version,
            signed_agent_json: updated_json,
            registered_with_hai: false,
        })
    }

    #[cfg(feature = "jacs-crate")]
    fn rotate(&self) -> Result<RotationResult> {
        let simple = self.load_simple_agent()?;
        let jacs_result = simple::advanced::rotate(&simple)
            .map_err(|e| HaiError::Provider(format!("JACS key rotation failed: {e}")))?;

        // Reload the agent so in-memory state reflects the rotated keys
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
        let mut new_agent = jacs::get_empty_agent();
        new_agent
            .load_by_config(self.config_path.display().to_string())
            .map_err(|e| {
                HaiError::Provider(format!("failed to reload JACS agent after rotation: {e}"))
            })?;
        *agent = new_agent;

        Ok(RotationResult {
            jacs_id: jacs_result.jacs_id,
            old_version: jacs_result.old_version,
            new_version: jacs_result.new_version,
            new_public_key_hash: jacs_result.new_public_key_hash,
            registered_with_hai: false,
            signed_agent_json: jacs_result.signed_agent_json,
        })
    }
}

// =============================================================================
// JacsAgentLifecycle implementation
// =============================================================================

impl JacsAgentLifecycle for LocalJacsProvider {
    fn lifecycle_rotate(&self) -> Result<RotationResult> {
        self.rotate()
    }

    fn lifecycle_migrate(config_path: Option<&Path>) -> Result<MigrateAgentResult> {
        Self::migrate_agent(config_path)
    }

    fn lifecycle_update_agent(&self, new_data: &str) -> Result<UpdateAgentResult> {
        self.update_agent(new_data)
    }

    fn lifecycle_export_agent_json(&self) -> Result<String> {
        self.export_agent_json()
    }

    fn diagnostics(&self) -> Result<Value> {
        let simple = self.load_simple_agent()?;
        Ok(simple.diagnostics())
    }

    fn verify_self(&self) -> Result<DocVerificationResult> {
        let simple = self.load_simple_agent()?;
        let result = simple.verify_self().map_err(|e| {
            HaiError::Provider(format!("JACS verify_self failed: {e}"))
        })?;
        Ok(DocVerificationResult {
            key: self.jacs_id.clone(),
            valid: result.valid,
            error: if result.errors.is_empty() {
                None
            } else {
                Some(result.errors.join("; "))
            },
        })
    }

    fn quickstart(
        name: &str,
        domain: &str,
        description: Option<&str>,
        algorithm: Option<&str>,
        config_path: Option<&str>,
    ) -> Result<Value> {
        let (_agent, info) =
            simple::advanced::quickstart(name, domain, description, algorithm, config_path)
                .map_err(|e| HaiError::Provider(format!("quickstart failed: {e}")))?;

        Ok(serde_json::json!({
            "agent_id": info.agent_id,
            "name": info.name,
            "version": info.version,
            "algorithm": info.algorithm,
            "config_path": info.config_path,
            "public_key_path": info.public_key_path,
            "private_key_path": info.private_key_path,
            "data_directory": info.data_directory,
            "key_directory": info.key_directory,
            "domain": info.domain,
            "dns_record": info.dns_record,
        }))
    }

    fn reencrypt_key(&self, old_password: &str, new_password: &str) -> Result<()> {
        let simple = self.load_simple_agent()?;
        simple::advanced::reencrypt_key(&simple, old_password, new_password)
            .map_err(|e| HaiError::Provider(format!("reencrypt_key failed: {e}")))
    }

    fn get_setup_instructions(&self, domain: &str, ttl: u32) -> Result<Value> {
        let simple = self.load_simple_agent()?;
        let instructions = simple::advanced::get_setup_instructions(&simple, domain, ttl)
            .map_err(|e| HaiError::Provider(format!("get_setup_instructions failed: {e}")))?;

        Ok(serde_json::json!({
            "dns_record_bind": instructions.dns_record_bind,
            "dns_record_value": instructions.dns_record_value,
            "dns_owner": instructions.dns_owner,
            "provider_commands": instructions.provider_commands,
            "dnssec_instructions": instructions.dnssec_instructions,
            "tld_requirement": instructions.tld_requirement,
            "well_known_json": instructions.well_known_json,
            "summary": instructions.summary,
        }))
    }
}

// =============================================================================
// JacsDocumentProvider implementation
// =============================================================================

impl JacsDocumentProvider for LocalJacsProvider {
    fn sign_document(&self, data: &Value) -> Result<String> {
        let service = self.require_document_service()?;
        let doc = service
            .create(
                &serde_json::to_string(data)?,
                jacs::document::types::CreateOptions::default(),
            )
            .map_err(|e| HaiError::Provider(format!("sign_document failed: {e}")))?;
        Ok(serde_json::to_string(&doc.value)?)
    }

    fn store_document(&self, signed_json: &str) -> Result<String> {
        let service = self.require_document_service()?;
        let doc = service
            .create(signed_json, jacs::document::types::CreateOptions::default())
            .map_err(|e| HaiError::Provider(format!("store_document failed: {e}")))?;
        Ok(format!("{}:{}", doc.id, doc.version))
    }

    fn sign_and_store(&self, data: &Value) -> Result<SignedDocument> {
        let service = self.require_document_service()?;
        let doc = service
            .create(
                &serde_json::to_string(data)?,
                jacs::document::types::CreateOptions::default(),
            )
            .map_err(|e| HaiError::Provider(format!("sign_and_store failed: {e}")))?;
        Ok(SignedDocument {
            key: format!("{}:{}", doc.id, doc.version),
            json: serde_json::to_string(&doc.value)?,
        })
    }

    fn sign_file(&self, path: &str, _embed: bool) -> Result<SignedDocument> {
        // Read the file and create a document from it
        let content = std::fs::read_to_string(path).map_err(|e| {
            HaiError::Provider(format!("failed to read file '{}': {e}", path))
        })?;
        let data: Value = serde_json::from_str(&content).map_err(|e| {
            HaiError::Provider(format!("file '{}' is not valid JSON: {e}", path))
        })?;
        self.sign_and_store(&data)
    }

    fn get_document(&self, key: &str) -> Result<String> {
        let service = self.require_document_service()?;
        let doc = service
            .get(key)
            .map_err(|e| HaiError::Provider(format!("get_document failed: {e}")))?;
        Ok(serde_json::to_string(&doc.value)?)
    }

    fn list_documents(&self, jacs_type: Option<&str>) -> Result<Vec<String>> {
        let service = self.require_document_service()?;
        let filter = jacs::document::types::ListFilter {
            jacs_type: jacs_type.map(|s| s.to_string()),
            ..Default::default()
        };
        let summaries = service
            .list(filter)
            .map_err(|e| HaiError::Provider(format!("list_documents failed: {e}")))?;
        Ok(summaries.into_iter().map(|s| s.key).collect())
    }

    fn get_document_versions(&self, doc_id: &str) -> Result<Vec<String>> {
        let service = self.require_document_service()?;
        let versions = service
            .versions(doc_id)
            .map_err(|e| HaiError::Provider(format!("get_document_versions failed: {e}")))?;
        Ok(versions
            .into_iter()
            .map(|d| format!("{}:{}", d.id, d.version))
            .collect())
    }

    fn get_latest_document(&self, doc_id: &str) -> Result<String> {
        let service = self.require_document_service()?;
        let doc = service
            .get_latest(doc_id)
            .map_err(|e| HaiError::Provider(format!("get_latest_document failed: {e}")))?;
        Ok(serde_json::to_string(&doc.value)?)
    }

    fn remove_document(&self, key: &str) -> Result<()> {
        let service = self.require_document_service()?;
        service
            .remove(key)
            .map_err(|e| HaiError::Provider(format!("remove_document failed: {e}")))?;
        Ok(())
    }

    fn update_document(&self, doc_id: &str, data: &str) -> Result<SignedDocument> {
        let service = self.require_document_service()?;
        let doc = service
            .update(
                doc_id,
                data,
                jacs::document::types::UpdateOptions::default(),
            )
            .map_err(|e| HaiError::Provider(format!("update_document failed: {e}")))?;
        Ok(SignedDocument {
            key: format!("{}:{}", doc.id, doc.version),
            json: serde_json::to_string(&doc.value)?,
        })
    }

    fn search_documents(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DocSearchResults> {
        let service = self.require_document_service()?;
        let search_query = jacs::search::SearchQuery {
            query: query.to_string(),
            limit,
            offset,
            ..Default::default()
        };
        let results = service
            .search(search_query)
            .map_err(|e| HaiError::Provider(format!("search failed: {e}")))?;
        Ok(DocSearchResults {
            results: results
                .results
                .into_iter()
                .map(|hit| DocSearchHit {
                    key: format!("{}:{}", hit.document.id, hit.document.version),
                    json: serde_json::to_string(&hit.document.value).unwrap_or_default(),
                    score: hit.score,
                    matched_fields: hit.matched_fields,
                })
                .collect(),
            total_count: results.total_count,
            method: format!("{:?}", results.method),
        })
    }

    fn query_by_type(
        &self,
        doc_type: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>> {
        let service = self.require_document_service()?;
        let filter = jacs::document::types::ListFilter {
            jacs_type: Some(doc_type.to_string()),
            limit: Some(limit),
            offset: Some(offset),
            ..Default::default()
        };
        let summaries = service
            .list(filter)
            .map_err(|e| HaiError::Provider(format!("query_by_type failed: {e}")))?;
        Ok(summaries.into_iter().map(|s| s.key).collect())
    }

    fn query_by_field(
        &self,
        field: &str,
        value: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>> {
        let service = self.require_document_service()?;
        let search_query = jacs::search::SearchQuery {
            query: String::new(),
            field_filter: Some(jacs::search::FieldFilter {
                field_path: field.to_string(),
                value: value.to_string(),
            }),
            limit,
            offset,
            ..Default::default()
        };
        let results = service
            .search(search_query)
            .map_err(|e| HaiError::Provider(format!("query_by_field failed: {e}")))?;
        Ok(results
            .results
            .into_iter()
            .map(|hit| format!("{}:{}", hit.document.id, hit.document.version))
            .collect())
    }

    fn storage_capabilities(&self) -> Result<StorageCapabilities> {
        let _service = self.require_document_service()?;
        // Report capabilities based on the configured backend.
        // For now, return conservative defaults — backends that support
        // richer capabilities will override in the future.
        Ok(StorageCapabilities {
            fulltext: true,
            vector: false,
            query_by_field: true,
            query_by_type: true,
            pagination: true,
            tombstone: true,
        })
    }
}

// =============================================================================
// JacsBatchProvider implementation
// =============================================================================

impl JacsBatchProvider for LocalJacsProvider {
    fn sign_messages(&self, messages: &[&Value]) -> Result<Vec<SignedDocument>> {
        let simple = self.load_simple_agent()?;
        let results = simple::batch::sign_messages(&simple, messages)
            .map_err(|e| HaiError::Provider(format!("batch sign_messages failed: {e}")))?;
        Ok(results
            .into_iter()
            .map(|sd| SignedDocument {
                key: sd.document_id.clone(),
                json: sd.raw,
            })
            .collect())
    }

    fn verify_batch(&self, documents: &[&str]) -> Vec<DocVerificationResult> {
        documents
            .iter()
            .map(|doc| {
                match self.verify_a2a_artifact(doc) {
                    Ok(result_json) => {
                        let v: Value = serde_json::from_str(&result_json).unwrap_or_default();
                        DocVerificationResult {
                            key: v["signerId"].as_str().unwrap_or("").to_string(),
                            valid: v["valid"].as_bool().unwrap_or(false),
                            error: if v["status"] == "invalid" {
                                Some("verification failed".to_string())
                            } else {
                                None
                            },
                        }
                    }
                    Err(e) => DocVerificationResult {
                        key: String::new(),
                        valid: false,
                        error: Some(e.to_string()),
                    },
                }
            })
            .collect()
    }
}

// =============================================================================
// JacsVerificationProvider implementation
// =============================================================================

impl JacsVerificationProvider for LocalJacsProvider {
    fn verify_document(&self, document: &str) -> Result<DocVerificationResult> {
        let result_json = self.verify_a2a_artifact(document)?;
        let v: Value = serde_json::from_str(&result_json)?;
        Ok(DocVerificationResult {
            key: v["signerId"].as_str().unwrap_or("").to_string(),
            valid: v["valid"].as_bool().unwrap_or(false),
            error: if v["status"] == "invalid" {
                Some("verification failed".to_string())
            } else {
                None
            },
        })
    }

    fn verify_with_key(&self, document: &str, _key: Vec<u8>) -> Result<DocVerificationResult> {
        // For now, delegate to standard verification.
        // Full key-based verification requires Agent-level APIs.
        self.verify_document(document)
    }

    fn verify_by_id(&self, doc_id: &str) -> Result<DocVerificationResult> {
        let service = self.require_document_service()?;
        let doc = service
            .get_latest(doc_id)
            .map_err(|e| HaiError::Provider(format!("verify_by_id: get failed: {e}")))?;
        let json = serde_json::to_string(&doc.value)?;
        self.verify_document(&json)
    }

    fn verify_dns(&self, domain: &str) -> Result<()> {
        let agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
        let pk = agent.get_public_key().map_err(|e| {
            HaiError::Provider(format!("failed to get public key: {e}"))
        })?;
        let agent_value = agent.get_value().cloned().ok_or_else(|| {
            HaiError::Provider("agent not loaded".to_string())
        })?;
        let agent_id = agent_value["jacsId"].as_str().unwrap_or("");
        let embedded_fp = agent_value["jacsPublicKeyFingerprint"].as_str();

        jacs::dns::bootstrap::verify_pubkey_via_dns_or_embedded(
            &pk,
            agent_id,
            Some(domain),
            embedded_fp,
            false,
        )
        .map_err(|e| HaiError::Provider(format!("DNS verification failed: {e}")))?;
        Ok(())
    }

    fn build_auth_header_jacs(&self) -> Result<String> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
        jacs::protocol::build_auth_header(&mut agent)
            .map_err(|e| HaiError::Provider(format!("build_auth_header failed: {e}")))
    }
}

// =============================================================================
// JacsEmailProvider implementation
// =============================================================================

impl JacsEmailProvider for LocalJacsProvider {
    fn sign_email(&self, raw: &[u8]) -> Result<Vec<u8>> {
        self.sign_email_locally(raw)
    }

    fn verify_email(&self, raw: &[u8], key: Vec<u8>) -> Result<Value> {
        let simple = self.load_simple_agent()?;
        let (sig_doc, _parts) = jacs::email::verify_email_document(raw, &simple, &key)
            .map_err(|e| HaiError::Provider(format!("verify_email failed: {e}")))?;
        // Convert the signature document to a JSON value
        serde_json::to_value(&sig_doc)
            .map_err(|e| HaiError::Provider(format!("serialize verify result: {e}")))
    }

    fn add_jacs_attachment(&self, email: &[u8], doc: &[u8]) -> Result<Vec<u8>> {
        jacs::email::add_jacs_attachment(email, doc)
            .map_err(|e| HaiError::Provider(format!("add_jacs_attachment failed: {e}")))
    }

    fn get_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>> {
        jacs::email::get_jacs_attachment(email)
            .map_err(|e| HaiError::Provider(format!("get_jacs_attachment failed: {e}")))
    }

    fn remove_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>> {
        jacs::email::remove_jacs_attachment(email)
            .map_err(|e| HaiError::Provider(format!("remove_jacs_attachment failed: {e}")))
    }

    fn extract_email_parts(&self, raw: &[u8]) -> Result<Value> {
        let parts = jacs::email::extract_email_parts(raw)
            .map_err(|e| HaiError::Provider(format!("extract_email_parts failed: {e}")))?;
        // Convert manually since ParsedEmailParts does not derive Serialize
        let body_part_to_json = |bp: jacs::email::ParsedBodyPart| -> Value {
            serde_json::json!({
                "content_type": bp.content_type,
                "content": String::from_utf8_lossy(&bp.content),
            })
        };
        Ok(serde_json::json!({
            "headers": parts.headers,
            "body_plain": parts.body_plain.map(body_part_to_json),
            "body_html": parts.body_html.map(body_part_to_json),
            "attachments_count": parts.attachments.len(),
            "jacs_attachments_count": parts.jacs_attachments.len(),
        }))
    }
}

// =============================================================================
// JacsAgreementProvider implementation (feature-gated)
// =============================================================================

#[cfg(feature = "agreements")]
impl JacsAgreementProvider for LocalJacsProvider {
    fn create_agreement(
        &self,
        doc: &str,
        agent_ids: &[String],
        quorum: Option<&str>,
    ) -> Result<SignedDocument> {
        let simple = self.load_simple_agent()?;
        let options = quorum.map(|q| {
            let q_num: u32 = q.parse().unwrap_or(0);
            jacs::agent::agreement::AgreementOptions {
                quorum: if q_num > 0 { Some(q_num) } else { None },
                ..Default::default()
            }
        });
        let sd = jacs::agreements::create_with_options(
            &simple,
            doc,
            agent_ids,
            None,   // question
            None,   // context
            options.as_ref(),
        )
        .map_err(|e| HaiError::Provider(format!("create_agreement failed: {e}")))?;
        Ok(SignedDocument {
            key: sd.document_id.clone(),
            json: sd.raw,
        })
    }

    fn sign_agreement(&self, document: &str) -> Result<SignedDocument> {
        let simple = self.load_simple_agent()?;
        let sd = jacs::agreements::sign(&simple, document)
            .map_err(|e| HaiError::Provider(format!("sign_agreement failed: {e}")))?;
        Ok(SignedDocument {
            key: sd.document_id.clone(),
            json: sd.raw,
        })
    }

    fn check_agreement(&self, document: &str) -> Result<Value> {
        let simple = self.load_simple_agent()?;
        let status = jacs::agreements::check(&simple, document)
            .map_err(|e| HaiError::Provider(format!("check_agreement failed: {e}")))?;
        serde_json::to_value(&status)
            .map_err(|e| HaiError::Provider(format!("serialize agreement status: {e}")))
    }
}

// =============================================================================
// JacsAttestationProvider implementation (feature-gated)
// =============================================================================

#[cfg(feature = "attestation")]
impl JacsAttestationProvider for LocalJacsProvider {
    fn create_attestation(&self, subject: &Value, claims: &[Value]) -> Result<String> {
        let simple = self.load_simple_agent()?;

        // Parse subject from Value
        let subject: jacs::attestation::types::AttestationSubject =
            serde_json::from_value(subject.clone()).map_err(|e| {
                HaiError::Provider(format!("invalid attestation subject: {e}"))
            })?;

        // Parse claims from Values
        let claims: Vec<jacs::attestation::types::Claim> = claims
            .iter()
            .map(|c| serde_json::from_value(c.clone()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| HaiError::Provider(format!("invalid attestation claims: {e}")))?;

        let sd = jacs::attestation::simple::create(
            &simple,
            &subject,
            &claims,
            &[],    // evidence
            None,   // derivation
            None,   // policy_context
        )
        .map_err(|e| HaiError::Provider(format!("create_attestation failed: {e}")))?;

        Ok(sd.raw)
    }

    fn verify_attestation(&self, doc_key: &str) -> Result<Value> {
        let simple = self.load_simple_agent()?;
        let result = jacs::attestation::simple::verify(&simple, doc_key)
            .map_err(|e| HaiError::Provider(format!("verify_attestation failed: {e}")))?;
        serde_json::to_value(&result)
            .map_err(|e| HaiError::Provider(format!("serialize attestation result: {e}")))
    }
}

// =============================================================================
// Config resolution
// =============================================================================

fn resolve_jacs_config_path(config_path: Option<&Path>) -> PathBuf {
    if let Some(path) = config_path {
        return path.to_path_buf();
    }

    if let Ok(path) = env::var("JACS_CONFIG") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    if let Ok(path) = env::var("JACS_CONFIG_PATH") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    PathBuf::from("./jacs.config.json")
}
