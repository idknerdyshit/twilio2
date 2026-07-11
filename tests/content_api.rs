#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]

mod support;

use serde_json::json;
use support::{HttpsMockServer, MockResponse, test_creds};
use twilio2::{
    ContentAction, ContentCard, ContentMedia, ContentQuickReply, ContentText, ContentTypes,
    CreateContentRequest, DeleteContentRequest, ListContentRequest, SubmitWhatsAppApprovalRequest,
    TwilioError, UpdateContentRequest, WhatsAppTemplateCategory,
};

const SID: &str = "HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn content_json(sid: &str) -> String {
    json!({
        "sid": sid,
        "account_sid": "ACaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "friendly_name": "order_update",
        "language": "en",
        "variables": {"1": "Customer"},
        "types": {
            "twilio/text": {"body": "Hello {{1}}"},
            "vendor/future": {"secret": "kept"}
        },
        "url": format!("https://content.twilio.com/v1/Content/{sid}"),
        "links": {"approval_fetch": "https://content.twilio.com/approval"},
        "date_created": "2026-07-11T12:00:00Z",
        "date_updated": "2026-07-11T12:00:00Z"
    })
    .to_string()
}

fn content_page(next: Option<&str>, sid: &str) -> String {
    let item: serde_json::Value = serde_json::from_str(&content_json(sid)).unwrap();
    json!({
        "contents": [item],
        "meta": {
            "page": 0,
            "page_size": 1,
            "key": "contents",
            "next_page_url": next
        }
    })
    .to_string()
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_content_lifecycle_and_approvals_use_expected_wire_contract() {
    let next = "__BASE_URL__/v1/Content?PageSize=1&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(content_json(SID)),
        MockResponse::json(content_page(Some(next), SID)),
        MockResponse::json(content_page(None, "HXbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
        MockResponse::json(
            json!({
                "sid": SID,
                "account_sid": null,
                "friendly_name": null,
                "language": "en",
                "variables": null,
                "types": null,
                "links": null,
                "url": null
            })
            .to_string(),
        ),
        MockResponse::json(content_json(SID)),
        MockResponse::json(json!({"category":"UTILITY","status":"received","name":"order_update","content_type":"twilio/text"}).to_string()),
        MockResponse::json(json!({"sid":SID,"account_sid":"ACaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","whatsapp":{"category":"UTILITY","status":"approved","name":"order_update","content_type":"twilio/text","new_field":true},"url":"https://content.twilio.com/approval"}).to_string()),
        MockResponse::no_content(),
    ]).await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());
    let custom = json!({"opaque": true});
    let types = ContentTypes::new()
        .text(ContentText::new("Hello {{1}}"))
        .media(ContentMedia::new(["https://example.test/image.jpg"]).body("Media"))
        .quick_reply(
            ContentQuickReply::new("Choose").action(ContentAction::quick_reply("track", "Track")),
        )
        .card(
            ContentCard::new()
                .title("Order")
                .action(ContentAction::url("Open", "https://example.test")),
        )
        .custom("vendor/future", &custom)
        .unwrap();

    let created = account
        .content()
        .v1()
        .contents()
        .create(CreateContentRequest::new("order_update", "en", types).variable("1", "Customer"))
        .await
        .unwrap();
    assert_eq!(created.sid.as_deref(), Some(SID));
    assert_eq!(created.types.raw()["vendor/future"]["secret"], "kept");
    assert_eq!(created.types.text().unwrap().unwrap().body, "Hello {{1}}");

    let first = account
        .content()
        .v1()
        .contents()
        .list(ListContentRequest::new().page_size(1))
        .await
        .unwrap();
    let second = account
        .content()
        .v1()
        .contents()
        .list_page_url(first.meta.next_page_url.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(second.contents.len(), 1);
    account.content().v1().content(SID).fetch().await.unwrap();
    account
        .content()
        .v1()
        .content(SID)
        .update(UpdateContentRequest::new().friendly_name("renamed"))
        .await
        .unwrap();
    let approvals = account.content().v1().content(SID).approval_requests();
    approvals
        .submit_whatsapp(SubmitWhatsAppApprovalRequest::new(
            "order_update",
            WhatsAppTemplateCategory::Utility,
        ))
        .await
        .unwrap();
    let status = approvals.fetch().await.unwrap();
    assert_eq!(status.whatsapp.unwrap().status.as_deref(), Some("approved"));
    account
        .content()
        .v1()
        .content(SID)
        .delete(DeleteContentRequest::new().delete_in_waba(true))
        .await
        .unwrap();

    let requests = server.requests();
    let paths: Vec<_> = requests
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str()))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("POST", "/v1/Content"),
            ("GET", "/v1/Content?PageSize=1"),
            ("GET", "/v1/Content?PageSize=1&Page=1&PageToken=next"),
            ("GET", "/v1/Content/HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ("PUT", "/v1/Content/HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            (
                "POST",
                "/v1/Content/HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/ApprovalRequests/whatsapp"
            ),
            (
                "GET",
                "/v1/Content/HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/ApprovalRequests"
            ),
            (
                "DELETE",
                "/v1/Content/HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa?deleteInWaba=true"
            ),
        ]
    );
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    assert_eq!(
        requests[0].header("authorization"),
        Some("Basic QUMxMjM6dG9rZW4=")
    );
    let create: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(create["types"]["vendor/future"], custom);
    assert_eq!(create["types"]["twilio/card"]["title"], "Order");
    assert_eq!(requests[4].method, "PUT");
}

#[cfg(feature = "async")]
#[tokio::test]
async fn content_validation_and_pagination_reject_unsafe_inputs() {
    let server = HttpsMockServer::start(vec![MockResponse::json(content_page(
        Some("https://evil.example/v1/Content?PageSize=1&PageToken=secret"),
        SID,
    ))])
    .await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());
    let error = account
        .content()
        .v1()
        .contents()
        .create(CreateContentRequest::new("name", "en", ContentTypes::new()))
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidRequest(message) if message.contains("Types")));
    let error = account
        .content()
        .v1()
        .content(SID)
        .approval_requests()
        .submit_whatsapp(SubmitWhatsAppApprovalRequest::new(
            "Bad-Name",
            WhatsAppTemplateCategory::Utility,
        ))
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidRequest(_)));
    let error = account
        .content()
        .v1()
        .content("HXbad")
        .fetch()
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidRequest(message) if message.contains("HX SID")));
    let error = account
        .content()
        .v1()
        .contents()
        .list(ListContentRequest::new().page_size(501))
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidRequest(_)));
    let error = account
        .content()
        .v1()
        .contents()
        .list(ListContentRequest::new().page_size(1))
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidResponseMetadata(_)));
    assert_eq!(server.requests().len(), 1);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn content_list_all_collects_validated_pages() {
    let next = "__BASE_URL__/v1/Content?PageSize=50&Page=1&PageToken=next";
    let server = HttpsMockServer::start(vec![
        MockResponse::json(content_page(Some(next), SID)),
        MockResponse::json(content_page(None, "HXbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
    ])
    .await;
    let client = support::client_for(&server);
    let items = client
        .account(test_creds())
        .content()
        .v1()
        .contents()
        .list_all()
        .collect_all()
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(server.requests()[0].path, "/v1/Content?PageSize=50");
}

#[test]
fn content_debug_output_is_redacted() {
    let custom = json!({"secret": "custom-secret"});
    let types = ContentTypes::new()
        .text(ContentText::new("body-secret"))
        .custom("vendor/x", &custom)
        .unwrap();
    let request =
        CreateContentRequest::new("friendly-secret", "en", types).variable("1", "variable-secret");
    let rendered = format!("{request:?}");
    for secret in [
        "body-secret",
        "custom-secret",
        "friendly-secret",
        "variable-secret",
    ] {
        assert!(!rendered.contains(secret));
    }
    assert!(rendered.contains("<redacted>"));
}

#[cfg(feature = "sync")]
#[test]
fn blocking_content_create_fetch_and_delete_work() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(HttpsMockServer::start(vec![
        MockResponse::created_json(content_json(SID)),
        MockResponse::json(content_json(SID)),
        MockResponse::no_content(),
    ]));
    let client = support::blocking_client_for(&server);
    let account = client.account(test_creds());
    let types = ContentTypes::new().text(ContentText::new("Hello"));
    account
        .content()
        .v1()
        .contents()
        .create(CreateContentRequest::new("name", "en", types))
        .unwrap();
    account.content().v1().content(SID).fetch().unwrap();
    account
        .content()
        .v1()
        .content(SID)
        .delete(DeleteContentRequest::new())
        .unwrap();
    let requests = server.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[1].path, format!("/v1/Content/{SID}"));
    assert_eq!(requests[2].method, "DELETE");
}
