use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use haiai::{
    JacsDocumentProvider, RemoteJacsProvider, RemoteJacsProviderOptions, StaticJacsProvider,
};
use httpmock::{Method, MockServer};
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    fn as_string(&self) -> String {
        String::from_utf8(self.0.lock().expect("logs lock").clone()).expect("utf8 logs")
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        CapturedWriter(Arc::clone(&self.0))
    }
}

struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("logs lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn make_provider(base_url: String) -> RemoteJacsProvider<StaticJacsProvider> {
    RemoteJacsProvider::new(
        StaticJacsProvider::new("trace-agent"),
        RemoteJacsProviderOptions {
            base_url,
            ..RemoteJacsProviderOptions::default()
        },
    )
    .expect("provider")
}

fn capture_logs<F>(f: F) -> String
where
    F: FnOnce(),
{
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();

    let _guard = tracing::subscriber::set_default(subscriber);
    f();
    drop(_guard);
    logs.as_string()
}

#[test]
fn remote_store_document_traces_url_status_and_outcome() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(Method::POST).path("/api/v1/records");
        then.status(200)
            .json_body(serde_json::json!({"key":"trace-doc:v1"}));
    });
    let provider = make_provider(server.base_url());

    let logs = capture_logs(|| {
        let key = provider
            .store_document(r#"{"hello":"trace"}"#)
            .expect("store document");
        assert_eq!(key, "trace-doc:v1");
    });

    assert!(logs.contains("remote record POST starting"), "{logs}");
    assert!(logs.contains("/api/v1/records"), "{logs}");
    assert!(logs.contains("remote record POST completed"), "{logs}");
    assert!(logs.contains("status=200"), "{logs}");
}

#[test]
fn remote_store_document_traces_error_status() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(Method::POST).path("/api/v1/records");
        then.status(503)
            .json_body(serde_json::json!({"error":"temporarily unavailable"}));
    });
    let provider = make_provider(server.base_url());

    let logs = capture_logs(|| {
        let err = provider
            .store_document(r#"{"hello":"trace"}"#)
            .expect_err("server error should fail");
        assert!(err.to_string().contains("temporarily unavailable"));
    });

    assert!(logs.contains("remote record request failed"), "{logs}");
    assert!(logs.contains("status=503"), "{logs}");
}
