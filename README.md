# twilio2

`twilio2` is a small `reqwest` client for Twilio Programmable Messaging's
Messages REST API.

It covers the Messages resource and its Message subresources:

- create, fetch, list, update/redact/cancel, and delete messages
- follow Twilio `next_page_uri` pagination for messages and media
- fetch Media metadata, download Media bytes, list Media, and delete Media
- create Message Feedback

The client stores only a shared `reqwest::Client` and base URL. Account SID and
Auth Token values are passed per request through `TwilioCreds`; the auth token is
redacted from `Debug` output. API-key authentication, inbound webhook parsing,
signature verification, and higher-level provider traits are intentionally
outside this crate.

Custom base URLs must use HTTPS.

## Setup

`twilio2` accepts an injected `reqwest::Client` and enables `reqwest`'s rustls
backend by default so HTTPS works out of the box. Add `reqwest` directly if your
application builds the client:

```toml
[dependencies]
twilio2 = "0.2"
reqwest = { version = "0.13", default-features = false, features = ["rustls"] }
```

To use a different TLS backend, disable default features and choose one
explicitly:

```toml
[dependencies]
twilio2 = { version = "0.2", default-features = false, features = ["native-tls"] }
reqwest = { version = "0.13", default-features = false, features = ["native-tls"] }
```

The `rustls-no-provider` feature is also available for applications that install
their own rustls crypto provider before constructing `reqwest::Client`.

Cargo features are additive. If you enable `native-tls` or `rustls-no-provider`
without disabling default features, Cargo will also compile the default `rustls`
backend.

## API Shape

Version `0.2` is intentionally breaking. Message creation and listing use
borrowed request structs instead of the old positional convenience methods:

```rust,no_run
use twilio2::{
    CreateMessageRequest, DEFAULT_BASE_URL, TwilioClient, TwilioCreds,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = TwilioClient::try_new(reqwest::Client::new(), DEFAULT_BASE_URL)?;
let creds = TwilioCreds {
    account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    auth_token: "secret",
};

let mut request = CreateMessageRequest::new("+15551234567");
request.from = Some("+15557654321");
request.body = Some("hello");

let message = client.create_message(creds, request).await?;

if let Some(sid) = message.sid {
    println!("{sid}");
}
# Ok(())
# }
```

For media downloads, `fetch_media` calls Twilio's `.json` Media endpoint and
`download_media` calls the extensionless Media endpoint, returning
`TwilioMediaContent { content_type, bytes }`.

## Observability

The crate instruments outbound Twilio calls with `tracing` spans and events, but
does not install a subscriber or exporter. Applications decide whether those
events go to logs, OpenTelemetry, tests, or nowhere.

Each request runs inside a `twilio2.request` span with the operation name and HTTP
method. Diagnostics intentionally avoid auth tokens, `Authorization` headers,
full URLs, phone numbers, message bodies, SIDs, page URIs, media URLs, content
variables, persistent actions, and tags.

Transport/decode/API diagnostics are sanitized before being logged or stored in
`TwilioError`: known sensitive request values are removed, Basic/Bearer
credentials are redacted, sensitive key-value fields are replaced with
`<redacted>`, and URLs are redacted. `Debug` output for returned structs also
redacts message/media identifiers, message bodies, phone numbers, and page URIs
to reduce accidental application log leaks.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license
