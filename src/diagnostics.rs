//! Explicitly sensitive request/response inspection hooks.
//!
//! This module is available only with the `sensitive-diagnostics` feature. It is
//! intended for local protocol debugging and can expose auth tokens, URLs, phone
//! numbers, sender identifiers, message text, headers, and raw response bodies.
//! Normal tracing, public errors, and `Debug` output remain redacted.

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use http::HeaderMap;

use crate::common::{REDACTED, RawResponse};

/// A cloneable handle that dispatches sensitive diagnostic events to a caller
/// supplied sink.
///
/// Values of this type are redacted in [`Debug`](fmt::Debug). The events sent
/// to the sink are intentionally not redacted.
#[derive(Clone)]
pub struct SensitiveDiagnostics {
    sink: Arc<dyn SensitiveDiagnosticSink>,
    enabled: bool,
}

impl SensitiveDiagnostics {
    /// Create diagnostics from a sink that receives every event.
    pub fn new(sink: impl SensitiveDiagnosticSink) -> Self {
        Self {
            sink: Arc::new(sink),
            enabled: true,
        }
    }

    /// Create a diagnostics handle that intentionally drops every event.
    ///
    /// Use this as a per-request override to disable a client-wide sensitive
    /// diagnostics sink for a single request.
    #[must_use]
    pub fn noop() -> Self {
        Self {
            sink: Arc::new(NoopSink),
            enabled: false,
        }
    }

    /// Start building diagnostics from separate event callbacks.
    #[must_use]
    pub fn builder() -> SensitiveDiagnosticsBuilder {
        SensitiveDiagnosticsBuilder::new()
    }

    pub(crate) fn record(&self, event: SensitiveDiagnosticEvent) {
        if self.enabled {
            self.sink.record(event);
        }
    }

    pub(crate) fn captures_events(&self) -> bool {
        self.enabled
    }
}

impl fmt::Debug for SensitiveDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveDiagnostics")
            .field("sink", &format_args!("[REDACTED]"))
            .finish()
    }
}

/// A sink for sensitive diagnostic events.
///
/// Implement this trait for custom collectors, or pass a closure to
/// [`SensitiveDiagnostics::new`]. The callback is synchronous; async callers
/// should avoid blocking inside it.
pub trait SensitiveDiagnosticSink: Send + Sync + 'static {
    /// Record one sensitive diagnostic event.
    fn record(&self, event: SensitiveDiagnosticEvent);
}

impl<F> SensitiveDiagnosticSink for F
where
    F: Fn(SensitiveDiagnosticEvent) + Send + Sync + 'static,
{
    fn record(&self, event: SensitiveDiagnosticEvent) {
        self(event);
    }
}

#[derive(Debug)]
struct NoopSink;

impl SensitiveDiagnosticSink for NoopSink {
    fn record(&self, _event: SensitiveDiagnosticEvent) {}
}

/// Builder for a [`SensitiveDiagnostics`] sink from event-specific callbacks.
#[derive(Default)]
pub struct SensitiveDiagnosticsBuilder {
    event: Option<EventCallback>,
    request: Option<RequestCallback>,
    response: Option<ResponseCallback>,
    transport_error: Option<TransportErrorCallback>,
}

type EventCallback = Arc<dyn Fn(SensitiveDiagnosticEvent) + Send + Sync>;
type RequestCallback = Arc<dyn Fn(SensitiveRequestSnapshot) + Send + Sync>;
type ResponseCallback = Arc<dyn Fn(SensitiveResponseSnapshot) + Send + Sync>;
type TransportErrorCallback = Arc<dyn Fn(SensitiveTransportErrorSnapshot) + Send + Sync>;

impl SensitiveDiagnosticsBuilder {
    fn new() -> Self {
        Self::default()
    }

    /// Register a callback for every sensitive diagnostic event.
    #[must_use]
    pub fn on_event(
        mut self,
        callback: impl Fn(SensitiveDiagnosticEvent) + Send + Sync + 'static,
    ) -> Self {
        self.event = Some(Arc::new(callback));
        self
    }

    /// Register a callback for request snapshots.
    #[must_use]
    pub fn on_request(
        mut self,
        callback: impl Fn(SensitiveRequestSnapshot) + Send + Sync + 'static,
    ) -> Self {
        self.request = Some(Arc::new(callback));
        self
    }

    /// Register a callback for response snapshots.
    #[must_use]
    pub fn on_response(
        mut self,
        callback: impl Fn(SensitiveResponseSnapshot) + Send + Sync + 'static,
    ) -> Self {
        self.response = Some(Arc::new(callback));
        self
    }

    /// Register a callback for transport-error snapshots.
    #[must_use]
    pub fn on_transport_error(
        mut self,
        callback: impl Fn(SensitiveTransportErrorSnapshot) + Send + Sync + 'static,
    ) -> Self {
        self.transport_error = Some(Arc::new(callback));
        self
    }

    /// Build a [`SensitiveDiagnostics`] handle.
    #[must_use]
    pub fn build(self) -> SensitiveDiagnostics {
        SensitiveDiagnostics::new(BuilderSink {
            event: self.event,
            request: self.request,
            response: self.response,
            transport_error: self.transport_error,
        })
    }
}

impl fmt::Debug for SensitiveDiagnosticsBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveDiagnosticsBuilder")
            .field("on_event", &redacted_callback(self.event.is_some()))
            .field("on_request", &redacted_callback(self.request.is_some()))
            .field("on_response", &redacted_callback(self.response.is_some()))
            .field(
                "on_transport_error",
                &redacted_callback(self.transport_error.is_some()),
            )
            .finish()
    }
}

struct BuilderSink {
    event: Option<EventCallback>,
    request: Option<RequestCallback>,
    response: Option<ResponseCallback>,
    transport_error: Option<TransportErrorCallback>,
}

impl SensitiveDiagnosticSink for BuilderSink {
    fn record(&self, event: SensitiveDiagnosticEvent) {
        if let Some(callback) = &self.event {
            callback(event.clone());
        }

        match event {
            SensitiveDiagnosticEvent::Request(snapshot) => {
                if let Some(callback) = &self.request {
                    callback(snapshot);
                }
            }
            SensitiveDiagnosticEvent::Response(snapshot) => {
                if let Some(callback) = &self.response {
                    callback(snapshot);
                }
            }
            SensitiveDiagnosticEvent::TransportError(snapshot) => {
                if let Some(callback) = &self.transport_error {
                    callback(snapshot);
                }
            }
        }
    }
}

/// A sensitive event emitted by the client executor.
#[derive(Clone)]
pub enum SensitiveDiagnosticEvent {
    /// A request attempt is about to be handed to the transport.
    Request(SensitiveRequestSnapshot),
    /// A response was received and fully read.
    Response(SensitiveResponseSnapshot),
    /// A transport error prevented a usable response from being produced.
    TransportError(SensitiveTransportErrorSnapshot),
}

impl fmt::Debug for SensitiveDiagnosticEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(snapshot) => f.debug_tuple("Request").field(snapshot).finish(),
            Self::Response(snapshot) => f.debug_tuple("Response").field(snapshot).finish(),
            Self::TransportError(snapshot) => {
                f.debug_tuple("TransportError").field(snapshot).finish()
            }
        }
    }
}

/// Request data known to the twilio2 executor for one HTTP attempt.
#[derive(Clone)]
pub struct SensitiveRequestSnapshot {
    /// HTTP method.
    pub method: http::Method,
    /// Full URL after base URL and query parameters are resolved.
    pub url: String,
    /// Operation label.
    pub operation: &'static str,
    /// 1-based request attempt number.
    pub attempt: u32,
    /// Maximum configured retries for this operation.
    pub max_retries: u32,
    /// Caller-provided trace label, when present.
    pub trace_label: Option<String>,
    /// Request headers known to the transport, including injected authorization.
    pub headers: HeaderMap,
    /// Request body bytes, when the operation has a buffered body.
    pub body: Option<Bytes>,
}

impl SensitiveRequestSnapshot {
    pub(crate) fn from_request(
        request: &reqwest::Request,
        operation: &'static str,
        attempt: u32,
        max_retries: u32,
        trace_label: Option<&str>,
    ) -> Self {
        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .map(Bytes::copy_from_slice);
        Self {
            method: request.method().clone(),
            url: request.url().to_string(),
            operation,
            attempt,
            max_retries,
            trace_label: trace_label.map(ToOwned::to_owned),
            headers: request.headers().clone(),
            body,
        }
    }
}

impl fmt::Debug for SensitiveRequestSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = self.body.as_ref().map_or_else(
            || "None".to_owned(),
            |body| format!("Some([{REDACTED}; {} bytes])", body.len()),
        );
        f.debug_struct("SensitiveRequestSnapshot")
            .field("method", &self.method)
            .field(
                "url",
                &format_args!("[{REDACTED}; {} chars]", self.url.len()),
            )
            .field("operation", &self.operation)
            .field("attempt", &self.attempt)
            .field("max_retries", &self.max_retries)
            .field(
                "trace_label",
                &redacted_optional(self.trace_label.is_some()),
            )
            .field(
                "headers",
                &format_args!("[{REDACTED}; {}]", self.headers.len()),
            )
            .field("body", &format_args!("{body}"))
            .finish()
    }
}

/// Response data known to the twilio2 executor after the body is fully read.
#[derive(Clone)]
pub struct SensitiveResponseSnapshot {
    /// HTTP method for the originating request.
    pub method: http::Method,
    /// Full URL for the originating request.
    pub url: String,
    /// Operation label.
    pub operation: &'static str,
    /// 1-based request attempt number.
    pub attempt: u32,
    /// Maximum configured retries for this operation.
    pub max_retries: u32,
    /// Caller-provided trace label, when present.
    pub trace_label: Option<String>,
    /// HTTP response status code.
    pub status: u16,
    /// Raw response headers.
    pub headers: HeaderMap,
    /// Raw response body bytes.
    pub body: Bytes,
}

impl SensitiveResponseSnapshot {
    pub(crate) fn from_raw(request: &SensitiveRequestSnapshot, raw: &RawResponse) -> Self {
        Self {
            method: request.method.clone(),
            url: request.url.clone(),
            operation: request.operation,
            attempt: request.attempt,
            max_retries: request.max_retries,
            trace_label: request.trace_label.clone(),
            status: raw.status,
            headers: raw.headers.clone(),
            body: Bytes::copy_from_slice(&raw.body),
        }
    }
}

impl fmt::Debug for SensitiveResponseSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveResponseSnapshot")
            .field("method", &self.method)
            .field(
                "url",
                &format_args!("[{REDACTED}; {} chars]", self.url.len()),
            )
            .field("operation", &self.operation)
            .field("attempt", &self.attempt)
            .field("max_retries", &self.max_retries)
            .field(
                "trace_label",
                &redacted_optional(self.trace_label.is_some()),
            )
            .field("status", &self.status)
            .field(
                "headers",
                &format_args!("[{REDACTED}; {}]", self.headers.len()),
            )
            .field(
                "body",
                &format_args!("[{REDACTED}; {} bytes]", self.body.len()),
            )
            .finish()
    }
}

/// A transport error plus the request attempt that produced it.
#[derive(Clone)]
pub struct SensitiveTransportErrorSnapshot {
    /// Request attempt associated with the transport failure.
    pub request: SensitiveRequestSnapshot,
    /// Where the failure happened.
    pub stage: SensitiveTransportErrorStage,
    /// Raw unsanitized display string from the transport error.
    pub error: String,
}

impl SensitiveTransportErrorSnapshot {
    pub(crate) fn new(
        request: SensitiveRequestSnapshot,
        stage: SensitiveTransportErrorStage,
        error: String,
    ) -> Self {
        Self {
            request,
            stage,
            error,
        }
    }
}

impl fmt::Debug for SensitiveTransportErrorSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveTransportErrorSnapshot")
            .field("request", &self.request)
            .field("stage", &self.stage)
            .field(
                "error",
                &format_args!("[{REDACTED}; {} chars]", self.error.len()),
            )
            .finish()
    }
}

/// Stage of an HTTP attempt where a transport failure occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveTransportErrorStage {
    /// The executor could not build the transport request.
    BuildRequest,
    /// Sending the request failed before a response could be read.
    Send,
    /// A response existed, but reading the body failed.
    ReadBody,
}

fn redacted_callback(is_some: bool) -> impl fmt::Debug {
    redacted_optional(is_some)
}

fn redacted_optional(is_some: bool) -> impl fmt::Debug {
    struct RedactedOptional(bool);
    impl fmt::Debug for RedactedOptional {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if self.0 {
                f.write_str("Some([REDACTED])")
            } else {
                f.write_str("None")
            }
        }
    }
    RedactedOptional(is_some)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]
mod tests {
    use super::*;

    #[test]
    fn builder_callbacks_receive_matching_events() {
        let all = std::sync::Arc::new(std::sync::Mutex::new(0_u32));
        let requests = std::sync::Arc::new(std::sync::Mutex::new(0_u32));
        let responses = std::sync::Arc::new(std::sync::Mutex::new(0_u32));
        let errors = std::sync::Arc::new(std::sync::Mutex::new(0_u32));

        let diagnostics = SensitiveDiagnostics::builder()
            .on_event({
                let all = all.clone();
                move |_| *all.lock().unwrap() += 1
            })
            .on_request({
                let requests = requests.clone();
                move |_| *requests.lock().unwrap() += 1
            })
            .on_response({
                let responses = responses.clone();
                move |_| *responses.lock().unwrap() += 1
            })
            .on_transport_error({
                let errors = errors.clone();
                move |_| *errors.lock().unwrap() += 1
            })
            .build();

        let request = request_snapshot();
        diagnostics.record(SensitiveDiagnosticEvent::Request(request.clone()));
        diagnostics.record(SensitiveDiagnosticEvent::Response(response_snapshot(
            &request,
        )));
        diagnostics.record(SensitiveDiagnosticEvent::TransportError(
            SensitiveTransportErrorSnapshot::new(
                request,
                SensitiveTransportErrorStage::Send,
                "raw transport secret".to_owned(),
            ),
        ));

        assert_eq!(*all.lock().unwrap(), 3);
        assert_eq!(*requests.lock().unwrap(), 1);
        assert_eq!(*responses.lock().unwrap(), 1);
        assert_eq!(*errors.lock().unwrap(), 1);
    }

    #[test]
    fn debug_output_redacts_sensitive_material() {
        let request = request_snapshot();
        let response = response_snapshot(&request);
        let error = SensitiveTransportErrorSnapshot::new(
            request.clone(),
            SensitiveTransportErrorStage::Send,
            "raw error https://secret.example/path?token=sk-secret".to_owned(),
        );

        for debug in [
            format!("{:?}", SensitiveDiagnostics::noop()),
            format!("{:?}", SensitiveDiagnostics::builder().on_event(|_| {})),
            format!("{request:?}"),
            format!("{response:?}"),
            format!("{error:?}"),
            format!("{:?}", SensitiveDiagnosticEvent::TransportError(error)),
        ] {
            for forbidden in [
                "https://secret.example",
                "Bearer sk-secret",
                "secret-body",
                "secret-response-header",
                "secret-response-body",
                "token=sk-secret",
                "trace-secret",
            ] {
                assert!(
                    !debug.contains(forbidden),
                    "sensitive diagnostics Debug leaked {forbidden:?}: {debug}"
                );
            }
            assert!(debug.contains("REDACTED"));
        }
    }

    fn request_snapshot() -> SensitiveRequestSnapshot {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-secret".parse().unwrap());
        headers.insert("idempotency-key", "idem-secret".parse().unwrap());
        SensitiveRequestSnapshot {
            method: http::Method::POST,
            url: "https://secret.example/Messages.json?token=sk-secret".to_owned(),
            operation: "messages.create",
            attempt: 1,
            max_retries: 2,
            trace_label: Some("trace-secret".to_owned()),
            headers,
            body: Some(Bytes::from_static(b"secret-body")),
        }
    }

    fn response_snapshot(request: &SensitiveRequestSnapshot) -> SensitiveResponseSnapshot {
        let mut headers = HeaderMap::new();
        headers.insert("x-secret", "secret-response-header".parse().unwrap());
        SensitiveResponseSnapshot {
            method: request.method.clone(),
            url: request.url.clone(),
            operation: request.operation,
            attempt: request.attempt,
            max_retries: request.max_retries,
            trace_label: request.trace_label.clone(),
            status: 200,
            headers,
            body: Bytes::from_static(b"secret-response-body"),
        }
    }
}
