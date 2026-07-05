//! Structured tracing contract tests for the Twilio HTTP executor.

#![cfg(any(feature = "async", feature = "sync"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod support;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::Method;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
#[cfg(feature = "sync")]
use twilio2::BlockingTwilioClient;
#[cfg(feature = "async")]
use twilio2::TwilioClient;
use twilio2::{
    ApiFamily, Operation, RawResponse, RequestOptions, RequestSpec, RetryPolicy, TwilioError,
};

use support::{HttpsMockServer, MockResponse, test_creds};
#[cfg(feature = "sync")]
use support::{blocking_client_for, test_agent};
#[cfg(feature = "async")]
use support::{client_for, unused_https_base_url};

const TRACE_TARGET: &str = "twilio2::trace";
const SAFE_TRACE_LABEL: &str = "job-42";
const SENSITIVE_TRACE_LABEL: &str = "trace-secret-label";
const SENSITIVE_RESPONSE: &str = "+15551234567";

#[derive(Clone, Debug)]
struct CapturedEvent {
    level: String,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct CapturedSpan {
    name: String,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
struct TraceCapture {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
    spans: Arc<Mutex<Vec<CapturedSpan>>>,
}

impl TraceCapture {
    fn events_named(&self, name: &str) -> Vec<CapturedEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.field("event") == name)
            .cloned()
            .collect()
    }

    fn one_event(&self, name: &str) -> CapturedEvent {
        let events = self.events_named(name);
        assert_eq!(events.len(), 1, "expected exactly one {name}: {events:#?}");
        events.into_iter().next().unwrap()
    }

    fn spans_named(&self, name: &str) -> Vec<CapturedSpan> {
        self.spans
            .lock()
            .unwrap()
            .iter()
            .filter(|span| span.name == name)
            .cloned()
            .collect()
    }

    fn dump(&self) -> String {
        format!(
            "events={:#?}\nspans={:#?}",
            self.events.lock().unwrap(),
            self.spans.lock().unwrap()
        )
    }
}

impl CapturedEvent {
    fn field(&self, key: &str) -> &str {
        self.fields.get(key).map_or_else(
            || panic!("missing event field {key}: {self:#?}"),
            String::as_str,
        )
    }

    fn assert_level(&self, level: &str) {
        assert_eq!(self.level, level, "{self:#?}");
    }

    fn assert_elapsed(&self) {
        self.field("elapsed_ms").parse::<u64>().unwrap();
    }
}

impl CapturedSpan {
    fn field(&self, key: &str) -> &str {
        self.fields.get(key).map_or_else(
            || panic!("missing span field {key}: {self:#?}"),
            String::as_str,
        )
    }

    fn assert_elapsed(&self) {
        self.field("elapsed_ms").parse::<u64>().unwrap();
    }
}

struct CaptureLayer {
    capture: TraceCapture,
}

#[derive(Clone, Debug)]
struct SpanState {
    name: String,
    target: String,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if attrs.metadata().target() != TRACE_TARGET {
            return;
        }

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanState {
                name: attrs.metadata().name().to_owned(),
                target: attrs.metadata().target().to_owned(),
                fields: visitor.fields,
            });
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(state) = extensions.get_mut::<SpanState>() else {
            return;
        };

        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        state.fields.extend(visitor.fields);
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != TRACE_TARGET {
            return;
        }

        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.capture.events.lock().unwrap().push(CapturedEvent {
            level: event.metadata().level().to_string(),
            fields: visitor.fields,
        });
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let extensions = span.extensions();
        let Some(state) = extensions.get::<SpanState>() else {
            return;
        };
        if state.target == TRACE_TARGET {
            self.capture.spans.lock().unwrap().push(CapturedSpan {
                name: state.name.clone(),
                fields: state.fields.clone(),
            });
        }
    }
}

fn capture_traces() -> (TraceCapture, impl Drop) {
    let capture = TraceCapture::default();
    let subscriber = tracing_subscriber::registry().with(CaptureLayer {
        capture: capture.clone(),
    });
    let guard = tracing::subscriber::set_default(subscriber);
    (capture, guard)
}

#[cfg(feature = "sync")]
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

#[cfg(feature = "sync")]
fn start_server(
    runtime: &tokio::runtime::Runtime,
    responses: Vec<MockResponse>,
) -> HttpsMockServer {
    runtime.block_on(HttpsMockServer::start(responses))
}

#[derive(Clone, Copy)]
struct JsonOperation {
    operation: &'static str,
}

impl JsonOperation {
    fn new(operation: &'static str) -> Self {
        Self { operation }
    }
}

impl Operation for JsonOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::GET,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation(self.operation))
    }

    fn sensitive_values(&self) -> Vec<String> {
        vec![
            SENSITIVE_TRACE_LABEL.to_owned(),
            SENSITIVE_RESPONSE.to_owned(),
        ]
    }

    fn decode(
        &self,
        raw: RawResponse,
        sensitive_values: &[&str],
    ) -> Result<Self::Output, TwilioError> {
        twilio2::decode_json_response(&raw, sensitive_values)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
struct InvalidRequestOperation;

#[cfg(feature = "async")]
impl Operation for InvalidRequestOperation {
    type Output = serde_json::Value;

    fn request(&self, _account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Err(TwilioError::InvalidRequest(
            "operation state is invalid".to_owned(),
        ))
    }

    fn sensitive_values(&self) -> Vec<String> {
        vec![SENSITIVE_TRACE_LABEL.to_owned()]
    }

    fn decode(
        &self,
        raw: RawResponse,
        sensitive_values: &[&str],
    ) -> Result<Self::Output, TwilioError> {
        twilio2::decode_json_response(&raw, sensitive_values)
    }
}

fn assert_success_capture(capture: &TraceCapture, max_retries: &str, attempts: &str) {
    let attempt = capture.one_event("twilio2.request.attempt.response");
    attempt.assert_level("DEBUG");
    assert_eq!(attempt.field("method"), "GET");
    assert_eq!(attempt.field("operation"), "test.messages.list");
    assert_eq!(attempt.field("attempt"), attempts);
    assert_eq!(attempt.field("max_retries"), max_retries);
    assert_eq!(attempt.field("status"), "200");
    attempt.assert_elapsed();

    let success = capture.one_event("twilio2.operation.success");
    success.assert_level("DEBUG");
    assert_eq!(success.field("method"), "GET");
    assert_eq!(success.field("operation"), "test.messages.list");
    assert_eq!(success.field("attempts"), attempts);
    assert_eq!(success.field("max_retries"), max_retries);
    assert_eq!(success.field("status"), "200");
    success.assert_elapsed();
}

fn assert_single_span(capture: &TraceCapture, trace_label: Option<&str>) {
    let spans = capture.spans_named("twilio2.request");
    assert_eq!(spans.len(), 1, "{spans:#?}");
    let span = &spans[0];
    assert_eq!(span.field("method"), "GET");
    assert_eq!(span.field("operation"), "test.messages.list");
    assert_eq!(span.field("attempt"), "1");
    assert_eq!(span.field("status"), "200");
    span.assert_elapsed();
    match trace_label {
        Some(trace_label) => assert_eq!(span.field("trace_label"), trace_label),
        None => assert!(!span.fields.contains_key("trace_label")),
    }
}

fn assert_redacted(capture: &TraceCapture, forbidden: &[&str]) {
    let dump = capture.dump();
    for forbidden in forbidden {
        assert!(
            !dump.contains(forbidden),
            "trace leaked {forbidden:?}:\n{dump}"
        );
    }
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn success_emits_attempt_span_and_operation_success() {
    let server = HttpsMockServer::start(vec![MockResponse::json(r#"{"ok":true}"#)]).await;
    let (capture, _guard) = capture_traces();

    client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new(),
        )
        .await
        .unwrap();

    assert_success_capture(&capture, "0", "1");
    assert_single_span(&capture, None);
    assert_redacted(&capture, &[&server.base_url, "token", "AC123"]);
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn safe_trace_label_is_emitted_and_sensitive_label_is_omitted() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(r#"{"ok":true}"#),
        MockResponse::json(r#"{"ok":true}"#),
    ])
    .await;

    let (safe_capture, safe_guard) = capture_traces();
    client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().trace_label(SAFE_TRACE_LABEL),
        )
        .await
        .unwrap();
    assert_eq!(
        safe_capture
            .one_event("twilio2.request.attempt.response")
            .field("trace_label"),
        SAFE_TRACE_LABEL
    );
    assert_eq!(
        safe_capture
            .one_event("twilio2.operation.success")
            .field("trace_label"),
        SAFE_TRACE_LABEL
    );
    assert_single_span(&safe_capture, Some(SAFE_TRACE_LABEL));

    drop(safe_guard);
    let (sensitive_capture, _sensitive_guard) = capture_traces();
    client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().trace_label(SENSITIVE_TRACE_LABEL),
        )
        .await
        .unwrap();
    assert!(
        !sensitive_capture
            .one_event("twilio2.request.attempt.response")
            .fields
            .contains_key("trace_label")
    );
    assert!(
        !sensitive_capture
            .one_event("twilio2.operation.success")
            .fields
            .contains_key("trace_label")
    );
    assert_single_span(&sensitive_capture, None);
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn api_failure_emits_attempt_response_and_operation_failure() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        400,
        format!(r#"{{"message":"bad {SENSITIVE_RESPONSE}"}}"#),
    )])
    .await;
    let (capture, _guard) = capture_traces();

    let err = client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, TwilioError::Api { status: 400, .. }));
    let attempt = capture.one_event("twilio2.request.attempt.response");
    assert_eq!(attempt.field("status"), "400");
    let failure = capture.one_event("twilio2.operation.failure");
    failure.assert_level("WARN");
    assert_eq!(failure.field("method"), "GET");
    assert_eq!(failure.field("operation"), "test.messages.list");
    assert_eq!(failure.field("attempts"), "1");
    assert_eq!(failure.field("status"), "400");
    assert_eq!(failure.field("error_kind"), "api");
    assert_redacted(&capture, &[&server.base_url, SENSITIVE_RESPONSE, "token"]);
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn malformed_2xx_decode_failure_emits_operation_failure() {
    let server = HttpsMockServer::start(vec![MockResponse::json(r#"{"ok":"#)]).await;
    let (capture, _guard) = capture_traces();

    let err = client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, TwilioError::Decode(_)));
    assert_eq!(
        capture
            .one_event("twilio2.request.attempt.response")
            .field("status"),
        "200"
    );
    let failure = capture.one_event("twilio2.operation.failure");
    assert_eq!(failure.field("status"), "200");
    assert_eq!(failure.field("error_kind"), "decode");
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn retry_event_links_transient_failure_to_success() {
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(503, r#"{"message":"unavailable"}"#),
        MockResponse::json(r#"{"ok":true}"#),
    ])
    .await;
    let (capture, _guard) = capture_traces();

    client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().retry(
                RetryPolicy::none()
                    .with_max_retries(1)
                    .with_base_delay(Duration::from_millis(0))
                    .with_jitter(false),
            ),
        )
        .await
        .unwrap();

    let attempts = capture.events_named("twilio2.request.attempt.response");
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    assert_eq!(attempts[0].field("attempt"), "1");
    assert_eq!(attempts[0].field("status"), "503");
    assert_eq!(attempts[1].field("attempt"), "2");
    assert_eq!(attempts[1].field("status"), "200");

    let retry = capture.one_event("twilio2.request.retry");
    retry.assert_level("WARN");
    assert_eq!(retry.field("method"), "GET");
    assert_eq!(retry.field("attempt"), "1");
    assert_eq!(retry.field("next_attempt"), "2");
    assert_eq!(retry.field("max_retries"), "1");
    assert_eq!(retry.field("delay_ms"), "0");
    assert_eq!(retry.field("delay_source"), "backoff");
    assert_eq!(retry.field("error_kind"), "api");
    assert_eq!(retry.field("status"), "503");

    let success = capture.one_event("twilio2.operation.success");
    assert_eq!(success.field("attempts"), "2");
    assert_eq!(success.field("max_retries"), "1");
    assert_eq!(success.field("status"), "200");
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn transport_failure_is_redacted_and_structured() {
    let base_url = unused_https_base_url().await;
    let client = TwilioClient::try_with_config(
        support::test_http_client(),
        support::twilio_config(&base_url),
    )
    .unwrap();
    let (capture, _guard) = capture_traces();

    let err = client
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().trace_label(SAFE_TRACE_LABEL).retry(
                RetryPolicy::none()
                    .with_max_retries(1)
                    .with_base_delay(Duration::from_millis(0))
                    .with_jitter(false),
            ),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, TwilioError::Transport(_)));
    let attempts = capture.events_named("twilio2.request.attempt.error");
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    for (i, event) in attempts.iter().enumerate() {
        event.assert_level("WARN");
        assert_eq!(event.field("method"), "GET");
        assert_eq!(event.field("attempt"), &(i + 1).to_string());
        assert_eq!(event.field("error_kind"), "transport");
        assert_eq!(event.field("trace_label"), SAFE_TRACE_LABEL);
    }
    let retry = capture.one_event("twilio2.request.retry");
    assert_eq!(retry.field("error_kind"), "transport");
    assert_eq!(retry.field("trace_label"), SAFE_TRACE_LABEL);
    let failure = capture.one_event("twilio2.operation.failure");
    assert_eq!(failure.field("attempts"), "2");
    assert_eq!(failure.field("error_kind"), "transport");
    assert_eq!(failure.field("trace_label"), SAFE_TRACE_LABEL);
    assert_redacted(&capture, &[&base_url, "token", "AC123"]);
}

#[cfg(feature = "async")]
#[tokio::test(flavor = "current_thread")]
async fn request_build_failure_omits_sensitive_trace_label() {
    let (capture, _guard) = capture_traces();
    let server = HttpsMockServer::start(Vec::new()).await;

    let err = client_for(&server)
        .account(test_creds())
        .send_with_options(
            InvalidRequestOperation,
            RequestOptions::new().trace_label(SENSITIVE_TRACE_LABEL),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, TwilioError::InvalidRequest(_)));
    let failure = capture.one_event("twilio2.operation.failure");
    assert_eq!(failure.field("method"), "UNKNOWN");
    assert_eq!(failure.field("attempts"), "0");
    assert_eq!(failure.field("error_kind"), "invalid_request");
    assert!(!failure.fields.contains_key("trace_label"));
}

#[cfg(feature = "sync")]
#[test]
fn sync_success_emits_attempt_span_and_operation_success() {
    let runtime = runtime();
    let server = start_server(&runtime, vec![MockResponse::json(r#"{"ok":true}"#)]);
    let (capture, _guard) = capture_traces();

    blocking_client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new(),
        )
        .unwrap();

    assert_success_capture(&capture, "0", "1");
    assert_single_span(&capture, None);
    assert_redacted(&capture, &[&server.base_url, "token", "AC123"]);
}

#[cfg(feature = "sync")]
#[test]
fn sync_api_failure_emits_attempt_response_and_operation_failure() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![MockResponse::status_json(
            400,
            format!(r#"{{"message":"bad {SENSITIVE_RESPONSE}"}}"#),
        )],
    );
    let (capture, _guard) = capture_traces();

    let err = blocking_client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new(),
        )
        .unwrap_err();

    assert!(matches!(err, TwilioError::Api { status: 400, .. }));
    let attempt = capture.one_event("twilio2.request.attempt.response");
    assert_eq!(attempt.field("status"), "400");
    let failure = capture.one_event("twilio2.operation.failure");
    failure.assert_level("WARN");
    assert_eq!(failure.field("method"), "GET");
    assert_eq!(failure.field("operation"), "test.messages.list");
    assert_eq!(failure.field("attempts"), "1");
    assert_eq!(failure.field("status"), "400");
    assert_eq!(failure.field("error_kind"), "api");
    assert_redacted(&capture, &[&server.base_url, SENSITIVE_RESPONSE, "token"]);
}

#[cfg(feature = "sync")]
#[test]
fn sync_retry_event_links_transient_failure_to_success() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![
            MockResponse::status_json(503, r#"{"message":"unavailable"}"#),
            MockResponse::json(r#"{"ok":true}"#),
        ],
    );
    let (capture, _guard) = capture_traces();

    blocking_client_for(&server)
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().retry(
                RetryPolicy::none()
                    .with_max_retries(1)
                    .with_base_delay(Duration::from_millis(0))
                    .with_jitter(false),
            ),
        )
        .unwrap();

    let attempts = capture.events_named("twilio2.request.attempt.response");
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    assert_eq!(attempts[0].field("attempt"), "1");
    assert_eq!(attempts[0].field("status"), "503");
    assert_eq!(attempts[1].field("attempt"), "2");
    assert_eq!(attempts[1].field("status"), "200");

    let retry = capture.one_event("twilio2.request.retry");
    retry.assert_level("WARN");
    assert_eq!(retry.field("method"), "GET");
    assert_eq!(retry.field("attempt"), "1");
    assert_eq!(retry.field("next_attempt"), "2");
    assert_eq!(retry.field("max_retries"), "1");
    assert_eq!(retry.field("delay_ms"), "0");
    assert_eq!(retry.field("delay_source"), "backoff");
    assert_eq!(retry.field("error_kind"), "api");
    assert_eq!(retry.field("status"), "503");

    let success = capture.one_event("twilio2.operation.success");
    assert_eq!(success.field("attempts"), "2");
    assert_eq!(success.field("max_retries"), "1");
    assert_eq!(success.field("status"), "200");
}

#[cfg(feature = "sync")]
#[test]
fn sync_transport_failure_is_redacted_and_structured() {
    let runtime = runtime();
    let base_url = runtime.block_on(support::unused_https_base_url());
    let client =
        BlockingTwilioClient::try_with_config(test_agent(), support::twilio_config(&base_url))
            .unwrap();
    let (capture, _guard) = capture_traces();

    let err = client
        .account(test_creds())
        .send_with_options(
            JsonOperation::new("test.messages.list"),
            RequestOptions::new().trace_label(SAFE_TRACE_LABEL).retry(
                RetryPolicy::none()
                    .with_max_retries(1)
                    .with_base_delay(Duration::from_millis(0))
                    .with_jitter(false),
            ),
        )
        .unwrap_err();

    assert!(matches!(err, TwilioError::Transport(_)));
    let attempts = capture.events_named("twilio2.request.attempt.error");
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    for (i, event) in attempts.iter().enumerate() {
        event.assert_level("WARN");
        assert_eq!(event.field("method"), "GET");
        assert_eq!(event.field("attempt"), &(i + 1).to_string());
        assert_eq!(event.field("error_kind"), "transport");
        assert_eq!(event.field("trace_label"), SAFE_TRACE_LABEL);
    }
    let retry = capture.one_event("twilio2.request.retry");
    assert_eq!(retry.field("error_kind"), "transport");
    assert_eq!(retry.field("trace_label"), SAFE_TRACE_LABEL);
    let failure = capture.one_event("twilio2.operation.failure");
    assert_eq!(failure.field("attempts"), "2");
    assert_eq!(failure.field("error_kind"), "transport");
    assert_eq!(failure.field("trace_label"), SAFE_TRACE_LABEL);
    assert_redacted(&capture, &[&base_url, "token", "AC123"]);
}
