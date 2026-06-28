# twilio2

`twilio2` is a small `reqwest` client for the Twilio Programmable Messaging REST
API.

It covers the low-level message operations needed by SMS provider adapters:

- create a message
- fetch a message by SID
- list messages by `To`/`From` with pagination

The client stores only a shared `reqwest::Client` and base URL. Account SID and
auth token values are passed per request through `TwilioCreds`; the auth token is
redacted from `Debug` output.

Inbound webhook parsing/signature verification and any higher-level provider
trait adapter are intentionally outside this crate.

## Example

```rust,no_run
use twilio2::{TwilioClient, TwilioCreds, DEFAULT_BASE_URL};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = TwilioClient::try_new(reqwest::Client::new(), DEFAULT_BASE_URL)?;
let creds = TwilioCreds {
    account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    auth_token: "secret",
};

let message = client
    .create_message(creds, "+15551234567", "+15557654321", "hello")
    .await?;

println!("{}", message.sid);
# Ok(())
# }
```

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license
