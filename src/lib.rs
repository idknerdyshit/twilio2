//! `twilio2` is a thin `reqwest` client for Twilio Programmable Messaging.
//!
//! Account SID + Auth Token credentials are passed to account-scoped handles
//! using HTTP basic auth and are never stored on [`TwilioClient`]. Request
//! structs borrow caller-owned values for the same reason: the client should not
//! retain auth tokens, phone numbers, callback URLs, sender IDs, or message
//! bodies after a request completes.
//!
//! The crate covers the legacy Messages REST API and the Messaging v1 Services
//! API, including Service sender subresources.
//!
//! # Examples
//!
//! ## Messages
//!
//! ```rust,no_run
//! use twilio2::{
//!     CreateMessageRequest, ListMessagesRequest, TwilioClient, TwilioCreds,
//!     UpdateMessageRequest,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioCreds {
//!     account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
//!     auth_token: "secret",
//! };
//!
//! let request = CreateMessageRequest::new("+15551234567")
//!     .from("+15557654321")
//!     .body("hello");
//!
//! let account = client.account(creds);
//! let created = account.messages().create(request).await?;
//!
//! let page = account
//!     .messages()
//!     .list(ListMessagesRequest::new().page_size(20))
//!     .await?;
//! let all = account.messages().list_all().collect_all().await?;
//!
//! let fetched = account.message("SMxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").fetch().await?;
//! let redacted = account
//!     .message("SMxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
//!     .update(UpdateMessageRequest::redact_body())
//!     .await?;
//! account.message("SMxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").delete().await?;
//!
//! # let _ = (created, fetched, redacted, page, all);
//! # Ok(())
//! # }
//! ```
//!
//! ## Messaging Services
//!
//! ```rust,no_run
//! use twilio2::{
//!     CreateServiceRequest, HttpMethod, ListServicesRequest, TwilioClient,
//!     TwilioCreds, UpdateServiceRequest,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioCreds {
//!     account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
//!     auth_token: "secret",
//! };
//! let account = client.account(creds);
//!
//! let service = account
//!     .services()
//!     .create(
//!         CreateServiceRequest::new("alerts")
//!             .inbound_request_url("https://example.com/inbound")
//!             .inbound_method(HttpMethod::Post),
//!     )
//!     .await?;
//!
//! let services = account
//!     .services()
//!     .list(ListServicesRequest::new().page_size(20))
//!     .await?;
//! if let Some(next_page_url) = services.meta.next_page_url.as_deref() {
//!     let next_page = account.services().list_page_url(next_page_url).await?;
//!     # let _ = next_page;
//! }
//!
//! let fetched = account.service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").fetch().await?;
//! let updated = account
//!     .service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
//!     .update(
//!         UpdateServiceRequest::new()
//!             .friendly_name("alerts-v2")
//!             .clear_status_callback(),
//!     )
//!     .await?;
//! account.service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").delete().await?;
//!
//! # let _ = (service, fetched, updated);
//! # Ok(())
//! # }
//! ```
//!
//! ## Service subresources
//!
//! ```rust,no_run
//! use twilio2::{
//!     CreateDestinationAlphaSenderRequest, CreateServicePhoneNumberRequest,
//!     ListDestinationAlphaSendersRequest, ListServiceSubresourcesRequest,
//!     TwilioClient, TwilioCreds,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioCreds {
//!     account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
//!     auth_token: "secret",
//! };
//! let service = client
//!     .account(creds)
//!     .service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
//!
//! let phone_number = service
//!     .phone_numbers()
//!     .create(CreateServicePhoneNumberRequest::new(
//!         "PNxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
//!     ))
//!     .await?;
//! let phone_numbers = service
//!     .phone_numbers()
//!     .list(ListServiceSubresourcesRequest::new().page_size(50))
//!     .await?;
//!
//! let alpha_sender = service
//!     .destination_alpha_senders()
//!     .create(
//!         CreateDestinationAlphaSenderRequest::new("MyCo")
//!             .iso_country_code("FR"),
//!     )
//!     .await?;
//! let alpha_senders = service
//!     .destination_alpha_senders()
//!     .list(
//!         ListDestinationAlphaSendersRequest::new()
//!             .iso_country_code("FR")
//!             .page_size(50),
//!     )
//!     .await?;
//!
//! # let _ = (phone_number, phone_numbers, alpha_sender, alpha_senders);
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom base URLs
//!
//! ```rust
//! use twilio2::{TwilioClient, TwilioClientConfig};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = TwilioClientConfig::new()
//!     .rest_base_url("https://proxy.example.com/twilio-rest")
//!     .messaging_base_url("https://proxy.example.com/twilio-messaging/v1");
//! let client = TwilioClient::from_config_and_http_client(config, reqwest::Client::new())?;
//! # let _ = client;
//! # Ok(())
//! # }
//! ```

#[cfg(not(any(
    feature = "rustls",
    feature = "native-tls",
    feature = "rustls-no-provider"
)))]
compile_error!(
    "twilio2 requires HTTPS support. Enable default features, or enable one of: rustls, native-tls, rustls-no-provider."
);

mod client;
mod common;
mod messages;
mod services;

pub use client::{TwilioAccount, TwilioClient};
pub use common::{
    ApiFamily, ApiResponse, DEFAULT_MESSAGING_BASE_URL, DEFAULT_PAGE_SIZE, DEFAULT_REST_BASE_URL,
    Operation, RawResponse, RequestOptions, RequestSpec, ResponseMeta, RetryPolicy,
    TwilioClientConfig, TwilioConfig, TwilioCreds, TwilioError, TwilioMediaContent,
    TwilioPaginator, V1PageMeta, decode_json_response,
};
pub use messages::{
    AddressRetention, ContentRetention, CreateMessageFeedbackRequest, CreateMessageRequest,
    ListMediaRequest, ListMessagesRequest, MessageFeedbackOutcome, MessageFeedbackResource,
    MessageMediaResource, MessageResource, MessagesResource, RiskCheck, ScheduleType, TrafficType,
    TwilioMedia, TwilioMediaPage, TwilioMessage, TwilioMessageFeedback, TwilioMessagePage,
    UpdateMessageRequest, UpdateMessageStatus,
};
pub use services::{
    CreateAlphaSenderRequest, CreateChannelSenderRequest, CreateDestinationAlphaSenderRequest,
    CreateServicePhoneNumberRequest, CreateServiceRequest, CreateServiceShortCodeRequest,
    HttpMethod, ListDestinationAlphaSendersRequest, ListServiceSubresourcesRequest,
    ListServicesRequest, ScanMessageContent, ServiceAlphaSendersResource,
    ServiceChannelSendersResource, ServiceDestinationAlphaSendersResource,
    ServicePhoneNumbersResource, ServiceResource, ServiceShortCodesResource, ServiceUsecase,
    ServicesResource, TwilioAlphaSender, TwilioAlphaSenderPage, TwilioChannelSender,
    TwilioChannelSenderPage, TwilioDestinationAlphaSender, TwilioDestinationAlphaSenderPage,
    TwilioService, TwilioServicePage, TwilioServicePhoneNumber, TwilioServicePhoneNumberPage,
    TwilioServiceShortCode, TwilioServiceShortCodePage, UpdateServiceRequest,
};
