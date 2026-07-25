#![cfg(feature = "sync")]
#![allow(clippy::missing_panics_doc, clippy::unwrap_used)]

mod support;

use std::time::Duration;

use support::{HttpsMockServer, MockResponse, blocking_client_for, test_creds};
use twilio2::{
    BulkMessageContent, BulkMessageRecipient, BulkMessageRecipientChannel, ListBulkMessagesRequest,
    SendBulkMessagesRequest,
};

const OPERATION_ID: &str = "comms_operation_01h9krwprkeee8fzqspvwy6nq8";

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
            r#"{{"id":"{OPERATION_ID}","status":"CANCELED","stats":{{"total":0}}}}"#
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
    assert_eq!(operation.status.as_deref(), Some("CANCELED"));
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/Messages");
    assert_eq!(requests[1].path, "/v1/Messages?pageSize=50");
    assert_eq!(
        requests[2].path,
        format!("/v1/Messages/Operations/{OPERATION_ID}")
    );
}
