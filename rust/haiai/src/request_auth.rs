//! Explicit opt-in request-auth-v2 boundary. Legacy/default requests delegate
//! unchanged to reqwest. Only a v2 pending marker triggers final-byte signing.

use crate::{
    error::{HaiError, Result},
    jacs::JacsProvider,
};
use std::sync::Arc;

pub(crate) const PENDING_V2: &str = "JACS internal-pending-v2";

pub(crate) struct RequestClient<P: JacsProvider> {
    raw: reqwest::Client,
    v2: reqwest::Client,
    provider: Arc<P>,
    pub(crate) audience: Option<String>,
}

impl<P: JacsProvider> std::ops::Deref for RequestClient<P> {
    type Target = reqwest::Client;
    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl<P: JacsProvider> RequestClient<P> {
    pub(crate) fn new(raw: reqwest::Client, v2: reqwest::Client, provider: Arc<P>) -> Self {
        Self {
            raw,
            v2,
            provider,
            audience: None,
        }
    }
    pub(crate) fn raw_client(&self) -> &reqwest::Client {
        &self.raw
    }
    fn request(
        &self,
        method: reqwest::Method,
        url: impl reqwest::IntoUrl,
    ) -> RequestBuilder<'_, P> {
        RequestBuilder {
            inner: self.raw.request(method, url),
            client: self,
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
    pub(crate) fn patch(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::PATCH, url)
    }
    pub(crate) fn delete(&self, url: impl reqwest::IntoUrl) -> RequestBuilder<'_, P> {
        self.request(reqwest::Method::DELETE, url)
    }
}

pub(crate) struct RequestBuilder<'a, P: JacsProvider> {
    inner: reqwest::RequestBuilder,
    client: &'a RequestClient<P>,
}

impl<P: JacsProvider> RequestBuilder<'_, P> {
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
    pub(crate) async fn send(self) -> Result<reqwest::Response> {
        // Default/legacy path preserves reqwest's original serialization and
        // redirect behavior. No new signer call occurs on this path.
        if self.client.audience.is_none() {
            return Ok(self.inner.send().await?);
        }
        let mut request = self.inner.build()?;
        let pending = request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some(PENDING_V2);
        if !pending {
            return Ok(self.client.raw.execute(request).await?);
        }
        let bytes = match request.body() {
            Some(body) => body.as_bytes().ok_or_else(|| HaiError::Validation {
                field: "body".into(), message: "v2 authentication requires an explicit bounded entity, not a streaming request body".into(),
            })?,
            None => &[],
        };
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(HaiError::Validation {
                field: "body".into(),
                message: "v2 request body exceeds 10 MiB".into(),
            });
        }
        let header = self.client.provider.build_request_auth_header(
            request.method().as_str(),
            request.url().as_str(),
            bytes,
            self.client.audience.as_deref().expect("v2 selected"),
        )?;
        request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&header)
                .map_err(|error| HaiError::Provider(error.to_string()))?,
        );
        // V2 alone uses the no-redirect executor. The exact request is sent
        // once; an outer retry constructs a fresh nonce over identical bytes.
        Ok(self.client.v2.execute(request).await?)
    }
}
