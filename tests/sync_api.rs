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
    A2PBrandType, A2PUsecase, A2PVettingProvider, ApiFamily, AppleTypingEvent,
    BlockingTwilioClient, BulkConsentsRequest, BulkContactsRequest, ChannelSenderConfiguration,
    ChannelSenderHttpMethod, ChannelSenderProfile, ChannelSenderWebhook, ConsentItem,
    ConsentSource, ConsentStatus, ContactItem, CreateA2PBrandRegistrationRequest,
    CreateA2PBrandVettingRequest, CreateMessageRequest, CreateMessagingV2ChannelSenderRequest,
    CreateMessagingV2TypingIndicatorRequest, CreateMessagingV3TypingIndicatorRequest,
    CreateTollfreeVerificationRequest, CreateUsa2pRequest, FetchDeactivationsRequest,
    FetchUsa2pUsecasesRequest, ListA2PBrandRegistrationsRequest, ListA2PBrandVettingsRequest,
    ListMessagesRequest, ListMessagingV2ChannelSendersRequest,
    ListPricingMessagingCountriesRequest, ListTollfreeVerificationsRequest, ListUsa2pRequest,
    MessagingGeoPermissionUpdateItem, MessagingV2Channel, Operation, RawResponse, RequestOptions,
    RequestSpec, RetryPolicy, SafeListNumberRequest, TollfreeBusinessRegistrationAuthority,
    TollfreeBusinessType, TollfreeMessageVolume, TollfreeOptInType, TollfreeUseCaseCategory,
    TollfreeVerificationStatus, TollfreeVettingProvider, TwilioClientConfig, TwilioError,
    TwilioInboundSmsPrice, TwilioOutboundSmsPrice, TwilioPricingMessaging,
    TwilioPricingMessagingCountry, TwilioPricingMessagingCountryPage,
    TwilioPricingMessagingCountrySummary, TwilioSmsPrice, UpdateMessagingGeoPermissionsRequest,
    UpdateMessagingV2ChannelSenderRequest, UpdateTollfreeVerificationRequest,
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
        .messaging_base_url("https://proxy.example.test/messaging")
        .pricing_base_url("https://proxy.example.test/pricing")
        .timeout(Duration::from_secs(7))
        .user_agent("test-agent/1.0");

    let client = BlockingTwilioClient::from_config_and_agent(config.clone(), test_agent()).unwrap();
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
    assert!(!format!("{config:?}").contains("proxy.example.test"));
    assert!(!format!("{retained:?}").contains("proxy.example.test"));

    let default = BlockingTwilioClient::from_agent(test_agent());
    assert_eq!(
        default.config().rest_base_url_ref(),
        twilio2::DEFAULT_REST_BASE_URL
    );
    assert_eq!(
        default.config().pricing_base_url_ref(),
        twilio2::DEFAULT_PRICING_BASE_URL
    );
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

    let constructor_err = BlockingTwilioClient::from_config_and_agent(
        TwilioClientConfig::new()
            .messaging_base_url("https://proxy.example.test/messaging/v3")
            .pricing_base_url("https://proxy.example.test/pricing"),
        test_agent(),
    )
    .err()
    .expect("versioned Messaging root should fail constructor validation");
    assert!(
        matches!(constructor_err, TwilioError::InvalidBaseUrl(message) if message.contains("product roots"))
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

    let messaging: TwilioPricingMessaging = account.pricing().v1().messaging().fetch().unwrap();
    assert_eq!(messaging.name.as_deref(), Some("Messaging"));
    assert!(!format!("{messaging:?}").contains("pricing.twilio.com"));

    let country: TwilioPricingMessagingCountry = account
        .pricing()
        .v1()
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

    let countries = account.pricing().v1().messaging().countries();
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
        .v1()
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
        .v1()
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
#[allow(clippy::too_many_lines)]
fn a2p_and_accounts_features_have_blocking_wire_shape() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![
            MockResponse::created_json(a2p_brand_json("BNcreate")),
            MockResponse::json(page_json(
                "data",
                "a2p/BrandRegistrations",
                &[a2p_brand_json("BNlist")],
                Some("/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next"),
            )),
            MockResponse::json(page_json("data", "a2p/BrandRegistrations", &[], None)),
            MockResponse::json(a2p_brand_json("BNfetch")),
            MockResponse::json(a2p_brand_json("BNupdate")),
            MockResponse::created_json(a2p_vetting_json("VTcreate")),
            MockResponse::json(page_json(
                "data",
                "a2p/BrandRegistrations/BNbrand/Vettings",
                &[a2p_vetting_json("VTlist")],
                Some(
                    "/v1/a2p/BrandRegistrations/BNbrand/Vettings?PageSize=2&Page=1&PageToken=next",
                ),
            )),
            MockResponse::json(page_json(
                "data",
                "a2p/BrandRegistrations/BNbrand/Vettings",
                &[],
                None,
            )),
            MockResponse::json(a2p_vetting_json("VTfetch")),
            MockResponse::created_json(usa2p_json("QEcreate")),
            MockResponse::json(page_json(
                "compliance",
                "Services/MG123/Compliance/Usa2p",
                &[usa2p_json("QElist")],
                Some("/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=1&PageToken=next"),
            )),
            MockResponse::json(page_json(
                "compliance",
                "Services/MG123/Compliance/Usa2p",
                &[],
                None,
            )),
            MockResponse::json(usa2p_json("QEfetch")),
            MockResponse::json(usa2p_usecases_json()),
            MockResponse::no_content(),
            MockResponse::json(bulk_contacts_response_json()),
            MockResponse::json(bulk_consents_response_json()),
            MockResponse::created_json(safe_list_json("+18001234567")),
            MockResponse::json(safe_list_json("+18001234567")),
            MockResponse::no_content(),
        ],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    let brand = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .create(
            CreateA2PBrandRegistrationRequest::new()
                .customer_profile_bundle_sid("BUcustomer")
                .a2p_profile_bundle_sid("BUa2p")
                .brand_type(A2PBrandType::Standard)
                .mock(true),
        )
        .unwrap();
    let brand_page = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list(ListA2PBrandRegistrationsRequest::new().page_size(2).page(0))
        .unwrap();
    let brand_next = format!(
        "{}/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let brand_second = account
        .messaging()
        .v1()
        .a2p_brand_registrations()
        .list_page_url(&brand_next)
        .unwrap();
    let brand_fetch = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNfetch")
        .fetch()
        .unwrap();
    let brand_update = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNupdate")
        .update()
        .unwrap();
    let vetting = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .create(
            CreateA2PBrandVettingRequest::new(A2PVettingProvider::CampaignVerify)
                .vetting_id("vetting-token"),
        )
        .unwrap();
    let vetting_page = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .list(ListA2PBrandVettingsRequest::new().page_size(2).page(0))
        .unwrap();
    let vetting_next = format!(
        "{}/v1/a2p/BrandRegistrations/BNbrand/Vettings?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let vetting_second = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .list_page_url(&vetting_next)
        .unwrap();
    let vetting_fetch = account
        .messaging()
        .v1()
        .a2p_brand_registration("BNbrand")
        .vettings()
        .fetch("VTfetch")
        .unwrap();

    let usa2p_request = CreateUsa2pRequest::new()
        .brand_registration_sid("BN123")
        .description("Transactional alerts for customer account activity.")
        .message_flow("Customers opt in during account signup and settings.")
        .message_samples(&[
            "Your account login code is 123456.",
            "A new device signed in to your account.",
        ])
        .us_app_to_person_usecase(A2PUsecase::Marketing)
        .has_embedded_links(true)
        .has_embedded_phone(false);
    let usa2p = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .create(usa2p_request)
        .unwrap();
    let usa2p_page = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .list(ListUsa2pRequest::new().page_size(2).page(0))
        .unwrap();
    let usa2p_next = format!(
        "{}/v1/Services/MG123/Compliance/Usa2p?PageSize=2&Page=1&PageToken=next",
        server.base_url
    );
    let usa2p_second = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .list_page_url(&usa2p_next)
        .unwrap();
    let usa2p_fetch = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .fetch("QEfetch")
        .unwrap();
    let usecases = account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p_usecases()
        .fetch(FetchUsa2pUsecasesRequest::new().brand_registration_sid("BN123"))
        .unwrap();
    account
        .messaging()
        .v1()
        .service("MG123")
        .usa2p()
        .delete("QEdelete")
        .unwrap();

    let contacts = account
        .contacts()
        .bulk_upsert(BulkContactsRequest::new().item(ContactItem::new(
            "+19999999999",
            "ad388b5a46b33b874b0d41f7226db2ef",
            "US",
            "12345",
        )))
        .unwrap();
    let consents = account
        .consents()
        .bulk_upsert(BulkConsentsRequest::new().item(ConsentItem::new(
            "+19999999999",
            "ad388b5a46b33b874b0d41f7226db2ef",
            "MG00000000000000000000000000000001",
            ConsentStatus::OptOut,
            ConsentSource::Website,
        )))
        .unwrap();
    let safe_added = account
        .global_safe_list()
        .add(SafeListNumberRequest::new("+18001234567"))
        .unwrap();
    let safe_checked = account
        .global_safe_list()
        .check(SafeListNumberRequest::new("+18001234567"))
        .unwrap();
    account
        .global_safe_list()
        .remove(SafeListNumberRequest::new("+18001234567"))
        .unwrap();

    assert_eq!(brand.sid.as_deref(), Some("BNcreate"));
    assert_eq!(
        brand_page.brand_registrations[0].sid.as_deref(),
        Some("BNlist")
    );
    assert!(brand_second.brand_registrations.is_empty());
    assert_eq!(brand_fetch.sid.as_deref(), Some("BNfetch"));
    assert_eq!(brand_update.sid.as_deref(), Some("BNupdate"));
    assert_eq!(vetting.brand_vetting_sid.as_deref(), Some("VTcreate"));
    assert_eq!(
        vetting_page.vettings[0].brand_vetting_sid.as_deref(),
        Some("VTlist")
    );
    assert!(vetting_second.vettings.is_empty());
    assert_eq!(vetting_fetch.brand_vetting_sid.as_deref(), Some("VTfetch"));
    assert_eq!(usa2p.sid.as_deref(), Some("QEcreate"));
    assert_eq!(usa2p_page.compliance[0].sid.as_deref(), Some("QElist"));
    assert!(usa2p_second.compliance.is_empty());
    assert_eq!(usa2p_fetch.sid.as_deref(), Some("QEfetch"));
    assert_eq!(
        usecases.us_app_to_person_usecases[0].code.as_deref(),
        Some("MARKETING")
    );
    assert_eq!(
        contacts.items[0].contact_id.as_deref(),
        Some("+19999999999")
    );
    assert_eq!(consents.items[0].status.as_deref(), Some("opt-out"));
    assert_eq!(safe_added.phone_number.as_deref(), Some("+18001234567"));
    assert_eq!(safe_checked.sid.as_deref(), Some("GN123"));

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/a2p/BrandRegistrations");
    assert!(
        requests[0]
            .body
            .contains("CustomerProfileBundleSid=BUcustomer")
    );
    assert_eq!(
        requests[1].path,
        "/v1/a2p/BrandRegistrations?PageSize=2&Page=0"
    );
    assert_eq!(
        requests[2].path,
        "/v1/a2p/BrandRegistrations?PageSize=2&Page=1&PageToken=next"
    );
    assert_eq!(
        requests[5].body,
        "VettingProvider=campaign-verify&VettingId=vetting-token"
    );
    assert_eq!(requests[9].path, "/v1/Services/MG123/Compliance/Usa2p");
    assert!(requests[9].body.contains("UsAppToPersonUsecase=MARKETING"));
    assert_eq!(
        requests[13].path,
        "/v1/Services/MG123/Compliance/Usa2p/Usecases?BrandRegistrationSid=BN123"
    );
    assert_eq!(requests[14].method, "DELETE");
    assert_eq!(requests[15].path, "/v1/Contacts/Bulk");
    assert_eq!(decoded_form_pairs(&requests[15].body)[0].0, "Items");
    assert_eq!(requests[16].path, "/v1/Consents/Bulk");
    assert_eq!(requests[17].body, "PhoneNumber=%2B18001234567");
    assert_eq!(
        requests[18].path,
        "/v1/SafeList/Numbers?PhoneNumber=%2B18001234567"
    );
    assert_eq!(requests[19].method, "DELETE");
}

#[test]
fn sync_messaging_v2_channel_senders_json_and_pagination_work() {
    let runtime = runtime();
    let next_page_url =
        "__BASE_URL__/v2/Channels/Senders?Channel=whatsapp&PageSize=1&Page=1&PageToken=next";
    let server = start_server(
        &runtime,
        vec![
            MockResponse::json(messaging_v2_channel_sender_json("XEcreate")),
            MockResponse::json(messaging_v2_channel_sender_page_json(
                "XEpage1",
                Some(next_page_url),
            )),
            MockResponse::json(messaging_v2_channel_sender_page_json("XEpage2", None)),
            MockResponse::json(messaging_v2_channel_sender_json("XEfetch")),
            MockResponse::json(messaging_v2_channel_sender_json("XEupdate")),
            MockResponse::no_content(),
        ],
    );
    let client = blocking_client_for(&server);
    let senders = client
        .account(test_creds())
        .messaging()
        .v2()
        .channel_senders();

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
                .profile(ChannelSenderProfile::new().name("Example Brand")),
        )
        .unwrap();
    assert_eq!(created.sid.as_deref(), Some("XEcreate"));
    assert_eq!(
        created
            .compliance
            .as_ref()
            .and_then(|compliance| compliance.registration_sid.as_deref()),
        Some("CR123")
    );

    let first = senders
        .list(ListMessagingV2ChannelSendersRequest::new(MessagingV2Channel::Whatsapp).page_size(1))
        .unwrap();
    assert_eq!(first.senders[0].sid.as_deref(), Some("XEpage1"));
    let second = senders
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .unwrap();
    assert_eq!(second.senders[0].sid.as_deref(), Some("XEpage2"));
    senders.sender("XEfetch").fetch().unwrap();
    senders
        .sender("XEupdate")
        .update(
            UpdateMessagingV2ChannelSenderRequest::new().webhook(
                ChannelSenderWebhook::new()
                    .callback_url("https://callback.example.test/status")
                    .callback_method(ChannelSenderHttpMethod::Put),
            ),
        )
        .unwrap();
    senders.sender("XEdelete").delete().unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v2/Channels/Senders");
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    let create_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(create_body["sender_id"], "whatsapp:+15551234567");
    assert_eq!(create_body["configuration"]["waba_id"], "WABA123");
    assert_eq!(
        create_body["webhook"]["callback_url"],
        "https://callback.example.test/channel"
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
    assert_eq!(update_body["webhook"]["callback_method"], "PUT");
    assert_eq!(requests[5].method, "DELETE");
    assert_eq!(requests[5].path, "/v2/Channels/Senders/XEdelete");
}

#[test]
fn blocking_new_messaging_endpoints_use_expected_wire_shape() {
    let runtime = runtime();
    let server = start_server(
        &runtime,
        vec![
            MockResponse::json(r#"{"domain_sid":"DN123"}"#),
            MockResponse::json(r#"{"success":true}"#),
            MockResponse::json(r#"{"success":true}"#),
            MockResponse::json(r#"{"permissions":[]}"#),
        ],
    );
    let client = blocking_client_for(&server);
    let account = client.account(test_creds());

    account
        .messaging()
        .v1()
        .link_shortening()
        .domain("DN123")
        .certificate()
        .fetch()
        .unwrap();
    account
        .messaging()
        .v2()
        .typing_indicators()
        .create(CreateMessagingV2TypingIndicatorRequest::whatsapp(
            "wamid.secret",
        ))
        .unwrap();
    account
        .messaging()
        .v3()
        .typing_indicators()
        .create(
            CreateMessagingV3TypingIndicatorRequest::rcs("rcs:brand_agent", "rcs:+15551234567")
                .event(AppleTypingEvent::Start),
        )
        .unwrap();
    account
        .messaging_geo_permissions()
        .update(
            UpdateMessagingGeoPermissionsRequest::new()
                .permission(MessagingGeoPermissionUpdateItem::country("US", true)),
        )
        .unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        assert_basic_auth(request);
    }
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].path,
        "/v1/LinkShortening/Domains/DN123/Certificate"
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/v2/Indicators/Typing.json");
    assert_eq!(requests[1].body, "channel=whatsapp&messageId=wamid.secret");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/v3/Indicators/Typing.json");
    let typing_body: serde_json::Value = serde_json::from_str(&requests[2].body).unwrap();
    assert_eq!(
        typing_body,
        serde_json::json!({
            "channel": "RCS",
            "from": "rcs:brand_agent",
            "to": "rcs:+15551234567",
            "event": "START"
        })
    );
    assert_eq!(requests[3].method, "PATCH");
    assert_eq!(requests[3].path, "/v1/Messaging/GeoPermissions");
    assert_eq!(decoded_form_pairs(&requests[3].body).len(), 1);
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
        .messaging()
        .v1()
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
            assert!(body.starts_with("<redacted response body; "));
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
        .messaging()
        .v1()
        .tollfree_verifications()
        .create(tollfree_create_request(
            &categories,
            &opt_in_image_urls,
            &opt_in_keywords,
        ))
        .unwrap();
    let first = account
        .messaging()
        .v1()
        .tollfree_verifications()
        .list(
            ListTollfreeVerificationsRequest::new()
                .status(TollfreeVerificationStatus::TwilioApproved)
                .trust_product_sids(&trust_product_sids)
                .page_size(1),
        )
        .unwrap();
    let second = account
        .messaging()
        .v1()
        .tollfree_verifications()
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
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
    let next = next_page_url.map_or_else(
        || "null".to_owned(),
        |value| {
            if value.starts_with('/') {
                format!(r#""__BASE_URL__{value}""#)
            } else {
                format!(r#""{value}""#)
            }
        },
    );
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

fn page_json(
    key: &str,
    collection_key: &str,
    items: &[String],
    next_page_url: Option<&str>,
) -> String {
    let next = next_page_url.map_or_else(
        || "null".to_owned(),
        |value| {
            if value.starts_with('/') {
                format!(r#""__BASE_URL__{value}""#)
            } else {
                format!(r#""{value}""#)
            }
        },
    );
    format!(
        r#"{{
            "meta":{{
                "page":0,
                "page_size":2,
                "first_page_url":"__BASE_URL__/v1/{collection_key}?PageSize=2&Page=0",
                "previous_page_url":null,
                "next_page_url":{next},
                "key":"{key}",
                "url":"__BASE_URL__/v1/{collection_key}?PageSize=2&Page=0"
            }},
            "{key}":[{items}]
        }}"#,
        items = items.join(",")
    )
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
            "links":{{}}
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
    r#"{"us_app_to_person_usecases":[{"code":"MARKETING","name":"Marketing","description":"Marketing messages","post_approval_required":true}]}"#
        .to_owned()
}

fn bulk_contacts_response_json() -> String {
    r#"{"items":[{"contact_id":"+19999999999","correlation_id":"ad388b5a46b33b874b0d41f7226db2ef","country_iso_code":"US","zip_code":"12345","error_code":0,"error_messages":[]}]}"#
        .to_owned()
}

fn bulk_consents_response_json() -> String {
    r#"{"items":[{"contact_id":"+19999999999","correlation_id":"ad388b5a46b33b874b0d41f7226db2ef","sender_id":"MG00000000000000000000000000000001","status":"opt-out","source":"website","error_code":0,"error_messages":[]}]}"#
        .to_owned()
}

fn safe_list_json(phone_number: &str) -> String {
    format!(r#"{{"sid":"GN123","phone_number":"{phone_number}"}}"#)
}

fn messaging_v2_channel_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "sid":"{sid}",
            "sender_id":"whatsapp:+15551234567",
            "status":"ONLINE",
            "configuration":{{"waba_id":"WABA123","verification_method":"sms"}},
            "webhook":{{"callback_url":"https://callback.example.test/channel","callback_method":"POST"}},
            "compliance":{{"registration_sid":"CR123"}},
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
