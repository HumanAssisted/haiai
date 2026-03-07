use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use haisdk::{HaiClient, HaiClientOptions, JacsProvider, LocalJacsProvider, NoopJacsProvider};

use crate::embedded_provider::EmbeddedJacsProvider;

#[derive(Debug, Clone, Default)]
struct CachedAgentState {
    hai_agent_id: Option<String>,
    agent_email: Option<String>,
}

pub struct HaiServerContext {
    base_url: String,
    fallback_jacs_id: String,
    default_config_path: Option<PathBuf>,
    embedded_provider: EmbeddedJacsProvider,
    cached_agent_state: StdMutex<BTreeMap<String, CachedAgentState>>,
}

impl HaiServerContext {
    pub fn from_process_env(
        fallback_jacs_id: String,
        default_config_path: Option<String>,
        embedded_provider: EmbeddedJacsProvider,
    ) -> Self {
        let base_url = std::env::var("HAI_URL").unwrap_or_else(|_| "https://hai.ai".to_string());
        let default_config_path = default_config_path.map(PathBuf::from);
        Self {
            base_url,
            fallback_jacs_id,
            default_config_path,
            embedded_provider,
            cached_agent_state: StdMutex::new(BTreeMap::new()),
        }
    }

    fn effective_config_path<'a>(&'a self, override_path: Option<&'a str>) -> Option<&'a Path> {
        override_path
            .map(Path::new)
            .or(self.default_config_path.as_deref())
    }

    fn validate_embedded_config_path(&self, override_path: Option<&str>) -> Result<(), String> {
        let Some(override_path) = override_path else {
            return Ok(());
        };
        let Some(default_config_path) = self.default_config_path.as_ref() else {
            return Err(
                "hai-mcp does not have a startup JACS config path; alternate config_path values are not supported."
                    .to_string(),
            );
        };

        let requested = absolutize_path(Path::new(override_path))?;
        if requested == *default_config_path {
            return Ok(());
        }

        Err(format!(
            "hai-mcp uses the embedded JACS identity loaded from {}. Alternate config_path values are not supported for this tool.",
            default_config_path.display()
        ))
    }

    pub fn noop_client_with_url(
        &self,
        base_url_override: Option<&str>,
    ) -> Result<HaiClient<NoopJacsProvider>, String> {
        let provider = NoopJacsProvider::new(self.fallback_jacs_id.clone());
        self.client_with_provider(provider, base_url_override)
    }

    pub fn embedded_provider(
        &self,
        config_path: Option<&str>,
    ) -> Result<EmbeddedJacsProvider, String> {
        self.validate_embedded_config_path(config_path)?;
        Ok(self.embedded_provider.clone())
    }

    pub fn embedded_client_with_url(
        &self,
        config_path: Option<&str>,
        base_url_override: Option<&str>,
    ) -> Result<HaiClient<EmbeddedJacsProvider>, String> {
        let provider = self.embedded_provider(config_path)?;
        let jacs_id = provider.jacs_id().to_string();
        let mut client = self.client_with_provider(provider, base_url_override)?;
        self.apply_cached_agent_state(&jacs_id, &mut client);
        Ok(client)
    }

    pub fn local_provider(&self, config_path: Option<&str>) -> Result<LocalJacsProvider, String> {
        LocalJacsProvider::from_config_path(self.effective_config_path(config_path)).map_err(|e| {
            format!("failed to load local JACS agent; set JACS_CONFIG or pass config_path: {e}")
        })
    }

    pub fn client_with_provider<P: JacsProvider>(
        &self,
        provider: P,
        base_url_override: Option<&str>,
    ) -> Result<HaiClient<P>, String> {
        let base_url = base_url_override.unwrap_or(&self.base_url).to_string();
        HaiClient::new(
            provider,
            HaiClientOptions {
                base_url,
                ..HaiClientOptions::default()
            },
        )
        .map_err(|e| e.to_string())
    }

    pub fn apply_cached_agent_state<P: JacsProvider>(
        &self,
        jacs_id: &str,
        client: &mut HaiClient<P>,
    ) {
        let cached = self
            .cached_agent_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(jacs_id)
            .cloned();

        if let Some(cached) = cached {
            if let Some(agent_id) = cached.hai_agent_id {
                client.set_hai_agent_id(agent_id);
            }
            if let Some(email) = cached.agent_email {
                client.set_agent_email(email);
            }
        }
    }

    pub fn remember_hai_agent_id(&self, jacs_id: &str, agent_id: &str) {
        if agent_id.is_empty() {
            return;
        }

        let mut cached = self
            .cached_agent_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cached.entry(jacs_id.to_string()).or_default().hai_agent_id = Some(agent_id.to_string());
    }

    pub fn remember_agent_email(&self, jacs_id: &str, email: &str) {
        if email.is_empty() {
            return;
        }

        let mut cached = self
            .cached_agent_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cached.entry(jacs_id.to_string()).or_default().agent_email = Some(email.to_string());
    }
}

fn absolutize_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| format!("failed to resolve current working directory: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haisdk::HaiClient;

    fn build_context(default_config_path: Option<&str>) -> HaiServerContext {
        HaiServerContext::from_process_env(
            "anonymous-agent".to_string(),
            default_config_path.map(ToString::to_string),
            EmbeddedJacsProvider::testing("agent-123"),
        )
    }

    fn apply_identity_overrides(
        context: &HaiServerContext,
        client: &mut HaiClient<impl JacsProvider>,
    ) {
        client.set_hai_agent_id("hai-agent-123".to_string());
        client.set_agent_email("agent@hai.ai".to_string());
        context.remember_hai_agent_id(client.jacs_id(), "hai-agent-123");
        context.remember_agent_email(client.jacs_id(), "agent@hai.ai");
    }

    #[test]
    fn cached_identity_is_restored_per_jacs_id() {
        let context = build_context(None);

        let mut seeded = context
            .client_with_provider(NoopJacsProvider::new("agent-123"), None)
            .expect("seed client");
        apply_identity_overrides(&context, &mut seeded);

        let mut restored = context
            .client_with_provider(NoopJacsProvider::new("agent-123"), None)
            .expect("restored client");
        context.apply_cached_agent_state("agent-123", &mut restored);

        assert_eq!(restored.hai_agent_id(), "hai-agent-123");
        assert_eq!(restored.agent_email(), Some("agent@hai.ai"));
    }

    #[test]
    fn explicit_config_path_overrides_default_for_local_provider_loading() {
        let context = build_context(Some("/tmp/default-jacs.config.json"));

        assert_eq!(
            context.effective_config_path(Some("/tmp/override.json")),
            Some(Path::new("/tmp/override.json"))
        );
        assert_eq!(
            context.effective_config_path(None),
            Some(Path::new("/tmp/default-jacs.config.json"))
        );
    }

    #[test]
    fn embedded_provider_rejects_drifted_config_paths() {
        let context = build_context(Some("/tmp/default-jacs.config.json"));

        let error = match context.embedded_provider(Some("/tmp/other-jacs.config.json")) {
            Ok(_) => panic!("drifted config path must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("embedded JACS identity"));

        assert!(context.embedded_provider(None).is_ok());
        assert!(context
            .embedded_provider(Some("/tmp/default-jacs.config.json"))
            .is_ok());
    }
}
