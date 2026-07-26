#![cfg(feature = "sync")]
#![allow(clippy::missing_panics_doc, clippy::unwrap_used)]

mod support;

use std::time::Duration;

use support::{HttpsMockServer, MockResponse, blocking_client_for, test_creds};
use twilio2::{
    BulkMessageContent, BulkMessageRecipient, BulkMessageRecipientChannel,
    BulkSenderPoolSenderRequest, ListBulkMessagesRequest, SendBulkMessagesRequest,
};

const OPERATION_ID: &str = "comms_operation_01h9krwprkeee8fzqspvwy6nq8";
const POOL_ID: &str = "comms_senderpool_01h9krwprkeee8fzqspvwy6nq8";
const SENDER_ID: &str = "comms_sender_01h9krwprkeee8fzqspvwy6nq8";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn blocking_send_list_and_wait_mirror_async_wire_contract() {
    let runtime = runtime();
    let server = runtime.block_on(HttpsMockServer::start(vec![
        MockResponse::status_json(
            202,
            format!(
                r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/Messages/Operations/{OPERATION_ID}"}}"#
            ),
        ),
        MockResponse::json(r#"{"messages":[]}"#),
        MockResponse::json(format!(
            r#"{{"id":"{OPERATION_ID}","status":"CANCELED","stats":{{"total":0,"recipients":0,"attempts":0,"scheduled":0,"queued":0,"sent":0,"delivered":0,"read":0,"undelivered":0,"unaddressable":0,"failed":0,"canceled":0}},"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z"}}"#
        )),
    ]));
    let client = blocking_client_for(&server);
    let messages = client
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .messages();
    let recipients = [BulkMessageRecipient::address(
        "+15551234567",
        BulkMessageRecipientChannel::Phone,
    )];
    let content = BulkMessageContent::text("hello");

    let submission = messages
        .send(SendBulkMessagesRequest::new(&recipients, &content))
        .unwrap();
    let page = messages
        .list(ListBulkMessagesRequest::new().page_size(50))
        .unwrap();
    let operation = messages
        .operation(OPERATION_ID)
        .wait(Duration::from_millis(1), Duration::from_secs(1))
        .unwrap();

    assert_eq!(submission.operation_id, OPERATION_ID);
    assert!(page.messages.is_empty());
    assert_eq!(operation.status.as_str(), "CANCELED");
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Messages");
    assert_eq!(requests[1].path, "/v1/Messages?pageSize=50");
    assert_eq!(
        requests[2].path,
        format!("/v1/Messages/Operations/{OPERATION_ID}")
    );
}

#[test]
fn blocking_sender_pool_add_serializes_a_top_level_array() {
    let runtime = runtime();
    let server = runtime.block_on(HttpsMockServer::start(vec![MockResponse::status_json(
        202,
        format!(
            r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/SenderPools/{POOL_ID}/Senders/Operations/{OPERATION_ID}"}}"#
        ),
    )]));
    let members = [BulkSenderPoolSenderRequest::new(SENDER_ID)];

    blocking_client_for(&server)
        .account(test_creds())
        .bulk_messaging()
        .v1()
        .sender_pools()
        .sender_pool(POOL_ID)
        .add_senders(&members)
        .unwrap();

    let requests = server.requests();
    assert_eq!(
        requests[0].path,
        format!("/v1/SenderPools/{POOL_ID}/Senders")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).unwrap(),
        serde_json::json!([{ "senderId": SENDER_ID }])
    );
}
