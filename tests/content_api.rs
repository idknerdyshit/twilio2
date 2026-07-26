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
    ContentCard, ContentCardAction, ContentMedia, ContentQuickReply, ContentQuickReplyAction,
    ContentSearchRequest, ContentText, ContentTypes, CreateContentRequest, DeleteContentRequest,
    ListContentRequest, SubmitWhatsAppApprovalRequest, TwilioError, UpdateContentRequest,
    WhatsAppTemplateCategory,
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
            ContentQuickReply::new("Choose")
                .action(ContentQuickReplyAction::new("Track").id("track")),
        )
        .card(
            ContentCard::new()
                .title("Order")
                .action(ContentCardAction::url("Open", "https://example.test")),
        )
        .custom("vendor/future", custom.clone())
        .unwrap();

    let created = account
        .content()
        .v1()
        .contents()
        .create(
            CreateContentRequest::new("en", types)
                .friendly_name("order_update")
                .variable("1", "Customer"),
        )
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
        .update(
            UpdateContentRequest::new(ContentTypes::new().text(ContentText::new("Hello {{1}}")))
                .friendly_name("renamed"),
        )
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
    let update: serde_json::Value = serde_json::from_str(&requests[4].body).unwrap();
    assert!(update.get("language").is_none());
}

#[cfg(feature = "async")]
#[tokio::test]
async fn content_routes_report_structured_redacted_api_errors() {
    let sensitive_message = "failed for HXbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb +15551234567";
    let server = HttpsMockServer::start(vec![
        MockResponse::status_json(
            400,
            json!({
                "code": 21606,
                "message": sensitive_message,
                "more_info": "https://www.twilio.com/docs/errors/21606",
                "status": 400
            })
            .to_string(),
        ),
        MockResponse::status_json(
            500,
            json!({
                "code": 20500,
                "message": "internal error for secret content",
                "more_info": "https://www.twilio.com/docs/errors/20500",
                "status": 500
            })
            .to_string(),
        ),
    ])
    .await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());

    let v1_error = account
        .content()
        .v1()
        .content(SID)
        .fetch()
        .await
        .unwrap_err();
    let TwilioError::Api {
        status, code, body, ..
    } = v1_error
    else {
        panic!("expected Content v1 API error");
    };
    assert_eq!(status, 400);
    assert_eq!(code, Some(21606));
    assert!(body.starts_with("<redacted response body; "));
    assert!(!body.contains(sensitive_message));
    assert!(!body.contains("twilio.com"));

    let v2_error = account
        .content()
        .v2()
        .contents()
        .list(ContentSearchRequest::new())
        .await
        .unwrap_err();
    let TwilioError::Api {
        status, code, body, ..
    } = v2_error
    else {
        panic!("expected Content v2 API error");
    };
    assert_eq!(status, 500);
    assert_eq!(code, Some(20500));
    assert!(body.starts_with("<redacted response body; "));
    assert!(!body.contains("secret content"));
    assert!(!body.contains("twilio.com"));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn content_routes_redact_malformed_success_responses() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(r#"{"sid":"HXsecret","types":"secret-content""#),
        MockResponse::json(r#"{"contents":[{"sid":"HXsecret"}],"meta":"secret-content""#),
        MockResponse::json(r#"{"contents":[{"sid":"HXsecret"}],"meta":"secret-content""#),
    ])
    .await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());

    let errors = [
        account
            .content()
            .v1()
            .content(SID)
            .fetch()
            .await
            .unwrap_err(),
        account
            .content()
            .v2()
            .contents()
            .list(ContentSearchRequest::new())
            .await
            .unwrap_err(),
        account
            .content()
            .v2()
            .content_and_approvals()
            .list(ContentSearchRequest::new())
            .await
            .unwrap_err(),
    ];

    for error in errors {
        let TwilioError::Decode(message) = error else {
            panic!("expected malformed Content response");
        };
        assert!(!message.contains("HXsecret"));
        assert!(!message.contains("secret-content"));
    }
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
        .create(CreateContentRequest::new(
            "en",
            ContentTypes::new().quick_reply(ContentQuickReply::new("Choose")),
        ))
        .await
        .unwrap_err();
    assert!(matches!(error, TwilioError::InvalidRequest(_)));
    let error = account
        .content()
        .v1()
        .contents()
        .create(CreateContentRequest::new("en", ContentTypes::new()).friendly_name("name"))
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
        .list(ListContentRequest::new().page_size(1001))
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
        .custom("vendor/x", custom)
        .unwrap();
    let request = CreateContentRequest::new("en", types)
        .friendly_name("friendly-secret")
        .variable("1", "variable-secret");
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

    let search = ContentSearchRequest::new()
        .content("search-body-secret")
        .content_name("search-name-secret")
        .page_token("page-token-secret");
    let rendered = format!("{search:?}");
    for secret in [
        "search-body-secret",
        "search-name-secret",
        "page-token-secret",
    ] {
        assert!(!rendered.contains(secret));
    }
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
        .create(CreateContentRequest::new("en", types).friendly_name("name"))
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
