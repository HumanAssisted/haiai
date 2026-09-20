use haiai::{CreateAgentOptions, HaiClient, HaiClientOptions, JacsProvider, LocalJacsProvider};
use jacs::response_context::{EventCausation, EventTransport, ResponseData, ResponseOperation};
use serde_json::json;
pub const TENANT: &str = "transport-test-tenant";
pub const REQUEST_AUDIENCE: &str = "transport-test-api";
const PASSWORD: &str = "transport-fixture-password";
pub static TEST_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct RestorePassword(Option<std::ffi::OsString>);

impl RestorePassword {
    pub fn set() -> Self {
        let previous = std::env::var_os("JACS_PRIVATE_KEY_PASSWORD");
        unsafe { std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", PASSWORD) };
        Self(previous)
    }
}

impl Drop for RestorePassword {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => unsafe { std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", value) },
            None => unsafe { std::env::remove_var("JACS_PRIVATE_KEY_PASSWORD") },
        }
    }
}

pub fn make_client(base_url: &str, fixture: &SignedEventFixture) -> HaiClient<LocalJacsProvider> {
    HaiClient::new(
        LocalJacsProvider::from_config_path(Some(&fixture.config_path), None)
            .expect("load client signer"),
        HaiClientOptions {
            base_url: base_url.to_string(),
            request_auth_audience: REQUEST_AUDIENCE.into(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
    .with_expected_event_context(TENANT.into(), REQUEST_AUDIENCE.into())
    .expect("pinned event context")
}

pub struct SignedEventFixture {
    _directory: tempfile::TempDir,
    config_path: std::path::PathBuf,
    signer: LocalJacsProvider,
    pub signer_id: String,
    public_key_pem: String,
}

impl SignedEventFixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("isolated signing fixture");
        let root = directory
            .path()
            .canonicalize()
            .expect("canonical fixture root");
        let config = root.join("jacs.config.json");
        let created = LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
            name: "transport-fixture".into(),
            password: PASSWORD.into(),
            algorithm: Some("ed25519".into()),
            data_directory: Some(root.join("data").display().to_string()),
            key_directory: Some(root.join("keys").display().to_string()),
            config_path: Some(config.display().to_string()),
            agent_type: None,
            description: None,
            domain: None,
            default_storage: None,
        })
        .expect("create real JACS signer");
        let signer = LocalJacsProvider::from_config_path(Some(&config), None).expect("load signer");
        let signer_id = format!("{}:{}", created.agent_id, created.version);
        let public_key_pem = signer.public_key_pem().expect("fixture public key");
        Self {
            _directory: directory,
            config_path: config,
            signer,
            signer_id,
            public_key_pem,
        }
    }

    pub fn sign(&self, payload: serde_json::Value, auth: &str, websocket: bool) -> String {
        let claims = jacs::protocol::inspect_unverified_request_auth_header(auth)
            .expect("received request-auth-v2 credential");
        assert_eq!(claims.audience, REQUEST_AUDIENCE);
        assert_eq!(claims.method, "GET");
        let channel = format!("jacs-auth-nonce:{}", claims.nonce);
        let event_type = payload["type"].as_str().expect("event type").to_string();
        let causation = if event_type == "benchmark_job" {
            EventCausation::Job {
                id: payload["job_id"].as_str().expect("job id").into(),
            }
        } else {
            EventCausation::None { id: () }
        };
        self.signer
            .sign_response_with_context(
                &ResponseData::PrivateEvent {
                    event_type,
                    contract: "hai.agent-event".into(),
                    contract_version: "2".into(),
                    transport: if websocket {
                        EventTransport::Channel(channel)
                    } else {
                        EventTransport::Stream(channel)
                    },
                    tenant: TENANT.into(),
                    audience: jacs::validation::normalize_agent_id(&claims.key_id).into(),
                    issuer: String::new(),
                    event_id: String::new(),
                    emitted_at: String::new(),
                    causation,
                    payload,
                },
                ResponseOperation::SignAsyncEvent,
            )
            .expect("sign contextual transport event")
            .signed_document
    }

    pub fn key_document(&self) -> serde_json::Value {
        json!({
            "keys": [{
                "signer_id": self.signer_id,
                "public_key": self.public_key_pem,
                "is_active": true
            }]
        })
    }
}
