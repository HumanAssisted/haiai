//! Single final-byte authentication boundary for HAI HTTP requests.
//! Public discovery/registration stays unsigned. Authenticated requests never
//! downgrade or follow redirects: a changed request requires a fresh proof.

use crate::{
    error::{HaiError, Result},
    jacs::JacsProvider,
};
use std::sync::Arc;

pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;

pub(crate) fn missing_request_context() -> HaiError {
    HaiError::Validation {
        field: "request_context".into(),
        message: "request authentication requires the final method, URL and body bytes; use build_request_auth_header or an authenticated HaiClient operation".into(),
    }
}

pub(crate) struct RequestClient<P: JacsProvider> {
    raw: reqwest::Client,
    authenticated: reqwest::Client,
    provider: Arc<P>,
    audience: String,
}

impl<P: JacsProvider> RequestClient<P> {
    pub(crate) fn new(
        raw: reqwest::Client,
        authenticated: reqwest::Client,
        provider: Arc<P>,
        audience: String,
    ) -> Self {
        Self {
            raw,
            authenticated,
            provider,
            audience,
        }
    }

    pub(crate) fn raw_client(&self) -> &reqwest::Client {
        &self.raw
    }
    pub(crate) fn audience(&self) -> &str {
        &self.audience
    }

    pub(crate) fn auth_header(&self, method: &str, url: &str, body: &[u8]) -> Result<String> {
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(HaiError::Validation {
                field: "body".into(),
                message: "request-auth-v2 body exceeds 10 MiB".into(),
            });
        }
        self.provider
            .build_request_auth_header(method, url, body, &self.audience)
            .inspect_err(|_| {
                tracing::warn!(
                    event = "jacs_request_auth_failed",
                    reason = "signing_failed",
                    "Request authentication failed before sending"
                );
            })
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: impl reqwest::IntoUrl,
    ) -> RequestBuilder<'_, P> {
        RequestBuilder {
            inner: self.raw.request(method, url),
            client: self,
            authenticated: false,
        }
    }
    pub(crate) fn get(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::GET, url)
    }
    pub(crate) fn post(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::POST, url)
    }
    pub(crate) fn put(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::PUT, url)
    }
    pub(crate) fn delete(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::DELETE, url)
    }

    pub(crate) async fn execute_authenticated(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response> {
        Ok(self.authenticated.execute(request).await?)
    }
}

pub(crate) struct RequestBuilder<'a, P: JacsProvider> {
    inner: reqwest::RequestBuilder,
    client: &'a RequestClient<P>,
    authenticated: bool,
}

impl<P: JacsProvider> RequestBuilder<'_, P> {
    pub(crate) fn authenticated(mut self) -> Self {
        self.authenticated = true;
        self
    }
    pub(crate) fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.inner = self.inner.header(name.as_ref(), value.as_ref());
        self
    }
    pub(crate) fn json<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.inner = self.inner.json(value);
        self
    }
    pub(crate) fn query<T: serde::Serialize + ?Sized>(mut self, value: &T) -> Self {
        self.inner = self.inner.query(value);
        self
    }
    pub(crate) fn body(mut self, value: impl Into<reqwest::Body>) -> Self {
        self.inner = self.inner.body(value);
        self
    }

    pub(crate) fn build(self) -> Result<reqwest::Request> {
        let mut request = self.inner.build()?;
        if self.authenticated {
            let bytes = match request.body() {
                Some(body) => body.as_bytes().ok_or_else(|| HaiError::Validation {
                    field: "body".into(),
                    message: "request-auth-v2 requires bounded entity bytes, not a streaming body"
                        .into(),
                })?,
                None => &[],
            };
            let header = self.client.auth_header(
                request.method().as_str(),
                request.url().as_str(),
                bytes,
            )?;
            request.headers_mut().insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&header)
                    .map_err(|error| HaiError::Provider(error.to_string()))?,
            );
        }
        Ok(request)
    }

    pub(crate) async fn send(self) -> Result<reqwest::Response> {
        let authenticated = self.authenticated;
        let client = self.client;
        let request = self.build()?;
        if authenticated {
            client.execute_authenticated(request).await
        } else {
            Ok(client.raw.execute(request).await?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jacs::StaticJacsProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn client() -> RequestClient<StaticJacsProvider> {
        let raw = reqwest::Client::new();
        let authenticated = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        RequestClient::new(
            raw,
            authenticated,
            Arc::new(StaticJacsProvider::new("request-fixture")),
            "hai.ai".into(),
        )
    }

    #[test]
    fn serializer_runs_once_before_authentication_not_again_at_send_boundary() {
        struct Counted<'a>(&'a AtomicUsize);
        impl serde::Serialize for Counted<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_u64(self.0.fetch_add(1, Ordering::SeqCst) as u64)
            }
        }
        let count = AtomicUsize::new(0);
        let request = client()
            .post("https://hai.example/api")
            .authenticated()
            .json(&Counted(&count))
            .build()
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(request.body().unwrap().as_bytes().unwrap(), b"0");
        assert!(request.headers()[reqwest::header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .starts_with("JACS v2."));
    }

    #[test]
    fn streaming_request_body_is_rejected_before_execution() {
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(vec![1, 2, 3])]);
        let error = client()
            .post("https://hai.example/api")
            .authenticated()
            .body(reqwest::Body::wrap_stream(stream))
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("bounded entity bytes"));
    }

    #[test]
    fn public_discovery_and_bootstrap_do_not_gain_an_auth_header() {
        let request = client()
            .get("https://hai.example/.well-known/hai-keys.json")
            .build()
            .unwrap();
        assert!(!request
            .headers()
            .contains_key(reqwest::header::AUTHORIZATION));
        let registration = client()
            .post("https://hai.example/api/v1/agents/register")
            .json(&serde_json::json!({"agent_json":"signed-document"}))
            .build()
            .unwrap();
        assert!(!registration
            .headers()
            .contains_key(reqwest::header::AUTHORIZATION));
    }
}
