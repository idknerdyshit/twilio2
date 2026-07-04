#![allow(clippy::unwrap_used, clippy::missing_panics_doc)]

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::CertifiedKey;
use reqwest::Method;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use twilio2::{
    AddressRetention, ApiFamily, ContentRetention, CreateAlphaSenderRequest,
    CreateChannelSenderRequest, CreateDestinationAlphaSenderRequest, CreateMessageFeedbackRequest,
    CreateMessageRequest, CreateServicePhoneNumberRequest, CreateServiceRequest,
    CreateServiceShortCodeRequest, CreateTollfreeVerificationRequest, FetchDeactivationsRequest,
    HttpMethod, ListAccountShortCodesRequest, ListDestinationAlphaSendersRequest, ListMediaRequest,
    ListMessagesRequest, ListServiceSubresourcesRequest, ListServicesRequest,
    ListTollfreeVerificationsRequest, MessageFeedbackOutcome, Operation, RawResponse,
    RequestOptions, RequestSpec, RetryPolicy, RiskCheck, ScanMessageContent, ScheduleType,
    ServiceUsecase, TollfreeBusinessRegistrationAuthority, TollfreeBusinessType,
    TollfreeMessageVolume, TollfreeOptInType, TollfreeUseCaseCategory, TollfreeVerificationStatus,
    TollfreeVettingProvider, TrafficType, TwilioAccountShortCode, TwilioAccountShortCodePage,
    TwilioAlphaSender, TwilioAlphaSenderPage, TwilioChannelSender, TwilioChannelSenderPage,
    TwilioClient, TwilioClientConfig, TwilioConfig, TwilioCreds, TwilioDeactivation,
    TwilioDestinationAlphaSender, TwilioDestinationAlphaSenderPage, TwilioError, TwilioMedia,
    TwilioMediaPage, TwilioMessage, TwilioMessageFeedback, TwilioMessagePage, TwilioServicePage,
    TwilioServicePhoneNumber, TwilioServicePhoneNumberPage, TwilioServiceShortCode,
    TwilioServiceShortCodePage, TwilioTollfreeVerification, TwilioTollfreeVerificationPage,
    UpdateAccountShortCodeRequest, UpdateMessageRequest, UpdateServiceRequest,
    UpdateTollfreeVerificationRequest, V1PageMeta,
};

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: Vec<u8>,
    content_type: String,
    content_length: Option<usize>,
    headers: Vec<(String, String)>,
}

impl MockResponse {
    fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    fn created_json(body: impl Into<String>) -> Self {
        Self {
            status: 201,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    fn status_json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    fn bytes(content_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            content_type: content_type.into(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    fn no_content() -> Self {
        Self {
            status: 204,
            body: Vec::new(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn truncated(status: u16, body: impl Into<String>, content_length: usize) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: Some(content_length),
            headers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl RecordedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct HttpsMockServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl HttpsMockServer {
    async fn start(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("https://{addr}");
        let acceptor = tls_acceptor();
        let expected_requests = responses.len();
        let responses = responses
            .into_iter()
            .map(|mut response| {
                let body =
                    String::from_utf8_lossy(&response.body).replace("__BASE_URL__", &base_url);
                response.body = body.into_bytes();
                response
            })
            .collect::<VecDeque<_>>();
        let responses = Arc::new(Mutex::new(responses));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let task_responses = Arc::clone(&responses);
        let task_requests = Arc::clone(&requests);

        tokio::spawn(async move {
            for _ in 0..expected_requests {
                let (stream, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let responses = Arc::clone(&task_responses);
                let requests = Arc::clone(&task_requests);

                tokio::spawn(async move {
                    let mut stream = acceptor.accept(stream).await.unwrap();
                    let request = read_http_request(&mut stream).await.unwrap();
                    let response = {
                        let mut responses = responses.lock().unwrap();
                        responses.pop_front().unwrap()
                    };
                    {
                        let mut requests = requests.lock().unwrap();
                        requests.push(request);
                    }
                    write_http_response(&mut stream, response).await.unwrap();
                });
            }
        });

        Self { base_url, requests }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn tls_acceptor() -> TlsAcceptor {
    install_test_crypto_provider();

    let CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
            .unwrap();
    let cert_chain = vec![cert.der().clone()];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .unwrap();

    TlsAcceptor::from(Arc::new(config))
}

async fn read_http_request<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> std::io::Result<RecordedRequest> {
    let mut raw = Vec::new();
    let mut chunk = [0; 1024];
    while header_end(&raw).is_none() {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
    }

    let header_end = header_end(&raw).unwrap();
    let header_text = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_owned();
    let path = request_parts.next().unwrap().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
    let mut body = raw[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    Ok(RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_http_response<S: AsyncWrite + Unpin>(
    stream: &mut S,
    response: MockResponse,
) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        307 => "Temporary Redirect",
        _ => "Error",
    };
    let content_length = response.content_length.unwrap_or(response.body.len());
    let mut headers = format!(
        "HTTP/1.1 {} {reason}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n",
        response.status, response.content_type, content_length
    );
    for (name, value) in &response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

fn test_http_client() -> reqwest::Client {
    install_test_crypto_provider();

    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .unwrap()
}

#[cfg(feature = "rustls-no-provider")]
fn install_test_crypto_provider() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[cfg(not(feature = "rustls-no-provider"))]
fn install_test_crypto_provider() {}

fn test_creds() -> TwilioCreds<'static> {
    TwilioCreds {
        account_sid: "AC123",
        auth_token: "token",
    }
}

fn client_for(server: &HttpsMockServer) -> TwilioClient {
    TwilioClient::try_with_config(
        test_http_client(),
        TwilioConfig::new()
            .rest_base_url(&server.base_url)
            .messaging_base_url(format!("{}/v1", server.base_url)),
    )
    .unwrap()
}

fn assert_basic_auth(request: &RecordedRequest) {
    assert_eq!(
        request.header("authorization"),
        Some("Basic QUMxMjM6dG9rZW4=")
    );
}

fn assert_invalid_request(err: TwilioError, expected: &str) {
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains(expected)
    ));
}

fn assert_decode_error(err: &TwilioError) {
    assert!(matches!(err, TwilioError::Decode(_)));
}

fn assert_api_error_redacted(err: TwilioError, expected_status: u16, leaked: &[&str]) {
    match err {
        TwilioError::Api { status, body } => {
            assert_eq!(status, expected_status);
            for value in leaked {
                assert!(
                    !body.contains(value),
                    "API error body leaked {value:?}: {body}"
                );
            }
        }
        other => assert!(
            matches!(other, TwilioError::Api { .. }),
            "expected API error"
        ),
    }
}

fn assert_debug_redacts(value: &impl std::fmt::Debug, leaked: &[&str]) {
    let rendered = format!("{value:?}");
    for value in leaked {
        assert!(
            !rendered.contains(value),
            "Debug output leaked {value:?}: {rendered}"
        );
    }
    assert!(
        rendered.contains("<redacted>"),
        "Debug output did not include redaction marker: {rendered}"
    );
}

#[test]
fn constructors_config_and_debug_are_ergonomic_and_redacted() {
    let config = TwilioClientConfig::new()
        .rest_base_url("https://proxy.example.test/rest")
        .messaging_base_url("https://proxy.example.test/messaging/v1")
        .timeout(Duration::from_secs(7))
        .user_agent("test-agent/1.0");

    let client =
        TwilioClient::from_config_and_http_client(config.clone(), test_http_client()).unwrap();
    let retained = client.config();
    assert_eq!(retained.rest_base_url, "https://proxy.example.test/rest");
    assert_eq!(
        retained.messaging_base_url,
        "https://proxy.example.test/messaging/v1"
    );
    assert_debug_redacts(&config, &["proxy.example.test"]);
    assert_debug_redacts(&retained, &["proxy.example.test"]);

    let default = TwilioClient::from_http_client(test_http_client());
    assert_eq!(
        default.config().rest_base_url,
        twilio2::DEFAULT_REST_BASE_URL
    );
}

#[derive(Clone, Copy)]
struct MessagesValueOperation;

impl Operation for MessagesValueOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::GET,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation("custom.messages")
        .query("PageSize", "1"))
    }

    fn sensitive_values(&self) -> Vec<String> {
        vec!["trace-secret".to_owned()]
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
struct UnsafePostOperation;

impl Operation for UnsafePostOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::POST,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation("custom.post")
        .form_param("Body", "hello"))
    }

    fn decode(
        &self,
        raw: RawResponse,
        sensitive_values: &[&str],
    ) -> Result<Self::Output, TwilioError> {
        twilio2::decode_json_response(&raw, sensitive_values)
    }
}

#[tokio::test]
async fn custom_operation_supports_options_meta_and_raw_response() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(message_page_json(
            &[full_message_json("SMop", "sent", "hello")],
            None,
        ))
        .header("retry-after", "3"),
    ])
    .await;
    let client = client_for(&server);
    let options = RequestOptions::new()
        .query("Trace", "trace-secret")
        .header("x-request-id", "req-123")
        .timeout(Duration::from_secs(5))
        .retry(RetryPolicy::none().with_max_retries(1));

    let response = client
        .account(test_creds())
        .send_with_response_with_options(MessagesValueOperation, options)
        .await
        .unwrap();

    assert_eq!(response.meta.status, 200);
    assert_eq!(response.meta.retry_after, Some(Duration::from_secs(3)));
    assert!(response.raw.body.len() > 20);
    assert_debug_redacts(&response.raw, &["SMop", "hello"]);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=1&Trace=trace-secret"
    );
    assert_eq!(requests[0].header("x-request-id"), Some("req-123"));
    assert_basic_auth(&requests[0]);
}

#[tokio::test]
async fn request_options_reject_blocked_headers_and_unsafe_retries() {
    let header_err = RequestOptions::new()
        .try_header("Authorization", "Basic wrong")
        .unwrap_err();
    assert_invalid_request(header_err, "cannot be overridden");

    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let deferred_header_err = client
        .account(test_creds())
        .send_with_options(
            MessagesValueOperation,
            RequestOptions::new().header("Authorization", "Basic wrong"),
        )
        .await
        .unwrap_err();
    assert_invalid_request(deferred_header_err, "cannot be overridden");
    assert!(server.requests().is_empty());

    let err = client
        .account(test_creds())
        .send_with_options(
            UnsafePostOperation,
            RequestOptions::new().retry(RetryPolicy::none().with_max_retries(1)),
        )
        .await
        .unwrap_err();
    assert_invalid_request(err, "safe HTTP methods");
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn messages_list_all_collects_pages_with_existing_continuation_validation() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&PageSize=1&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(message_page_json(
            &[full_message_json("SMpage1", "sent", "one")],
            Some(next_page_uri),
        )),
        MockResponse::json(message_page_json(
            &[full_message_json("SMpage2", "sent", "two")],
            None,
        )),
    ])
    .await;
    let client = client_for(&server);
    let messages = client
        .account(test_creds())
        .messages()
        .list_all_with(ListMessagesRequest::new().to("+15551234567").page_size(1))
        .collect_all()
        .await
        .unwrap();

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].sid.as_deref(), Some("SMpage1"));
    assert_eq!(messages[1].sid.as_deref(), Some("SMpage2"));
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&PageSize=1"
    );
    assert_eq!(requests[1].path, next_page_uri);
}

#[tokio::test]
async fn messages_builder_sends_expected_requests() {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(full_message_json("SMcreated", "queued", "hello")),
        MockResponse::json(full_message_json("SMfetched", "delivered", "hello")),
        MockResponse::json(full_message_json("SMredacted", "sent", "")),
        MockResponse::json(full_message_json("SMcanceled", "canceled", "hello")),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let mut create = CreateMessageRequest::new("+15551234567");
    create.from = Some("+15557654321");
    create.body = Some("hello");
    create.media_urls = &["https://example.test/a.png", "https://example.test/b.png"];
    create.persistent_actions = &["mailto:test@example.test"];
    create.status_callback = Some("https://example.test/status");
    create.application_sid = Some("AP123");
    create.provide_feedback = Some(true);
    create.attempt = Some(2);
    create.validity_period = Some(3600);
    create.content_retention = Some(ContentRetention::Retain);
    create.address_retention = Some(AddressRetention::Obfuscate);
    create.smart_encoded = Some(true);
    create.traffic_type = Some(TrafficType::Free);
    create.shorten_urls = Some(false);
    create.schedule_type = Some(ScheduleType::Fixed);
    create.send_at = Some("2026-07-03T12:00:00Z");
    create.send_as_mms = Some(true);
    create.content_sid = Some("HX123");
    create.content_variables_json = Some(r#"{"name":"Ada"}"#);
    create.risk_check = Some(RiskCheck::Disable);
    create.fallback_from = Some("+15550000000");
    create.tags_json = Some(r#"{"campaign":"spring"}"#);

    let created = account.messages().create(create).await.unwrap();
    let fetched = account.message("SM fetch/123").fetch().await.unwrap();
    let redacted = account
        .message("SMredact")
        .update(UpdateMessageRequest::redact_body())
        .await
        .unwrap();
    let canceled = account
        .message("SMcancel")
        .update(UpdateMessageRequest::cancel())
        .await
        .unwrap();
    account.message("SMdelete").delete().await.unwrap();

    assert_eq!(created.sid.as_deref(), Some("SMcreated"));
    assert_eq!(fetched.status.as_deref(), Some("delivered"));
    assert_eq!(redacted.body.as_deref(), Some(""));
    assert_eq!(canceled.status.as_deref(), Some("canceled"));
    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
    assert!(requests[0].body.contains("To=%2B15551234567"));
    assert!(requests[0].body.contains("From=%2B15557654321"));
    assert!(requests[0].body.contains("Body=hello"));
    assert!(
        requests[0]
            .body
            .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fa.png")
    );
    assert!(
        requests[0]
            .body
            .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fb.png")
    );
    assert!(
        requests[0]
            .body
            .contains("PersistentAction=mailto%3Atest%40example.test")
    );
    assert!(requests[0].body.contains("ContentSid=HX123"));
    assert!(
        requests[0]
            .body
            .contains("ContentVariables=%7B%22name%22%3A%22Ada%22%7D")
    );
    assert!(requests[0].body.contains("RiskCheck=disable"));
    assert!(
        requests[0]
            .body
            .contains("Tags=%7B%22campaign%22%3A%22spring%22%7D")
    );
    assert_basic_auth(&requests[0]);
    assert_eq!(requests[1].method, "GET");
    assert_eq!(
        requests[1].path,
        "/2010-04-01/Accounts/AC123/Messages/SM%20fetch%2F123.json"
    );
    assert_eq!(requests[2].body, "Body=");
    assert_eq!(requests[3].body, "Status=canceled");
    assert_eq!(requests[4].method, "DELETE");
    assert_eq!(
        requests[4].path,
        "/2010-04-01/Accounts/AC123/Messages/SMdelete.json"
    );
}

#[tokio::test]
async fn messages_list_paginates_with_stable_filters() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&DateSent=2026-07-01&DateSent%3C=2026-07-31&DateSent%3E=2026-06-01&PageSize=2&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(message_page_json(
            &[full_message_json("SMlist", "sent", "listed")],
            Some(next_page_uri),
        )),
        MockResponse::json(message_page_json(&[], None)),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let mut list = ListMessagesRequest::new();
    list.to = Some("+15551234567");
    list.from = Some("+15557654321");
    list.date_sent = Some("2026-07-01");
    list.date_sent_before = Some("2026-07-31");
    list.date_sent_after = Some("2026-06-01");
    list.page_size = Some(2);
    list.page = Some(0);
    list.page_token = Some("start");

    let first = account.messages().list(list).await.unwrap();
    let second = account
        .messages()
        .list_page_uri(first.next_page_uri.as_deref().unwrap())
        .await
        .unwrap();

    assert_eq!(first.messages[0].sid.as_deref(), Some("SMlist"));
    assert_eq!(first.next_page_uri.as_deref(), Some(next_page_uri));
    assert_eq!(first.page, Some(0));
    assert_eq!(first.page_size, Some(2));
    assert!(second.messages.is_empty());

    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&DateSent=2026-07-01&DateSent%3C=2026-07-31&DateSent%3E=2026-06-01&PageSize=2&Page=0&PageToken=start"
    );
    assert_eq!(requests[1].path, next_page_uri);
}

#[tokio::test]
async fn message_media_and_feedback_work_through_resource_builders() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=1&Page=1&PageToken=next";
    let list_body = format!(
        r#"{{
            "media_list": [{media}],
            "next_page_uri": "{next_page_uri}",
            "page": 0,
            "page_size": 1
        }}"#,
        media = media_json("MElist")
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::json(media_json("MEmeta")),
        MockResponse::bytes("image/png", vec![1, 2, 3, 4]),
        MockResponse::json(list_body),
        MockResponse::json(r#"{"media_list":[],"next_page_uri":null}"#),
        MockResponse::no_content(),
        MockResponse::json(
            r#"{"account_sid":"AC123","message_sid":"SM123","outcome":"confirmed","date_created":"Fri, 24 May 2019 17:44:46 +0000","uri":"/feedback"}"#,
        ),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let meta = account
        .message("SM123")
        .media()
        .fetch("MEmeta")
        .await
        .unwrap();
    let content = account
        .message("SM123")
        .media()
        .download("MEraw")
        .await
        .unwrap();
    let mut list = ListMediaRequest::new();
    list.date_created = Some("2026-07-01");
    list.date_created_before = Some("2026-07-31");
    list.date_created_after = Some("2026-06-01");
    list.page_size = Some(1);
    list.page = Some(0);
    list.page_token = Some("start");
    let first = account.message("SM123").media().list(list).await.unwrap();
    let second = account
        .message("SM123")
        .media()
        .list_page_uri(first.next_page_uri.as_deref().unwrap())
        .await
        .unwrap();
    account
        .message("SM123")
        .media()
        .delete("MEdelete")
        .await
        .unwrap();
    let feedback = account
        .message("SM123")
        .feedback()
        .create(CreateMessageFeedbackRequest::new(
            MessageFeedbackOutcome::Confirmed,
        ))
        .await
        .unwrap();

    assert_eq!(meta.sid.as_deref(), Some("MEmeta"));
    assert_eq!(content.content_type.as_deref(), Some("image/png"));
    assert_eq!(content.bytes, vec![1, 2, 3, 4]);
    assert_eq!(first.media[0].sid.as_deref(), Some("MElist"));
    assert!(second.media.is_empty());
    assert_eq!(feedback.outcome.as_deref(), Some("confirmed"));
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEmeta.json"
    );
    assert_eq!(
        requests[1].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEraw"
    );
    assert_eq!(
        requests[2].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=1&Page=0&PageToken=start"
    );
    assert_eq!(requests[3].path, next_page_uri);
    assert_eq!(
        requests[4].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEdelete.json"
    );
    assert_eq!(
        requests[5].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Feedback.json"
    );
    assert_eq!(requests[5].body, "Outcome=confirmed");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn services_crud_list_and_page_url_use_messaging_base() {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(service_json("MGcreate", "Created")),
        MockResponse::json(service_json("MGfetch", "Fetched")),
        MockResponse::json(service_page_json(
            &server_url_placeholder(),
            "services",
            "services",
            &[service_json("MGlist", "Listed")],
            Some("/v1/Services?PageSize=2&Page=1&PageToken=next"),
        )),
        MockResponse::json(service_page_json(
            &server_url_placeholder(),
            "services",
            "services",
            &[],
            None,
        )),
        MockResponse::json(service_json("MGupdate", "Updated")),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let create = CreateServiceRequest::new("Created")
        .inbound_request_url("https://example.test/inbound")
        .inbound_method(HttpMethod::Post)
        .fallback_url("https://example.test/fallback")
        .fallback_method(HttpMethod::Get)
        .status_callback("https://example.test/status")
        .sticky_sender(true)
        .mms_converter(false)
        .smart_encoding(true)
        .scan_message_content(ScanMessageContent::Inherit)
        .fallback_to_long_code(false)
        .area_code_geomatch(true)
        .synchronous_validation(false)
        .validity_period(600)
        .usecase(ServiceUsecase::Marketing)
        .use_inbound_webhook_on_number(true);
    let created = account.services().create(create).await.unwrap();
    let fetched = account.service("MGfetch").fetch().await.unwrap();
    let first = account
        .services()
        .list(ListServicesRequest::new().page_size(2).page(0))
        .await
        .unwrap();
    let next_url = format!(
        "{}/v1/Services?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let second = account.services().list_page_url(&next_url).await.unwrap();
    let updated = account
        .service("MGupdate")
        .update(
            UpdateServiceRequest::new()
                .friendly_name("Updated")
                .clear_inbound_request_url()
                .clear_fallback_url()
                .clear_status_callback(),
        )
        .await
        .unwrap();
    account.service("MGdelete").delete().await.unwrap();

    assert_eq!(created.sid.as_deref(), Some("MGcreate"));
    assert_eq!(fetched.sid.as_deref(), Some("MGfetch"));
    assert_eq!(first.meta.key.as_deref(), Some("services"));
    assert_eq!(first.services[0].sid.as_deref(), Some("MGlist"));
    assert!(second.services.is_empty());
    assert_eq!(updated.friendly_name.as_deref(), Some("Updated"));

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Services");
    for expected in [
        "FriendlyName=Created",
        "InboundRequestUrl=https%3A%2F%2Fexample.test%2Finbound",
        "InboundMethod=POST",
        "FallbackUrl=https%3A%2F%2Fexample.test%2Ffallback",
        "FallbackMethod=GET",
        "StatusCallback=https%3A%2F%2Fexample.test%2Fstatus",
        "StickySender=true",
        "MmsConverter=false",
        "SmartEncoding=true",
        "ScanMessageContent=inherit",
        "FallbackToLongCode=false",
        "AreaCodeGeomatch=true",
        "SynchronousValidation=false",
        "ValidityPeriod=600",
        "Usecase=marketing",
        "UseInboundWebhookOnNumber=true",
    ] {
        assert!(
            requests[0].body.contains(expected),
            "missing {expected} in {}",
            requests[0].body
        );
    }
    assert_eq!(requests[1].path, "/v1/Services/MGfetch");
    assert_eq!(requests[2].path, "/v1/Services?PageSize=2&Page=0");
    assert_eq!(
        requests[3].path,
        "/v1/Services?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[4].path, "/v1/Services/MGupdate");
    assert_eq!(
        requests[4].body,
        "FriendlyName=Updated&InboundRequestUrl=&FallbackUrl=&StatusCallback="
    );
    assert_eq!(requests[5].method, "DELETE");
    assert_eq!(requests[5].path, "/v1/Services/MGdelete");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn service_subresources_use_exact_forms_keys_and_page_urls() {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(phone_number_json("PNcreate")),
        MockResponse::json(phone_number_json("PNfetch")),
        MockResponse::json(phone_number_page_json("PNlist", Some("PhoneNumbers"))),
        MockResponse::json(phone_number_empty_page_json()),
        MockResponse::no_content(),
        MockResponse::created_json(short_code_json("SCcreate")),
        MockResponse::json(short_code_json("SCfetch")),
        MockResponse::json(short_code_page_json("SClist", Some("ShortCodes"))),
        MockResponse::json(short_code_empty_page_json()),
        MockResponse::no_content(),
        MockResponse::created_json(alpha_sender_json("AIcreate")),
        MockResponse::json(alpha_sender_json("AIfetch")),
        MockResponse::json(alpha_sender_page_json("AIlist", Some("AlphaSenders"))),
        MockResponse::json(alpha_sender_empty_page_json()),
        MockResponse::no_content(),
        MockResponse::created_json(channel_sender_json("XEcreate")),
        MockResponse::json(channel_sender_json("XEfetch")),
        MockResponse::json(channel_sender_page_json("XElist", Some("ChannelSenders"))),
        MockResponse::json(channel_sender_empty_page_json()),
        MockResponse::no_content(),
        MockResponse::created_json(destination_alpha_sender_json("AIcreate")),
        MockResponse::json(destination_alpha_sender_json("AIfetch")),
        MockResponse::json(destination_alpha_sender_page_json(
            "AIlist",
            Some("DestinationAlphaSenders?IsoCountryCode=FR"),
        )),
        MockResponse::json(destination_alpha_sender_empty_page_json()),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let service = account.service("MG123");

    service
        .phone_numbers()
        .create(CreateServicePhoneNumberRequest::new("PNcreate"))
        .await
        .unwrap();
    service.phone_numbers().fetch("PNfetch").await.unwrap();
    let page = service
        .phone_numbers()
        .list(ListServiceSubresourcesRequest::new().page_size(2))
        .await
        .unwrap();
    service
        .phone_numbers()
        .list_page_url(page.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    service.phone_numbers().delete("PNdelete").await.unwrap();

    service
        .short_codes()
        .create(CreateServiceShortCodeRequest::new("SCcreate"))
        .await
        .unwrap();
    service.short_codes().fetch("SCfetch").await.unwrap();
    let page = service
        .short_codes()
        .list(ListServiceSubresourcesRequest::new().page_size(2))
        .await
        .unwrap();
    service
        .short_codes()
        .list_page_url(page.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    service.short_codes().delete("SCdelete").await.unwrap();

    service
        .alpha_senders()
        .create(CreateAlphaSenderRequest::new("MyCo"))
        .await
        .unwrap();
    service.alpha_senders().fetch("AIfetch").await.unwrap();
    let page = service
        .alpha_senders()
        .list(ListServiceSubresourcesRequest::new().page_size(2))
        .await
        .unwrap();
    service
        .alpha_senders()
        .list_page_url(page.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    service.alpha_senders().delete("AIdelete").await.unwrap();

    service
        .channel_senders()
        .create(CreateChannelSenderRequest::new("XEcreate"))
        .await
        .unwrap();
    service.channel_senders().fetch("XEfetch").await.unwrap();
    let page = service
        .channel_senders()
        .list(ListServiceSubresourcesRequest::new().page_size(2))
        .await
        .unwrap();
    assert_eq!(
        page.senders[0].messaging_service_sid.as_deref(),
        Some("MG123")
    );
    service
        .channel_senders()
        .list_page_url(page.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    service.channel_senders().delete("XEdelete").await.unwrap();

    service
        .destination_alpha_senders()
        .create(CreateDestinationAlphaSenderRequest::new("MyCo").iso_country_code("FR"))
        .await
        .unwrap();
    service
        .destination_alpha_senders()
        .fetch("AIfetch")
        .await
        .unwrap();
    let page = service
        .destination_alpha_senders()
        .list(
            ListDestinationAlphaSendersRequest::new()
                .iso_country_code("FR")
                .page_size(2),
        )
        .await
        .unwrap();
    assert_eq!(
        page.alpha_senders[0].iso_country_code.as_deref(),
        Some("FR")
    );
    service
        .destination_alpha_senders()
        .list_page_url(page.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    service
        .destination_alpha_senders()
        .delete("AIdelete")
        .await
        .unwrap();

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Services/MG123/PhoneNumbers");
    assert_eq!(requests[0].body, "PhoneNumberSid=PNcreate");
    assert_eq!(requests[1].path, "/v1/Services/MG123/PhoneNumbers/PNfetch");
    assert_eq!(
        requests[2].path,
        "/v1/Services/MG123/PhoneNumbers?PageSize=2"
    );
    assert_eq!(
        requests[3].path,
        "/v1/Services/MG123/PhoneNumbers?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[5].body, "ShortCodeSid=SCcreate");
    assert_eq!(requests[10].body, "AlphaSender=MyCo");
    assert_eq!(requests[15].body, "Sid=XEcreate");
    assert_eq!(requests[20].body, "AlphaSender=MyCo&IsoCountryCode=FR");
    assert_eq!(
        requests[22].path,
        "/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=2"
    );
    assert_eq!(
        requests[23].path,
        "/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=2&Page=1&PageToken=next"
    );
}

#[tokio::test]
async fn deactivations_and_account_short_codes_use_expected_wire_shape() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&ShortCode=12345&PageSize=2&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(
            307,
            r#"{"redirect_to":"https://storage.example.test/deactivations?signature=secret"}"#,
        ),
        MockResponse::json(account_short_code_json("SCfetch", "Fetched")),
        MockResponse::json(account_short_code_page_json(
            &[account_short_code_json("SClist", "Listed")],
            Some(next_page_uri),
        )),
        MockResponse::json(account_short_code_page_json(&[], None)),
        MockResponse::json(account_short_code_json("SCupdate", "Updated")),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let deactivation = account
        .deactivations()
        .fetch(FetchDeactivationsRequest::new("2023-08-13"))
        .await
        .unwrap();
    let fetched = account.short_code("SCfetch").fetch().await.unwrap();
    let first = account
        .short_codes()
        .list(
            ListAccountShortCodesRequest::new()
                .friendly_name("Alerts")
                .short_code("12345")
                .page_size(2)
                .page(0),
        )
        .await
        .unwrap();
    let second = account
        .short_codes()
        .list_page_uri(first.next_page_uri.as_deref().unwrap())
        .await
        .unwrap();
    let updated = account
        .short_code("SCupdate")
        .update(
            UpdateAccountShortCodeRequest::new()
                .friendly_name("Updated")
                .api_version("2010-04-01")
                .clear_sms_url()
                .sms_method(HttpMethod::Post)
                .sms_fallback_url("https://example.test/fallback")
                .sms_fallback_method(HttpMethod::Get),
        )
        .await
        .unwrap();

    assert_eq!(
        deactivation.redirect_to.as_deref(),
        Some("https://storage.example.test/deactivations?signature=secret")
    );
    assert_eq!(fetched.sid.as_deref(), Some("SCfetch"));
    assert_eq!(first.short_codes[0].sid.as_deref(), Some("SClist"));
    assert!(second.short_codes.is_empty());
    assert_eq!(updated.friendly_name.as_deref(), Some("Updated"));

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/Deactivations?Date=2023-08-13");
    assert_eq!(
        requests[1].path,
        "/2010-04-01/Accounts/AC123/SMS/ShortCodes/SCfetch.json"
    );
    assert_eq!(
        requests[2].path,
        "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&ShortCode=12345&PageSize=2&Page=0"
    );
    assert_eq!(requests[3].path, next_page_uri);
    assert_eq!(
        requests[4].path,
        "/2010-04-01/Accounts/AC123/SMS/ShortCodes/SCupdate.json"
    );
    assert_eq!(
        requests[4].body,
        "FriendlyName=Updated&ApiVersion=2010-04-01&SmsUrl=&SmsMethod=POST&SmsFallbackUrl=https%3A%2F%2Fexample.test%2Ffallback&SmsFallbackMethod=GET"
    );
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn account_short_codes_list_all_collects_pages_with_contract_filters() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&PageSize=1&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(account_short_code_page_json(
            &[account_short_code_json("SCpage1", "Alerts")],
            Some(next_page_uri),
        )),
        MockResponse::json(account_short_code_page_json(
            &[account_short_code_json("SCpage2", "Alerts")],
            None,
        )),
    ])
    .await;
    let client = client_for(&server);
    let short_codes = client
        .account(test_creds())
        .short_codes()
        .list_all_with(
            ListAccountShortCodesRequest::new()
                .friendly_name("Alerts")
                .page_size(1),
        )
        .collect_all()
        .await
        .unwrap();

    assert_eq!(short_codes.len(), 2);
    assert_eq!(short_codes[0].sid.as_deref(), Some("SCpage1"));
    assert_eq!(short_codes[1].sid.as_deref(), Some("SCpage2"));
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&PageSize=1"
    );
    assert_eq!(requests[1].path, next_page_uri);
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn malformed_deactivation_response_is_decode_error() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        307,
        r#"{"redirect_to":123}"#,
    )])
    .await;
    let client = client_for(&server);
    let err = client
        .account(test_creds())
        .deactivations()
        .fetch(FetchDeactivationsRequest::new("2023-08-13"))
        .await
        .unwrap_err();

    assert_decode_error(&err);
    let rendered = format!("{err:?}");
    for leaked in [
        "AC123",
        "auth-token",
        "storage.example.test",
        "signature=secret",
    ] {
        assert!(
            !rendered.contains(leaked),
            "decode diagnostic leaked {leaked:?}: {rendered}"
        );
    }
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/Deactivations?Date=2023-08-13");
    assert_basic_auth(&requests[0]);
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn tollfree_verifications_crud_list_and_repeated_keys_work() {
    let categories = [
        TollfreeUseCaseCategory::TwoFactorAuthentication,
        TollfreeUseCaseCategory::Marketing,
        TollfreeUseCaseCategory::PollingAndVotingNonPolitical,
    ];
    let opt_in_image_urls = [
        "https://example.test/opt-in-1.png",
        "https://example.test/opt-in-2.png",
    ];
    let opt_in_keywords = ["START", "JOIN"];
    let trust_product_sids = ["BUtrust1", "BUtrust2"];
    let next_page_url = format!(
        "{}/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PNlist&Status=TWILIO_APPROVED&ExternalReferenceId=external-123&IncludeSubAccounts=true&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=2&Page=1&PageToken=next",
        server_url_placeholder()
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(tollfree_verification_json("HHcreate", "PENDING_REVIEW")),
        MockResponse::json(tollfree_verification_json("HHfetch", "TWILIO_APPROVED")),
        MockResponse::json(tollfree_verification_page_json(
            &[tollfree_verification_json("HHlist", "TWILIO_APPROVED")],
            Some(&next_page_url),
        )),
        MockResponse::json(tollfree_verification_page_json(&[], None)),
        MockResponse::json(tollfree_verification_json("HHupdate", "IN_REVIEW")),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let create = CreateTollfreeVerificationRequest::new()
        .business_name("Owl, Inc.")
        .business_website("https://example.test")
        .notification_email("support@example.test")
        .use_case_categories(&categories)
        .use_case_summary("Account security and marketing alerts")
        .production_message_sample("Your code is 123456")
        .opt_in_image_urls(&opt_in_image_urls)
        .opt_in_type(TollfreeOptInType::Verbal)
        .message_volume(TollfreeMessageVolume::Thousand)
        .tollfree_phone_number_sid("PNcreate")
        .customer_profile_sid("BUcustomer")
        .business_street_address("123 Main Street")
        .business_city("Detroit")
        .business_state_province_region("MI")
        .business_postal_code("48201")
        .business_country("US")
        .business_contact_first_name("Ada")
        .business_contact_last_name("Lovelace")
        .business_contact_email("ada@example.test")
        .business_contact_phone("+15551234567")
        .external_reference_id("external-123")
        .business_registration_number("123456789")
        .business_registration_authority(TollfreeBusinessRegistrationAuthority::Ein)
        .business_registration_country("US")
        .business_type(TollfreeBusinessType::PrivateProfit)
        .business_registration_phone_number("+15557654321")
        .doing_business_as("Owl Alerts")
        .opt_in_confirmation_message("Thanks for opting in")
        .help_message_sample("Reply HELP for help")
        .privacy_policy_url("https://example.test/privacy")
        .terms_and_conditions_url("https://example.test/terms")
        .age_gated_content(false)
        .opt_in_keywords(&opt_in_keywords)
        .vetting_provider(TollfreeVettingProvider::CampaignVerify)
        .vetting_id("vetting-123");
    let created = account
        .tollfree_verifications()
        .create(create)
        .await
        .unwrap();
    let fetched = account
        .tollfree_verification("HHfetch")
        .fetch()
        .await
        .unwrap();
    let first = account
        .tollfree_verifications()
        .list(
            ListTollfreeVerificationsRequest::new()
                .tollfree_phone_number_sid("PNlist")
                .status(TollfreeVerificationStatus::TwilioApproved)
                .external_reference_id("external-123")
                .include_sub_accounts(true)
                .trust_product_sids(&trust_product_sids)
                .page_size(2)
                .page(0),
        )
        .await
        .unwrap();
    let second = account
        .tollfree_verifications()
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    let updated = account
        .tollfree_verification("HHupdate")
        .update(
            UpdateTollfreeVerificationRequest::new()
                .business_name("Owl Updated")
                .edit_reason("Website fixed")
                .opt_in_keywords(&opt_in_keywords)
                .age_gated_content(false),
        )
        .await
        .unwrap();
    account
        .tollfree_verification("HHdelete")
        .delete()
        .await
        .unwrap();

    assert_eq!(created.sid.as_deref(), Some("HHcreate"));
    assert_eq!(fetched.status.as_deref(), Some("TWILIO_APPROVED"));
    assert_eq!(first.meta.key.as_deref(), Some("verifications"));
    assert_eq!(
        first.tollfree_verifications[0].sid.as_deref(),
        Some("HHlist")
    );
    assert!(second.tollfree_verifications.is_empty());
    assert_eq!(updated.sid.as_deref(), Some("HHupdate"));

    let requests = server.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[0].path, "/v1/Tollfree/Verifications");
    for expected in [
        "BusinessName=Owl%2C+Inc.",
        "BusinessWebsite=https%3A%2F%2Fexample.test",
        "NotificationEmail=support%40example.test",
        "UseCaseCategories=TWO_FACTOR_AUTHENTICATION",
        "UseCaseCategories=MARKETING",
        "UseCaseCategories=POLLING_AND_VOTING_NON_POLITICAL",
        "ProductionMessageSample=Your+code+is+123456",
        "OptInImageUrls=https%3A%2F%2Fexample.test%2Fopt-in-1.png",
        "OptInImageUrls=https%3A%2F%2Fexample.test%2Fopt-in-2.png",
        "OptInType=VERBAL",
        "MessageVolume=1%2C000",
        "TollfreePhoneNumberSid=PNcreate",
        "BusinessRegistrationAuthority=EIN",
        "BusinessType=PRIVATE_PROFIT",
        "AgeGatedContent=false",
        "OptInKeywords=START",
        "OptInKeywords=JOIN",
        "VettingProvider=CAMPAIGN_VERIFY",
        "VettingId=vetting-123",
    ] {
        assert!(
            requests[0].body.contains(expected),
            "missing {expected} in {}",
            requests[0].body
        );
    }
    assert_eq!(requests[1].path, "/v1/Tollfree/Verifications/HHfetch");
    assert_eq!(
        requests[2].path,
        "/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PNlist&Status=TWILIO_APPROVED&ExternalReferenceId=external-123&IncludeSubAccounts=true&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=2&Page=0"
    );
    assert_eq!(
        requests[3].path,
        "/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PNlist&Status=TWILIO_APPROVED&ExternalReferenceId=external-123&IncludeSubAccounts=true&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[4].path, "/v1/Tollfree/Verifications/HHupdate");
    assert_eq!(
        requests[4].body,
        "BusinessName=Owl+Updated&EditReason=Website+fixed&AgeGatedContent=false&OptInKeywords=START&OptInKeywords=JOIN"
    );
    assert_eq!(requests[5].method, "DELETE");
    assert_eq!(requests[5].path, "/v1/Tollfree/Verifications/HHdelete");
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn tollfree_verifications_list_all_collects_pages_with_contract_filters() {
    let trust_product_sids = ["BUtrust1", "BUtrust2"];
    let next_page_url = format!(
        "{}/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1&Page=1&PageToken=next",
        server_url_placeholder()
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::json(tollfree_verification_page_json(
            &[tollfree_verification_json("HHpage1", "TWILIO_APPROVED")],
            Some(&next_page_url),
        )),
        MockResponse::json(tollfree_verification_page_json(
            &[tollfree_verification_json("HHpage2", "TWILIO_APPROVED")],
            None,
        )),
    ])
    .await;
    let client = client_for(&server);
    let verifications = client
        .account(test_creds())
        .tollfree_verifications()
        .list_all_with(
            ListTollfreeVerificationsRequest::new()
                .status(TollfreeVerificationStatus::TwilioApproved)
                .trust_product_sids(&trust_product_sids)
                .page_size(1),
        )
        .collect_all()
        .await
        .unwrap();

    assert_eq!(verifications.len(), 2);
    assert_eq!(verifications[0].sid.as_deref(), Some("HHpage1"));
    assert_eq!(verifications[1].sid.as_deref(), Some("HHpage2"));
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].path,
        "/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1"
    );
    assert_eq!(
        requests[1].path,
        "/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1&Page=1&PageToken=next"
    );
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn new_endpoint_optional_setters_use_expected_form_keys() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(account_short_code_json("SCupdate", "Updated")),
        MockResponse::json(tollfree_verification_json("HHupdate", "IN_REVIEW")),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    account
        .short_code("SCupdate")
        .update(UpdateAccountShortCodeRequest::new().clear_sms_fallback_url())
        .await
        .unwrap();
    account
        .tollfree_verification("HHupdate")
        .update(
            UpdateTollfreeVerificationRequest::new()
                .business_street_address2("Suite 101")
                .additional_information("Additional context"),
        )
        .await
        .unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/SMS/ShortCodes/SCupdate.json"
    );
    assert_eq!(requests[0].body, "SmsFallbackUrl=");
    assert_eq!(requests[1].path, "/v1/Tollfree/Verifications/HHupdate");
    assert_eq!(
        requests[1].body,
        "BusinessStreetAddress2=Suite+101&AdditionalInformation=Additional+context"
    );
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn message_request_validation_catches_local_errors() {
    let mut create = CreateMessageRequest::new("");
    create.from = Some("+15557654321");
    create.body = Some("hello");

    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let err = account
        .messages()
        .create(create)
        .await
        .expect_err("empty To should fail before transport");
    assert!(matches!(err, TwilioError::InvalidRequest(_)));

    let overlong_body = "x".repeat(1_601);
    let mut create = CreateMessageRequest::new("+15551234567");
    create.from = Some("+15557654321");
    create.body = Some(&overlong_body);
    let err = account
        .messages()
        .create(create)
        .await
        .expect_err("overlong message Body should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains("Body")
    ));

    for validity_period in [0, 36_001] {
        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.body = Some("hello");
        create.validity_period = Some(validity_period);
        let err = account
            .messages()
            .create(create)
            .await
            .expect_err("out-of-range message ValidityPeriod should fail before transport");
        assert!(matches!(
            err,
            TwilioError::InvalidRequest(message) if message.contains("ValidityPeriod")
        ));
    }

    let mut create = CreateMessageRequest::new("+15551234567");
    create.from = Some("+15557654321");
    create.body = Some("hello");
    create.shorten_urls = Some(true);
    let err = account
        .messages()
        .create(create)
        .await
        .expect_err("ShortenUrls without MessagingServiceSid should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message)
            if message.contains("ShortenUrls") && message.contains("MessagingServiceSid")
    ));

    let mut create = CreateMessageRequest::new("+15551234567");
    create.from = Some("+15557654321");
    create.body = Some("hello");
    create.content_variables_json = Some(r#"{"name":"Ada"}"#);
    let err = account
        .messages()
        .create(create)
        .await
        .expect_err("ContentVariables without ContentSid should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message)
            if message.contains("ContentVariables") && message.contains("ContentSid")
    ));

    let err = account
        .message("SM123")
        .update(UpdateMessageRequest {
            body: Some("replacement"),
            status: None,
        })
        .await
        .expect_err("non-empty message update Body should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains("Body")
    ));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn service_request_validation_catches_local_errors() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let err = account
        .service("MG123")
        .update(UpdateServiceRequest::new())
        .await
        .expect_err("empty service update should fail before transport");
    assert!(matches!(err, TwilioError::InvalidRequest(_)));

    let overlong_friendly_name = "x".repeat(65);
    let err = account
        .services()
        .create(CreateServiceRequest::new(&overlong_friendly_name))
        .await
        .expect_err("overlong service FriendlyName should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains("FriendlyName")
    ));

    let err = account
        .service("MG123")
        .update(UpdateServiceRequest::new().friendly_name(&overlong_friendly_name))
        .await
        .expect_err("overlong service update FriendlyName should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains("FriendlyName")
    ));

    for validity_period in [0, 36_001] {
        let err = account
            .services()
            .create(CreateServiceRequest::new("valid").validity_period(validity_period))
            .await
            .expect_err("out-of-range service ValidityPeriod should fail before transport");
        assert!(matches!(
            err,
            TwilioError::InvalidRequest(message) if message.contains("ValidityPeriod")
        ));

        let err = account
            .service("MG123")
            .update(UpdateServiceRequest::new().validity_period(validity_period))
            .await
            .expect_err("out-of-range service update ValidityPeriod should fail before transport");
        assert!(matches!(
            err,
            TwilioError::InvalidRequest(message) if message.contains("ValidityPeriod")
        ));
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn new_endpoint_validation_catches_local_errors() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    assert_invalid_request(
        account
            .deactivations()
            .fetch(FetchDeactivationsRequest::new("2026-02-30"))
            .await
            .unwrap_err(),
        "Date",
    );
    assert_invalid_request(
        account
            .short_code("SC123")
            .update(UpdateAccountShortCodeRequest::new())
            .await
            .unwrap_err(),
        "at least one field",
    );
    assert_invalid_request(
        account
            .tollfree_verifications()
            .create(CreateTollfreeVerificationRequest::new())
            .await
            .unwrap_err(),
        "BusinessName",
    );
    assert_invalid_request(
        account
            .tollfree_verification("HH123")
            .update(UpdateTollfreeVerificationRequest::new())
            .await
            .unwrap_err(),
        "at least one field",
    );

    let categories = [TollfreeUseCaseCategory::Raw("")];
    assert_invalid_request(
        account
            .tollfree_verifications()
            .create(
                CreateTollfreeVerificationRequest::new()
                    .business_name("Owl")
                    .business_website("https://example.test")
                    .notification_email("support@example.test")
                    .use_case_categories(&categories)
                    .use_case_summary("Alerts")
                    .production_message_sample("Sample")
                    .opt_in_image_urls(&["https://example.test/opt.png"])
                    .opt_in_type(TollfreeOptInType::Verbal)
                    .message_volume(TollfreeMessageVolume::Ten)
                    .tollfree_phone_number_sid("PN123"),
            )
            .await
            .unwrap_err(),
        "UseCaseCategories",
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn page_size_validation_catches_local_errors() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    for page_size in [0, 1001] {
        let mut messages = ListMessagesRequest::new();
        messages.page_size = Some(page_size);
        assert_invalid_request(
            account.messages().list(messages).await.unwrap_err(),
            "PageSize",
        );

        let mut media = ListMediaRequest::new();
        media.page_size = Some(page_size);
        assert_invalid_request(
            account
                .message("SM123")
                .media()
                .list(media)
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .services()
                .list(ListServicesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .service("MG123")
                .phone_numbers()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .service("MG123")
                .short_codes()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .service("MG123")
                .alpha_senders()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .service("MG123")
                .channel_senders()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .service("MG123")
                .destination_alpha_senders()
                .list(ListDestinationAlphaSendersRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .short_codes()
                .list(ListAccountShortCodesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .tollfree_verifications()
                .list(ListTollfreeVerificationsRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn malformed_success_responses_are_decode_errors() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(r#"{"sid":123}"#),
        MockResponse::json(r#"{"sid":123}"#),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    assert_decode_error(&account.message("SMbad").fetch().await.unwrap_err());
    assert_decode_error(&account.service("MGbad").fetch().await.unwrap_err());
}

#[tokio::test]
async fn representative_api_errors_are_classified_and_sanitized() {
    let body = r#"{"message":"denied","sid":"SMsecret","to":"+15551234567","url":"https://example.test/private"}"#;
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(500, body),
        MockResponse::status_json(404, body),
        MockResponse::status_json(400, body),
        MockResponse::status_json(503, body),
        MockResponse::status_json(409, body),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let leaked = ["SMsecret", "+15551234567", "https://example.test/private"];

    assert_api_error_redacted(
        account
            .messages()
            .list(ListMessagesRequest::new())
            .await
            .unwrap_err(),
        500,
        &leaked,
    );
    assert_api_error_redacted(
        account.message("SMsecret").delete().await.unwrap_err(),
        404,
        &leaked,
    );
    assert_api_error_redacted(
        account
            .message("SM123")
            .media()
            .delete("MEsecret")
            .await
            .unwrap_err(),
        400,
        &leaked,
    );
    assert_api_error_redacted(
        account.service("MGsecret").delete().await.unwrap_err(),
        503,
        &leaked,
    );
    assert_api_error_redacted(
        account
            .service("MGsecret")
            .phone_numbers()
            .delete("PNsecret")
            .await
            .unwrap_err(),
        409,
        &leaked,
    );
}

#[tokio::test]
async fn errors_and_debug_are_sanitized() {
    let server = HttpsMockServer::start(vec![
        MockResponse::truncated(429, r#"{"message":"rate","To":"+15551234567"}"#, 128),
        MockResponse::truncated(200, r#"{"sid":"SM"#, 128),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let api_err = account
        .message("SM123")
        .fetch()
        .await
        .expect_err("429 should be API error");
    assert!(matches!(api_err, TwilioError::Api { status: 429, .. }));
    let rendered = format!("{api_err:?}");
    assert!(!rendered.contains("+15551234567"));

    let transport_err = account
        .message("SM456")
        .fetch()
        .await
        .expect_err("truncated 200 should be transport error");
    assert!(matches!(transport_err, TwilioError::Transport(_)));
}

#[tokio::test]
async fn api_error_bodies_are_diagnostic_limited() {
    let tail = "tail-secret";
    let body = format!("{}{tail}", "x".repeat(10_000));
    let server = HttpsMockServer::start(vec![MockResponse::status_json(500, body)]).await;
    let client = client_for(&server);

    let err = client
        .account(test_creds())
        .message("SM123")
        .fetch()
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::Api { .. }));
    let TwilioError::Api { status, body } = err else {
        return;
    };

    assert_eq!(status, 500);
    assert!(body.len() <= 2051, "diagnostic body was not capped");
    assert!(body.ends_with('…'));
    assert!(!body.contains(tail));
}

#[test]
fn message_debug_output_redacts_sensitive_values() {
    let message = message_from_parts();
    assert_debug_redacts(
        &message,
        &[
            "secret body",
            "+15557654321",
            "+15551234567",
            "AC123",
            "MG123",
            "SM123",
            "/2010-04-01",
        ],
    );

    let page = TwilioMessagePage {
        messages: vec![message],
        next_page_uri: Some("/2010-04-01/Accounts/AC123/Messages.json?Page=1".to_owned()),
        first_page_uri: Some("/2010-04-01/Accounts/AC123/Messages.json?Page=0".to_owned()),
        previous_page_uri: None,
        uri: Some("/2010-04-01/Accounts/AC123/Messages.json?Page=0".to_owned()),
        page: Some(0),
        page_size: Some(50),
        start: Some(0),
        end: Some(0),
    };
    assert_debug_redacts(&page, &["secret body", "SM123", "/2010-04-01"]);

    assert_debug_redacts(&media_from_parts(), &["AC123", "SM123", "ME123", "/media"]);
    assert_debug_redacts(&feedback_from_parts(), &["AC123", "SM123", "/feedback"]);
}

#[test]
fn media_page_and_v1_meta_debug_output_redacts_sensitive_values() {
    let media_page = TwilioMediaPage {
        media: vec![media_from_parts()],
        next_page_uri: Some("/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=1".into()),
        first_page_uri: Some("/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=0".into()),
        previous_page_uri: None,
        uri: Some("/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=0".into()),
        page: Some(0),
        page_size: Some(50),
        start: Some(0),
        end: Some(0),
    };
    assert_debug_redacts(&media_page, &["AC123", "SM123", "ME123", "/2010-04-01"]);

    let meta = v1_meta_from_parts("services");
    assert_debug_redacts(
        &meta,
        &[
            "https://example.test/first",
            "https://example.test/previous",
            "https://example.test/next",
            "https://example.test/current",
        ],
    );
}

#[test]
fn service_debug_output_redacts_sensitive_values() {
    let service = service_from_parts();
    assert_debug_redacts(
        &service,
        &[
            "Friendly",
            "AC123",
            "MG123",
            "https://example.test/status",
            "https://example.test/phone_numbers",
        ],
    );

    let page = TwilioServicePage {
        services: vec![service],
        meta: v1_meta_from_parts("services"),
    };
    assert_debug_redacts(
        &page,
        &["Friendly", "AC123", "MG123", "https://example.test"],
    );
}

#[test]
fn service_subresource_debug_output_redacts_sensitive_values() {
    assert_debug_redacts(
        &TwilioServicePhoneNumberPage {
            phone_numbers: vec![phone_number_from_parts()],
            meta: v1_meta_from_parts("phone_numbers"),
        },
        &[
            "AC123",
            "MG123",
            "PN123",
            "+15551234567",
            "https://example.test",
        ],
    );
    assert_debug_redacts(
        &TwilioServiceShortCodePage {
            short_codes: vec![short_code_from_parts()],
            meta: v1_meta_from_parts("short_codes"),
        },
        &["AC123", "MG123", "SC123", "12345", "https://example.test"],
    );
    assert_debug_redacts(
        &TwilioAlphaSenderPage {
            alpha_senders: vec![alpha_sender_from_parts()],
            meta: v1_meta_from_parts("alpha_senders"),
        },
        &["AC123", "MG123", "AI123", "MyCo", "https://example.test"],
    );
    assert_debug_redacts(
        &TwilioChannelSenderPage {
            senders: vec![channel_sender_from_parts()],
            meta: v1_meta_from_parts("senders"),
        },
        &[
            "AC123",
            "MG123",
            "XE123",
            "whatsapp:+15551234567",
            "https://example.test",
        ],
    );
    assert_debug_redacts(
        &TwilioDestinationAlphaSenderPage {
            alpha_senders: vec![destination_alpha_sender_from_parts()],
            meta: v1_meta_from_parts("alpha_senders"),
        },
        &["AC123", "MG123", "AI123", "MyCo", "https://example.test"],
    );
}

#[test]
fn new_endpoint_debug_output_redacts_sensitive_values() {
    assert_debug_redacts(
        &TwilioDeactivation {
            redirect_to: Some("https://storage.example.test/deactivations?signature=secret".into()),
        },
        &["storage.example.test", "signature=secret"],
    );

    let short_code = account_short_code_from_parts();
    assert_debug_redacts(
        &short_code,
        &[
            "AC123",
            "SC123",
            "12345",
            "Alerts",
            "https://example.test/sms",
            "/2010-04-01",
        ],
    );
    assert_debug_redacts(
        &TwilioAccountShortCodePage {
            short_codes: vec![short_code],
            next_page_uri: Some("/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?Page=1".to_owned()),
            first_page_uri: Some(
                "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?Page=0".to_owned(),
            ),
            previous_page_uri: None,
            uri: Some("/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?Page=0".to_owned()),
            page: Some(0),
            page_size: Some(50),
            start: Some(0),
            end: Some(0),
        },
        &["AC123", "SC123", "12345", "/2010-04-01"],
    );

    let verification = tollfree_verification_from_parts();
    assert_debug_redacts(
        &verification,
        &[
            "HH123",
            "AC123",
            "PN123",
            "+18003334444",
            "Owl, Inc.",
            "Ada",
            "ada@example.test",
            "https://example.test/privacy",
            "Your code is 123456",
            "START",
            "https://example.test/opt.png",
        ],
    );
    assert_debug_redacts(
        &TwilioTollfreeVerificationPage {
            tollfree_verifications: vec![verification],
            meta: v1_meta_from_parts("verifications"),
        },
        &["HH123", "AC123", "PN123", "https://example.test"],
    );
}

fn full_message_json(sid: &str, status: &str, body: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "api_version": "2010-04-01",
            "body": "{body}",
            "date_created": "Fri, 24 May 2019 17:44:46 +0000",
            "date_sent": "Fri, 24 May 2019 17:44:50 +0000",
            "date_updated": "Fri, 24 May 2019 17:44:50 +0000",
            "direction": "outbound-api",
            "error_code": null,
            "error_message": null,
            "from": "+15557654321",
            "messaging_service_sid": "MG123",
            "num_media": "0",
            "num_segments": "1",
            "price": "-0.00750",
            "price_unit": "USD",
            "sid": "{sid}",
            "status": "{status}",
            "subresource_uris": {{
                "media": "/2010-04-01/Accounts/AC123/Messages/{sid}/Media.json",
                "feedback": "/2010-04-01/Accounts/AC123/Messages/{sid}/Feedback.json"
            }},
            "to": "+15551234567",
            "uri": "/2010-04-01/Accounts/AC123/Messages/{sid}.json"
        }}"#
    )
}

fn message_page_json(messages: &[String], next_page_uri: Option<&str>) -> String {
    let next = next_page_uri.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "messages": [{messages}],
            "next_page_uri": {next},
            "first_page_uri": "/2010-04-01/Accounts/AC123/Messages.json?Page=0",
            "previous_page_uri": null,
            "uri": "/2010-04-01/Accounts/AC123/Messages.json?Page=0",
            "page": 0,
            "page_size": 2,
            "start": 0,
            "end": 0
        }}"#,
        messages = messages.join(",")
    )
}

fn media_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "content_type": "image/jpeg",
            "date_created": "Sun, 16 Aug 2015 15:53:54 +0000",
            "date_updated": "Sun, 16 Aug 2015 15:53:55 +0000",
            "parent_sid": "SM123",
            "sid": "{sid}",
            "uri": "/2010-04-01/Accounts/AC123/Messages/SM123/Media/{sid}.json"
        }}"#
    )
}

fn service_json(sid: &str, friendly_name: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "friendly_name": "{friendly_name}",
            "sid": "{sid}",
            "date_created": "2015-07-30T20:12:31Z",
            "date_updated": "bad date",
            "sticky_sender": true,
            "mms_converter": true,
            "smart_encoding": false,
            "fallback_to_long_code": true,
            "scan_message_content": "inherit",
            "synchronous_validation": true,
            "area_code_geomatch": true,
            "validity_period": 600,
            "inbound_request_url": "https://www.example.com/",
            "inbound_method": "POST",
            "fallback_url": null,
            "fallback_method": "POST",
            "status_callback": "https://www.example.com",
            "usecase": "marketing",
            "us_app_to_person_registered": false,
            "use_inbound_webhook_on_number": false,
            "links": {{
                "phone_numbers": "https://example.test/phone_numbers"
            }},
            "url": "https://messaging.twilio.com/v1/Services/{sid}"
        }}"#
    )
}

fn service_page_json(
    base_url: &str,
    key: &str,
    collection_key: &str,
    services: &[String],
    next_path: Option<&str>,
) -> String {
    page_json(base_url, key, collection_key, services, next_path)
}

fn page_json(
    base_url: &str,
    key: &str,
    collection_key: &str,
    items: &[String],
    next_path: Option<&str>,
) -> String {
    let next = next_path.map_or_else(
        || "null".to_owned(),
        |path| format!(r#""{base_url}{path}""#),
    );
    format!(
        r#"{{
            "meta": {{
                "page": 0,
                "page_size": 2,
                "first_page_url": "{base_url}/v1/{collection_key}?PageSize=2&Page=0",
                "previous_page_url": null,
                "next_page_url": {next},
                "key": "{key}",
                "url": "{base_url}/v1/{collection_key}?PageSize=2&Page=0"
            }},
            "{key}": [{items}]
        }}"#,
        items = items.join(",")
    )
}

fn server_url_placeholder() -> String {
    "__BASE_URL__".to_owned()
}

fn phone_number_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MG123",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "phone_number":"+15551234567",
            "country_code":"US",
            "capabilities":["SMS","MMS"],
            "url":"https://messaging.twilio.com/v1/Services/MG123/PhoneNumbers/{sid}"
        }}"#
    )
}

fn short_code_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MG123",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "short_code":"12345",
            "country_code":"US",
            "capabilities":["SMS"],
            "url":"https://messaging.twilio.com/v1/Services/MG123/ShortCodes/{sid}"
        }}"#
    )
}

fn alpha_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MG123",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "alpha_sender":"MyCo",
            "capabilities":["SMS"],
            "url":"https://messaging.twilio.com/v1/Services/MG123/AlphaSenders/{sid}"
        }}"#
    )
}

fn channel_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "messaging_service_sid":"MG123",
            "sid":"{sid}",
            "sender":"whatsapp:+15551234567",
            "sender_type":"WhatsApp",
            "country_code":"US",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "url":"https://messaging.twilio.com/v1/Services/MG123/ChannelSenders/{sid}"
        }}"#
    )
}

fn destination_alpha_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MG123",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "alpha_sender":"MyCo",
            "capabilities":["SMS"],
            "iso_country_code":"FR",
            "url":"https://messaging.twilio.com/v1/Services/MG123/DestinationAlphaSenders/{sid}"
        }}"#
    )
}

fn phone_number_page_json(sid: &str, next: Option<&str>) -> String {
    page_json(
        "__BASE_URL__",
        "phone_numbers",
        "Services/MG123/PhoneNumbers",
        &[phone_number_json(sid)],
        next.map(|_| "/v1/Services/MG123/PhoneNumbers?PageSize=2&Page=1&PageToken=next"),
    )
}

fn phone_number_empty_page_json() -> String {
    page_json(
        "__BASE_URL__",
        "phone_numbers",
        "Services/MG123/PhoneNumbers",
        &[],
        None,
    )
}

fn short_code_page_json(sid: &str, next: Option<&str>) -> String {
    page_json(
        "__BASE_URL__",
        "short_codes",
        "Services/MG123/ShortCodes",
        &[short_code_json(sid)],
        next.map(|_| "/v1/Services/MG123/ShortCodes?PageSize=2&Page=1&PageToken=next"),
    )
}

fn short_code_empty_page_json() -> String {
    page_json(
        "__BASE_URL__",
        "short_codes",
        "Services/MG123/ShortCodes",
        &[],
        None,
    )
}

fn alpha_sender_page_json(sid: &str, next: Option<&str>) -> String {
    page_json(
        "__BASE_URL__",
        "alpha_senders",
        "Services/MG123/AlphaSenders",
        &[alpha_sender_json(sid)],
        next.map(|_| "/v1/Services/MG123/AlphaSenders?PageSize=2&Page=1&PageToken=next"),
    )
}

fn alpha_sender_empty_page_json() -> String {
    page_json(
        "__BASE_URL__",
        "alpha_senders",
        "Services/MG123/AlphaSenders",
        &[],
        None,
    )
}

fn channel_sender_page_json(sid: &str, next: Option<&str>) -> String {
    page_json(
        "__BASE_URL__",
        "senders",
        "Services/MG123/ChannelSenders",
        &[channel_sender_json(sid)],
        next.map(|_| "/v1/Services/MG123/ChannelSenders?PageSize=2&Page=1&PageToken=next"),
    )
}

fn channel_sender_empty_page_json() -> String {
    page_json(
        "__BASE_URL__",
        "senders",
        "Services/MG123/ChannelSenders",
        &[],
        None,
    )
}

fn destination_alpha_sender_page_json(sid: &str, next: Option<&str>) -> String {
    page_json(
        "__BASE_URL__",
        "alpha_senders",
        "Services/MG123/DestinationAlphaSenders",
        &[destination_alpha_sender_json(sid)],
        next.map(|_| {
            "/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=2&Page=1&PageToken=next"
        }),
    )
}

fn destination_alpha_sender_empty_page_json() -> String {
    page_json(
        "__BASE_URL__",
        "alpha_senders",
        "Services/MG123/DestinationAlphaSenders",
        &[],
        None,
    )
}

fn account_short_code_json(sid: &str, friendly_name: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "api_version": "2010-04-01",
            "date_created": "Thu, 01 Apr 2010 00:00:00 +0000",
            "date_updated": "Thu, 01 Apr 2010 00:00:00 +0000",
            "friendly_name": "{friendly_name}",
            "short_code": "12345",
            "sid": "{sid}",
            "sms_fallback_method": "POST",
            "sms_fallback_url": "https://example.test/fallback",
            "sms_method": "POST",
            "sms_url": "https://example.test/sms",
            "uri": "/2010-04-01/Accounts/AC123/SMS/ShortCodes/{sid}.json"
        }}"#
    )
}

fn account_short_code_page_json(short_codes: &[String], next_page_uri: Option<&str>) -> String {
    let next = next_page_uri.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "short_codes": [{short_codes}],
            "next_page_uri": {next},
            "first_page_uri": "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?Page=0",
            "previous_page_uri": null,
            "uri": "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?Page=0",
            "page": 0,
            "page_size": 2,
            "start": 0,
            "end": 0
        }}"#,
        short_codes = short_codes.join(",")
    )
}

fn tollfree_verification_json(sid: &str, status: &str) -> String {
    format!(
        r#"{{
            "sid": "{sid}",
            "account_sid": "AC123",
            "customer_profile_sid": "BUcustomer",
            "trust_product_sid": "BUtrust",
            "regulated_item_sid": "RA123",
            "date_created": "2021-01-27T14:18:35Z",
            "date_updated": "2021-01-27T14:18:36Z",
            "business_name": "Owl, Inc.",
            "business_street_address": "123 Main Street",
            "business_street_address2": "Suite 101",
            "business_city": "Detroit",
            "business_state_province_region": "MI",
            "business_postal_code": "48201",
            "business_country": "US",
            "business_website": "https://example.test",
            "business_contact_first_name": "Ada",
            "business_contact_last_name": "Lovelace",
            "business_contact_email": "ada@example.test",
            "business_contact_phone": "+15551234567",
            "notification_email": "support@example.test",
            "use_case_categories": ["TWO_FACTOR_AUTHENTICATION", "MARKETING"],
            "use_case_summary": "Account security and marketing alerts",
            "production_message_sample": "Your code is 123456",
            "opt_in_image_urls": ["https://example.test/opt.png"],
            "opt_in_type": "VERBAL",
            "message_volume": "1,000",
            "additional_information": "Additional context",
            "tollfree_phone_number_sid": "PN123",
            "tollfree_phone_number": "+18003334444",
            "status": "{status}",
            "rejection_reason": null,
            "error_code": null,
            "edit_expiration": null,
            "edit_allowed": true,
            "rejection_reasons": null,
            "resource_links": {{
                "customer_profile": "https://trusthub.twilio.com/v1/CustomerProfiles/BUcustomer"
            }},
            "url": "https://messaging.twilio.com/v1/Tollfree/Verifications/{sid}",
            "external_reference_id": "external-123",
            "business_registration_number": "123456789",
            "business_registration_authority": "EIN",
            "business_registration_country": "US",
            "business_type": "PRIVATE_PROFIT",
            "business_registration_phone_number": "+15557654321",
            "doing_business_as": "Owl Alerts",
            "age_gated_content": false,
            "help_message_sample": "Reply HELP for help",
            "opt_in_confirmation_message": "Thanks for opting in",
            "opt_in_keywords": ["START"],
            "privacy_policy_url": "https://example.test/privacy",
            "terms_and_conditions_url": "https://example.test/terms",
            "vetting_id": "vetting-123",
            "vetting_id_expiration": null,
            "vetting_provider": "CAMPAIGN_VERIFY"
        }}"#
    )
}

fn tollfree_verification_page_json(
    verifications: &[String],
    next_page_url: Option<&str>,
) -> String {
    let next = next_page_url.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "meta": {{
                "page": 0,
                "page_size": 2,
                "first_page_url": "__BASE_URL__/v1/Tollfree/Verifications?PageSize=2&Page=0",
                "previous_page_url": null,
                "next_page_url": {next},
                "key": "verifications",
                "url": "__BASE_URL__/v1/Tollfree/Verifications?PageSize=2&Page=0"
            }},
            "verifications": [{verifications}]
        }}"#,
        verifications = verifications.join(",")
    )
}

fn service_from_parts() -> twilio2::TwilioService {
    twilio2::TwilioService {
        account_sid: Some("AC123".to_owned()),
        friendly_name: Some("Friendly".to_owned()),
        sid: Some("MG123".to_owned()),
        date_created: None,
        date_updated: None,
        sticky_sender: Some(true),
        mms_converter: Some(false),
        smart_encoding: Some(true),
        fallback_to_long_code: Some(false),
        scan_message_content: Some("inherit".to_owned()),
        synchronous_validation: Some(false),
        area_code_geomatch: Some(true),
        validity_period: Some(600),
        inbound_request_url: Some("https://example.test/inbound".to_owned()),
        inbound_method: Some("POST".to_owned()),
        fallback_url: Some("https://example.test/fallback".to_owned()),
        fallback_method: Some("GET".to_owned()),
        status_callback: Some("https://example.test/status".to_owned()),
        usecase: Some("marketing".to_owned()),
        us_app_to_person_registered: Some(false),
        use_inbound_webhook_on_number: Some(true),
        links: Some(BTreeMap::from([(
            "phone_numbers".to_owned(),
            "https://example.test/phone_numbers".to_owned(),
        )])),
        url: Some("https://messaging.twilio.com/v1/Services/MG123".to_owned()),
    }
}

fn message_from_parts() -> TwilioMessage {
    TwilioMessage {
        body: Some("secret body".to_owned()),
        num_segments: Some("1".to_owned()),
        direction: Some("outbound-api".to_owned()),
        from: Some("+15557654321".to_owned()),
        to: Some("+15551234567".to_owned()),
        date_updated: None,
        price: Some("-0.00750".to_owned()),
        error_message: None,
        uri: Some("/2010-04-01/Accounts/AC123/Messages/SM123.json".to_owned()),
        account_sid: Some("AC123".to_owned()),
        num_media: Some("0".to_owned()),
        status: Some("sent".to_owned()),
        messaging_service_sid: Some("MG123".to_owned()),
        sid: Some("SM123".to_owned()),
        date_sent: None,
        date_created: None,
        error_code: None,
        price_unit: Some("USD".to_owned()),
        api_version: Some("2010-04-01".to_owned()),
        subresource_uris: Some(BTreeMap::from([(
            "media".to_owned(),
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json".to_owned(),
        )])),
    }
}

fn media_from_parts() -> TwilioMedia {
    TwilioMedia {
        account_sid: Some("AC123".to_owned()),
        content_type: Some("image/jpeg".to_owned()),
        date_created: None,
        date_updated: None,
        parent_sid: Some("SM123".to_owned()),
        sid: Some("ME123".to_owned()),
        uri: Some("/media/ME123".to_owned()),
    }
}

fn feedback_from_parts() -> TwilioMessageFeedback {
    TwilioMessageFeedback {
        account_sid: Some("AC123".to_owned()),
        message_sid: Some("SM123".to_owned()),
        outcome: Some("confirmed".to_owned()),
        date_created: None,
        date_updated: None,
        uri: Some("/feedback/SM123".to_owned()),
    }
}

fn v1_meta_from_parts(key: &str) -> V1PageMeta {
    V1PageMeta {
        page: Some(0),
        page_size: Some(50),
        first_page_url: Some("https://example.test/first".to_owned()),
        previous_page_url: Some("https://example.test/previous".to_owned()),
        next_page_url: Some("https://example.test/next".to_owned()),
        key: Some(key.to_owned()),
        url: Some("https://example.test/current".to_owned()),
    }
}

fn phone_number_from_parts() -> TwilioServicePhoneNumber {
    TwilioServicePhoneNumber {
        account_sid: Some("AC123".to_owned()),
        service_sid: Some("MG123".to_owned()),
        sid: Some("PN123".to_owned()),
        date_created: None,
        date_updated: None,
        phone_number: Some("+15551234567".to_owned()),
        country_code: Some("US".to_owned()),
        capabilities: Some(vec!["SMS".to_owned(), "MMS".to_owned()]),
        url: Some("https://example.test/phone_numbers/PN123".to_owned()),
    }
}

fn short_code_from_parts() -> TwilioServiceShortCode {
    TwilioServiceShortCode {
        account_sid: Some("AC123".to_owned()),
        service_sid: Some("MG123".to_owned()),
        sid: Some("SC123".to_owned()),
        date_created: None,
        date_updated: None,
        short_code: Some("12345".to_owned()),
        country_code: Some("US".to_owned()),
        capabilities: Some(vec!["SMS".to_owned()]),
        url: Some("https://example.test/short_codes/SC123".to_owned()),
    }
}

fn alpha_sender_from_parts() -> TwilioAlphaSender {
    TwilioAlphaSender {
        account_sid: Some("AC123".to_owned()),
        service_sid: Some("MG123".to_owned()),
        sid: Some("AI123".to_owned()),
        date_created: None,
        date_updated: None,
        alpha_sender: Some("MyCo".to_owned()),
        capabilities: Some(vec!["SMS".to_owned()]),
        url: Some("https://example.test/alpha_senders/AI123".to_owned()),
    }
}

fn channel_sender_from_parts() -> TwilioChannelSender {
    TwilioChannelSender {
        account_sid: Some("AC123".to_owned()),
        messaging_service_sid: Some("MG123".to_owned()),
        sid: Some("XE123".to_owned()),
        sender: Some("whatsapp:+15551234567".to_owned()),
        sender_type: Some("WhatsApp".to_owned()),
        country_code: Some("US".to_owned()),
        date_created: None,
        date_updated: None,
        url: Some("https://example.test/channel_senders/XE123".to_owned()),
    }
}

fn destination_alpha_sender_from_parts() -> TwilioDestinationAlphaSender {
    TwilioDestinationAlphaSender {
        sid: Some("AI123".to_owned()),
        account_sid: Some("AC123".to_owned()),
        service_sid: Some("MG123".to_owned()),
        date_created: None,
        date_updated: None,
        alpha_sender: Some("MyCo".to_owned()),
        capabilities: Some(vec!["SMS".to_owned()]),
        url: Some("https://example.test/destination_alpha_senders/AI123".to_owned()),
        iso_country_code: Some("FR".to_owned()),
    }
}

fn account_short_code_from_parts() -> TwilioAccountShortCode {
    TwilioAccountShortCode {
        account_sid: Some("AC123".to_owned()),
        api_version: Some("2010-04-01".to_owned()),
        date_created: None,
        date_updated: None,
        friendly_name: Some("Alerts".to_owned()),
        short_code: Some("12345".to_owned()),
        sid: Some("SC123".to_owned()),
        sms_fallback_method: Some("POST".to_owned()),
        sms_fallback_url: Some("https://example.test/fallback".to_owned()),
        sms_method: Some("POST".to_owned()),
        sms_url: Some("https://example.test/sms".to_owned()),
        uri: Some("/2010-04-01/Accounts/AC123/SMS/ShortCodes/SC123.json".to_owned()),
    }
}

fn tollfree_verification_from_parts() -> TwilioTollfreeVerification {
    TwilioTollfreeVerification {
        sid: Some("HH123".to_owned()),
        account_sid: Some("AC123".to_owned()),
        customer_profile_sid: Some("BUcustomer".to_owned()),
        trust_product_sid: Some("BUtrust".to_owned()),
        regulated_item_sid: Some("RA123".to_owned()),
        date_created: None,
        date_updated: None,
        business_name: Some("Owl, Inc.".to_owned()),
        business_street_address: Some("123 Main Street".to_owned()),
        business_street_address2: Some("Suite 101".to_owned()),
        business_city: Some("Detroit".to_owned()),
        business_state_province_region: Some("MI".to_owned()),
        business_postal_code: Some("48201".to_owned()),
        business_country: Some("US".to_owned()),
        business_website: Some("https://example.test".to_owned()),
        business_contact_first_name: Some("Ada".to_owned()),
        business_contact_last_name: Some("Lovelace".to_owned()),
        business_contact_email: Some("ada@example.test".to_owned()),
        business_contact_phone: Some("+15551234567".to_owned()),
        notification_email: Some("support@example.test".to_owned()),
        use_case_categories: Some(vec![
            "TWO_FACTOR_AUTHENTICATION".to_owned(),
            "MARKETING".to_owned(),
        ]),
        use_case_summary: Some("Account security and marketing alerts".to_owned()),
        production_message_sample: Some("Your code is 123456".to_owned()),
        opt_in_image_urls: Some(vec!["https://example.test/opt.png".to_owned()]),
        opt_in_type: Some("VERBAL".to_owned()),
        message_volume: Some("1,000".to_owned()),
        additional_information: Some("Additional context".to_owned()),
        tollfree_phone_number_sid: Some("PN123".to_owned()),
        tollfree_phone_number: Some("+18003334444".to_owned()),
        status: Some("TWILIO_APPROVED".to_owned()),
        rejection_reason: Some("Rejected for secret reason".to_owned()),
        error_code: Some(30_000),
        edit_expiration: None,
        edit_allowed: Some(true),
        rejection_reasons: Some(serde_json::json!({"secret": "reason"})),
        resource_links: Some(BTreeMap::from([(
            "customer_profile".to_owned(),
            "https://trusthub.twilio.com/v1/CustomerProfiles/BUcustomer".to_owned(),
        )])),
        url: Some("https://messaging.twilio.com/v1/Tollfree/Verifications/HH123".to_owned()),
        external_reference_id: Some("external-123".to_owned()),
        business_registration_number: Some("123456789".to_owned()),
        business_registration_authority: Some("EIN".to_owned()),
        business_registration_country: Some("US".to_owned()),
        business_type: Some("PRIVATE_PROFIT".to_owned()),
        business_registration_phone_number: Some("+15557654321".to_owned()),
        doing_business_as: Some("Owl Alerts".to_owned()),
        age_gated_content: Some(false),
        help_message_sample: Some("Reply HELP for help".to_owned()),
        opt_in_confirmation_message: Some("Thanks for opting in".to_owned()),
        opt_in_keywords: Some(vec!["START".to_owned()]),
        privacy_policy_url: Some("https://example.test/privacy".to_owned()),
        terms_and_conditions_url: Some("https://example.test/terms".to_owned()),
        vetting_id: Some("vetting-123".to_owned()),
        vetting_id_expiration: None,
        vetting_provider: Some("CAMPAIGN_VERIFY".to_owned()),
    }
}
