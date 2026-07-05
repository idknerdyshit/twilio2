#![cfg(feature = "sync")]
#![allow(clippy::unwrap_used, clippy::panic, clippy::missing_panics_doc)]

mod support;

use std::time::Duration;

use http::Method;
use rust_decimal::Decimal;
#[cfg(feature = "sensitive-diagnostics")]
use support::twilio_config;
use support::{HttpsMockServer, MockResponse, blocking_client_for, test_agent, test_creds};
use twilio2::{
    ApiFamily, BlockingTwilioClient, CreateMessageRequest, CreateTollfreeVerificationRequest,
    FetchDeactivationsRequest, ListMessagesRequest, ListPricingMessagingCountriesRequest,
    ListTollfreeVerificationsRequest, Operation, RawResponse, RequestOptions, RequestSpec,
    RetryPolicy, TollfreeBusinessRegistrationAuthority, TollfreeBusinessType,
    TollfreeMessageVolume, TollfreeOptInType, TollfreeUseCaseCategory, TollfreeVerificationStatus,
    TollfreeVettingProvider, TwilioClientConfig, TwilioError, TwilioInboundSmsPrice,
    TwilioOutboundSmsPrice, TwilioPricingMessaging, TwilioPricingMessagingCountry,
    TwilioPricingMessagingCountryPage, TwilioPricingMessagingCountrySummary, TwilioSmsPrice,
    UpdateTollfreeVerificationRequest,
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn start_server(
    runtime: &tokio::runtime::Runtime,
    responses: Vec<MockResponse>,
) -> HttpsMockServer {
    runtime.block_on(HttpsMockServer::start(responses))
}

fn assert_basic_auth(request: &support::RecordedRequest) {
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

#[test]
fn constructors_config_and_debug_are_ergonomic_and_redacted() {
    let config = TwilioClientConfig::new()
        .rest_base_url("https://proxy.example.test/rest")
        .messaging_base_url("https://proxy.example.test/messaging/v1")
        .pricing_base_url("https://proxy.example.test/pricing/v1")
        .timeout(Duration::from_secs(7))
        .user_agent("test-agent/1.0");

    let client = BlockingTwilioClient::from_config_and_agent(config.clone(), test_agent()).unwrap();
    let retained = client.config();
    assert_eq!(retained.rest_base_url, "https://proxy.example.test/rest");
    assert_eq!(
        retained.messaging_base_url,
        "https://proxy.example.test/messaging/v1"
    );
    assert_eq!(
        retained.pricing_base_url,
        "https://proxy.example.test/pricing/v1"
    );
    assert!(!format!("{config:?}").contains("proxy.example.test"));
    assert!(!format!("{retained:?}").contains("proxy.example.test"));

    let default = BlockingTwilioClient::from_agent(test_agent());
    assert_eq!(
        default.config().rest_base_url,
        twilio2::DEFAULT_REST_BASE_URL
    );
    assert_eq!(
        default.config().pricing_base_url,
        twilio2::DEFAULT_PRICING_BASE_URL
    );
}

#[test]
fn pricing_messaging_fetch_country_detail_and_pagination_work() {
    let runtime = runtime();
    let next_page_url = "__BASE_URL__/v1/Messaging/Countries?PageSize=2&Page=1&PageToken=next";
    let server = start_server(
        &runtime,
        vec![
            MockResponse::json(pricing_messaging_json()),
            MockResponse::json(pricing_country_detail_json("US")),
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
        ],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    let messaging: TwilioPricingMessaging = account.pricing().messaging().fetch().unwrap();
    assert_eq!(messaging.name.as_deref(), Some("Messaging"));
    assert!(!format!("{messaging:?}").contains("pricing.twilio.com"));

    let country: TwilioPricingMessagingCountry = account
        .pricing()
        .messaging()
        .countries()
        .fetch("us")
        .unwrap();
    let inbound: &TwilioInboundSmsPrice = &country.inbound_sms_prices[0];
    assert_eq!(inbound.base_price, Some(Decimal::new(5, 2)));
    assert_eq!(inbound.current_price, Some(Decimal::new(4, 2)));
    let outbound: &TwilioOutboundSmsPrice = &country.outbound_sms_prices[0];
    let sms_price: &TwilioSmsPrice = &outbound.prices[0];
    assert_eq!(sms_price.current_price, Some(Decimal::new(45, 3)));

    let countries = account.pricing().messaging().countries();
    let first: TwilioPricingMessagingCountryPage = countries
        .list(
            ListPricingMessagingCountriesRequest::new()
                .page_size(2)
                .page(0),
        )
        .unwrap();
    let first_country: &TwilioPricingMessagingCountrySummary = &first.countries[0];
    assert_eq!(first_country.iso_country.as_deref(), Some("US"));
    let second = countries
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .unwrap();
    assert_eq!(second.countries[0].iso_country.as_deref(), Some("CA"));

    let all = countries
        .list_all_with(ListPricingMessagingCountriesRequest::new().page_size(2))
        .collect_all()
        .unwrap();
    assert_eq!(all.len(), 2);

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Messaging");
    assert_eq!(requests[1].path, "/v1/Messaging/Countries/US");
    assert_eq!(
        requests[2].path,
        "/v1/Messaging/Countries?PageSize=2&Page=0"
    );
    assert_eq!(
        requests[3].path,
        "/v1/Messaging/Countries?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(requests[4].path, "/v1/Messaging/Countries?PageSize=2");
    assert_basic_auth(&requests[0]);
}

#[test]
fn pricing_messaging_countries_validate_requests_and_page_urls() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![MockResponse::json(pricing_country_page_json(
            &[pricing_country_summary_json("United States", "US")],
            Some("__BASE_URL__/v1/Messaging/Countries?PageSize=3&Page=1&PageToken=next"),
            0,
            2,
        ))],
    );
    let client = blocking_client_for(&server);
    let countries = client
        .account(test_creds())
        .pricing()
        .messaging()
        .countries();

    let err = countries
        .list(ListPricingMessagingCountriesRequest::new().page_size(0))
        .unwrap_err();
    assert_invalid_request(err, "PageSize");

    let err = countries.fetch("usa").unwrap_err();
    assert_invalid_request(err, "IsoCountry");

    let err = countries
        .list_page_url("https://example.test/v1/Messaging/Countries?Page=1")
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    let err = countries
        .list(ListPricingMessagingCountriesRequest::new().page_size(2))
        .unwrap_err();
    assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));

    assert_eq!(server.requests().len(), 1);
}

#[test]
fn pricing_continuation_api_errors_redact_page_url() {
    let runtime = runtime();
    let page_token = "pricing-cursor-secret";
    let server = start_server(
        &runtime,
        vec![MockResponse::status_json(
            400,
            format!(
                r#"{{"message":"bad cursor __BASE_URL__/v1/Messaging/Countries?PageSize=2&Page=1&PageToken={page_token}"}}"#
            ),
        )],
    );
    let next_page_url = format!(
        "{}/v1/Messaging/Countries?PageSize=2&Page=1&PageToken={page_token}",
        server.base_url
    );

    let err = blocking_client_for(&server)
        .account(test_creds())
        .pricing()
        .messaging()
        .countries()
        .list_page_url(&next_page_url)
        .unwrap_err();

    match err {
        TwilioError::Api { status, body } => {
            assert_eq!(status, 400);
            assert!(
                !body.contains(&next_page_url),
                "API error body leaked cursor: {body}"
            );
            assert!(
                !body.contains(page_token),
                "API error body leaked page token: {body}"
            );
        }
        other => panic!("expected API error, got {other:?}"),
    }
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
        vec!["trace-secret".to_owned(), "+15551234567".to_owned()]
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

#[test]
fn custom_operation_supports_options_meta_raw_response_and_auth() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![
            MockResponse::json(message_page_json(
                &[message_json("SMop", "sent", "hello")],
                None,
            ))
            .header("retry-after", "3"),
        ],
    );
    let client = blocking_client_for(&server);
    let options = RequestOptions::new()
        .query("Trace", "trace-secret")
        .header("x-request-id", "req-123")
        .timeout(Duration::from_secs(5))
        .retry(RetryPolicy::none().with_max_retries(1));

    let response = client
        .account(test_creds())
        .send_with_response_with_options(MessagesValueOperation, options)
        .unwrap();

    assert_eq!(response.meta.status, 200);
    assert_eq!(response.meta.retry_after, Some(Duration::from_secs(3)));
    assert!(response.raw.body.len() > 20);
    assert!(!format!("{:?}", response.raw).contains("SMop"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=1&Trace=trace-secret"
    );
    assert_eq!(requests[0].header("x-request-id"), Some("req-123"));
    assert_basic_auth(&requests[0]);
}

#[test]
fn request_options_reject_blocked_headers_and_unsafe_retries_before_transport() {
    let header_err = RequestOptions::new()
        .try_header("Authorization", "Basic wrong")
        .unwrap_err();
    assert_invalid_request(header_err, "cannot be overridden");

    let runtime = runtime();
    let server = start_server(&runtime, Vec::new());
    let client = blocking_client_for(&server);
    let deferred_header_err = client
        .account(test_creds())
        .send_with_options(
            MessagesValueOperation,
            RequestOptions::new().header("Authorization", "Basic wrong"),
        )
        .unwrap_err();
    assert_invalid_request(deferred_header_err, "cannot be overridden");
    assert!(server.requests().is_empty());

    let err = client
        .account(test_creds())
        .send_with_options(
            UnsafePostOperation,
            RequestOptions::new().retry(RetryPolicy::none().with_max_retries(1)),
        )
        .unwrap_err();
    assert_invalid_request(err, "safe HTTP methods");
    assert!(server.requests().is_empty());
}

#[test]
fn messages_create_list_and_blocking_paginator_use_expected_wire_shape() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![
            MockResponse::created_json(message_json("SMcreated", "queued", "hello")),
            MockResponse::json(message_page_json(
                &[message_json("SMpage1", "sent", "one")],
                Some("/2010-04-01/Accounts/AC123/Messages.json?PageSize=50&Page=1&PageToken=next"),
            )),
            MockResponse::json(message_page_json(
                &[message_json("SMpage2", "sent", "two")],
                None,
            )),
        ],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    let created = account
        .messages()
        .create(
            CreateMessageRequest::new("+15551234567")
                .from("+15557654321")
                .body("hello"),
        )
        .unwrap();
    let all = account.messages().list_all().collect_all().unwrap();

    assert_eq!(created.sid.as_deref(), Some("SMcreated"));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].sid.as_deref(), Some("SMpage1"));
    assert_eq!(all[1].sid.as_deref(), Some("SMpage2"));

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
    assert!(requests[0].body.contains("To=%2B15551234567"));
    assert!(requests[0].body.contains("From=%2B15557654321"));
    assert!(requests[0].body.contains("Body=hello"));
    assert_basic_auth(&requests[0]);
    assert_eq!(
        requests[1].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=50"
    );
    assert_eq!(
        requests[2].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=50&Page=1&PageToken=next"
    );
}

#[test]
fn deactivations_accept_307_and_list_validation_happens_before_transport() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![MockResponse::status_json(
            307,
            r#"{"redirect_to":"https://storage.example.test/deactivations?signature=secret"}"#,
        )],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    let deactivation = account
        .deactivations()
        .fetch(FetchDeactivationsRequest::new("2025-01-31"))
        .unwrap();
    assert_eq!(
        deactivation.redirect_to.as_deref(),
        Some("https://storage.example.test/deactivations?signature=secret")
    );

    let err = account
        .messages()
        .list(ListMessagesRequest::new().page_size(0))
        .unwrap_err();
    assert_invalid_request(err, "PageSize");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn pagination_rejects_adversarial_next_page_uri() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![MockResponse::json(message_page_json(
            &[message_json("SMpage1", "sent", "one")],
            Some("https://evil.example.test/2010-04-01/Accounts/AC123/Messages.json?PageToken=x"),
        ))],
    );
    let client = blocking_client_for(&server);

    let err = client
        .account(test_creds())
        .messages()
        .list(ListMessagesRequest::new())
        .unwrap_err();
    assert!(matches!(
        err,
        TwilioError::InvalidRequest(_)
            | TwilioError::InvalidBaseUrl(_)
            | TwilioError::InvalidResponseMetadata(_)
    ));
}

#[test]
fn large_api_errors_are_truncated_redacted_and_safe_get_retries() {
    let runtime = runtime();
    let huge = format!(r#"{{"message":"{} token +15551234567"}}"#, "x".repeat(4096));
    let server = start_server(
        &runtime,
        vec![
            MockResponse::status_json(503, r#"{"message":"retry"}"#),
            MockResponse::json(message_page_json(
                &[message_json("SMretry", "sent", "retry")],
                None,
            )),
            MockResponse::status_json(400, huge),
        ],
    );
    let client = blocking_client_for(&server);
    let retry = RetryPolicy::none()
        .with_max_retries(1)
        .with_base_delay(Duration::ZERO)
        .with_max_delay(Duration::ZERO)
        .with_jitter(false);

    let value = client
        .account(test_creds())
        .send_with_options(MessagesValueOperation, RequestOptions::new().retry(retry))
        .unwrap();
    assert_eq!(value["messages"][0]["sid"], "SMretry");

    let err = client
        .account(test_creds())
        .send(MessagesValueOperation)
        .unwrap_err();
    match err {
        TwilioError::Api { status, body } => {
            assert_eq!(status, 400);
            assert!(body.len() <= 2051);
            assert!(!body.contains("token"));
            assert!(!body.contains("+15551234567"));
        }
        other => panic!("expected API error, got {other:?}"),
    }

    assert_eq!(server.requests().len(), 3);
}

#[test]
fn tollfree_sync_repeated_form_and_query_keys_use_expected_wire_shape() {
    let runtime = runtime();
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
    let next_page_url = "__BASE_URL__/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1&Page=1&PageToken=next";
    let server = start_server(
        &runtime,
        vec![
            MockResponse::created_json(tollfree_verification_json("HHcreate", "PENDING_REVIEW")),
            MockResponse::json(tollfree_verification_page_json(
                &[tollfree_verification_json("HHlist", "TWILIO_APPROVED")],
                Some(next_page_url),
            )),
            MockResponse::json(tollfree_verification_page_json(&[], None)),
            MockResponse::json(tollfree_verification_json("HHupdate", "IN_REVIEW")),
        ],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    let created = account
        .tollfree_verifications()
        .create(tollfree_create_request(
            &categories,
            &opt_in_image_urls,
            &opt_in_keywords,
        ))
        .unwrap();
    let first = account
        .tollfree_verifications()
        .list(
            ListTollfreeVerificationsRequest::new()
                .status(TollfreeVerificationStatus::TwilioApproved)
                .trust_product_sids(&trust_product_sids)
                .page_size(1),
        )
        .unwrap();
    let second = account
        .tollfree_verifications()
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
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
        .unwrap();

    assert_eq!(created.sid.as_deref(), Some("HHcreate"));
    assert_eq!(
        first.tollfree_verifications[0].sid.as_deref(),
        Some("HHlist")
    );
    assert!(second.tollfree_verifications.is_empty());
    assert_eq!(updated.sid.as_deref(), Some("HHupdate"));

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].path, "/v1/Tollfree/Verifications");
    assert_repeated_tollfree_body(&requests[0].body);
    assert_eq!(
        requests[1].path,
        "/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1"
    );
    assert_eq!(
        requests[2].path,
        "/v1/Tollfree/Verifications?Status=TWILIO_APPROVED&TrustProductSid=BUtrust1&TrustProductSid=BUtrust2&PageSize=1&Page=1&PageToken=next"
    );
    assert_eq!(requests[3].path, "/v1/Tollfree/Verifications/HHupdate");
    assert_eq!(
        requests[3].body,
        "BusinessName=Owl+Updated&EditReason=Website+fixed&AgeGatedContent=false&OptInKeywords=START&OptInKeywords=JOIN"
    );
    for request in &requests {
        assert_basic_auth(request);
    }
}

#[test]
#[cfg(feature = "sensitive-diagnostics")]
fn sensitive_diagnostics_capture_sync_request_and_response() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![MockResponse::json(message_page_json(
            &[message_json("SMdiag", "sent", "diag")],
            None,
        ))],
    );
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let diagnostics = twilio2::SensitiveDiagnostics::new({
        let events = std::sync::Arc::clone(&events);
        move |event| events.lock().unwrap().push(event)
    });
    let client = BlockingTwilioClient::from_config_and_agent(
        TwilioClientConfig::new()
            .base_urls(twilio_config(&server.base_url))
            .with_sensitive_diagnostics(diagnostics),
        test_agent(),
    )
    .unwrap();

    let page = client
        .account(test_creds())
        .messages()
        .list(ListMessagesRequest::new())
        .unwrap();
    assert_eq!(page.messages[0].sid.as_deref(), Some("SMdiag"));

    let events = events.lock().unwrap();
    assert!(events.iter().any(|event| {
        matches!(
            event,
            twilio2::SensitiveDiagnosticEvent::Request(request)
                if request.headers.get("authorization").is_some()
                    && request.url.contains("/2010-04-01/Accounts/AC123/Messages.json")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            twilio2::SensitiveDiagnosticEvent::Response(response)
                if response.status == 200 && !response.body.is_empty()
        )
    }));
}

fn tollfree_create_request<'a>(
    categories: &'a [TollfreeUseCaseCategory<'a>],
    opt_in_image_urls: &'a [&'a str],
    opt_in_keywords: &'a [&'a str],
) -> CreateTollfreeVerificationRequest<'a> {
    CreateTollfreeVerificationRequest::new()
        .business_name("Owl, Inc.")
        .business_website("https://example.test")
        .notification_email("support@example.test")
        .use_case_categories(categories)
        .use_case_summary("Account security and marketing alerts")
        .production_message_sample("Your code is 123456")
        .opt_in_image_urls(opt_in_image_urls)
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
        .opt_in_keywords(opt_in_keywords)
        .vetting_provider(TollfreeVettingProvider::CampaignVerify)
        .vetting_id("vetting-123")
}

fn assert_repeated_tollfree_body(body: &str) {
    for expected in [
        "BusinessName=Owl%2C+Inc.",
        "BusinessWebsite=https%3A%2F%2Fexample.test",
        "UseCaseCategories=TWO_FACTOR_AUTHENTICATION",
        "UseCaseCategories=MARKETING",
        "UseCaseCategories=POLLING_AND_VOTING_NON_POLITICAL",
        "OptInImageUrls=https%3A%2F%2Fexample.test%2Fopt-in-1.png",
        "OptInImageUrls=https%3A%2F%2Fexample.test%2Fopt-in-2.png",
        "OptInKeywords=START",
        "OptInKeywords=JOIN",
    ] {
        assert!(body.contains(expected), "missing {expected} in {body}");
    }
}

fn tollfree_verification_json(sid: &str, status: &str) -> String {
    format!(
        r#"{{
            "sid": "{sid}",
            "account_sid": "AC123",
            "status": "{status}",
            "date_created": "2021-01-27T14:18:35Z",
            "date_updated": "2021-01-27T14:18:36Z"
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
                "page_size": 1,
                "first_page_url": "__BASE_URL__/v1/Tollfree/Verifications?PageSize=1&Page=0",
                "previous_page_url": null,
                "next_page_url": {next},
                "key": "verifications",
                "url": "__BASE_URL__/v1/Tollfree/Verifications?PageSize=1&Page=0"
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

fn message_json(sid: &str, status: &str, body: &str) -> String {
    format!(
        r#"{{
            "sid":"{sid}",
            "account_sid":"AC123",
            "to":"+15551234567",
            "from":"+15557654321",
            "body":"{body}",
            "status":"{status}"
        }}"#
    )
}

fn message_page_json(messages: &[String], next_page_uri: Option<&str>) -> String {
    let messages = messages.join(",");
    let next_page_uri =
        next_page_uri.map_or_else(|| "null".to_owned(), |uri| format!(r#""{uri}""#));
    format!(
        r#"{{
            "messages":[{messages}],
            "next_page_uri":{next_page_uri}
        }}"#
    )
}
