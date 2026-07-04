//! Feature-gated raw diagnostics contract tests.

#![cfg(feature = "sensitive-diagnostics")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Method;
use twilio2::{
    ApiFamily, Operation, RawResponse, RequestOptions, RequestSpec, RetryPolicy,
    SensitiveDiagnosticEvent, SensitiveDiagnostics, SensitiveTransportErrorStage, TwilioClient,
    TwilioClientConfig, TwilioError,
};

use support::{
    HttpsMockServer, MockResponse, test_creds, test_http_client, twilio_config,
    unused_https_base_url,
};

const SAFE_TRACE_LABEL: &str = "job-42";
const MESSAGE_BODY: &str = "body secret";
const PHONE_SECRET: &str = "+15551234567";
const RESPONSE_SECRET: &str = "response secret";

type EventCapture = Arc<Mutex<Vec<SensitiveDiagnosticEvent>>>;

fn capture() -> (SensitiveDiagnostics, EventCapture) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&events);
    let diagnostics = SensitiveDiagnostics::new(move |event| {
        sink_events.lock().unwrap().push(event);
    });
    (diagnostics, events)
}

fn captured(events: &EventCapture) -> Vec<SensitiveDiagnosticEvent> {
    events.lock().unwrap().clone()
}

fn client_with(base_url: &str, diagnostics: SensitiveDiagnostics) -> TwilioClient {
    TwilioClient::from_config_and_http_client(
        TwilioClientConfig::new()
            .base_urls(twilio_config(base_url))
            .with_sensitive_diagnostics(diagnostics),
        test_http_client(),
    )
    .unwrap()
}

#[derive(Clone, Copy)]
struct GetJsonOperation;

impl Operation for GetJsonOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::GET,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation("test.messages.list"))
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
struct PostFormOperation;

impl Operation for PostFormOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::POST,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation("test.messages.create")
        .form_param("Body", MESSAGE_BODY)
        .form_param("To", PHONE_SECRET))
    }

    fn sensitive_values(&self) -> Vec<String> {
        vec![MESSAGE_BODY.to_owned(), PHONE_SECRET.to_owned()]
    }

    fn decode(
        &self,
        raw: RawResponse,
        sensitive_values: &[&str],
    ) -> Result<Self::Output, TwilioError> {
        twilio2::decode_json_response(&raw, sensitive_values)
    }
}

fn assert_header(headers: &http::HeaderMap, name: &str, expected: &str) {
    let value = headers
        .get(name)
        .unwrap_or_else(|| panic!("missing header {name}: {headers:#?}"))
        .to_str()
        .unwrap();
    assert_eq!(value, expected);
}

#[tokio::test(flavor = "current_thread")]
async fn client_default_sink_captures_request_and_response() {
    let server = HttpsMockServer::start(vec![MockResponse::json(r#"{"ok":true}"#)]).await;
    let (diagnostics, events) = capture();
    let client = client_with(&server.base_url, diagnostics);

    client
        .account(test_creds())
        .send_with_options(
            GetJsonOperation,
            RequestOptions::new().trace_label(SAFE_TRACE_LABEL),
        )
        .await
        .unwrap();

    let events = captured(&events);
    assert_eq!(events.len(), 2, "{events:#?}");
    let SensitiveDiagnosticEvent::Request(request) = &events[0] else {
        panic!("expected request event: {events:#?}");
    };
    assert_eq!(request.method, http::Method::GET);
    assert_eq!(
        request.url,
        format!(
            "{}/2010-04-01/Accounts/AC123/Messages.json",
            server.base_url
        )
    );
    assert_eq!(request.operation, "test.messages.list");
    assert_eq!(request.attempt, 1);
    assert_eq!(request.max_retries, 0);
    assert_eq!(request.trace_label.as_deref(), Some(SAFE_TRACE_LABEL));
    assert_header(&request.headers, "authorization", "Basic QUMxMjM6dG9rZW4=");
    assert!(request.body.is_none());

    let SensitiveDiagnosticEvent::Response(response) = &events[1] else {
        panic!("expected response event: {events:#?}");
    };
    assert_eq!(response.method, http::Method::GET);
    assert_eq!(response.url, request.url);
    assert_eq!(response.status, 200);
    assert_eq!(response.body.as_ref(), br#"{"ok":true}"#);

    let debug = format!("{events:#?}");
    assert!(!debug.contains(&server.base_url), "{debug}");
    assert!(!debug.contains("token"), "{debug}");
    assert!(debug.contains("REDACTED") || debug.contains("redacted"));
}

#[tokio::test(flavor = "current_thread")]
async fn request_override_wins_and_noop_disables_client_default() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(r#"{"created":true}"#).header("x-sensitive-response", RESPONSE_SECRET),
        MockResponse::json(r#"{"ok":true}"#),
    ])
    .await;
    let (client_diagnostics, client_events) = capture();
    let (request_diagnostics, request_events) = capture();
    let client = client_with(&server.base_url, client_diagnostics);

    client
        .account(test_creds())
        .send_with_options(
            PostFormOperation,
            RequestOptions::new()
                .trace_label(SAFE_TRACE_LABEL)
                .sensitive_diagnostics(request_diagnostics),
        )
        .await
        .unwrap();

    assert!(captured(&client_events).is_empty());
    let request_events = captured(&request_events);
    assert_eq!(request_events.len(), 2, "{request_events:#?}");
    let SensitiveDiagnosticEvent::Request(request) = &request_events[0] else {
        panic!("expected request event: {request_events:#?}");
    };
    assert_eq!(request.method, http::Method::POST);
    assert_eq!(request.trace_label.as_deref(), Some(SAFE_TRACE_LABEL));
    assert_header(&request.headers, "authorization", "Basic QUMxMjM6dG9rZW4=");
    let body = std::str::from_utf8(request.body.as_ref().expect("request body")).unwrap();
    assert!(body.contains("Body=body+secret"), "{body}");
    assert!(body.contains("To=%2B15551234567"), "{body}");

    let SensitiveDiagnosticEvent::Response(response) = &request_events[1] else {
        panic!("expected response event: {request_events:#?}");
    };
    assert_eq!(response.status, 200);
    assert_eq!(
        response
            .headers
            .get("x-sensitive-response")
            .unwrap()
            .to_str()
            .unwrap(),
        RESPONSE_SECRET
    );

    client
        .account(test_creds())
        .send_with_options(
            GetJsonOperation,
            RequestOptions::new().sensitive_diagnostics(SensitiveDiagnostics::noop()),
        )
        .await
        .unwrap();
    assert!(captured(&client_events).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn retry_attempts_emit_request_and_response_snapshots() {
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(503, r#"{"message":"unavailable"}"#),
        MockResponse::json(r#"{"ok":true}"#),
    ])
    .await;
    let (diagnostics, events) = capture();
    let client = client_with(&server.base_url, diagnostics);

    client
        .account(test_creds())
        .send_with_options(
            GetJsonOperation,
            RequestOptions::new().retry(
                RetryPolicy::none()
                    .with_max_retries(1)
                    .with_base_delay(Duration::from_millis(0))
                    .with_jitter(false),
            ),
        )
        .await
        .unwrap();

    let events = captured(&events);
    assert_eq!(events.len(), 4, "{events:#?}");
    for (event, expected_attempt, expected_status) in [
        (&events[0], 1, None),
        (&events[1], 1, Some(503)),
        (&events[2], 2, None),
        (&events[3], 2, Some(200)),
    ] {
        match (event, expected_status) {
            (SensitiveDiagnosticEvent::Request(request), None) => {
                assert_eq!(request.attempt, expected_attempt);
                assert_eq!(request.max_retries, 1);
            }
            (SensitiveDiagnosticEvent::Response(response), Some(status)) => {
                assert_eq!(response.attempt, expected_attempt);
                assert_eq!(response.max_retries, 1);
                assert_eq!(response.status, status);
            }
            _ => panic!("unexpected event ordering: {events:#?}"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn large_api_error_captures_full_raw_response_but_returns_capped_public_error() {
    let tail = "tail-sensitive";
    let body = format!("{}{tail}", "x".repeat(10_000));
    let server = HttpsMockServer::start(vec![MockResponse::status_json(500, body.clone())]).await;
    let (diagnostics, events) = capture();
    let client = client_with(&server.base_url, diagnostics);

    let err = client
        .account(test_creds())
        .send_with_options(GetJsonOperation, RequestOptions::new())
        .await
        .unwrap_err();

    let TwilioError::Api {
        status,
        body: public_body,
    } = err
    else {
        panic!("expected API error");
    };
    assert_eq!(status, 500);
    assert!(public_body.len() <= 2051, "{public_body}");
    assert!(public_body.ends_with('…'), "{public_body}");
    assert!(!public_body.contains(tail), "{public_body}");

    let events = captured(&events);
    assert_eq!(events.len(), 2, "{events:#?}");
    let SensitiveDiagnosticEvent::Response(response) = &events[1] else {
        panic!("expected response event: {events:#?}");
    };
    assert_eq!(response.status, 500);
    assert_eq!(response.body.as_ref(), body.as_bytes());
    assert!(std::str::from_utf8(&response.body).unwrap().contains(tail));
}

#[tokio::test(flavor = "current_thread")]
async fn transport_failure_emits_raw_sensitive_event_but_returns_scrubbed_error() {
    let base_url = unused_https_base_url().await;
    let (diagnostics, events) = capture();
    let client = client_with(&base_url, diagnostics);

    let err = client
        .account(test_creds())
        .send_with_options(GetJsonOperation, RequestOptions::new())
        .await
        .unwrap_err();

    let events = captured(&events);
    assert_eq!(events.len(), 2, "{events:#?}");
    let SensitiveDiagnosticEvent::Request(request) = &events[0] else {
        panic!("expected request event: {events:#?}");
    };
    let SensitiveDiagnosticEvent::TransportError(error) = &events[1] else {
        panic!("expected transport error event: {events:#?}");
    };
    assert_eq!(error.stage, SensitiveTransportErrorStage::Send);
    assert_eq!(error.request.url, request.url);
    assert!(!error.error.is_empty(), "{error:#?}");
    assert!(error.error.contains("127.0.0.1"), "{error:#?}");

    let message = err.to_string();
    assert!(!message.contains("token"), "{message}");
    assert!(!message.contains("AC123"), "{message}");
}
