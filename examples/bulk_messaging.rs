mod support;

use support::{ExampleResult, HttpsMockServer, MockResponse, client_for, creds};
use twilio2::{
    BulkMessageContent, BulkMessageRecipient, BulkMessageRecipientChannel, SendBulkMessagesRequest,
};

const OPERATION_ID: &str = "comms_operation_01h9krwprkeee8fzqspvwy6nq8";

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let server = HttpsMockServer::start(vec![MockResponse::accepted_json(format!(
        r#"{{"operationId":"{OPERATION_ID}","operationLocation":"__BASE_URL__/v1/Messages/Operations/{OPERATION_ID}"}}"#
    ))])
    .await?;
    let client = client_for(&server)?;
    let recipients = [BulkMessageRecipient::address(
        "+15551234567",
        BulkMessageRecipientChannel::Phone,
    )];
    let content = BulkMessageContent::text("hello from twilio2");

    let submission = client
        .account(creds())
        .bulk_messaging()
        .v1()
        .messages()
        .send(SendBulkMessagesRequest::new(&recipients, &content))
        .await?;

    assert_eq!(submission.operation_id, OPERATION_ID);
    let requests = server.requests()?;
    assert_eq!(requests[0].path, "/v1/Messages");
    Ok(())
}
