use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex as StdMutex;

use haisdk::{
    HaiClient, HaiClientOptions, JacsProvider, LocalJacsProvider, NoopJacsProvider,
};

#[derive(Debug, Clone, Default)]
struct CachedAgentState {
    hai_agent_id: Option<String>,
    agent_email: Option<String>,
}

#[derive(Debug)]
pub struct HaiServerContext {
    base_url: String,
    fallback_jacs_id: String,
    default_config_path: Option<String>,
    cached_agent_state: StdMutex<BTreeMap<String, CachedAgentState>>,
}

impl HaiServerContext {
    pub fn from_process_env(fallback_jacs_id: String, default_config_path: Option<String>) -> Self {
        let base_url = std::env::var("HAI_URL").unwrap_or_else(|_| "https://hai.ai".to_string());
        Self {
            base_url,
            fallback_jacs_id,
            default_config_path,
            cached_agent_state: StdMutex::new(BTreeMap::new()),
        }
    }

    fn effective_config_path<'a>(&'a self, override_path: Option<&'a str>) -> Option<&'a Path> {
        override_path
            .map(Path::new)
            .or_else(|| self.default_config_path.as_deref().map(Path::new))
    }

    pub fn noop_client_with_url(
        &self,
        base_url_override: Option<&str>,
    ) -> Result<HaiClient<NoopJacsProvider>, String> {
        let provider = NoopJacsProvider::new(self.fallback_jacs_id.clone());
        self.client_with_provider(provider, base_url_override)
    }

    pub fn local_provider(&self, config_path: Option<&str>) -> Result<LocalJacsProvider, String> {
        LocalJacsProvider::from_config_path(self.effective_config_path(config_path)).map_err(|e| {
            format!("failed to load local JACS agent; set JACS_CONFIG or pass config_path: {e}")
        })
    }

    pub fn local_client_with_url(
        &self,
        config_path: Option<&str>,
        base_url_override: Option<&str>,
    ) -> Result<HaiClient<LocalJacsProvider>, String> {
        let provider = self.local_provider(config_path)?;
        let jacs_id = provider.jacs_id().to_string();
        let mut client = self.client_with_provider(provider, base_url_override)?;
        self.apply_cached_agent_state(&jacs_id, &mut client);
        Ok(client)
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
        cached
            .entry(jacs_id.to_string())
            .or_default()
            .hai_agent_id = Some(agent_id.to_string());
    }

    pub fn remember_agent_email(&self, jacs_id: &str, email: &str) {
        if email.is_empty() {
            return;
        }

        let mut cached = self
            .cached_agent_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cached
            .entry(jacs_id.to_string())
            .or_default()
            .agent_email = Some(email.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haisdk::{HaiClient, NoopJacsProvider};

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
        let context = HaiServerContext::from_process_env("anonymous-agent".to_string(), None);

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
    fn explicit_config_path_overrides_default() {
        let context = HaiServerContext::from_process_env(
            "anonymous-agent".to_string(),
            Some("/tmp/default-jacs.config.json".to_string()),
        );

        assert_eq!(
            context.effective_config_path(Some("/tmp/override.json")),
            Some(Path::new("/tmp/override.json"))
        );
        assert_eq!(
            context.effective_config_path(None),
            Some(Path::new("/tmp/default-jacs.config.json"))
        );
    }
}
