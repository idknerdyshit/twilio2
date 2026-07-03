mod support;

use support::{ExampleResult, HttpsMockServer, MockResponse, client_for, creds, missing};
use twilio2::{CreateMessageRequest, ListMessagesRequest, UpdateMessageRequest};

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let server = HttpsMockServer::start(vec![
        MockResponse::created_json(message_json("SMcreated", "queued", "hello")),
        MockResponse::json(message_page_json(Some(
            "/2010-04-01/Accounts/AC123/Messages.json?PageSize=1&Page=1&PageToken=next",
        ))),
        MockResponse::json(empty_message_page_json()),
        MockResponse::json(message_json("SMfetched", "delivered", "hello")),
        MockResponse::json(message_json("SMredacted", "sent", "")),
        MockResponse::no_content(),
    ])
    .await?;
    let client = client_for(&server)?;
    let account = client.account(creds());

    let mut create = CreateMessageRequest::new("+15551234567");
    create.from = Some("+15557654321");
    create.body = Some("hello from twilio2");
    create.status_callback = Some("https://example.test/message-status");

    let created = account.messages().create(create).await?;
    let mut list = ListMessagesRequest::new();
    list.page_size = Some(1);
    list.page = Some(0);
    let first_page = account.messages().list(list).await?;
    let next_page_uri = first_page
        .next_page_uri
        .as_deref()
        .ok_or_else(|| missing("Messages next_page_uri"))?;
    let next_page = account.messages().list_page_uri(next_page_uri).await?;
    let fetched = account.message("SMfetched").fetch().await?;
    let redacted = account
        .message("SMredacted")
        .update(UpdateMessageRequest::redact_body())
        .await?;
    account.message("SMdelete").delete().await?;

    assert_eq!(created.sid.as_deref(), Some("SMcreated"));
    assert!(next_page.messages.is_empty());
    assert_eq!(fetched.status.as_deref(), Some("delivered"));
    assert_eq!(redacted.body.as_deref(), Some(""));

    let requests = server.requests()?;
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
    assert!(requests[0].body.contains("To=%2B15551234567"));
    assert!(requests[0].body.contains("From=%2B15557654321"));
    assert_eq!(
        requests[1].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=1&Page=0"
    );
    assert_eq!(
        requests[2].path,
        "/2010-04-01/Accounts/AC123/Messages.json?PageSize=1&Page=1&PageToken=next"
    );
    assert_eq!(requests[4].body, "Body=");
    assert_eq!(requests[5].method, "DELETE");

    Ok(())
}

fn message_json(sid: &str, status: &str, body: &str) -> String {
    format!(
        r#"{{
            "account_sid": "AC123",
            "api_version": "2010-04-01",
            "body": "{body}",
            "date_created": "Fri, 24 May 2019 17:44:46 +0000",
            "date_sent": "Fri, 24 May 2019 17:44:50 +0000",
            "date_updated": "Fri, 24 May 2019 17:44:50 +0000",
            "direction": "outbound-api",
            "error_code": null,
            "error_message": null,
            "from": "+15557654321",
            "messaging_service_sid": "MG123",
            "num_media": "0",
            "num_segments": "1",
            "price": "-0.00750",
            "price_unit": "USD",
            "sid": "{sid}",
            "status": "{status}",
            "subresource_uris": {{
                "media": "/2010-04-01/Accounts/AC123/Messages/{sid}/Media.json",
                "feedback": "/2010-04-01/Accounts/AC123/Messages/{sid}/Feedback.json"
            }},
            "to": "+15551234567",
            "uri": "/2010-04-01/Accounts/AC123/Messages/{sid}.json"
        }}"#
    )
}

fn message_page_json(next_page_uri: Option<&str>) -> String {
    let next_page_uri =
        next_page_uri.map_or_else(|| "null".to_owned(), |value| format!(r#""{value}""#));
    format!(
        r#"{{
            "messages": [{message}],
            "next_page_uri": {next_page_uri},
            "page": 0,
            "page_size": 1
        }}"#,
        message = message_json("SMlisted", "sent", "listed")
    )
}

fn empty_message_page_json() -> String {
    r#"{"messages":[],"next_page_uri":null,"page":1,"page_size":1}"#.to_owned()
}
