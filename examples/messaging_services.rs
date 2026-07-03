mod support;

use support::{
    ExampleResult, HttpsMockServer, MockResponse, RecordedRequest, client_for, creds, missing,
};
use twilio2::{
    CreateDestinationAlphaSenderRequest, CreateServicePhoneNumberRequest, CreateServiceRequest,
    HttpMethod, ListDestinationAlphaSendersRequest, ListServiceSubresourcesRequest,
    ListServicesRequest, ServiceResource, TwilioAccount, UpdateServiceRequest,
};

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(service_json("MGalerts", "alerts")),
        MockResponse::json(service_page_json(Some(
            "/v1/Services?PageSize=1&Page=1&PageToken=next",
        ))),
        MockResponse::json(empty_service_page_json()),
        MockResponse::json(service_json("MGalerts", "alerts-v2")),
        MockResponse::created_json(phone_number_json("PNattached")),
        MockResponse::json(phone_number_page_json(Some(
            "/v1/Services/MGalerts/PhoneNumbers?PageSize=1&Page=1&PageToken=next",
        ))),
        MockResponse::json(empty_phone_number_page_json()),
        MockResponse::created_json(destination_alpha_sender_json("AIattached")),
        MockResponse::json(destination_alpha_sender_page_json(Some(
            "/v1/Services/MGalerts/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=1&Page=1&PageToken=next",
        ))),
        MockResponse::json(empty_destination_alpha_sender_page_json()),
        MockResponse::no_content(),
    ])
    .await?;
    let client = client_for(&server)?;
    let account = client.account(creds());
    let service = account.service("MGalerts");

    run_service_flow(account).await?;
    run_phone_number_flow(service).await?;
    run_destination_alpha_sender_flow(service).await?;
    service.delete().await?;
    assert_requests(&server.requests()?);

    Ok(())
}

async fn run_service_flow(account: TwilioAccount<'_>) -> ExampleResult<()> {
    let created = account
        .services()
        .create(
            CreateServiceRequest::new("alerts")
                .inbound_request_url("https://example.test/inbound")
                .inbound_method(HttpMethod::Post)
                .status_callback("https://example.test/status"),
        )
        .await?;
    let services_page = account
        .services()
        .list(ListServicesRequest::new().page_size(1).page(0))
        .await?;
    let services_next_page_url = services_page
        .meta
        .next_page_url
        .as_deref()
        .ok_or_else(|| missing("Services next_page_url"))?;
    let services_next = account
        .services()
        .list_page_url(services_next_page_url)
        .await?;
    let updated = account
        .service("MGalerts")
        .update(
            UpdateServiceRequest::new()
                .friendly_name("alerts-v2")
                .clear_status_callback(),
        )
        .await?;

    assert_eq!(created.sid.as_deref(), Some("MGalerts"));
    assert!(services_next.services.is_empty());
    assert_eq!(updated.friendly_name.as_deref(), Some("alerts-v2"));

    Ok(())
}

async fn run_phone_number_flow(service: ServiceResource<'_>) -> ExampleResult<()> {
    let phone_number = service
        .phone_numbers()
        .create(CreateServicePhoneNumberRequest::new("PNattached"))
        .await?;
    let phone_numbers_page = service
        .phone_numbers()
        .list(ListServiceSubresourcesRequest::new().page_size(1))
        .await?;
    let phone_numbers_next_page_url = phone_numbers_page
        .meta
        .next_page_url
        .as_deref()
        .ok_or_else(|| missing("PhoneNumbers next_page_url"))?;
    let phone_numbers_next = service
        .phone_numbers()
        .list_page_url(phone_numbers_next_page_url)
        .await?;

    assert_eq!(phone_number.sid.as_deref(), Some("PNattached"));
    assert!(phone_numbers_next.phone_numbers.is_empty());

    Ok(())
}

async fn run_destination_alpha_sender_flow(service: ServiceResource<'_>) -> ExampleResult<()> {
    let alpha_sender = service
        .destination_alpha_senders()
        .create(CreateDestinationAlphaSenderRequest::new("MyCo").iso_country_code("FR"))
        .await?;
    let alpha_senders_page = service
        .destination_alpha_senders()
        .list(
            ListDestinationAlphaSendersRequest::new()
                .iso_country_code("FR")
                .page_size(1),
        )
        .await?;
    let alpha_senders_next_page_url = alpha_senders_page
        .meta
        .next_page_url
        .as_deref()
        .ok_or_else(|| missing("DestinationAlphaSenders next_page_url"))?;
    let alpha_senders_next = service
        .destination_alpha_senders()
        .list_page_url(alpha_senders_next_page_url)
        .await?;

    assert_eq!(alpha_sender.iso_country_code.as_deref(), Some("FR"));
    assert!(alpha_senders_next.alpha_senders.is_empty());

    Ok(())
}

fn assert_requests(requests: &[RecordedRequest]) {
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/Services");
    assert!(requests[0].body.contains("FriendlyName=alerts"));
    assert_eq!(requests[1].path, "/v1/Services?PageSize=1&Page=0");
    assert_eq!(
        requests[2].path,
        "/v1/Services?PageSize=1&Page=1&PageToken=next"
    );
    assert_eq!(requests[3].path, "/v1/Services/MGalerts");
    assert_eq!(
        requests[5].path,
        "/v1/Services/MGalerts/PhoneNumbers?PageSize=1"
    );
    assert_eq!(
        requests[8].path,
        "/v1/Services/MGalerts/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=1"
    );
    assert_eq!(requests[10].method, "DELETE");
}

fn service_json(sid: &str, friendly_name: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "friendly_name": "{friendly_name}",
            "sid": "{sid}",
            "date_created": "2015-07-30T20:12:31Z",
            "date_updated": "2015-07-30T20:12:33Z",
            "sticky_sender": true,
            "mms_converter": true,
            "smart_encoding": false,
            "fallback_to_long_code": true,
            "scan_message_content": "inherit",
            "synchronous_validation": true,
            "area_code_geomatch": true,
            "validity_period": 600,
            "inbound_request_url": "https://example.test/inbound",
            "inbound_method": "POST",
            "fallback_url": null,
            "fallback_method": "POST",
            "status_callback": "https://example.test/status",
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

fn service_page_json(next_path: Option<&str>) -> String {
    page_json(
        "services",
        "Services",
        &[service_json("MGlisted", "listed")],
        next_path,
    )
}

fn empty_service_page_json() -> String {
    page_json("services", "Services", &[], None)
}

fn phone_number_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MGalerts",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "phone_number":"+15551234567",
            "country_code":"US",
            "capabilities":["SMS","MMS"],
            "url":"https://messaging.twilio.com/v1/Services/MGalerts/PhoneNumbers/{sid}"
        }}"#
    )
}

fn phone_number_page_json(next_path: Option<&str>) -> String {
    page_json(
        "phone_numbers",
        "Services/MGalerts/PhoneNumbers",
        &[phone_number_json("PNlisted")],
        next_path,
    )
}

fn empty_phone_number_page_json() -> String {
    page_json("phone_numbers", "Services/MGalerts/PhoneNumbers", &[], None)
}

fn destination_alpha_sender_json(sid: &str) -> String {
    format!(
        r#"{{
            "account_sid":"AC123",
            "service_sid":"MGalerts",
            "sid":"{sid}",
            "date_created":"2015-07-30T20:12:31Z",
            "date_updated":"2015-07-30T20:12:33Z",
            "alpha_sender":"MyCo",
            "capabilities":["SMS"],
            "iso_country_code":"FR",
            "url":"https://messaging.twilio.com/v1/Services/MGalerts/DestinationAlphaSenders/{sid}"
        }}"#
    )
}

fn destination_alpha_sender_page_json(next_path: Option<&str>) -> String {
    page_json(
        "alpha_senders",
        "Services/MGalerts/DestinationAlphaSenders",
        &[destination_alpha_sender_json("AIlisted")],
        next_path,
    )
}

fn empty_destination_alpha_sender_page_json() -> String {
    page_json(
        "alpha_senders",
        "Services/MGalerts/DestinationAlphaSenders",
        &[],
        None,
    )
}

fn page_json(
    key: &str,
    collection_path: &str,
    items: &[String],
    next_path: Option<&str>,
) -> String {
    let base_url = "__BASE_URL__";
    let next = next_path.map_or_else(
        || "null".to_owned(),
        |path| format!(r#""{base_url}{path}""#),
    );
    format!(
        r#"{{
            "meta": {{
                "page": 0,
                "page_size": 1,
                "first_page_url": "{base_url}/v1/{collection_path}?PageSize=1&Page=0",
                "previous_page_url": null,
                "next_page_url": {next},
                "key": "{key}",
                "url": "{base_url}/v1/{collection_path}?PageSize=1&Page=0"
            }},
            "{key}": [{items}]
        }}"#,
        items = items.join(",")
    )
}
