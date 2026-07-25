#![allow(clippy::missing_panics_doc, clippy::unwrap_used)]

mod support;

use serde_json::json;
use support::{HttpsMockServer, MockResponse, test_creds};
use twilio2::{ContentSearchRequest, ListContentRequest, TwilioError};

const SID_A: &str = "HXaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SID_B: &str = "HXbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn content(sid: &str) -> serde_json::Value {
    json!({
        "sid": sid,
        "account_sid": "ACaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "friendly_name": "order update",
        "language": "en",
        "variables": {},
        "types": {"twilio/text": {"body": "Hello"}},
        "date_created": "2026-07-11T12:00:00Z",
        "date_updated": "Fri, 11 Jul 2026 12:00:00 +0000"
    })
}

fn page(item: &serde_json::Value, next: Option<&str>) -> String {
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

fn approval_content(sid: &str) -> serde_json::Value {
    let mut value = content(sid);
    value["approval_requests"] = json!({
        "whatsapp": {
            "type": "whatsapp",
            "name": "order_update",
            "category": "UTILITY",
            "status": "approved"
        }
    });
    value["future_approval_field"] = json!({"opaque": true});
    value
}

fn legacy_content() -> serde_json::Value {
    json!({
        "sid": SID_A,
        "account_sid": "ACaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "friendly_name": "legacy order",
        "language": "en",
        "legacy_template_name": "old_order_update",
        "legacy_body": "Hello {{1}}",
        "date_created": "2026-07-11T12:00:00Z"
    })
}

fn query_pairs(path: &str) -> Vec<(String, String)> {
    url::Url::parse(&format!("https://example.test{path}"))
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_v1_lists_content_approvals_and_legacy_mappings() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(page(&approval_content(SID_A), None)),
        MockResponse::json(page(&legacy_content(), None)),
    ])
    .await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());

    let approvals = account
        .content()
        .v1()
        .content_and_approvals()
        .list(ListContentRequest::new().page_size(7))
        .await
        .unwrap();
    assert_eq!(approvals.contents[0].content.sid.as_deref(), Some(SID_A));
    assert_eq!(
        approvals.contents[0].approval_requests["whatsapp"]
            .status
            .as_deref(),
        Some("approved")
    );
    assert_eq!(
        approvals.contents[0].extra["future_approval_field"]["opaque"],
        true
    );

    let legacy = account
        .content()
        .v1()
        .legacy_contents()
        .list(ListContentRequest::new().page_size(9))
        .await
        .unwrap();
    assert_eq!(
        legacy.contents[0].legacy_template_name.as_deref(),
        Some("old_order_update")
    );

    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/ContentAndApprovals?PageSize=7");
    assert_eq!(requests[1].path, "/v1/LegacyContent?PageSize=9");
    assert_eq!(
        requests[0].header("authorization"),
        Some("Basic QUMxMjM6dG9rZW4=")
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_v2_search_encodes_repeated_filters_for_both_collections() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(page(&content(SID_A), None)),
        MockResponse::json(page(&approval_content(SID_A), None)),
    ])
    .await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());
    let request = ContentSearchRequest::new()
        .language("en")
        .language("fr")
        .content_type("twilio/text")
        .content_type("twilio/card")
        .channel_eligibility("whatsapp:approved")
        .channel_eligibility("rcs:eligible")
        .content("hello world")
        .content_name("order update")
        .sort_by_date("desc")
        .sort_by_content_name("asc")
        .date_created_before("2026-07-12T00:00:00Z")
        .date_created_after("2026-07-01T00:00:00Z")
        .page_size(25);

    account
        .content()
        .v2()
        .contents()
        .list(request.clone())
        .await
        .unwrap();
    account
        .content()
        .v2()
        .content_and_approvals()
        .list(request)
        .await
        .unwrap();

    let requests = server.requests();
    assert!(requests[0].path.starts_with("/v2/Content?"));
    assert!(requests[1].path.starts_with("/v2/ContentAndApprovals?"));
    for request in &requests {
        let pairs = query_pairs(&request.path);
        let values = |key: &str| {
            pairs
                .iter()
                .filter(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(values("Language"), ["en", "fr"]);
        assert_eq!(values("ContentType"), ["twilio/text", "twilio/card"]);
        assert_eq!(
            values("ChannelEligibility"),
            ["whatsapp:approved", "rcs:eligible"]
        );
        assert_eq!(values("Content"), ["hello world"]);
        assert_eq!(values("ContentName"), ["order update"]);
        assert_eq!(values("SortByDate"), ["desc"]);
        assert_eq!(values("SortByContentName"), ["asc"]);
        assert_eq!(values("PageSize"), ["25"]);
    }
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_v2_list_all_with_preserves_filters_on_continuation() {
    let next = concat!(
        "__BASE_URL__/v2/Content?Language=en&Language=fr&ContentType=twilio%2Ftext&",
        "ChannelEligibility=whatsapp%3Aapproved&Content=hello&SortByDate=desc&",
        "SortByContentName=asc&PageSize=2&Page=1&PageToken=next"
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::json(page(&content(SID_A), Some(next))),
        MockResponse::json(page(&content(SID_B), None)),
    ])
    .await;
    let client = support::client_for(&server);
    let items = client
        .account(test_creds())
        .content()
        .v2()
        .contents()
        .list_all_with(
            ContentSearchRequest::new()
                .language("en")
                .language("fr")
                .content_type("twilio/text")
                .channel_eligibility("whatsapp:approved")
                .content("hello")
                .sort_by_date("desc")
                .sort_by_content_name("asc")
                .page_size(2),
        )
        .collect_all()
        .await
        .unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].sid.as_deref(), Some(SID_A));
    assert_eq!(items[1].sid.as_deref(), Some(SID_B));
    let requests = server.requests();
    assert_eq!(
        query_pairs(&requests[0].path),
        query_pairs(&requests[1].path)[..8]
    );
    assert!(requests[1].path.contains("PageToken=next"));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_pagination_rejects_resource_version_and_filter_changes() {
    for next in [
        "https://evil.example/v2/Content?Language=en&PageSize=1&PageToken=next",
        "__BASE_URL__/v2/ContentAndApprovals?Language=en&PageSize=1&PageToken=next",
        "__BASE_URL__/v1/Content?Language=en&PageSize=1&PageToken=next",
        "__BASE_URL__/v2/Content?Language=fr&PageSize=1&PageToken=next",
        "__BASE_URL__/v2/Content?Language=en&SortByDate=asc&PageSize=1&PageToken=next",
    ] {
        let server =
            HttpsMockServer::start(vec![MockResponse::json(page(&content(SID_A), Some(next)))])
                .await;
        let client = support::client_for(&server);
        let error = client
            .account(test_creds())
            .content()
            .v2()
            .contents()
            .list(
                ContentSearchRequest::new()
                    .language("en")
                    .sort_by_date("desc")
                    .page_size(1),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, TwilioError::InvalidResponseMetadata(_)));
        assert_eq!(server.requests().len(), 1);
    }
}

#[cfg(feature = "async")]
#[tokio::test]
async fn async_v2_search_validation_fails_before_transport() {
    let server = HttpsMockServer::start(Vec::new()).await;
    let client = support::client_for(&server);
    let account = client.account(test_creds());
    for request in [
        ContentSearchRequest::new().page_size(1001),
        ContentSearchRequest::new().channel_eligibility("whatsapp"),
        ContentSearchRequest::new().date_created_before("not-a-date"),
        ContentSearchRequest::new().content("x".repeat(1025)),
        ContentSearchRequest::new().sort_by_date(" "),
        ContentSearchRequest::new().sort_by_content_name(""),
    ] {
        let error = account
            .content()
            .v2()
            .contents()
            .list(request)
            .await
            .unwrap_err();
        assert!(matches!(error, twilio2::TwilioError::InvalidRequest(_)));
    }
    assert!(server.requests().is_empty());
}

#[cfg(feature = "sync")]
#[test]
fn blocking_v1_and_v2_list_endpoints_work() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(HttpsMockServer::start(vec![
        MockResponse::json(page(&approval_content(SID_A), None)),
        MockResponse::json(page(&legacy_content(), None)),
        MockResponse::json(page(&content(SID_A), None)),
        MockResponse::json(page(&approval_content(SID_A), None)),
    ]));
    let client = support::blocking_client_for(&server);
    let account = client.account(test_creds());

    account
        .content()
        .v1()
        .content_and_approvals()
        .list(ListContentRequest::new().page_size(1))
        .unwrap();
    account
        .content()
        .v1()
        .legacy_contents()
        .list(ListContentRequest::new().page_size(1))
        .unwrap();
    let search = ContentSearchRequest::new()
        .language("en")
        .content_type("twilio/text")
        .page_size(1);
    account
        .content()
        .v2()
        .contents()
        .list(search.clone())
        .unwrap();
    account
        .content()
        .v2()
        .content_and_approvals()
        .list(search)
        .unwrap();

    let paths: Vec<_> = server.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths[0], "/v1/ContentAndApprovals?PageSize=1");
    assert_eq!(paths[1], "/v1/LegacyContent?PageSize=1");
    assert!(paths[2].starts_with("/v2/Content?Language=en"));
    assert!(paths[3].starts_with("/v2/ContentAndApprovals?Language=en"));
}
