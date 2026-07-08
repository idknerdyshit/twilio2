# twilio2

`twilio2` is a small async and blocking client for Twilio Programmable Messaging.

It covers:

- Messages: create, fetch, list, update/redact/cancel, and delete
- Message Media: fetch metadata, download bytes, list, pagination, and delete
- Message Feedback creation
- Messaging v1 Deactivations report redirects
- Legacy account-level ShortCodes: fetch, list, pagination, and update
- Messaging Services: create, fetch, list, update, pagination, and delete
- Service sender subresources: PhoneNumbers, ShortCodes, AlphaSenders,
  ChannelSenders, and DestinationAlphaSenders
- A2P 10DLC Brand Registrations, Brand Vettings, service Usa2p campaign
  registrations, and Usa2p usecase discovery
- Messaging v1 Toll-free Verifications: create, fetch, list, update, and delete
- Accounts v1 Messaging feature APIs: Contacts bulk upsert, Consents bulk
  upsert, and Global Safe List number add/check/remove
- Compliance Toolkit message controls exposed on Programmable Messaging
  endpoints, including message intent
- Pricing v1 Messaging Countries: list, pagination, and per-country SMS prices

The client stores only shared transport state and parsed base URLs. Account SID
and Auth Token values, or Account SID plus API Key SID/Secret values, are passed
through `TwilioAuth` to an account-scoped handle; credential values are redacted
from `Debug` output. Separate Twilio products such as Content, Conversations,
Verify, WhatsApp Business Platform, and RCS product APIs are not folded into
this crate unless a parameter or endpoint is exposed through Programmable
Messaging. Inbound webhook parsing, signature verification, and higher-level
provider traits remain intentionally outside this crate.

Custom base URLs must use HTTPS. If a custom proxy is used for Messaging v1
pagination, it must rewrite Twilio's absolute `next_page_url` values to the
configured proxy origin or pagination will be rejected.
Pricing v1 pagination follows the same rule for `pricing_base_url`.

## Setup

`twilio2` enables the async `reqwest` API and rustls by default:

```toml
[dependencies]
twilio2 = "0.3"
```

For a blocking API, disable defaults and choose `sync` plus a TLS backend:

```toml
[dependencies]
twilio2 = { version = "0.3", default-features = false, features = ["sync", "rustls"] }
```

For an async API with a different TLS backend, disable default features and
choose `async` plus that backend:

```toml
[dependencies]
twilio2 = { version = "0.3", default-features = false, features = ["async", "native-tls"] }
```

The `rustls-no-provider` feature is also available for applications that install
their own rustls crypto provider.

Cargo features are additive. You may enable both `async` and `sync`, and TLS
features are not mutually exclusive. If default features are disabled, enable at
least one of `async`/`sync` and at least one TLS backend.

## API Shape

Version `0.3` uses account/resource builders throughout. Construct a client with
`TwilioClient::from_config`, `TwilioClient::from_http_client`, or
`TwilioClient::try_with_config`, then call resource methods such as
`client.account(&creds).messages().create(...)`. `TwilioClient` never stores
credentials. `TwilioAuth` owns redacted credential buffers that are zeroized
when dropped; caller-owned source strings and transport-created header copies
remain outside that guarantee:

```rust,no_run
use twilio2::{CreateMessageRequest, ListMessagesRequest, TwilioClient, TwilioAuth};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = TwilioClient::from_config(Default::default())?;
let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
let api_key_creds = TwilioAuth::api_key(
    "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "SKxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "secret",
);

let request = CreateMessageRequest::new("+15551234567")
    .from("+15557654321")
    .body("hello");

let message = client.account(&creds).messages().create(request).await?;
let all = client
    .account(&creds)
    .messages()
    .list_all_with(ListMessagesRequest::new().page_size(50))
    .collect_all()
    .await?;

if let Some(sid) = message.sid {
    println!("{sid}");
}
# let _ = all;
# let _ = api_key_creds;
# Ok(())
# }
```

The blocking API mirrors the async API and removes only `.await`:

```rust,no_run
use twilio2::{BlockingTwilioClient, CreateMessageRequest, TwilioAuth};

fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = BlockingTwilioClient::from_config(Default::default())?;
let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");

let request = CreateMessageRequest::new("+15551234567")
    .from("+15557654321")
    .body("hello");

let account = client.account(&creds);
let created = account.messages().create(request)?;
let all = account.messages().list_all().collect_all()?;

# let _ = (created, all);
Ok(())
}
```

Messaging Services use Twilio's Messaging v1 API:

```rust,no_run
use twilio2::{CreateServiceRequest, HttpMethod, TwilioClient, TwilioAuth};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = TwilioClient::from_config(Default::default())?;
# let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
let service = client
    .account(&creds)
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

Pricing Messaging Countries use Twilio's Pricing v1 API:

```rust,no_run
use twilio2::{ListPricingMessagingCountriesRequest, TwilioClient, TwilioAuth};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
# let client = TwilioClient::from_config(Default::default())?;
# let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
let countries = client
    .account(&creds)
    .pricing()
    .messaging()
    .countries()
    .list_all_with(ListPricingMessagingCountriesRequest::new().page_size(50))
    .collect_all()
    .await?;

let us_prices = client
    .account(&creds)
    .pricing()
    .messaging()
    .countries()
    .fetch("US")
    .await?;
# let _ = (countries, us_prices);
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
    .messaging_base_url("https://proxy.example.com/twilio-messaging/v1")
    .pricing_base_url("https://proxy.example.com/twilio-pricing/v1")
    .accounts_base_url("https://proxy.example.com/twilio-accounts/v1");
let client = TwilioClient::from_config_and_http_client(config, reqwest::Client::new())?;
# let _ = client;
# Ok(())
# }
```

`TwilioClient::new(reqwest::Client)` and
`TwilioClient::try_with_config(reqwest::Client, TwilioConfig)` remain available
as compatibility constructors.

For blocking callers, use `BlockingTwilioClient::from_config_and_agent`,
`BlockingTwilioClient::from_agent`, or
`BlockingTwilioClient::try_with_config(ureq::Agent, TwilioConfig)`.

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
friendly names, API keys, contact IDs, consent records, Safe List numbers, A2P
campaign text, opt-in/out/help text, and message samples.

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
