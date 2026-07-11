#![cfg(feature = "async")]
#![allow(clippy::unwrap_used, clippy::missing_panics_doc)]

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rcgen::CertifiedKey;
use reqwest::Method;
use rust_decimal::Decimal;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use twilio2::{
    A2PBrandType, A2PUsecase, A2PVettingProvider, AddressRetention, ApiFamily, AppleTypingEvent,
    BulkConsentsRequest, BulkContactsRequest, ChannelSenderConfiguration, ChannelSenderHttpMethod,
    ChannelSenderProfile, ChannelSenderProfileEmail, ChannelSenderWebhook, ConsentItem,
    ConsentSource, ConsentStatus, ContactItem, ContentRetention, CreateA2PBrandRegistrationRequest,
    CreateA2PBrandVettingRequest, CreateAlphaSenderRequest, CreateChannelSenderRequest,
    CreateDestinationAlphaSenderRequest, CreateMessageFeedbackRequest, CreateMessageRequest,
    CreateMessagingV2ChannelSenderRequest, CreateMessagingV2TypingIndicatorRequest,
    CreateMessagingV3TypingIndicatorRequest, CreatePreregisteredUsa2pRequest,
    CreateServicePhoneNumberRequest, CreateServiceRequest, CreateServiceShortCodeRequest,
    CreateTollfreeVerificationRequest, CreateUsa2pRequest, FetchDeactivationsRequest,
    FetchPricingOriginBasedNumberRequest, FetchUsa2pUsecasesRequest, HttpMethod,
    ListA2PBrandRegistrationsRequest, ListA2PBrandVettingsRequest, ListAccountShortCodesRequest,
    ListDestinationAlphaSendersRequest, ListMediaRequest, ListMessagesRequest,
    ListMessagingGeoPermissionsRequest, ListMessagingV2ChannelSendersRequest,
    ListPricingCountriesRequest, ListPricingMessagingCountriesRequest,
    ListServiceSubresourcesRequest, ListServicesRequest, ListTollfreeVerificationsRequest,
    ListUsa2pRequest, MessageFeedbackOutcome, MessageIntent, MessagingGeoPermissionUpdateItem,
    MessagingV2Channel, Operation, RawResponse, RequestOptions, RequestSpec, RetryPolicy,
    RiskCheck, SafeListNumberRequest, ScanMessageContent, ScheduleType, ServiceUsecase,
    TollfreeBusinessRegistrationAuthority, TollfreeBusinessType, TollfreeMessageVolume,
    TollfreeOptInType, TollfreeUseCaseCategory, TollfreeVerificationStatus,
    TollfreeVettingProvider, TrafficType, TwilioA2PBrandRegistration,
    TwilioA2PBrandRegistrationPage, TwilioA2PBrandVetting, TwilioA2PBrandVettingPage,
    TwilioAccountShortCode, TwilioAccountShortCodePage, TwilioAlphaSender, TwilioAlphaSenderPage,
    TwilioAuth, TwilioBulkConsentResult, TwilioBulkConsentsResponse, TwilioBulkContactResult,
    TwilioBulkContactsResponse, TwilioChannelSender, TwilioChannelSenderPage, TwilioClient,
    TwilioClientConfig, TwilioConfig, TwilioDeactivation, TwilioDestinationAlphaSender,
    TwilioDestinationAlphaSenderPage, TwilioError, TwilioInboundSmsPrice, TwilioMedia,
    TwilioMediaPage, TwilioMessage, TwilioMessageFeedback, TwilioMessagePage,
    TwilioOutboundSmsPrice, TwilioPricingMessaging, TwilioPricingMessagingCountry,
    TwilioPricingMessagingCountryPage, TwilioPricingMessagingCountrySummary, TwilioSafeListNumber,
    TwilioServicePage, TwilioServicePhoneNumber, TwilioServicePhoneNumberPage,
    TwilioServiceShortCode, TwilioServiceShortCodePage, TwilioSmsPrice, TwilioTollfreeVerification,
    TwilioTollfreeVerificationPage, TwilioUsa2p, TwilioUsa2pPage, UpdateAccountShortCodeRequest,
    UpdateLinkShorteningDomainCertificateRequest, UpdateLinkShorteningDomainConfigRequest,
    UpdateMessageRequest, UpdateMessagingGeoPermissionsRequest,
    UpdateMessagingV2ChannelSenderRequest, UpdateServiceRequest, UpdateTollfreeVerificationRequest,
    V1PageMeta,
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

fn test_http_client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    install_test_crypto_provider();

    builder.danger_accept_invalid_certs(true).no_proxy()
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

fn test_creds() -> &'static TwilioAuth {
    static CREDS: LazyLock<TwilioAuth> = LazyLock::new(|| TwilioAuth::auth_token("AC123", "token"));
    &CREDS
}

fn client_for(server: &HttpsMockServer) -> TwilioClient {
    TwilioClient::from_config_with_http_builder(
        TwilioClientConfig::new().base_urls(
            TwilioConfig::new()
                .rest_base_url(&server.base_url)
                .messaging_base_url(&server.base_url)
                .pricing_base_url(&server.base_url)
                .accounts_base_url(format!("{}/v1", server.base_url)),
        ),
        test_http_client,
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

#[tokio::test]
async fn pricing_messaging_fetch_and_country_detail_decode_prices() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(pricing_messaging_json()),
        MockResponse::json(pricing_country_detail_json("US")),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let messaging: TwilioPricingMessaging =
        account.pricing().v1().messaging().fetch().await.unwrap();
    assert_eq!(messaging.name.as_deref(), Some("Messaging"));
    assert!(messaging.links.as_ref().unwrap().contains_key("countries"));
    assert_debug_redacts(&messaging, &["pricing.twilio.com", "Countries"]);

    let country: TwilioPricingMessagingCountry = account
        .pricing()
        .v1()
        .messaging()
        .countries()
        .fetch("us")
        .await
        .unwrap();
    assert_eq!(country.country.as_deref(), Some("United States"));
    assert_eq!(country.iso_country.as_deref(), Some("US"));
    assert_eq!(country.price_unit.as_deref(), Some("USD"));
    let inbound: &TwilioInboundSmsPrice = &country.inbound_sms_prices[0];
    assert_eq!(inbound.base_price, Some(Decimal::new(5, 2)));
    assert_eq!(inbound.current_price, Some(Decimal::new(4, 2)));
    let outbound: &TwilioOutboundSmsPrice = &country.outbound_sms_prices[0];
    assert_eq!(outbound.carrier.as_deref(), Some("att"));
    let sms_price: &TwilioSmsPrice = &outbound.prices[0];
    assert_eq!(sms_price.base_price, Some(Decimal::new(5, 2)));
    assert_eq!(sms_price.current_price, Some(Decimal::new(45, 3)));
    assert_debug_redacts(&country, &["pricing.twilio.com"]);

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/Messaging");
    assert_eq!(requests[1].path, "/v1/Messaging/Countries/US");
    assert_basic_auth(&requests[0]);
    assert_basic_auth(&requests[1]);
}

#[tokio::test]
async fn pricing_messaging_countries_list_paginates_and_list_all() {
    let next_page_url = "__BASE_URL__/v1/Messaging/Countries?PageSize=2&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(pricing_country_page_json(
            &[pricing_country_summary_json("United States", "US")],
            Some(next_page_url),
            0,
            2,
        )),
        MockResponse::json(pricing_country_page_json(
            &[pricing_country_summary_json("Canada", "CA")],
            None,
            1,
            2,
        )),
        MockResponse::json(pricing_country_page_json(
            &[pricing_country_summary_json("United States", "US")],
            Some(next_page_url),
            0,
            2,
        )),
        MockResponse::json(pricing_country_page_json(
            &[pricing_country_summary_json("Canada", "CA")],
            None,
            1,
            2,
        )),
    ])
    .await;
    let client = client_for(&server);
    let countries = client
        .account(test_creds())
        .pricing()
        .v1()
        .messaging()
        .countries();

    let first: TwilioPricingMessagingCountryPage = countries
        .list(
            ListPricingMessagingCountriesRequest::new()
                .page_size(2)
                .page(0),
        )
        .await
        .unwrap();
    assert_eq!(first.meta.key.as_deref(), Some("countries"));
    let first_country: &TwilioPricingMessagingCountrySummary = &first.countries[0];
    assert_eq!(first_country.iso_country.as_deref(), Some("US"));
    let next = first.meta.next_page_url.as_deref().unwrap();
    let second = countries.list_page_url(next).await.unwrap();
    assert_eq!(second.countries[0].iso_country.as_deref(), Some("CA"));

    let all = countries
        .list_all_with(ListPricingMessagingCountriesRequest::new().page_size(2))
        .collect_all()
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].iso_country.as_deref(), Some("US"));
    assert_eq!(all[1].iso_country.as_deref(), Some("CA"));

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].path,
        "/v1/Messaging/Countries?PageSize=2&Page=0"
    );
    assert_eq!(
        requests[1].path,
        "/v1/Messaging/Countries?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[2].path, "/v1/Messaging/Countries?PageSize=2");
    assert_eq!(
        requests[3].path,
        "/v1/Messaging/Countries?PageSize=2&Page=1&PageToken=next"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn pricing_completion_resources_use_versioned_product_roots() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(pricing_generic_country_page_json(
            "v1/PhoneNumbers/Countries",
            None,
            0,
            2,
        )),
        MockResponse::json(pricing_phone_number_country_json("US")),
        MockResponse::json(pricing_generic_country_page_json(
            "v1/Voice/Countries",
            None,
            0,
            2,
        )),
        MockResponse::json(pricing_voice_country_json("US")),
        MockResponse::json(pricing_generic_country_page_json(
            "v2/Voice/Countries",
            None,
            0,
            2,
        )),
        MockResponse::json(pricing_origin_voice_country_json("US")),
        MockResponse::json(pricing_origin_voice_number_json("15551234567")),
        MockResponse::json(pricing_generic_country_page_json(
            "v2/Trunking/Countries",
            None,
            0,
            2,
        )),
        MockResponse::json(pricing_trunking_country_json("US")),
        MockResponse::json(pricing_trunking_number_json("15551234567")),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let phone_numbers = account.pricing().v1().phone_numbers().countries();
    let phone_page = phone_numbers
        .list(ListPricingCountriesRequest::new().page_size(2))
        .await
        .unwrap();
    assert_eq!(phone_page.countries[0].iso_country.as_deref(), Some("US"));
    let phone_country = phone_numbers.fetch("us").await.unwrap();
    assert_eq!(phone_country.phone_number_prices.len(), 1);

    let v1_voice = account.pricing().v1().voice();
    v1_voice
        .countries()
        .list(ListPricingCountriesRequest::new().page_size(2))
        .await
        .unwrap();
    let v1_voice_country = v1_voice.countries().fetch("US").await.unwrap();
    assert_eq!(v1_voice_country.outbound_prefix_prices.len(), 1);

    let v2_voice = account.pricing().v2().voice();
    v2_voice
        .countries()
        .list(ListPricingCountriesRequest::new().page_size(2))
        .await
        .unwrap();
    let v2_voice_country = v2_voice.countries().fetch("US").await.unwrap();
    assert_eq!(v2_voice_country.outbound_prefix_prices.len(), 1);
    let v2_voice_number = v2_voice
        .number("15551234567")
        .fetch(FetchPricingOriginBasedNumberRequest::new().origination_number("15550001111"))
        .await
        .unwrap();
    assert_eq!(
        v2_voice_number.destination_number.as_deref(),
        Some("15551234567")
    );

    let trunking = account.pricing().v2().trunking();
    trunking
        .countries()
        .list(ListPricingCountriesRequest::new().page_size(2))
        .await
        .unwrap();
    let trunking_country = trunking.countries().fetch("US").await.unwrap();
    assert_eq!(trunking_country.terminating_prefix_prices.len(), 1);
    let trunking_number = trunking
        .number("15551234567")
        .fetch(FetchPricingOriginBasedNumberRequest::new().origination_number("15550001111"))
        .await
        .unwrap();
    assert_eq!(
        trunking_number.destination_number.as_deref(),
        Some("15551234567")
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 10);
    let expected_paths = [
        "/v1/PhoneNumbers/Countries?PageSize=2",
        "/v1/PhoneNumbers/Countries/US",
        "/v1/Voice/Countries?PageSize=2",
        "/v1/Voice/Countries/US",
        "/v2/Voice/Countries?PageSize=2",
        "/v2/Voice/Countries/US",
        "/v2/Voice/Numbers/15551234567?OriginationNumber=15550001111",
        "/v2/Trunking/Countries?PageSize=2",
        "/v2/Trunking/Countries/US",
        "/v2/Trunking/Numbers/15551234567?OriginationNumber=15550001111",
    ];
    for (request, path) in requests.iter().zip(expected_paths) {
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, path);
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn pricing_messaging_countries_validate_requests_and_page_urls() {
    let server = HttpsMockServer::start(vec![MockResponse::json(pricing_country_page_json(
        &[pricing_country_summary_json("United States", "US")],
        Some("__BASE_URL__/v1/Messaging/Countries?PageSize=3&Page=1&PageToken=next"),
        0,
        2,
    ))])
    .await;
    let client = client_for(&server);
    let countries = client
        .account(test_creds())
        .pricing()
        .v1()
        .messaging()
        .countries();

    let err = countries
        .list(ListPricingMessagingCountriesRequest::new().page_size(0))
        .await
        .unwrap_err();
    assert_invalid_request(err, "PageSize");

    let err = countries.fetch("usa").await.unwrap_err();
    assert_invalid_request(err, "IsoCountry");

    let err = countries
        .list_page_url("https://example.test/v1/Messaging/Countries?Page=1")
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    let err = countries
        .list(ListPricingMessagingCountriesRequest::new().page_size(2))
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    assert!(server.requests().len() == 1);
}

#[tokio::test]
async fn pricing_continuation_api_errors_redact_page_url() {
    let page_token = "pricing-cursor-secret";
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        400,
        format!(
            r#"{{"message":"bad cursor __BASE_URL__/v1/Messaging/Countries?PageSize=2&Page=1&PageToken={page_token}"}}"#
        ),
    )])
    .await;
    let next_page_url = format!(
        "{}/v1/Messaging/Countries?PageSize=2&Page=1&PageToken={page_token}",
        server.base_url
    );

    let err = client_for(&server)
        .account(test_creds())
        .pricing()
        .v1()
        .messaging()
        .countries()
        .list_page_url(&next_page_url)
        .await
        .unwrap_err();

    assert_api_error_redacted(err, 400, &[&next_page_url, page_token]);
}

#[test]
fn constructors_config_and_debug_are_ergonomic_and_redacted() {
    let config = TwilioClientConfig::new()
        .rest_base_url("https://proxy.example.test/rest")
        .messaging_base_url("https://proxy.example.test/messaging")
        .pricing_base_url("https://proxy.example.test/pricing")
        .timeout(Duration::from_secs(7))
        .user_agent("test-agent/1.0");

    let client =
        TwilioClient::from_config_with_http_builder(config.clone(), test_http_client).unwrap();
    let retained = client.config();
    assert_eq!(
        retained.rest_base_url_ref(),
        "https://proxy.example.test/rest"
    );
    assert_eq!(
        retained.messaging_base_url_ref(),
        "https://proxy.example.test/messaging"
    );
    assert_eq!(
        retained.pricing_base_url_ref(),
        "https://proxy.example.test/pricing"
    );
    assert_debug_redacts(&config, &["proxy.example.test"]);
    assert_debug_redacts(&retained, &["proxy.example.test"]);

    let default = TwilioClient::from_config(TwilioClientConfig::default()).unwrap();
    assert_eq!(
        default.config().rest_base_url_ref(),
        twilio2::DEFAULT_REST_BASE_URL
    );
    assert_eq!(
        default.config().pricing_base_url_ref(),
        twilio2::DEFAULT_PRICING_BASE_URL
    );
}

#[tokio::test]
async fn custom_http_builder_cannot_enable_redirects() {
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(307, r"{}").header("location", "/redirected"),
    ])
    .await;
    let client = TwilioClient::from_config_with_http_builder(
        TwilioClientConfig::new().base_urls(
            TwilioConfig::new()
                .rest_base_url(&server.base_url)
                .messaging_base_url(&server.base_url)
                .pricing_base_url(&server.base_url)
                .accounts_base_url(format!("{}/v1", server.base_url)),
        ),
        |builder| test_http_client(builder).redirect(reqwest::redirect::Policy::limited(10)),
    )
    .unwrap();

    let error = client
        .account(test_creds())
        .messages()
        .create(
            CreateMessageRequest::new("+15551234567")
                .from("+15557654321")
                .body("secret body"),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::Api { status: 307, .. }));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn messaging_and_pricing_config_reject_versioned_product_roots() {
    let messaging_err = RequestOptions::new()
        .try_messaging_base_url("https://proxy.example.test/messaging/v1")
        .unwrap_err();
    assert!(
        matches!(messaging_err, TwilioError::InvalidBaseUrl(message) if message.contains("product roots"))
    );

    let pricing_err = RequestOptions::new()
        .try_pricing_base_url("https://proxy.example.test/pricing/v2")
        .unwrap_err();
    assert!(
        matches!(pricing_err, TwilioError::InvalidBaseUrl(message) if message.contains("product roots"))
    );

    let constructor_err = TwilioClient::from_config_with_http_builder(
        TwilioClientConfig::new()
            .messaging_base_url("https://proxy.example.test/messaging/v3")
            .pricing_base_url("https://proxy.example.test/pricing"),
        test_http_client,
    )
    .err()
    .expect("versioned Messaging root should fail constructor validation");
    assert!(
        matches!(constructor_err, TwilioError::InvalidBaseUrl(message) if message.contains("product roots"))
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

#[derive(Clone, Copy)]
struct BlockedSpecHeaderOperation;

impl Operation for BlockedSpecHeaderOperation {
    type Output = serde_json::Value;

    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::new(
            ApiFamily::Rest,
            Method::GET,
            ["2010-04-01", "Accounts", account_sid, "Messages.json"],
        )
        .operation("custom.blocked-header")
        .header("Authorization", "Basic wrong"))
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

    let spec_header_err = client
        .account(test_creds())
        .send_with_options(BlockedSpecHeaderOperation, RequestOptions::new())
        .await
        .unwrap_err();
    assert_invalid_request(spec_header_err, "cannot be overridden");
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
async fn media_list_all_collects_pages_with_existing_continuation_validation() {
    let next_page_uri = "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&PageSize=1&Page=1&PageToken=next";
    let first_page = format!(
        r#"{{"media_list":[{}],"next_page_uri":"{next_page_uri}"}}"#,
        media_json("MEpage1")
    );
    let second_page = format!(
        r#"{{"media_list":[{}],"next_page_uri":null}}"#,
        media_json("MEpage2")
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::json(first_page),
        MockResponse::json(second_page),
    ])
    .await;
    let client = client_for(&server);
    let media = client
        .account(test_creds())
        .message("SM123")
        .media()
        .list_all_with(
            ListMediaRequest::new()
                .date_created("2026-07-01")
                .page_size(1),
        )
        .collect_all()
        .await
        .unwrap();

    assert_eq!(media.len(), 2);
    assert_eq!(media[0].sid.as_deref(), Some("MEpage1"));
    assert_eq!(media[1].sid.as_deref(), Some("MEpage2"));
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&PageSize=1"
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

    let create = CreateMessageRequest::new("+15551234567")
        .from("+15557654321")
        .body("hello")
        .media_urls(&["https://example.test/a.png", "https://example.test/b.png"])
        .persistent_actions(&["mailto:test@example.test"])
        .status_callback("https://example.test/status")
        .application_sid("AP123")
        .provide_feedback(true)
        .attempt(2)
        .validity_period(3600)
        .content_retention(ContentRetention::Retain)
        .address_retention(AddressRetention::Obfuscate)
        .smart_encoded(true)
        .traffic_type(TrafficType::Free)
        .shorten_urls(false)
        .schedule_type(ScheduleType::Fixed)
        .send_at("2026-07-03T12:00:00Z")
        .send_as_mms(true)
        .content_sid("HX123")
        .content_variables_json(r#"{"name":"Ada"}"#)
        .risk_check(RiskCheck::Disable)
        .message_intent(MessageIntent::Marketing)
        .fallback_from("+15550000000")
        .tags_json(r#"{"campaign":"spring"}"#);

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
    assert!(requests[0].body.contains("MessageIntent=marketing"));
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

    let list = ListMessagesRequest::new()
        .to("+15551234567")
        .from("+15557654321")
        .date_sent("2026-07-01")
        .date_sent_before("2026-07-31")
        .date_sent_after("2026-06-01")
        .page_size(2)
        .page(0)
        .page_token("start");

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
    let list = ListMediaRequest::new()
        .date_created("2026-07-01")
        .date_created_before("2026-07-31")
        .date_created_after("2026-06-01")
        .page_size(1)
        .page(0)
        .page_token("start");
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
    let created = account
        .messaging()
        .v1()
        .services()
        .create(create)
        .await
        .unwrap();
    let fetched = account
        .messaging()
        .v1()
        .service("MGfetch")
        .fetch()
        .await
        .unwrap();
    let first = account
        .messaging()
        .v1()
        .services()
        .list(ListServicesRequest::new().page_size(2).page(0))
        .await
        .unwrap();
    let next_url = format!(
        "{}/v1/Services?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let second = account
        .messaging()
        .v1()
        .services()
        .list_page_url(&next_url)
        .await
        .unwrap();
    let updated = account
        .messaging()
        .v1()
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
    account
        .messaging()
        .v1()
        .service("MGdelete")
        .delete()
        .await
        .unwrap();

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
async fn a2p_brand_registrations_and_vettings_use_messaging_v1() {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(a2p_brand_json("BNcreate")),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "data",
            "a2p/BrandRegistrations",
            &[a2p_brand_json("BNlist")],
            Some("/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next"),
        )),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "data",
            "a2p/BrandRegistrations",
            &[],
            None,
        )),
        MockResponse::json(a2p_brand_json("BNfetch")),
        MockResponse::json(a2p_brand_json("BNupdate")),
        MockResponse::created_json(a2p_vetting_json("VTcreate")),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "data",
            "a2p/BrandRegistrations/BNbrand/Vettings",
            &[a2p_vetting_json("VTlist")],
            Some("/v1/a2p/BrandRegistrations/BNbrand/Vettings?VettingProvider=campaign-verify&PageSize=2&Page=1&PageToken=next"),
        )),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "data",
            "a2p/BrandRegistrations/BNbrand/Vettings",
            &[],
            None,
        )),
        MockResponse::json(a2p_vetting_json("VTfetch")),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let created = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .create(
            CreateA2PBrandRegistrationRequest::new()
                .customer_profile_bundle_sid("BUcustomer")
                .a2p_profile_bundle_sid("BUa2p")
                .brand_type(A2PBrandType::Standard)
                .skip_automatic_sec_vet(true)
                .mock(true),
        )
        .await
        .unwrap();
    let first = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list(ListA2PBrandRegistrationsRequest::new().page_size(2).page(0))
        .await
        .unwrap();
    let next_url = format!(
        "{}/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let second = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list_page_url(&next_url)
        .await
        .unwrap();
    let fetched = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNfetch")
        .fetch()
        .await
        .unwrap();
    let updated = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNupdate")
        .update()
        .await
        .unwrap();
    let vettings = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings();
    let vetting_created = vettings
        .create(
            CreateA2PBrandVettingRequest::new(A2PVettingProvider::CampaignVerify)
                .vetting_id("vetting-token"),
        )
        .await
        .unwrap();
    let vetting_first = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .list(
            ListA2PBrandVettingsRequest::new()
                .vetting_provider(A2PVettingProvider::CampaignVerify)
                .page_size(2)
                .page(0),
        )
        .await
        .unwrap();
    let vetting_next_url = format!(
        "{}/v1/a2p/BrandRegistrations/BNbrand/Vettings?VettingProvider=campaign-verify&PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let vetting_second = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .list_page_url(&vetting_next_url)
        .await
        .unwrap();
    let vetting_fetched = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .fetch("VTfetch")
        .await
        .unwrap();

    assert_eq!(created.sid.as_deref(), Some("BNcreate"));
    assert_eq!(first.brand_registrations[0].sid.as_deref(), Some("BNlist"));
    assert!(second.brand_registrations.is_empty());
    assert_eq!(fetched.sid.as_deref(), Some("BNfetch"));
    assert_eq!(updated.sid.as_deref(), Some("BNupdate"));
    assert_eq!(
        vetting_created.brand_vetting_sid.as_deref(),
        Some("VTcreate")
    );
    assert_eq!(
        vetting_first.vettings[0].brand_vetting_sid.as_deref(),
        Some("VTlist")
    );
    assert!(vetting_second.vettings.is_empty());
    assert_eq!(
        vetting_fetched.brand_vetting_sid.as_deref(),
        Some("VTfetch")
    );

    let requests = server.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/a2p/BrandRegistrations");
    for expected in [
        "CustomerProfileBundleSid=BUcustomer",
        "A2PProfileBundleSid=BUa2p",
        "BrandType=STANDARD",
        "SkipAutomaticSecVet=true",
        "Mock=true",
    ] {
        assert!(requests[0].body.contains(expected));
    }
    assert_eq!(
        requests[1].path,
        "/v1/a2p/BrandRegistrations?PageSize=2&Page=0"
    );
    assert_eq!(
        requests[2].path,
        "/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[3].path, "/v1/a2p/BrandRegistrations/BNfetch");
    assert_eq!(requests[4].path, "/v1/a2p/BrandRegistrations/BNupdate");
    assert_eq!(
        requests[5].path,
        "/v1/a2p/BrandRegistrations/BNbrand/Vettings"
    );
    assert_eq!(
        requests[5].body,
        "VettingProvider=campaign-verify&VettingId=vetting-token"
    );
    assert_eq!(
        requests[6].path,
        "/v1/a2p/BrandRegistrations/BNbrand/Vettings?VettingProvider=campaign-verify&PageSize=2&Page=0"
    );
    assert_eq!(
        requests[7].path,
        "/v1/a2p/BrandRegistrations/BNbrand/Vettings?VettingProvider=campaign-verify&PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(
        requests[8].path,
        "/v1/a2p/BrandRegistrations/BNbrand/Vettings/VTfetch"
    );
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn service_usa2p_and_usecases_use_messaging_v1() {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(usa2p_json("QEcreate")),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "compliance",
            "Services/MG123/Compliance/Usa2p",
            &[usa2p_json("QElist")],
            Some("/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=1&PageToken=next"),
        )),
        MockResponse::json(page_json(
            &server_url_placeholder(),
            "compliance",
            "Services/MG123/Compliance/Usa2p",
            &[],
            None,
        )),
        MockResponse::json(usa2p_json("QEfetch")),
        MockResponse::json(usa2p_usecases_json()),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let usa2p = account.messaging().v1().service("MG123").usa2p();
    let request = CreateUsa2pRequest::new()
        .brand_registration_sid("BN123")
        .description("Transactional alerts for customer account activity.")
        .message_flow("Customers opt in during account signup and settings.")
        .message_samples(&[
            "Your account login code is 123456.",
            "A new device signed in to your account.",
        ])
        .us_app_to_person_usecase(A2PUsecase::Marketing)
        .has_embedded_links(true)
        .has_embedded_phone(false)
        .subscriber_opt_in(true)
        .opt_in_message("You are opted in for account alerts.")
        .opt_out_message("You have opted out of account alerts.")
        .help_message("Reply HELP for account alert assistance.")
        .opt_in_keywords(&["START"])
        .opt_out_keywords(&["STOP"])
        .help_keywords(&["HELP"])
        .api_version("2010-04-01");

    let created = usa2p.create(request).await.unwrap();
    let first = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .list(ListUsa2pRequest::new().page_size(2).page(0))
        .await
        .unwrap();
    let next_url = format!(
        "{}/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let second = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .list_page_url(&next_url)
        .await
        .unwrap();
    let fetched = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .fetch("QEfetch")
        .await
        .unwrap();
    let usecases = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p_usecases()
        .fetch(FetchUsa2pUsecasesRequest::new().brand_registration_sid("BN123"))
        .await
        .unwrap();
    account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .delete("QEdelete")
        .await
        .unwrap();

    assert_eq!(created.sid.as_deref(), Some("QEcreate"));
    assert_eq!(first.compliance[0].sid.as_deref(), Some("QElist"));
    assert!(second.compliance.is_empty());
    assert_eq!(fetched.sid.as_deref(), Some("QEfetch"));
    assert_eq!(
        usecases.us_app_to_person_usecases[0].code.as_deref(),
        Some("MARKETING")
    );

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Services/MG123/Compliance/Usa2p");
    assert_eq!(
        requests[0].header("x-twilio-api-version"),
        Some("2010-04-01")
    );
    for expected in [
        "BrandRegistrationSid=BN123",
        "Description=Transactional+alerts+for+customer+account+activity.",
        "MessageFlow=Customers+opt+in+during+account+signup+and+settings.",
        "MessageSamples=Your+account+login+code+is+123456.",
        "MessageSamples=A+new+device+signed+in+to+your+account.",
        "UsAppToPersonUsecase=MARKETING",
        "HasEmbeddedLinks=true",
        "HasEmbeddedPhone=false",
        "SubscriberOptIn=true",
        "OptInKeywords=START",
        "OptOutKeywords=STOP",
        "HelpKeywords=HELP",
    ] {
        assert!(
            requests[0].body.contains(expected),
            "missing {expected} in {}",
            requests[0].body
        );
    }
    assert_eq!(
        requests[1].path,
        "/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=0"
    );
    assert_eq!(
        requests[2].path,
        "/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(
        requests[3].path,
        "/v1/Services/MG123/Compliance/Usa2p/QEfetch"
    );
    assert_eq!(
        requests[4].path,
        "/v1/Services/MG123/Compliance/Usa2p/Usecases?BrandRegistrationSid=BN123"
    );
    assert_eq!(requests[5].method, "DELETE");
    assert_eq!(
        requests[5].path,
        "/v1/Services/MG123/Compliance/Usa2p/QEdelete"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn accounts_messaging_features_use_accounts_v1() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(bulk_contacts_response_json()),
        MockResponse::json(bulk_consents_response_json()),
        MockResponse::created_json(safe_list_json("+18001234567")),
        MockResponse::json(safe_list_json("+18001234567")),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let contacts = account
        .contacts()
        .bulk_upsert(BulkContactsRequest::new().item(ContactItem::new(
            "+19999999999",
            "ad388b5a46b33b874b0d41f7226db2ef",
            "US",
            "12345",
        )))
        .await
        .unwrap();
    let consents = account
        .consents()
        .bulk_upsert(
            BulkConsentsRequest::new().item(
                ConsentItem::new(
                    "+19999999999",
                    "ad388b5a46b33b874b0d41f7226db2ef",
                    "MG00000000000000000000000000000001",
                    ConsentStatus::OptOut,
                    ConsentSource::Website,
                )
                .date_of_consent("2025-02-28T10:05:27Z"),
            ),
        )
        .await
        .unwrap();
    let added = account
        .global_safe_list()
        .add(SafeListNumberRequest::new("+18001234567"))
        .await
        .unwrap();
    let checked = account
        .global_safe_list()
        .check(SafeListNumberRequest::new("+18001234567"))
        .await
        .unwrap();
    account
        .global_safe_list()
        .remove(SafeListNumberRequest::new("+18001234567"))
        .await
        .unwrap();

    assert_eq!(
        contacts.items[0].contact_id.as_deref(),
        Some("+19999999999")
    );
    assert_eq!(consents.items[0].status.as_deref(), Some("opt-out"));
    assert_eq!(added.phone_number.as_deref(), Some("+18001234567"));
    assert_eq!(checked.sid.as_deref(), Some("GN123"));

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Contacts/Bulk");
    let pairs = decoded_form_pairs(&requests[0].body);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "Items");
    assert_eq!(
        pairs[0].1,
        r#"{"contact_id":"+19999999999","correlation_id":"ad388b5a46b33b874b0d41f7226db2ef","country_iso_code":"US","zip_code":"12345"}"#
    );
    assert_eq!(requests[1].path, "/v1/Consents/Bulk");
    let pairs = decoded_form_pairs(&requests[1].body);
    assert_eq!(
        pairs[0].1,
        r#"{"contact_id":"+19999999999","correlation_id":"ad388b5a46b33b874b0d41f7226db2ef","sender_id":"MG00000000000000000000000000000001","status":"opt-out","source":"website","date_of_consent":"2025-02-28T10:05:27Z"}"#
    );
    assert_eq!(requests[2].path, "/v1/SafeList/Numbers");
    assert_eq!(requests[2].body, "PhoneNumber=%2B18001234567");
    assert_eq!(
        requests[3].path,
        "/v1/SafeList/Numbers?PhoneNumber=%2B18001234567"
    );
    assert_eq!(
        requests[4].path,
        "/v1/SafeList/Numbers?PhoneNumber=%2B18001234567"
    );
    assert_eq!(requests[4].method, "DELETE");
}

#[tokio::test]
async fn api_key_auth_redacts_key_sid_secret_and_sensitive_request_values() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        400,
        r#"{"message":"SKapi-key-sid api-key-secret +18001234567"}"#,
    )])
    .await;
    let auth = TwilioAuth::api_key("ACapi-key-account", "SKapi-key-sid", "api-key-secret");
    let err = client_for(&server)
        .account(&auth)
        .global_safe_list()
        .check(SafeListNumberRequest::new("+18001234567"))
        .await
        .unwrap_err();

    let requests = server.requests();
    assert_eq!(
        requests[0].header("authorization"),
        Some("Basic U0thcGkta2V5LXNpZDphcGkta2V5LXNlY3JldA==")
    );
    assert_api_error_redacted(
        err,
        400,
        &["SKapi-key-sid", "api-key-secret", "+18001234567"],
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn new_api_request_validation_catches_local_errors() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    let too_many_contacts = BulkContactsRequest::new().items(vec![
        ContactItem::new(
            "+19999999999",
            "corr",
            "US",
            "12345"
        );
        26
    ]);
    let err = account
        .contacts()
        .bulk_upsert(too_many_contacts)
        .await
        .unwrap_err();
    assert_invalid_request(err, "at most 25");

    let err = account
        .contacts()
        .bulk_upsert(BulkContactsRequest::new().item(ContactItem::new("", "corr", "US", "12345")))
        .await
        .unwrap_err();
    assert_invalid_request(err, "contact_id");

    let err = account
        .consents()
        .bulk_upsert(BulkConsentsRequest::new().item(ConsentItem::new(
            "+19999999999",
            "corr",
            "",
            ConsentStatus::OptIn,
            ConsentSource::Offline,
        )))
        .await
        .unwrap_err();
    assert_invalid_request(err, "sender_id");

    let err = account
        .global_safe_list()
        .check(SafeListNumberRequest::new(""))
        .await
        .unwrap_err();
    assert_invalid_request(err, "PhoneNumber");

    let err = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .create(
            CreateUsa2pRequest::new()
                .brand_registration_sid("BN123")
                .description("Transactional alerts for customer account activity.")
                .message_flow("Customers opt in during account signup and settings.")
                .message_samples(&["only one sample"])
                .us_app_to_person_usecase(A2PUsecase::Marketing)
                .has_embedded_links(false)
                .has_embedded_phone(false),
        )
        .await
        .unwrap_err();
    assert_invalid_request(err, "MessageSamples");

    let err = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list(ListA2PBrandRegistrationsRequest::new().page_size(0))
        .await
        .unwrap_err();
    assert_invalid_request(err, "PageSize");

    let err = account
        .messages()
        .create(
            CreateMessageRequest::new("+15551234567")
                .from("+15557654321")
                .body("hello")
                .message_intent(MessageIntent::Custom("")),
        )
        .await
        .unwrap_err();
    assert_invalid_request(err, "MessageIntent");

    let err = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .create(
            CreateA2PBrandRegistrationRequest::new()
                .customer_profile_bundle_sid("BUcustomer")
                .a2p_profile_bundle_sid("BUa2p")
                .brand_type(A2PBrandType::Custom("")),
        )
        .await
        .unwrap_err();
    assert_invalid_request(err, "BrandType");

    let err = account
        .messaging()
        .v1()
        .a2p_brand_registration("BN123")
        .vettings()
        .list(ListA2PBrandVettingsRequest::new().vetting_provider(A2PVettingProvider::Custom("")))
        .await
        .unwrap_err();
    assert_invalid_request(err, "VettingProvider");

    let err = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list_page_url("https://example.test/v1/a2p/BrandRegistrations?Page=1")
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    let err = account
        .messaging()
        .v1()
        .a2p_brand_registration("BN123")
        .vettings()
        .list_page_url(&format!(
            "{}/v1/a2p/BrandRegistrations/BN123/Vettings?Unexpected=1",
            server.base_url
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    let err = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .list_page_url(&format!(
            "{}/v1/Services/MG999/Compliance/Usa2p?Page=1",
            server.base_url
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    assert!(server.requests().is_empty());
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
    let service = account.messaging().v1().service("MG123");

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
async fn service_subresource_list_all_collects_pages() {
    let next_page_url =
        "__BASE_URL__/v1/Services/MG123/PhoneNumbers?PageSize=2&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(phone_number_page_json("PNpage1", Some(next_page_url))),
        MockResponse::json(phone_number_page_json("PNpage2", None)),
    ])
    .await;
    let client = client_for(&server);
    let phone_numbers = client
        .account(test_creds())
        .messaging()
        .v1()
        .service("MG123")
        .phone_numbers()
        .list_all_with(ListServiceSubresourcesRequest::new().page_size(2))
        .collect_all()
        .await
        .unwrap();

    assert_eq!(phone_numbers.len(), 2);
    assert_eq!(phone_numbers[0].sid.as_deref(), Some("PNpage1"));
    assert_eq!(phone_numbers[1].sid.as_deref(), Some("PNpage2"));
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/v1/Services/MG123/PhoneNumbers?PageSize=2"
    );
    assert_eq!(
        requests[1].path,
        "/v1/Services/MG123/PhoneNumbers?PageSize=2&Page=1&PageToken=next"
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
        .messaging()
        .v1()
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
        .messaging()
        .v1()
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
        .messaging()
        .v1()
        .tollfree_verifications()
        .create(create)
        .await
        .unwrap();
    let fetched = account
        .messaging()
        .v1()
        .tollfree_verification("HHfetch")
        .fetch()
        .await
        .unwrap();
    let first = account
        .messaging()
        .v1()
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
        .messaging()
        .v1()
        .tollfree_verifications()
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    let updated = account
        .messaging()
        .v1()
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
        .messaging()
        .v1()
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
        .messaging()
        .v1()
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
        .messaging()
        .v1()
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
#[allow(clippy::too_many_lines)]
async fn messaging_completion_endpoints_use_expected_wire_shape() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(link_shortening_certificate_json()),
        MockResponse::json(link_shortening_certificate_json()),
        MockResponse::no_content(),
        MockResponse::json(link_shortening_config_json()),
        MockResponse::json(link_shortening_config_json()),
        MockResponse::json(link_shortening_dns_validation_json()),
        MockResponse::json(link_shortening_certificate_json()),
        MockResponse::json(link_shortening_association_json()),
        MockResponse::no_content(),
        MockResponse::json(link_shortening_config_json()),
        MockResponse::json(link_shortening_association_json()),
        MockResponse::json(link_shortening_certificate_json()),
        MockResponse::json(service_usecases_json()),
        MockResponse::json(preregistered_usa2p_json()),
        MockResponse::json(brand_registration_otp_json()),
        MockResponse::json(typing_success_json()),
        MockResponse::json(typing_success_json()),
        MockResponse::json(typing_success_json()),
        MockResponse::json(messaging_geo_permissions_json()),
        MockResponse::json(messaging_geo_permissions_json()),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let link_shortening = account.messaging().v1().link_shortening();
    let domain = link_shortening.domain("DN123");

    let cert = domain.certificate().fetch().await.unwrap();
    assert_eq!(cert.domain_sid.as_deref(), Some("DN123"));
    domain
        .certificate()
        .update(UpdateLinkShorteningDomainCertificateRequest::new(
            "CERTsecret",
        ))
        .await
        .unwrap();
    domain.certificate().delete().await.unwrap();
    let config = domain.config().fetch().await.unwrap();
    assert_eq!(config.domain_sid.as_deref(), Some("DN123"));
    domain
        .config()
        .update(
            UpdateLinkShorteningDomainConfigRequest::new()
                .callback_url("https://callback.example.test/ls")
                .continue_on_failure(true),
        )
        .await
        .unwrap();
    let dns = domain.validate_dns().await.unwrap();
    assert_eq!(dns.is_valid, Some(true));
    domain.request_managed_certificate().await.unwrap();
    domain.messaging_service("MG123").create().await.unwrap();
    domain.messaging_service("MG123").delete().await.unwrap();
    link_shortening
        .messaging_service("MG123")
        .domain_config()
        .await
        .unwrap();
    link_shortening
        .messaging_service("MG123")
        .domain()
        .await
        .unwrap();
    account
        .messaging()
        .v2()
        .link_shortening()
        .domain("DN123")
        .certificate()
        .fetch()
        .await
        .unwrap();
    let usecases = account
        .messaging()
        .v1()
        .services()
        .usecases()
        .fetch()
        .await
        .unwrap();
    assert_eq!(usecases.usecases.len(), 1);
    account
        .messaging()
        .v1()
        .services()
        .preregistered_usa2p()
        .create(CreatePreregisteredUsa2pRequest::new("CAMP123", "MG123").cnp_migration(true))
        .await
        .unwrap();
    account
        .messaging()
        .v1()
        .a2p_brand_registration("BN123")
        .sms_otp()
        .create()
        .await
        .unwrap();
    account
        .messaging()
        .v2()
        .typing_indicators()
        .create(CreateMessagingV2TypingIndicatorRequest::whatsapp(
            "wamid.secret",
        ))
        .await
        .unwrap();
    account
        .messaging()
        .v3()
        .typing_indicators()
        .create(
            CreateMessagingV3TypingIndicatorRequest::apple(
                "whatsapp:+15551234567",
                "whatsapp:+15557654321",
            )
            .event(AppleTypingEvent::Start),
        )
        .await
        .unwrap();
    account
        .messaging()
        .v3()
        .typing_indicators()
        .create(
            CreateMessagingV3TypingIndicatorRequest::rcs("rcs:brand_agent", "rcs:+15551234567")
                .event(AppleTypingEvent::End),
        )
        .await
        .unwrap();
    let geo_permissions = account
        .messaging_geo_permissions()
        .fetch(ListMessagingGeoPermissionsRequest::new().country_code("US,CA"))
        .await
        .unwrap();
    assert_eq!(
        geo_permissions.permissions[0].message.as_deref(),
        Some("High Risk Country")
    );
    account
        .messaging_geo_permissions()
        .update(
            UpdateMessagingGeoPermissionsRequest::new()
                .permission(MessagingGeoPermissionUpdateItem::country("US", true))
                .permission(MessagingGeoPermissionUpdateItem::country("CA", false)),
        )
        .await
        .unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 20);
    let expected = [
        ("GET", "/v1/LinkShortening/Domains/DN123/Certificate"),
        ("POST", "/v1/LinkShortening/Domains/DN123/Certificate"),
        ("DELETE", "/v1/LinkShortening/Domains/DN123/Certificate"),
        ("GET", "/v1/LinkShortening/Domains/DN123/Config"),
        ("POST", "/v1/LinkShortening/Domains/DN123/Config"),
        ("GET", "/v1/LinkShortening/Domains/DN123/ValidateDns"),
        (
            "POST",
            "/v1/LinkShortening/Domains/DN123/RequestManagedCert",
        ),
        (
            "POST",
            "/v1/LinkShortening/Domains/DN123/MessagingServices/MG123",
        ),
        (
            "DELETE",
            "/v1/LinkShortening/Domains/DN123/MessagingServices/MG123",
        ),
        (
            "GET",
            "/v1/LinkShortening/MessagingService/MG123/DomainConfig",
        ),
        ("GET", "/v1/LinkShortening/MessagingServices/MG123/Domain"),
        ("GET", "/v2/LinkShortening/Domains/DN123/Certificate"),
        ("GET", "/v1/Services/Usecases"),
        ("POST", "/v1/Services/PreregisteredUsa2p"),
        ("POST", "/v1/a2p/BrandRegistrations/BN123/SmsOtp"),
        ("POST", "/v2/Indicators/Typing.json"),
        ("POST", "/v3/Indicators/Typing.json"),
        ("POST", "/v3/Indicators/Typing.json"),
        ("GET", "/v1/Messaging/GeoPermissions?CountryCode=US%2CCA"),
        ("PATCH", "/v1/Messaging/GeoPermissions"),
    ];
    for (request, (method, path)) in requests.iter().zip(expected) {
        assert_eq!(request.method, method);
        assert_eq!(request.path, path);
        assert_basic_auth(request);
    }
    assert_eq!(requests[1].body, "TlsCert=CERTsecret");
    assert_eq!(
        requests[4].body,
        "CallbackUrl=https%3A%2F%2Fcallback.example.test%2Fls&ContinueOnFailure=true"
    );
    assert_eq!(
        requests[13].body,
        "CampaignId=CAMP123&MessagingServiceSid=MG123&CnpMigration=true"
    );
    assert_eq!(requests[15].body, "channel=whatsapp&messageId=wamid.secret");
    assert_eq!(
        requests[16].header("content-type"),
        Some("application/json")
    );
    let v3_body: serde_json::Value = serde_json::from_str(&requests[16].body).unwrap();
    assert_eq!(
        v3_body,
        serde_json::json!({
            "channel": "APPLE",
            "from": "whatsapp:+15551234567",
            "to": "whatsapp:+15557654321",
            "event": "START"
        })
    );
    let rcs_body: serde_json::Value = serde_json::from_str(&requests[17].body).unwrap();
    assert_eq!(
        rcs_body,
        serde_json::json!({
            "channel": "RCS",
            "from": "rcs:brand_agent",
            "to": "rcs:+15551234567",
            "event": "END"
        })
    );
    let permissions = decoded_form_pairs(&requests[19].body);
    assert_eq!(permissions.len(), 2);
    assert!(permissions.iter().all(|(key, _)| key == "Permissions"));
    let first_permission: serde_json::Value = serde_json::from_str(&permissions[0].1).unwrap();
    let second_permission: serde_json::Value = serde_json::from_str(&permissions[1].1).unwrap();
    assert_eq!(
        first_permission,
        serde_json::json!({"country_code":"US","type":"country","enabled":true})
    );
    assert_eq!(
        second_permission,
        serde_json::json!({"country_code":"CA","type":"country","enabled":false})
    );
}

#[tokio::test]
async fn messaging_geo_permissions_accepts_endpoint_sized_updates() {
    let server =
        HttpsMockServer::start(vec![MockResponse::json(messaging_geo_permissions_json())]).await;
    let client = client_for(&server);
    let mut request = UpdateMessagingGeoPermissionsRequest::new();
    for _ in 0..26 {
        request = request.permission(MessagingGeoPermissionUpdateItem::country("US", true));
    }

    client
        .account(test_creds())
        .messaging_geo_permissions()
        .update(request)
        .await
        .unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].path, "/v1/Messaging/GeoPermissions");
    let permissions = decoded_form_pairs(&requests[0].body);
    assert_eq!(permissions.len(), 26);
    assert!(permissions.iter().all(|(key, _)| key == "Permissions"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn messaging_v2_channel_senders_json_and_pagination_work() {
    let next_page_url =
        "__BASE_URL__/v2/Channels/Senders?Channel=whatsapp&PageSize=1&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(messaging_v2_channel_sender_json("XEcreate")),
        MockResponse::json(messaging_v2_channel_sender_page_json(
            "XEpage1",
            Some(next_page_url),
        )),
        MockResponse::json(messaging_v2_channel_sender_page_json("XEpage2", None)),
        MockResponse::json(messaging_v2_channel_sender_json("XEfetch")),
        MockResponse::json(messaging_v2_channel_sender_json("XEupdate")),
        MockResponse::no_content(),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let senders = account.messaging().v2().channel_senders();

    let created = senders
        .create(
            CreateMessagingV2ChannelSenderRequest::new("whatsapp:+15551234567")
                .configuration(
                    ChannelSenderConfiguration::new()
                        .waba_id("WABA123")
                        .verification_method("sms"),
                )
                .webhook(
                    ChannelSenderWebhook::new()
                        .callback_url("https://callback.example.test/channel")
                        .callback_method(ChannelSenderHttpMethod::Post),
                )
                .profile(ChannelSenderProfile::new().name("Example Brand").email(
                    ChannelSenderProfileEmail::new("ops@example.test", "support"),
                )),
        )
        .await
        .unwrap();
    assert_eq!(created.sid.as_deref(), Some("XEcreate"));
    let compliance = created
        .compliance
        .as_ref()
        .expect("compliance should deserialize");
    assert_eq!(compliance.registration_sid.as_deref(), Some("CR123"));
    assert_eq!(
        compliance
            .countries
            .as_ref()
            .and_then(|countries| countries.first())
            .and_then(|country| country.status.as_deref()),
        Some("ONLINE")
    );
    assert_debug_redacts(
        &created,
        &["CR123", "CRUS", "https://example.test/compliance"],
    );

    let first = senders
        .list(ListMessagingV2ChannelSendersRequest::new(MessagingV2Channel::Whatsapp).page_size(1))
        .await
        .unwrap();
    assert_eq!(first.senders[0].sid.as_deref(), Some("XEpage1"));
    let second = senders
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(second.senders[0].sid.as_deref(), Some("XEpage2"));
    senders.sender("XEfetch").fetch().await.unwrap();
    senders
        .sender("XEupdate")
        .update(
            UpdateMessagingV2ChannelSenderRequest::new().webhook(
                ChannelSenderWebhook::new()
                    .callback_url("https://callback.example.test/status")
                    .callback_method(ChannelSenderHttpMethod::Put),
            ),
        )
        .await
        .unwrap();
    senders.sender("XEdelete").delete().await.unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v2/Channels/Senders");
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    let create_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(
        create_body["sender_id"],
        serde_json::Value::String("whatsapp:+15551234567".to_owned())
    );
    assert_eq!(create_body["configuration"]["waba_id"], "WABA123");
    assert_eq!(create_body["webhook"]["callback_method"], "POST");
    assert_eq!(
        create_body["profile"]["emails"][0]["email"],
        "ops@example.test"
    );
    assert_eq!(
        requests[1].path,
        "/v2/Channels/Senders?Channel=whatsapp&PageSize=1"
    );
    assert_eq!(
        requests[2].path,
        "/v2/Channels/Senders?Channel=whatsapp&PageSize=1&Page=1&PageToken=next"
    );
    assert_eq!(requests[3].path, "/v2/Channels/Senders/XEfetch");
    assert_eq!(requests[4].method, "POST");
    assert_eq!(requests[4].path, "/v2/Channels/Senders/XEupdate");
    let update_body: serde_json::Value = serde_json::from_str(&requests[4].body).unwrap();
    assert_eq!(
        update_body["webhook"]["callback_url"],
        "https://callback.example.test/status"
    );
    assert_eq!(update_body["webhook"]["callback_method"], "PUT");
    assert_eq!(requests[5].method, "DELETE");
    assert_eq!(requests[5].path, "/v2/Channels/Senders/XEdelete");
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[tokio::test]
async fn messaging_v2_channel_senders_list_all_collects_pages() {
    let next_page_url =
        "__BASE_URL__/v2/Channels/Senders?Channel=whatsapp&PageSize=50&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(messaging_v2_channel_sender_page_json(
            "XEpage1",
            Some(next_page_url),
        )),
        MockResponse::json(messaging_v2_channel_sender_page_json("XEpage2", None)),
    ])
    .await;
    let client = client_for(&server);
    let senders = client
        .account(test_creds())
        .messaging()
        .v2()
        .channel_senders()
        .list_all(MessagingV2Channel::Whatsapp)
        .collect_all()
        .await
        .unwrap();

    assert_eq!(senders.len(), 2);
    assert_eq!(senders[0].sid.as_deref(), Some("XEpage1"));
    assert_eq!(senders[1].sid.as_deref(), Some("XEpage2"));
    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        "/v2/Channels/Senders?Channel=whatsapp&PageSize=50"
    );
    assert_eq!(
        requests[1].path,
        "/v2/Channels/Senders?Channel=whatsapp&PageSize=50&Page=1&PageToken=next"
    );
}

#[tokio::test]
async fn message_request_validation_catches_local_errors() {
    let create = CreateMessageRequest::new("")
        .from("+15557654321")
        .body("hello");

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
    let create = CreateMessageRequest::new("+15551234567")
        .from("+15557654321")
        .body(&overlong_body);
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
        let create = CreateMessageRequest::new("+15551234567")
            .from("+15557654321")
            .body("hello")
            .validity_period(validity_period);
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

    let create = CreateMessageRequest::new("+15551234567")
        .from("+15557654321")
        .body("hello")
        .shorten_urls(true);
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

    let create = CreateMessageRequest::new("+15551234567")
        .from("+15557654321")
        .body("hello")
        .content_variables_json(r#"{"name":"Ada"}"#);
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
        .update(UpdateMessageRequest::new().body("replacement"))
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
        .messaging()
        .v1()
        .service("MG123")
        .update(UpdateServiceRequest::new())
        .await
        .expect_err("empty service update should fail before transport");
    assert!(matches!(err, TwilioError::InvalidRequest(_)));

    let overlong_friendly_name = "x".repeat(65);
    let err = account
        .messaging()
        .v1()
        .services()
        .create(CreateServiceRequest::new(&overlong_friendly_name))
        .await
        .expect_err("overlong service FriendlyName should fail before transport");
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(message) if message.contains("FriendlyName")
    ));

    let err = account
        .messaging()
        .v1()
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
            .messaging()
            .v1()
            .services()
            .create(CreateServiceRequest::new("valid").validity_period(validity_period))
            .await
            .expect_err("out-of-range service ValidityPeriod should fail before transport");
        assert!(matches!(
            err,
            TwilioError::InvalidRequest(message) if message.contains("ValidityPeriod")
        ));

        let err = account
            .messaging()
            .v1()
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
            .messaging()
            .v1()
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
            .messaging()
            .v1()
            .tollfree_verifications()
            .create(CreateTollfreeVerificationRequest::new())
            .await
            .unwrap_err(),
        "BusinessName",
    );
    assert_invalid_request(
        account
            .messaging()
            .v1()
            .tollfree_verification("HH123")
            .update(UpdateTollfreeVerificationRequest::new())
            .await
            .unwrap_err(),
        "at least one field",
    );

    let categories = [TollfreeUseCaseCategory::Raw("")];
    assert_invalid_request(
        account
            .messaging()
            .v1()
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
    assert_invalid_request(
        account
            .messaging()
            .v2()
            .channel_senders()
            .create(CreateMessagingV2ChannelSenderRequest::new(
                "whatsapp:+15551234567",
            ))
            .await
            .unwrap_err(),
        "profile.name",
    );
    assert_invalid_request(
        account
            .messaging()
            .v2()
            .channel_senders()
            .create(
                CreateMessagingV2ChannelSenderRequest::new("whatsapp:+15551234567")
                    .profile(ChannelSenderProfile::new().name("   ")),
            )
            .await
            .unwrap_err(),
        "profile.name",
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn page_size_validation_catches_local_errors() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = client_for(&server);
    let account = client.account(test_creds());

    for page_size in [0, 1001] {
        let messages = ListMessagesRequest::new().page_size(page_size);
        assert_invalid_request(
            account.messages().list(messages).await.unwrap_err(),
            "PageSize",
        );

        let media = ListMediaRequest::new().page_size(page_size);
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
                .messaging()
                .v1()
                .services()
                .list(ListServicesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .messaging()
                .v1()
                .service("MG123")
                .phone_numbers()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .messaging()
                .v1()
                .service("MG123")
                .short_codes()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .messaging()
                .v1()
                .service("MG123")
                .alpha_senders()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .messaging()
                .v1()
                .service("MG123")
                .channel_senders()
                .list(ListServiceSubresourcesRequest::new().page_size(page_size))
                .await
                .unwrap_err(),
            "PageSize",
        );

        assert_invalid_request(
            account
                .messaging()
                .v1()
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
                .messaging()
                .v1()
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
    assert_decode_error(
        &account
            .messaging()
            .v1()
            .service("MGbad")
            .fetch()
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn representative_api_errors_are_classified_and_sanitized() {
    let body = r#"{"message":"denied for +19990001111","sid":"SMsecret","to":"+15551234567","url":"https://example.test/private"}"#;
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
    let leaked = [
        "SMsecret",
        "+15551234567",
        "+19990001111",
        "https://example.test/private",
    ];

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
        account
            .messaging()
            .v1()
            .service("MGsecret")
            .delete()
            .await
            .unwrap_err(),
        503,
        &leaked,
    );
    assert_api_error_redacted(
        account
            .messaging()
            .v1()
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
    assert!(body.starts_with("<redacted response body; "));
    assert!(!body.contains(tail));
}

#[test]
fn message_debug_output_redacts_sensitive_values() {
    let message = message_from_parts();
    assert_debug_redacts(
        &message,
        &[
            "secret body",
            "+15550001111",
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
#[allow(clippy::too_many_lines)]
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

    let brand = TwilioA2PBrandRegistration {
        sid: Some("BNsecret".into()),
        account_sid: Some("AC123".into()),
        customer_profile_bundle_sid: Some("BUcustomer".into()),
        a2p_profile_bundle_sid: Some("BUa2p".into()),
        date_created: Some("2026-07-05T00:00:00Z".into()),
        date_updated: Some("2026-07-05T00:00:00Z".into()),
        brand_type: Some("CUSTOM-BRAND-SECRET".into()),
        status: Some("APPROVED".into()),
        tcr_id: Some("TCRsecret".into()),
        failure_reason: Some("failure reason secret".into()),
        url: Some("https://messaging.twilio.com/v1/a2p/BrandRegistrations/BNsecret".into()),
        brand_score: Some(42),
        brand_feedback: vec!["brand feedback secret".into()],
        identity_status: Some("VERIFIED".into()),
        russell_3000: Some(false),
        government_entity: Some(false),
        tax_exempt_status: Some("tax secret".into()),
        skip_automatic_sec_vet: Some(true),
        mock: Some(true),
        errors: vec!["brand error secret".into()],
        links: Some(BTreeMap::from([(
            "vettings".into(),
            "https://messaging.twilio.com/v1/a2p/BrandRegistrations/BNsecret/Vettings".into(),
        )])),
    };
    assert_debug_redacts(
        &brand,
        &[
            "BNsecret",
            "AC123",
            "BUcustomer",
            "BUa2p",
            "CUSTOM-BRAND-SECRET",
            "TCRsecret",
            "failure reason secret",
            "messaging.twilio.com",
            "brand feedback secret",
            "tax secret",
            "brand error secret",
        ],
    );
    assert_debug_redacts(
        &TwilioA2PBrandRegistrationPage {
            brand_registrations: vec![brand],
            meta: v1_meta_from_parts("data"),
        },
        &["BNsecret", "AC123", "messaging.twilio.com"],
    );

    let vetting = TwilioA2PBrandVetting {
        account_sid: Some("AC123".into()),
        brand_sid: Some("BNsecret".into()),
        brand_vetting_sid: Some("VTsecret".into()),
        vetting_provider: Some("custom-provider-secret".into()),
        vetting_id: Some("vetting-id-secret".into()),
        vetting_class: Some("STANDARD".into()),
        vetting_status: Some("APPROVED".into()),
        date_created: Some("2026-07-05T00:00:00Z".into()),
        date_updated: Some("2026-07-05T00:00:00Z".into()),
        url: Some(
            "https://messaging.twilio.com/v1/a2p/BrandRegistrations/BNsecret/Vettings/VTsecret"
                .into(),
        ),
    };
    assert_debug_redacts(
        &vetting,
        &[
            "AC123",
            "BNsecret",
            "VTsecret",
            "custom-provider-secret",
            "vetting-id-secret",
            "messaging.twilio.com",
        ],
    );
    assert_debug_redacts(
        &TwilioA2PBrandVettingPage {
            vettings: vec![vetting],
            meta: v1_meta_from_parts("data"),
        },
        &["AC123", "BNsecret", "VTsecret", "messaging.twilio.com"],
    );

    let usa2p = TwilioUsa2p {
        sid: Some("QEsecret".into()),
        account_sid: Some("AC123".into()),
        brand_registration_sid: Some("BNsecret".into()),
        messaging_service_sid: Some("MGsecret".into()),
        description: Some("campaign description secret".into()),
        message_samples: vec!["message sample secret".into()],
        us_app_to_person_usecase: Some("custom-usecase-secret".into()),
        has_embedded_links: Some(true),
        has_embedded_phone: Some(false),
        subscriber_opt_in: Some(true),
        age_gated: Some(false),
        direct_lending: Some(false),
        campaign_status: Some("VERIFIED".into()),
        campaign_id: Some("campaign-id-secret".into()),
        is_externally_registered: Some(false),
        message_flow: Some("message flow secret".into()),
        opt_in_message: Some("opt in secret".into()),
        opt_out_message: Some("opt out secret".into()),
        help_message: Some("help secret".into()),
        opt_in_keywords: vec!["STARTSECRET".into()],
        opt_out_keywords: vec!["STOPSECRET".into()],
        help_keywords: vec!["HELPSECRET".into()],
        date_created: Some("2026-07-05T00:00:00Z".into()),
        date_updated: Some("2026-07-05T00:00:00Z".into()),
        url: Some(
            "https://messaging.twilio.com/v1/Services/MGsecret/Compliance/Usa2p/QEsecret".into(),
        ),
        mock: Some(false),
        errors: vec!["usa2p error secret".into()],
        rate_limits: Some(serde_json::json!({"secret":"rate-limit-secret"})),
    };
    assert_debug_redacts(
        &usa2p,
        &[
            "QEsecret",
            "AC123",
            "BNsecret",
            "MGsecret",
            "campaign description secret",
            "message sample secret",
            "custom-usecase-secret",
            "campaign-id-secret",
            "message flow secret",
            "opt in secret",
            "opt out secret",
            "help secret",
            "STARTSECRET",
            "STOPSECRET",
            "HELPSECRET",
            "messaging.twilio.com",
            "usa2p error secret",
            "rate-limit-secret",
        ],
    );
    assert_debug_redacts(
        &TwilioUsa2pPage {
            compliance: vec![usa2p],
            meta: v1_meta_from_parts("compliance"),
        },
        &["QEsecret", "AC123", "MGsecret", "messaging.twilio.com"],
    );

    assert_debug_redacts(
        &TwilioBulkContactsResponse {
            items: vec![TwilioBulkContactResult {
                contact_id: Some("+19999999999".into()),
                correlation_id: Some("contact-correlation-secret".into()),
                country_iso_code: Some("US".into()),
                zip_code: Some("12345".into()),
                error_code: Some(0),
                error_messages: vec!["contact error secret".into()],
            }],
        },
        &[
            "+19999999999",
            "contact-correlation-secret",
            "US",
            "12345",
            "contact error secret",
        ],
    );
    assert_debug_redacts(
        &TwilioBulkConsentsResponse {
            items: vec![TwilioBulkConsentResult {
                contact_id: Some("+18888888888".into()),
                correlation_id: Some("consent-correlation-secret".into()),
                sender_id: Some("MGsendersecret".into()),
                status: Some("custom-status-secret".into()),
                source: Some("custom-source-secret".into()),
                date_of_consent: Some("2026-07-05T00:00:00Z".into()),
                error_code: Some(0),
                error_messages: vec!["consent error secret".into()],
            }],
        },
        &[
            "+18888888888",
            "consent-correlation-secret",
            "MGsendersecret",
            "custom-status-secret",
            "custom-source-secret",
            "2026-07-05T00:00:00Z",
            "consent error secret",
        ],
    );
    assert_debug_redacts(
        &TwilioSafeListNumber {
            sid: Some("GNsecret".into()),
            phone_number: Some("+18001234567".into()),
        },
        &["GNsecret", "+18001234567"],
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

fn decoded_form_pairs(body: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

fn a2p_brand_json(sid: &str) -> String {
    format!(
        r#"{{
            "sid":"{sid}",
            "account_sid":"AC123",
            "customer_profile_bundle_sid":"BUcustomer",
            "a2p_profile_bundle_sid":"BUa2p",
            "date_created":"2026-07-05T00:00:00Z",
            "date_updated":"2026-07-05T00:00:00Z",
            "brand_type":"STANDARD",
            "status":"APPROVED",
            "tcr_id":"B123",
            "failure_reason":null,
            "url":"https://messaging.twilio.com/v1/a2p/BrandRegistrations/{sid}",
            "brand_score":42,
            "brand_feedback":["TAX_ID"],
            "identity_status":"VERIFIED",
            "russell_3000":false,
            "government_entity":false,
            "tax_exempt_status":null,
            "skip_automatic_sec_vet":true,
            "mock":true,
            "errors":[],
            "links":{{"brand_vettings":"https://messaging.twilio.com/v1/a2p/BrandRegistrations/{sid}/Vettings"}}
        }}"#
    )
}

fn a2p_vetting_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "brand_sid":"BNbrand",
            "brand_vetting_sid":"{sid}",
            "vetting_provider":"campaign-verify",
            "vetting_id":"vetting-token",
            "vetting_class":"STANDARD",
            "vetting_status":"APPROVED",
            "date_created":"2026-07-05T00:00:00Z",
            "date_updated":"2026-07-05T00:00:00Z",
            "url":"https://messaging.twilio.com/v1/a2p/BrandRegistrations/BNbrand/Vettings/{sid}"
        }}"#
    )
}

fn usa2p_json(sid: &str) -> String {
    format!(
        r#"{{
            "sid":"{sid}",
            "account_sid":"AC123",
            "brand_registration_sid":"BN123",
            "messaging_service_sid":"MG123",
            "description":"Transactional alerts for customer account activity.",
            "message_samples":["Your account login code is 123456.","A new device signed in to your account."],
            "us_app_to_person_usecase":"MARKETING",
            "has_embedded_links":true,
            "has_embedded_phone":false,
            "subscriber_opt_in":true,
            "age_gated":false,
            "direct_lending":false,
            "campaign_status":"VERIFIED",
            "campaign_id":"C123",
            "is_externally_registered":false,
            "message_flow":"Customers opt in during account signup and settings.",
            "opt_in_message":"You are opted in for account alerts.",
            "opt_out_message":"You have opted out of account alerts.",
            "help_message":"Reply HELP for account alert assistance.",
            "opt_in_keywords":["START"],
            "opt_out_keywords":["STOP"],
            "help_keywords":["HELP"],
            "date_created":"2026-07-05T00:00:00Z",
            "date_updated":"2026-07-05T00:00:00Z",
            "url":"https://messaging.twilio.com/v1/Services/MG123/Compliance/Usa2p/{sid}",
            "mock":false,
            "errors":[],
            "rate_limits":{{}}
        }}"#
    )
}

fn usa2p_usecases_json() -> String {
    r#"{
        "us_app_to_person_usecases":[
            {
                "code":"MARKETING",
                "name":"Marketing",
                "description":"Marketing messages",
                "post_approval_required":true
            }
        ]
    }"#
    .to_owned()
}

fn bulk_contacts_response_json() -> String {
    r#"{
        "items":[
            {
                "contact_id":"+19999999999",
                "correlation_id":"ad388b5a46b33b874b0d41f7226db2ef",
                "country_iso_code":"US",
                "zip_code":"12345",
                "error_code":0,
                "error_messages":[]
            }
        ]
    }"#
    .to_owned()
}

fn bulk_consents_response_json() -> String {
    r#"{
        "items":[
            {
                "contact_id":"+19999999999",
                "correlation_id":"ad388b5a46b33b874b0d41f7226db2ef",
                "sender_id":"MG00000000000000000000000000000001",
                "status":"opt-out",
                "source":"website",
                "date_of_consent":"2025-02-28T10:05:27Z",
                "error_code":0,
                "error_messages":[]
            }
        ]
    }"#
    .to_owned()
}

fn safe_list_json(phone_number: &str) -> String {
    format!(
        r#"{{
            "sid":"GN123",
            "phone_number":"{phone_number}"
        }}"#
    )
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

fn pricing_messaging_json() -> String {
    r#"{
        "name":"Messaging",
        "url":"https://pricing.twilio.com/v1/Messaging",
        "links":{"countries":"https://pricing.twilio.com/v1/Messaging/Countries"}
    }"#
    .to_owned()
}

fn pricing_country_summary_json(country: &str, iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"{country}",
            "iso_country":"{iso_country}",
            "url":"https://pricing.twilio.com/v1/Messaging/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_country_page_json(
    countries: &[String],
    next_page_url: Option<&str>,
    page: u32,
    page_size: u32,
) -> String {
    let countries = countries.join(",");
    let next = next_page_url.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "countries":[{countries}],
            "meta":{{
                "first_page_url":"__BASE_URL__/v1/Messaging/Countries?PageSize={page_size}&Page=0",
                "key":"countries",
                "next_page_url":{next},
                "page":{page},
                "page_size":{page_size},
                "previous_page_url":null,
                "url":"__BASE_URL__/v1/Messaging/Countries?PageSize={page_size}&Page={page}"
            }}
        }}"#
    )
}

fn pricing_generic_country_page_json(
    collection_path: &str,
    next_page_url: Option<&str>,
    page: u32,
    page_size: u32,
) -> String {
    let next = next_page_url.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "countries":[
                {{
                    "country":"United States",
                    "iso_country":"US",
                    "url":"https://pricing.twilio.com/{collection_path}/US"
                }}
            ],
            "meta":{{
                "first_page_url":"__BASE_URL__/{collection_path}?PageSize={page_size}&Page=0",
                "key":"countries",
                "next_page_url":{next},
                "page":{page},
                "page_size":{page_size},
                "previous_page_url":null,
                "url":"__BASE_URL__/{collection_path}?PageSize={page_size}&Page={page}"
            }}
        }}"#
    )
}

fn pricing_country_detail_json(iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"United States",
            "iso_country":"{iso_country}",
            "inbound_sms_prices":[
                {{"base_price":"0.05","current_price":0.04,"number_type":"mobile"}}
            ],
            "outbound_sms_prices":[
                {{
                    "carrier":"att",
                    "mcc":"310",
                    "mnc":"410",
                    "prices":[
                        {{"base_price":"0.05","current_price":0.045,"number_type":"mobile"}}
                    ]
                }}
            ],
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v1/Messaging/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_phone_number_country_json(iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"United States",
            "iso_country":"{iso_country}",
            "phone_number_prices":[
                {{"base_price":"1.00","current_price":"0.50","number_type":"local"}}
            ],
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v1/PhoneNumbers/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_voice_country_json(iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"United States",
            "iso_country":"{iso_country}",
            "outbound_prefix_prices":[
                {{
                    "prefixes":["1"],
                    "base_price":"0.02",
                    "current_price":"0.01",
                    "friendly_name":"United States"
                }}
            ],
            "inbound_call_prices":[
                {{"base_price":"0.02","current_price":"0.01","number_type":"local"}}
            ],
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v1/Voice/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_origin_voice_country_json(iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"United States",
            "iso_country":"{iso_country}",
            "outbound_prefix_prices":[
                {{
                    "origination_prefixes":["1"],
                    "destination_prefixes":["1"],
                    "base_price":"0.02",
                    "current_price":"0.01",
                    "friendly_name":"United States"
                }}
            ],
            "inbound_call_prices":[
                {{"base_price":"0.02","current_price":"0.01","number_type":"local"}}
            ],
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v2/Voice/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_trunking_country_json(iso_country: &str) -> String {
    format!(
        r#"{{
            "country":"United States",
            "iso_country":"{iso_country}",
            "terminating_prefix_prices":[
                {{
                    "origination_prefixes":["1"],
                    "destination_prefixes":["1"],
                    "base_price":"0.02",
                    "current_price":"0.01",
                    "friendly_name":"United States"
                }}
            ],
            "originating_call_prices":[
                {{"base_price":"0.02","current_price":"0.01","number_type":"local"}}
            ],
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v2/Trunking/Countries/{iso_country}"
        }}"#
    )
}

fn pricing_origin_voice_number_json(destination_number: &str) -> String {
    format!(
        r#"{{
            "destination_number":"{destination_number}",
            "origination_number":"15550001111",
            "country":"United States",
            "iso_country":"US",
            "outbound_call_prices":[
                {{"origination_prefixes":["1"],"base_price":"0.02","current_price":"0.01"}}
            ],
            "inbound_call_price":{{"base_price":"0.02","current_price":"0.01","number_type":"local"}},
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v2/Voice/Numbers/{destination_number}"
        }}"#
    )
}

fn pricing_trunking_number_json(destination_number: &str) -> String {
    format!(
        r#"{{
            "destination_number":"{destination_number}",
            "origination_number":"15550001111",
            "country":"United States",
            "iso_country":"US",
            "terminating_prefix_prices":[
                {{
                    "origination_prefixes":["1"],
                    "destination_prefixes":["1"],
                    "base_price":"0.02",
                    "current_price":"0.01",
                    "friendly_name":"United States"
                }}
            ],
            "originating_call_price":{{"base_price":"0.02","current_price":"0.01","number_type":"local"}},
            "price_unit":"USD",
            "url":"https://pricing.twilio.com/v2/Trunking/Numbers/{destination_number}"
        }}"#
    )
}

fn link_shortening_certificate_json() -> String {
    r#"{
        "domain_sid":"DN123",
        "certificate_sid":"CR123",
        "domain_name":"links.example.test",
        "managed":true,
        "requesting":false,
        "cert_in_validation":{"status":"valid","date_expires":"2026-08-01T00:00:00Z"},
        "url":"https://messaging.twilio.com/v1/LinkShortening/Domains/DN123/Certificate"
    }"#
    .to_owned()
}

fn link_shortening_config_json() -> String {
    r#"{
        "domain_sid":"DN123",
        "config_sid":"DC123",
        "messaging_service_sid":"MG123",
        "callback_url":"https://callback.example.test/ls",
        "fallback_url":"https://fallback.example.test/ls",
        "continue_on_failure":true,
        "disable_https":false,
        "url":"https://messaging.twilio.com/v1/LinkShortening/Domains/DN123/Config"
    }"#
    .to_owned()
}

fn link_shortening_dns_validation_json() -> String {
    r#"{
        "domain_sid":"DN123",
        "is_valid":true,
        "reason":"validated",
        "url":"https://messaging.twilio.com/v1/LinkShortening/Domains/DN123/ValidateDns"
    }"#
    .to_owned()
}

fn link_shortening_association_json() -> String {
    r#"{
        "domain_sid":"DN123",
        "messaging_service_sid":"MG123",
        "url":"https://messaging.twilio.com/v1/LinkShortening/Domains/DN123/MessagingServices/MG123"
    }"#
    .to_owned()
}

fn service_usecases_json() -> String {
    r#"{
        "usecases":[
            {"usecase":"marketing","description":"Marketing alerts","purpose":"Promotional messaging"}
        ]
    }"#
    .to_owned()
}

fn preregistered_usa2p_json() -> String {
    r#"{
        "sid":"QE123",
        "account_sid":"AC123",
        "messaging_service_sid":"MG123",
        "campaign_id":"CAMP123",
        "date_created":"2026-07-05T00:00:00Z"
    }"#
    .to_owned()
}

fn brand_registration_otp_json() -> String {
    r#"{
        "account_sid":"AC123",
        "brand_registration_sid":"BN123"
    }"#
    .to_owned()
}

fn typing_success_json() -> String {
    r#"{"success":true}"#.to_owned()
}

fn messaging_geo_permissions_json() -> String {
    r#"{
        "permissions":[
            {"country_code":"US","type":"country","enabled":true,"prefix":"+1","message":"High Risk Country","error_code":0,"error_messages":[]}
        ]
    }"#
    .to_owned()
}

fn messaging_v2_channel_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "sid":"{sid}",
            "sender_id":"whatsapp:+15551234567",
            "status":"ONLINE",
            "configuration":{{"waba_id":"WABA123","verification_method":"sms"}},
            "webhook":{{"callback_url":"https://callback.example.test/channel","callback_method":"POST"}},
            "profile":{{"name":"Example Brand"}},
            "compliance":{{"registration_sid":"CR123","countries":[{{"country":"US","registration_sid":"CRUS","status":"ONLINE","carriers":[{{"name":"Example Carrier","status":"APPROVED","url":"https://example.test/compliance"}}]}}]}},
            "url":"https://messaging.twilio.com/v2/Channels/Senders/{sid}"
        }}"#
    )
}

fn messaging_v2_channel_sender_page_json(sid: &str, next_page_url: Option<&str>) -> String {
    let next = next_page_url.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "senders":[{}],
            "meta":{{
                "first_page_url":"__BASE_URL__/v2/Channels/Senders?Channel=whatsapp&PageSize=1&Page=0",
                "key":"senders",
                "next_page_url":{next},
                "page":0,
                "page_size":1,
                "previous_page_url":null,
                "url":"__BASE_URL__/v2/Channels/Senders?Channel=whatsapp&PageSize=1&Page=0"
            }}
        }}"#,
        messaging_v2_channel_sender_json(sid)
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
        error_message: Some("remote error secret +15550001111".to_owned()),
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
