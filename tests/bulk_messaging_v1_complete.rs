#![cfg(feature = "async")]
#![allow(clippy::missing_panics_doc, clippy::unwrap_used)]

mod support;

use support::{HttpsMockServer, MockResponse, client_for, test_creds};
use twilio2::{
    BULK_EVENT_MESSAGE_SENT, BulkMessagingEventData, BulkSenderChannel,
    BulkSenderPoolSenderRequest, BulkSenderResolveRecipient, CreateBulkSenderPoolRequest,
    ListBulkSenderPoolOperationsRequest, ListBulkSenderPoolsRequest, ListBulkSendersRequest,
    ResolveBulkSendersRequest, SearchBulkSendersRequest, TwilioError, UpdateBulkSenderPoolRequest,
    parse_bulk_messaging_events,
};

const SENDER_ID: &str = "comms_sender_01h9krwprkeee8fzqspvwy6nq8";
const POOL_ID: &str = "comms_senderpool_01h9krwprkeee8fzqspvwy6nq8";
const OPERATION_ID: &str = "comms_operation_01h9krwprkeee8fzqspvwy6nq8";

#[tokio::test]
async fn senders_and_sender_pools_cover_the_documented_paths() {
    let server = HttpsMockServer::start(vec![
        MockResponse::json(r#"{"senders":[],"pagination":{"next":null,"self":"token"}}"#),
        MockResponse::json(format!(
            r#"{{"id":"{SENDER_ID}","displayName":null,"address":"+15551234567","channel":"FUTURE","status":"ACTIVATED","tags":{{}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
        )),
        MockResponse::json(r#"{"senders":[],"pagination":{"next":null}}"#),
        MockResponse::json(r#"{"results":[]}"#),
        MockResponse::status_json(202, format!(r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/SenderPools/Operations/{OPERATION_ID}"}}"#)),
        MockResponse::json(r#"{"senderPools":[],"pagination":{"next":null}}"#),
        MockResponse::json(format!(r#"{{"id":"{POOL_ID}","name":"campaign","tags":{{}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#)),
        MockResponse::status_json(202, format!(r#"{{"resourceId":"{POOL_ID}","resourceLocation":"__BASE_URL__/v1/SenderPools/{POOL_ID}"}}"#)),
        MockResponse::status_json(202, format!(r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/SenderPools/{POOL_ID}/Senders/Operations/{OPERATION_ID}"}}"#)),
        MockResponse::json(r#"{"senders":[],"pagination":{"next":null}}"#),
        MockResponse::status_json(202, format!(r#"{{"resourceId":"{SENDER_ID}","resourceLocation":"__BASE_URL__/v1/Senders/{SENDER_ID}"}}"#)),
        MockResponse::json(r#"{"operations":[],"pagination":{"next":null}}"#),
        MockResponse::status_json(202, format!(r#"{{"resourceId":"{POOL_ID}","resourceLocation":"__BASE_URL__/v1/SenderPools/{POOL_ID}"}}"#)),
    ])
    .await;
    let client = client_for(&server);
    let account = client.account(test_creds());
    let v1 = account.bulk_messaging().v1();
    let senders = v1.senders();
    senders.list(ListBulkSendersRequest::new()).await.unwrap();
    let sender = senders.sender(SENDER_ID).fetch().await.unwrap();
    assert_eq!(sender.channel.as_str(), "FUTURE");
    senders
        .search(SearchBulkSendersRequest::new(
            "+15551234567",
            BulkSenderChannel::Sms,
        ))
        .await
        .unwrap();
    let recipients = [BulkSenderResolveRecipient::new("+15551234567", "PHONE")];
    senders
        .resolve(ResolveBulkSendersRequest::new(&recipients))
        .await
        .unwrap();

    let pools = v1.sender_pools();
    pools
        .create(CreateBulkSenderPoolRequest::new("campaign"))
        .await
        .unwrap();
    pools.list(ListBulkSenderPoolsRequest::new()).await.unwrap();
    let pool = pools.sender_pool(POOL_ID);
    pool.fetch().await.unwrap();
    pool.update(UpdateBulkSenderPoolRequest::new().name("updated"))
        .await
        .unwrap();
    let members = [BulkSenderPoolSenderRequest::new(SENDER_ID)];
    pool.add_senders(&members).await.unwrap();
    pool.list_senders(ListBulkSendersRequest::new())
        .await
        .unwrap();
    pool.remove_sender(SENDER_ID).await.unwrap();
    pools
        .operations()
        .list(ListBulkSenderPoolOperationsRequest::new())
        .await
        .unwrap();
    pool.delete().await.unwrap();

    let requests = server.requests();
    let paths: Vec<_> = requests
        .iter()
        .map(|request| request.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            "/v1/Senders".to_owned(),
            format!("/v1/Senders/{SENDER_ID}"),
            "/v1/Senders/Search".to_owned(),
            "/v1/Senders/Resolve".to_owned(),
            "/v1/SenderPools".to_owned(),
            "/v1/SenderPools".to_owned(),
            format!("/v1/SenderPools/{POOL_ID}"),
            format!("/v1/SenderPools/{POOL_ID}"),
            format!("/v1/SenderPools/{POOL_ID}/Senders"),
            format!("/v1/SenderPools/{POOL_ID}/Senders"),
            format!("/v1/SenderPools/{POOL_ID}/Senders/{SENDER_ID}"),
            "/v1/SenderPools/Operations".to_owned(),
            format!("/v1/SenderPools/{POOL_ID}"),
        ]
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[8].body).unwrap(),
        serde_json::json!([{ "senderId": SENDER_ID }])
    );
}

#[tokio::test]
async fn sender_pool_paginators_preserve_filters_and_follow_tokens() {
    let sender = format!(
        r#"{{"id":"{SENDER_ID}","displayName":null,"address":"+15551234567","channel":"SMS","status":"ACTIVATED","tags":{{}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
    );
    let operation = format!(
        r#"{{"id":"{OPERATION_ID}","status":"COMPLETED","stats":{{"total":1,"queued":0,"created":1,"failed":0}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
    );
    let server = HttpsMockServer::start(vec![
        MockResponse::json(format!(
            r#"{{"senders":[{sender}],"pagination":{{"next":"member-token"}}}}"#
        )),
        MockResponse::json(r#"{"senders":[],"pagination":{"next":null}}"#),
        MockResponse::json(format!(
            r#"{{"operations":[{operation}],"pagination":{{"next":"operation-token"}}}}"#
        )),
        MockResponse::json(r#"{"operations":[],"pagination":{"next":null}}"#),
    ])
    .await;
    let client = client_for(&server);
    let pools = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .sender_pools();

    let members = pools
        .sender_pool(POOL_ID)
        .list_all_senders_with(
            ListBulkSendersRequest::new().status(twilio2::BulkSenderStatus::Activated),
        )
        .collect_all()
        .await
        .unwrap();
    let operations = pools
        .operations()
        .list_all_with(ListBulkSenderPoolOperationsRequest::new().status("COMPLETED"))
        .collect_all()
        .await
        .unwrap();

    assert_eq!(members.len(), 1);
    assert_eq!(operations.len(), 1);
    let paths: Vec<_> = server
        .requests()
        .into_iter()
        .map(|request| request.path)
        .collect();
    assert_eq!(
        paths,
        [
            format!("/v1/SenderPools/{POOL_ID}/Senders?status=ACTIVATED"),
            format!("/v1/SenderPools/{POOL_ID}/Senders?pageToken=member-token&status=ACTIVATED"),
            "/v1/SenderPools/Operations?status=COMPLETED".to_owned(),
            "/v1/SenderPools/Operations?status=COMPLETED&pageToken=operation-token".to_owned(),
        ]
    );
}

#[test]
fn event_batches_decode_string_object_and_unknown_payloads_redacted() {
    let body = format!(
        r#"[
          {{"specversion":"1.0","id":"one","source":"/comms","type":"{BULK_EVENT_MESSAGE_SENT}","dataschema":"schema","time":"2026-01-02T03:04:05Z","data":"{{\"operation_id\":\"operation\",\"message_id\":\"message\",\"downstream_id\":\"downstream\",\"attempt\":\"1\",\"from\":{{\"id\":\"sender\",\"address\":\"+15551234567\",\"channel\":\"SMS\"}},\"to\":{{\"id\":\"recipient\",\"address\":\"+15557654321\",\"channel\":\"PHONE\"}},\"tags\":{{\"campaign\":\"launch\"}}}}"}},
          {{"specversion":"1.0","id":"two","source":"/comms","type":"future.event","data":{{"secret":"value"}}}}
        ]"#
    );
    let events = parse_bulk_messaging_events(body.as_bytes()).unwrap();
    let BulkMessagingEventData::MessageSent(event) = &events[0].data else {
        unreachable!("expected a sent-message event");
    };
    assert_eq!(event.operation_id.as_deref(), Some("<redacted>"));
    assert_eq!(event.message_id.as_deref(), Some("<redacted>"));
    assert_eq!(event.downstream_id.as_deref(), Some("<redacted>"));
    assert_eq!(event.attempt.as_deref(), Some("<redacted>"));
    assert_eq!(
        event.from.as_ref().and_then(|value| value.id.as_deref()),
        Some("<redacted>")
    );
    assert_eq!(
        event.to.as_ref().and_then(|value| value.id.as_deref()),
        Some("<redacted>")
    );
    assert_eq!(
        event
            .tags
            .as_ref()
            .and_then(|tags| tags.get("campaign"))
            .map(String::as_str),
        None
    );
    assert!(matches!(events[1].data, BulkMessagingEventData::Unknown));
    let rendered = format!("{events:?}");
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains("value"));
}

#[test]
fn malformed_event_batches_are_decode_errors() {
    assert!(matches!(
        parse_bulk_messaging_events(b"not JSON"),
        Err(TwilioError::Decode(_))
    ));
}
