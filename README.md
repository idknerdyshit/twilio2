# twilio2

`twilio2` is a small `reqwest` client for Twilio Programmable Messaging.

It covers:

- Messages: create, fetch, list, update/redact/cancel, and delete
- Message Media: fetch metadata, download bytes, list, pagination, and delete
- Message Feedback creation
- Messaging Services: create, fetch, list, update, pagination, and delete
- Service sender subresources: PhoneNumbers, ShortCodes, AlphaSenders,
  ChannelSenders, and DestinationAlphaSenders

The client stores only a shared `reqwest::Client` and parsed base URLs. Account
SID and Auth Token values are passed through `TwilioCreds` to an account-scoped
handle; the auth token is redacted from `Debug` output. API-key authentication,
inbound webhook parsing, signature verification, A2P Compliance resources, and
higher-level provider traits are intentionally outside this crate.

Custom base URLs must use HTTPS. If a custom proxy is used for Messaging v1
pagination, it must rewrite Twilio's absolute `next_page_url` values to the
configured proxy origin or pagination will be rejected.

## Setup

`twilio2` can build its own pooled `reqwest::Client` from `TwilioClientConfig`,
or accept an injected `reqwest::Client` when your application already owns
transport setup. The crate enables `reqwest`'s rustls backend by default so
HTTPS works out of the box:

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

Version `0.2` uses account/resource builders throughout. Construct a client with
`TwilioClient::from_config`, `TwilioClient::from_http_client`, or
`TwilioClient::try_with_config`, then call resource methods such as
`client.account(creds).messages().create(...)`. `TwilioClient` never stores
credentials:

```rust,no_run
use twilio2::{CreateMessageRequest, ListMessagesRequest, TwilioClient, TwilioCreds};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = TwilioClient::from_config(Default::default())?;
let creds = TwilioCreds {
    account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    auth_token: "secret",
};

let request = CreateMessageRequest::new("+15551234567")
    .from("+15557654321")
    .body("hello");

let message = client.account(creds).messages().create(request).await?;
let all = client
    .account(creds)
    .messages()
    .list_all_with(ListMessagesRequest::new().page_size(50))
    .collect_all()
    .await?;

if let Some(sid) = message.sid {
    println!("{sid}");
}
# let _ = all;
# Ok(())
# }
```

Messaging Services use Twilio's Messaging v1 API:

```rust,no_run
use twilio2::{CreateServiceRequest, HttpMethod, TwilioClient, TwilioCreds};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = TwilioClient::from_config(Default::default())?;
# let creds = TwilioCreds { account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", auth_token: "secret" };
let service = client
    .account(creds)
    .services()
    .create(
        CreateServiceRequest::new("alerts")
            .inbound_request_url("https://example.com/inbound")
            .inbound_method(HttpMethod::Post),
    )
    .await?;
# let _ = service;
# Ok(())
# }
```

For media downloads, `message(...).media().fetch(...)` calls Twilio's `.json`
Media endpoint and `message(...).media().download(...)` calls the extensionless
Media endpoint, returning `TwilioMediaContent { content_type, bytes }`.

## Custom Base URLs

Use `TwilioClientConfig` when tests or proxies need custom endpoints:

```rust,no_run
use twilio2::{TwilioClient, TwilioClientConfig};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let config = TwilioClientConfig::new()
    .rest_base_url("https://proxy.example.com/twilio-rest")
    .messaging_base_url("https://proxy.example.com/twilio-messaging/v1");
let client = TwilioClient::from_config_and_http_client(config, reqwest::Client::new())?;
# let _ = client;
# Ok(())
# }
```

`TwilioClient::new(reqwest::Client)` and
`TwilioClient::try_with_config(reqwest::Client, TwilioConfig)` remain available
as compatibility constructors.

## Examples

The runnable examples use a local HTTPS mock server and never call Twilio:

```sh
cargo run --example messages_builder
cargo run --example messaging_services
```

## Observability

The crate instruments outbound Twilio calls with `tracing` spans and events, but
does not install a subscriber or exporter. Applications decide whether those
events go to logs, OpenTelemetry, tests, or nowhere.

Each request runs inside a `twilio2.request` span with the operation name and HTTP
method. Diagnostics intentionally avoid auth tokens, `Authorization` headers,
full URLs, phone numbers, message bodies, SIDs, page URLs/URIs, sender IDs,
media URLs, content variables, persistent actions, tags, callback URLs, and
friendly names.

Transport/decode/API diagnostics are sanitized before being logged or stored in
`TwilioError`: known sensitive request values are removed, Basic/Bearer
credentials are redacted, sensitive key-value fields are replaced with
`<redacted>`, and URLs are redacted. `Debug` output for returned structs also
redacts resource identifiers, message bodies, phone numbers, sender IDs, links,
and URLs to reduce accidental application log leaks.

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license
