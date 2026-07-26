#![cfg(feature = "async")]
#![allow(clippy::missing_panics_doc, clippy::unwrap_used)]

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use support::{
    HttpsMockServer, MockResponse, client_for, test_creds, test_http_client, twilio_config,
};
use twilio2::{
    BulkMessageChannel, BulkMessageContent, BulkMessageInlineModule, BulkMessageMedia,
    BulkMessageRcsModule, BulkMessageRecipient, BulkMessageRecipientChannel,
    BulkMessageResponseSender, BulkMessageRichCard, BulkMessageSender, BulkMessageSuggestion,
    BulkMessagingValue, ListBulkMessageOperationsRequest, ListBulkMessagesRequest,
    SendBulkMessagesRequest, TwilioClient, TwilioClientConfig, TwilioError,
};

const OPERATION_ID: &str = "comms_operation_01h9krwprkeee8fzqspvwy6nq8";
const MESSAGE_ID: &str = "comms_message_01h9krwprkeee8fzqspvwy6nq8";

#[tokio::test]
async fn send_accepts_body_metadata_and_uses_json_basic_auth() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        202,
        format!(
            r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/Messages/Operations/{OPERATION_ID}"}}"#
        ),
    )])
    .await;
    let client = client_for(&server);
    let recipients = [BulkMessageRecipient::address(
        "+15551234567",
        BulkMessageRecipientChannel::Phone,
    )];
    let content = BulkMessageContent::text_and_media(
        "hello",
        [BulkMessageMedia::new("https://media.example.test/a.png")],
    );
    let sender = BulkMessageSender::address("+15557654321", BulkMessageChannel::Sms);
    let mut tags = BTreeMap::new();
    tags.insert("campaign".to_owned(), "launch".to_owned());

    let submission = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages()
        .send(
            SendBulkMessagesRequest::new(&recipients, &content)
                .from(&sender)
                .tags(&tags),
        )
        .await
        .unwrap();

    assert_eq!(submission.operation_id, OPERATION_ID);
    assert!(submission.operation_location.is_some());
    let requests = server.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/Messages");
    assert_eq!(
        requests[0].header("authorization"),
        Some("Basic QUMxMjM6dG9rZW4=")
    );
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(body["to"][0]["addresses"][0]["channel"], "PHONE");
    assert_eq!(body["from"]["channel"], "SMS");
    assert_eq!(body["content"]["text"], "hello");
}

#[tokio::test]
async fn send_accepts_operation_location_under_configured_base_path() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        202,
        format!(
            r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/twilio-comms/v1/Messages/Operations/{OPERATION_ID}"}}"#
        ),
    )])
    .await;
    let bulk_base_url = format!("{}/twilio-comms", server.base_url);
    let config = TwilioClientConfig::new()
        .base_urls(twilio_config(&server.base_url).bulk_messaging_base_url(&bulk_base_url));
    let client = TwilioClient::from_config_with_http_builder(config, test_http_client).unwrap();
    let recipients = [BulkMessageRecipient::address(
        "+15551234567",
        BulkMessageRecipientChannel::Phone,
    )];
    let content = BulkMessageContent::text("hello");

    let submission = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages()
        .send(SendBulkMessagesRequest::new(&recipients, &content))
        .await
        .unwrap();

    assert_eq!(
        submission.operation_location.as_ref().map(url::Url::as_str),
        Some(format!("{bulk_base_url}/v1/Messages/Operations/{OPERATION_ID}").as_str())
    );
    assert_eq!(server.requests()[0].path, "/twilio-comms/v1/Messages");
}

#[tokio::test]
async fn send_serializes_rcs_rich_card_and_open_url_keys_as_camel_case() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        202,
        format!(r#"{{"operationId":"{OPERATION_ID}"}}"#),
    )])
    .await;
    let client = client_for(&server);
    let recipients = [BulkMessageRecipient::address(
        "+15551234567",
        BulkMessageRecipientChannel::Phone,
    )];
    let content = BulkMessageContent::inline([BulkMessageInlineModule::rcs(
        BulkMessageRcsModule::rich_card(BulkMessageRichCard::new("Account update").suggestion(
            BulkMessageSuggestion::open_url("View account", "https://example.test/account"),
        )),
    )]);

    client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages()
        .send(SendBulkMessagesRequest::new(&recipients, &content))
        .await
        .unwrap();

    let body: serde_json::Value = serde_json::from_str(&server.requests()[0].body).unwrap();
    let rcs = &body["content"]["modules"][0]["rcs"];
    assert!(rcs.get("richCard").is_some());
    assert!(rcs.get("rich_card").is_none());
    let suggestion = &rcs["richCard"]["standaloneCard"]["cardContent"]["suggestions"][0]["action"];
    assert!(suggestion.get("openUrlAction").is_some());
    assert!(suggestion.get("open_url_action").is_none());
}

#[tokio::test]
async fn list_pagination_preserves_filters_and_fetch_decodes_timestamps() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(format!(
            r#"{{"messages":[{{"id":"{MESSAGE_ID}","from":{{"address":"+15550000000","channel":"SMS","senderId":"comms_sender_01h9krwprkeee8fzqspvwy6nq8","senderPoolId":"comms_senderpool_01h9krwprkeee8fzqspvwy6nq8"}},"to":[{{"address":"+15551111111","channel":"PHONE"}}],"status":"NEW_STATUS","related":[],"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z","scheduledFor":null,"tags":{{}}}}],"nextPageToken":"secret-token"}}"#
        )),
        MockResponse::json(r#"{"messages":[]}"#),
        MockResponse::json(format!(
            r#"{{"id":"{MESSAGE_ID}","from":{{"address":"+15550000000","channel":"SMS","senderId":"comms_sender_01h9krwprkeee8fzqspvwy6nq8"}},"to":[{{"address":"+15551111111","channel":"PHONE"}}],"status":"DELIVERED","attempts":[],"related":[],"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z","scheduledFor":null,"tags":{{}}}}"#
        )),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let messages = account.bulk_messaging().v1().messages();
    let all = messages
        .list_all_with(ListBulkMessagesRequest::new().status("QUEUED").page_size(1))
        .collect_all()
        .await
        .unwrap();
    let fetched = messages.message(MESSAGE_ID).fetch().await.unwrap();

    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].status.as_ref().map(BulkMessagingValue::as_str),
        Some("NEW_STATUS")
    );
    assert!(matches!(
        &all[0].from,
        Some(BulkMessageResponseSender::Address {
            sender_pool_id: Some(sender_pool_id),
            ..
        }) if sender_pool_id == "comms_senderpool_01h9krwprkeee8fzqspvwy6nq8"
    ));
    assert_eq!(
        fetched.status.as_ref().map(BulkMessagingValue::as_str),
        Some("DELIVERED")
    );
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Messages?status=QUEUED&pageSize=1");
    assert_eq!(
        requests[1].path,
        "/v1/Messages?status=QUEUED&pageToken=secret-token&pageSize=1"
    );
    assert_eq!(requests[2].path, format!("/v1/Messages/{MESSAGE_ID}"));
}

#[tokio::test]
async fn message_list_decodes_documented_sparse_metadata() {
    let server = HttpsMockServer::start(vec![MockResponse::json(
        r#"{"messages":[{"id":"comms_message_01h9krwprkeee8fzqspvwy6nq8"}]}"#,
    )])
    .await;
    let page = client_for(&server)
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages()
        .list(ListBulkMessagesRequest::new())
        .await
        .unwrap();

    assert_eq!(page.messages.len(), 1);
    assert!(page.messages[0].from.is_none());
    assert!(page.messages[0].status.is_none());
    assert!(page.messages[0].to.is_empty());
}

#[tokio::test]
async fn operation_wait_and_list_work() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(format!(
            r#"{{"id":"{OPERATION_ID}","status":"PROCESSING","stats":{{"total":1,"recipients":1,"attempts":0,"scheduled":0,"queued":1,"sent":0,"delivered":0,"read":0,"undelivered":0,"unaddressable":0,"failed":0,"canceled":0}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
        )),
        MockResponse::json(format!(
            r#"{{"id":"{OPERATION_ID}","status":"COMPLETED","stats":{{"total":1,"recipients":1,"attempts":1,"scheduled":0,"queued":0,"sent":0,"delivered":1,"read":0,"undelivered":0,"unaddressable":0,"failed":0,"canceled":0}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
        )),
        MockResponse::json(format!(
            r#"{{"operations":[{{"id":"{OPERATION_ID}","status":"COMPLETED","stats":{{"total":1,"recipients":1,"attempts":1,"scheduled":0,"queued":0,"sent":0,"delivered":1,"read":0,"undelivered":0,"unaddressable":0,"failed":0,"canceled":0}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}]}}"#
        )),
    ])
    .await;
    let client = client_for(&server);
    let messages = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages();
    let operation = messages
        .operation(OPERATION_ID)
        .wait(Duration::from_millis(1), Duration::from_secs(1))
        .await
        .unwrap();
    let page = messages
        .operations()
        .list(ListBulkMessageOperationsRequest::new().status("COMPLETED"))
        .await
        .unwrap();

    assert_eq!(operation.status.as_str(), "COMPLETED");
    assert_eq!(page.operations.len(), 1);
}

#[tokio::test]
async fn validation_and_bulk_error_envelope_are_redacted() {
    let server = HttpsMockServer::start(vec![MockResponse::status_json(
        429,
        r#"{"errors":[{"code":21614,"message":"secret message","context":"$.to[0].address"},{"code":21617,"message":"secret message","context":"$.variables.customer_name"}]}"#,
    )
    .header("Retry-After", "17")])
    .await;
    let client = client_for(&server);
    let messages = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages();
    let invalid = messages
        .list(ListBulkMessagesRequest::new().page_size(0))
        .await
        .unwrap_err();
    assert!(matches!(invalid, TwilioError::InvalidRequest(_)));
    assert!(server.requests().is_empty());

    let error = messages
        .operations()
        .list(ListBulkMessageOperationsRequest::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        TwilioError::Api {
            code: Some(21614),
            ..
        }
    ));
    let TwilioError::Api {
        details,
        retry_after,
        ..
    } = &error
    else {
        unreachable!();
    };
    assert_eq!(details.len(), 2);
    assert_eq!(details[0].context.as_deref(), Some("$.to[0].address"));
    assert_eq!(details[1].context, None);
    assert_eq!(*retry_after, Some(Duration::from_secs(17)));
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("secret message"));
    assert!(!rendered.contains("secret.path"));
}
