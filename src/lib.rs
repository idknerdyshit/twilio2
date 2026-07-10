//! `twilio2` is a thin async and blocking client for Twilio Programmable Messaging.
//!
//! Account SID + Auth Token credentials, or Account SID + API Key SID/Secret
//! credentials, are passed to account-scoped handles using HTTP basic auth and
//! are never stored on [`TwilioClient`]. Request structs borrow caller-owned
//! values for the same reason: the client should not retain auth tokens, phone
//! numbers, callback URLs, sender IDs, consent records, Safe List numbers, A2P
//! campaign text, or message bodies after a request completes.
//!
//! The crate covers the Programmable Messaging endpoints exposed through the
//! legacy Messages and account-level `ShortCodes` REST APIs, Messaging v1
//! Services and sender subresources, Link Shortening, A2P 10DLC resources,
//! Deactivations, Toll-free Verifications, Accounts v1 Messaging feature APIs,
//! standalone Messaging v2 Channel Senders, Messaging v2/v3 typing indicators,
//! and Pricing v1/v2 Messaging, Phone Numbers, Voice, and Trunking resources.
//! Separate Twilio products such as `Content`, `Conversations`, `Verify`, and
//! the standalone `WhatsApp` Business Platform are not included unless exposed
//! through Programmable Messaging endpoints or parameters.
//!
//! # Examples
//!
//! ## Messages
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # {
//! use twilio2::{
//!     CreateMessageRequest, ListMessagesRequest, TwilioClient, TwilioAuth,
//!     UpdateMessageRequest,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
//!
//! let request = CreateMessageRequest::new("+15551234567")
//!     .from("+15557654321")
//!     .body("hello");
//!
//! let account = client.account(&creds);
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
//! # }
//! ```
//!
//! ## Blocking Messages
//!
//! ```rust,no_run
//! # #[cfg(feature = "sync")]
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use twilio2::{BlockingTwilioClient, CreateMessageRequest, TwilioAuth};
//!
//! let client = BlockingTwilioClient::from_config(Default::default())?;
//! let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
//! let account = client.account(&creds);
//!
//! let created = account
//!     .messages()
//!     .create(
//!         CreateMessageRequest::new("+15551234567")
//!             .from("+15557654321")
//!             .body("hello"),
//!     )?;
//! let all = account.messages().list_all().collect_all()?;
//!
//! # let _ = (created, all);
//! # Ok(())
//! # }
//! ```
//!
//! ## Messaging Services
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # {
//! use twilio2::{
//!     CreateServiceRequest, HttpMethod, ListServicesRequest, TwilioClient,
//!     TwilioAuth, UpdateServiceRequest,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
//! let account = client.account(&creds);
//!
//! let service = account
//!     .messaging().v1().services()
//!     .create(
//!         CreateServiceRequest::new("alerts")
//!             .inbound_request_url("https://example.com/inbound")
//!             .inbound_method(HttpMethod::Post),
//!     )
//!     .await?;
//!
//! let services = account
//!     .messaging().v1().services()
//!     .list(ListServicesRequest::new().page_size(20))
//!     .await?;
//! if let Some(next_page_url) = services.meta.next_page_url.as_deref() {
//!     let next_page = account.messaging().v1().services().list_page_url(next_page_url).await?;
//!     # let _ = next_page;
//! }
//!
//! let fetched = account.messaging().v1().service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").fetch().await?;
//! let updated = account
//!     .messaging().v1().service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
//!     .update(
//!         UpdateServiceRequest::new()
//!             .friendly_name("alerts-v2")
//!             .clear_status_callback(),
//!     )
//!     .await?;
//! account.messaging().v1().service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").delete().await?;
//!
//! # let _ = (service, fetched, updated);
//! # Ok(())
//! # }
//! # }
//! ```
//!
//! ## Service subresources
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # {
//! use twilio2::{
//!     CreateDestinationAlphaSenderRequest, CreateServicePhoneNumberRequest,
//!     ListDestinationAlphaSendersRequest, ListServiceSubresourcesRequest,
//!     TwilioClient, TwilioAuth,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::from_config(Default::default())?;
//! let creds = TwilioAuth::auth_token("ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "secret");
//! let service = client
//!     .account(&creds)
//!     .messaging().v1().service("MGxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
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
//! # }
//! ```
//!
//! ## Custom base URLs
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # {
//! use twilio2::{TwilioClient, TwilioClientConfig};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = TwilioClientConfig::new()
//!     .rest_base_url("https://proxy.example.com/twilio-rest")
//!     .messaging_base_url("https://proxy.example.com/twilio-messaging")
//!     .pricing_base_url("https://proxy.example.com/twilio-pricing")
//!     .accounts_base_url("https://proxy.example.com/twilio-accounts/v1");
//! let client = TwilioClient::from_config(config)?;
//! # let _ = client;
//! # Ok(())
//! # }
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

#[cfg(not(any(feature = "async", feature = "sync")))]
compile_error!(
    "twilio2 requires a transport API. Enable default features, or enable one of: async, sync."
);

mod a2p;
#[cfg(feature = "sync")]
mod blocking_client;
mod channel_senders;
#[cfg(feature = "async")]
mod client;
mod common;
mod deactivations;
#[cfg(feature = "sensitive-diagnostics")]
mod diagnostics;
mod link_shortening;
mod messages;
mod messaging;
mod messaging_features;
mod pricing;
mod secret;
mod services;
mod short_codes;
mod tollfree_verifications;
mod typing_indicators;

#[cfg(feature = "async")]
pub use a2p::A2PBrandRegistrationSmsOtpResource;
#[cfg(feature = "async")]
pub use a2p::{
    A2PBrandRegistrationResource, A2PBrandRegistrationsResource, A2PBrandVettingsResource,
    ServiceUsa2pResource, ServiceUsa2pUsecasesResource,
};
pub use a2p::{
    A2PBrandType, A2PUsecase, A2PVettingProvider, CreateA2PBrandRegistrationRequest,
    CreateA2PBrandVettingRequest, CreateUsa2pRequest, FetchUsa2pUsecasesRequest,
    ListA2PBrandRegistrationsRequest, ListA2PBrandVettingsRequest, ListUsa2pRequest,
    TwilioA2PBrandRegistration, TwilioA2PBrandRegistrationOtp, TwilioA2PBrandRegistrationPage,
    TwilioA2PBrandVetting, TwilioA2PBrandVettingPage, TwilioUsa2p, TwilioUsa2pPage,
    TwilioUsa2pUsecase, TwilioUsa2pUsecases,
};
#[cfg(feature = "sync")]
pub use a2p::{
    BlockingA2PBrandRegistrationResource, BlockingA2PBrandRegistrationSmsOtpResource,
    BlockingA2PBrandRegistrationsResource, BlockingA2PBrandVettingsResource,
    BlockingServiceUsa2pResource, BlockingServiceUsa2pUsecasesResource,
};
#[cfg(feature = "sync")]
pub use blocking_client::{BlockingTwilioAccount, BlockingTwilioClient};
#[cfg(feature = "sync")]
pub use channel_senders::{
    BlockingMessagingV2ChannelSenderResource, BlockingMessagingV2ChannelSendersResource,
};
pub use channel_senders::{
    ChannelSenderConfiguration, ChannelSenderHttpMethod, ChannelSenderProfile,
    ChannelSenderProfileEmail, ChannelSenderProfilePhoneNumber, ChannelSenderProfileWebsite,
    ChannelSenderWebhook, CreateMessagingV2ChannelSenderRequest,
    ListMessagingV2ChannelSendersRequest, MessagingV2Channel, TwilioChannelSenderCompliance,
    TwilioChannelSenderComplianceCarrier, TwilioChannelSenderComplianceCountry,
    TwilioChannelSenderConfiguration, TwilioChannelSenderOfflineReason, TwilioChannelSenderProfile,
    TwilioChannelSenderProfileEmail, TwilioChannelSenderProfilePhoneNumber,
    TwilioChannelSenderProfileWebsite, TwilioChannelSenderProperties, TwilioChannelSenderWebhook,
    TwilioMessagingV2ChannelSender, TwilioMessagingV2ChannelSenderPage,
    UpdateMessagingV2ChannelSenderRequest,
};
#[cfg(feature = "async")]
pub use channel_senders::{MessagingV2ChannelSenderResource, MessagingV2ChannelSendersResource};
#[cfg(feature = "async")]
pub use client::{TwilioAccount, TwilioClient};
#[cfg(feature = "sync")]
pub use common::BlockingTwilioPaginator;
#[cfg(feature = "async")]
pub use common::TwilioPaginator;
pub use common::{
    ApiFamily, ApiResponse, DEFAULT_ACCOUNTS_BASE_URL, DEFAULT_MESSAGING_BASE_URL,
    DEFAULT_PAGE_SIZE, DEFAULT_PRICING_BASE_URL, DEFAULT_REST_BASE_URL, Operation, RawResponse,
    RequestOptions, RequestSpec, ResponseMeta, RetryPolicy, TwilioAuth, TwilioClientConfig,
    TwilioConfig, TwilioError, TwilioMediaContent, V1PageMeta, decode_json_response,
};
#[cfg(feature = "sync")]
pub use deactivations::BlockingDeactivationsResource;
#[cfg(feature = "async")]
pub use deactivations::DeactivationsResource;
pub use deactivations::{FetchDeactivationsRequest, TwilioDeactivation};
#[cfg(feature = "sensitive-diagnostics")]
pub use diagnostics::{
    SensitiveDiagnosticEvent, SensitiveDiagnosticSink, SensitiveDiagnostics,
    SensitiveDiagnosticsBuilder, SensitiveRequestSnapshot, SensitiveResponseSnapshot,
    SensitiveTransportErrorSnapshot, SensitiveTransportErrorStage,
};
#[cfg(feature = "sync")]
pub use link_shortening::{
    BlockingMessagingV1LinkShorteningDomainCertificateResource,
    BlockingMessagingV1LinkShorteningDomainConfigResource,
    BlockingMessagingV1LinkShorteningDomainMessagingServiceResource,
    BlockingMessagingV1LinkShorteningDomainResource,
    BlockingMessagingV1LinkShorteningMessagingServiceResource,
    BlockingMessagingV1LinkShorteningResource,
    BlockingMessagingV2LinkShorteningDomainCertificateResource,
    BlockingMessagingV2LinkShorteningDomainResource, BlockingMessagingV2LinkShorteningResource,
};
#[cfg(feature = "async")]
pub use link_shortening::{
    MessagingV1LinkShorteningDomainCertificateResource,
    MessagingV1LinkShorteningDomainConfigResource,
    MessagingV1LinkShorteningDomainMessagingServiceResource,
    MessagingV1LinkShorteningDomainResource, MessagingV1LinkShorteningMessagingServiceResource,
    MessagingV1LinkShorteningResource, MessagingV2LinkShorteningDomainCertificateResource,
    MessagingV2LinkShorteningDomainResource, MessagingV2LinkShorteningResource,
};
pub use link_shortening::{
    TwilioCertificateValidationStatus, TwilioLinkShorteningDnsValidation,
    TwilioLinkShorteningDomainCertificate, TwilioLinkShorteningDomainConfig,
    TwilioLinkShorteningMessagingService, UpdateLinkShorteningDomainCertificateRequest,
    UpdateLinkShorteningDomainConfigRequest,
};
pub use messages::{
    AddressRetention, ContentRetention, CreateMessageFeedbackRequest, CreateMessageRequest,
    ListMediaRequest, ListMessagesRequest, MessageFeedbackOutcome, MessageIntent, RiskCheck,
    ScheduleType, TrafficType, TwilioMedia, TwilioMediaPage, TwilioMessage, TwilioMessageFeedback,
    TwilioMessagePage, UpdateMessageRequest, UpdateMessageStatus,
};
#[cfg(feature = "sync")]
pub use messages::{
    BlockingMessageFeedbackResource, BlockingMessageMediaResource, BlockingMessageResource,
    BlockingMessagesResource,
};
#[cfg(feature = "async")]
pub use messages::{
    MessageFeedbackResource, MessageMediaResource, MessageResource, MessagesResource,
};
#[cfg(feature = "sync")]
pub use messaging::{
    BlockingMessagingResource, BlockingMessagingV1Resource, BlockingMessagingV2Resource,
    BlockingMessagingV3Resource,
};
#[cfg(feature = "async")]
pub use messaging::{
    MessagingResource, MessagingV1Resource, MessagingV2Resource, MessagingV3Resource,
};
#[cfg(feature = "sync")]
pub use messaging_features::{
    BlockingConsentsResource, BlockingContactsResource, BlockingGlobalSafeListResource,
    BlockingMessagingGeoPermissionsResource,
};
pub use messaging_features::{
    BulkConsentsRequest, BulkContactsRequest, ConsentItem, ConsentSource, ConsentStatus,
    ContactItem, ListMessagingGeoPermissionsRequest, MessagingGeoPermissionUpdateItem,
    SafeListNumberRequest, TwilioBulkConsentResult, TwilioBulkConsentsResponse,
    TwilioBulkContactResult, TwilioBulkContactsResponse, TwilioMessagingGeoPermission,
    TwilioMessagingGeoPermissions, TwilioSafeListNumber, UpdateMessagingGeoPermissionsRequest,
};
#[cfg(feature = "async")]
pub use messaging_features::{
    ConsentsResource, ContactsResource, GlobalSafeListResource, MessagingGeoPermissionsResource,
};
#[cfg(feature = "sync")]
pub use pricing::{
    BlockingPricingMessagingCountriesResource, BlockingPricingMessagingResource,
    BlockingPricingResource, BlockingPricingV1PhoneNumberCountriesResource,
    BlockingPricingV1PhoneNumbersResource, BlockingPricingV1Resource,
    BlockingPricingV1VoiceCountriesResource, BlockingPricingV1VoiceResource,
    BlockingPricingV2Resource, BlockingPricingV2TrunkingCountriesResource,
    BlockingPricingV2TrunkingNumberResource, BlockingPricingV2TrunkingResource,
    BlockingPricingV2VoiceCountriesResource, BlockingPricingV2VoiceNumberResource,
    BlockingPricingV2VoiceResource,
};
pub use pricing::{
    FetchPricingOriginBasedNumberRequest, ListPricingCountriesRequest,
    ListPricingMessagingCountriesRequest, TwilioInboundCallPrice, TwilioInboundSmsPrice,
    TwilioOriginBasedOutboundCallPrice, TwilioOriginBasedPrefixPrice, TwilioOutboundSmsPrice,
    TwilioPhoneNumberPrice, TwilioPricingCountryPage, TwilioPricingCountrySummary,
    TwilioPricingMessaging, TwilioPricingMessagingCountry, TwilioPricingMessagingCountryPage,
    TwilioPricingMessagingCountrySummary, TwilioPricingOriginBasedVoiceCountry,
    TwilioPricingOriginBasedVoiceNumber, TwilioPricingPhoneNumberCountry,
    TwilioPricingTrunkingCountry, TwilioPricingTrunkingNumber, TwilioPricingVoiceCountry,
    TwilioSmsPrice, TwilioVoicePrefixPrice,
};
#[cfg(feature = "async")]
pub use pricing::{
    PricingMessagingCountriesResource, PricingMessagingResource, PricingResource,
    PricingV1PhoneNumberCountriesResource, PricingV1PhoneNumbersResource, PricingV1Resource,
    PricingV1VoiceCountriesResource, PricingV1VoiceResource, PricingV2Resource,
    PricingV2TrunkingCountriesResource, PricingV2TrunkingNumberResource, PricingV2TrunkingResource,
    PricingV2VoiceCountriesResource, PricingV2VoiceNumberResource, PricingV2VoiceResource,
};
pub use secret::Secret;
#[cfg(feature = "sync")]
pub use services::{
    BlockingPreregisteredUsa2pResource, BlockingServiceAlphaSendersResource,
    BlockingServiceChannelSendersResource, BlockingServiceDestinationAlphaSendersResource,
    BlockingServicePhoneNumbersResource, BlockingServiceResource,
    BlockingServiceShortCodesResource, BlockingServicesResource, BlockingServicesUsecasesResource,
};
pub use services::{
    CreateAlphaSenderRequest, CreateChannelSenderRequest, CreateDestinationAlphaSenderRequest,
    CreatePreregisteredUsa2pRequest, CreateServicePhoneNumberRequest, CreateServiceRequest,
    CreateServiceShortCodeRequest, HttpMethod, ListDestinationAlphaSendersRequest,
    ListServiceSubresourcesRequest, ListServicesRequest, ScanMessageContent, ServiceUsecase,
    TwilioAlphaSender, TwilioAlphaSenderPage, TwilioChannelSender, TwilioChannelSenderPage,
    TwilioDestinationAlphaSender, TwilioDestinationAlphaSenderPage, TwilioPreregisteredUsa2p,
    TwilioService, TwilioServicePage, TwilioServicePhoneNumber, TwilioServicePhoneNumberPage,
    TwilioServiceShortCode, TwilioServiceShortCodePage, TwilioServiceUsecase,
    TwilioServiceUsecases, UpdateServiceRequest,
};
#[cfg(feature = "async")]
pub use services::{
    PreregisteredUsa2pResource, ServiceAlphaSendersResource, ServiceChannelSendersResource,
    ServiceDestinationAlphaSendersResource, ServicePhoneNumbersResource, ServiceResource,
    ServiceShortCodesResource, ServicesResource, ServicesUsecasesResource,
};
#[cfg(feature = "async")]
pub use short_codes::{AccountShortCodeResource, AccountShortCodesResource};
#[cfg(feature = "sync")]
pub use short_codes::{BlockingAccountShortCodeResource, BlockingAccountShortCodesResource};
pub use short_codes::{
    ListAccountShortCodesRequest, TwilioAccountShortCode, TwilioAccountShortCodePage,
    UpdateAccountShortCodeRequest,
};
#[cfg(feature = "sync")]
pub use tollfree_verifications::{
    BlockingTollfreeVerificationResource, BlockingTollfreeVerificationsResource,
};
pub use tollfree_verifications::{
    CreateTollfreeVerificationRequest, ListTollfreeVerificationsRequest,
    TollfreeBusinessRegistrationAuthority, TollfreeBusinessType, TollfreeMessageVolume,
    TollfreeOptInType, TollfreeUseCaseCategory, TollfreeVerificationStatus,
    TollfreeVettingProvider, TwilioTollfreeVerification, TwilioTollfreeVerificationPage,
    UpdateTollfreeVerificationRequest,
};
#[cfg(feature = "async")]
pub use tollfree_verifications::{TollfreeVerificationResource, TollfreeVerificationsResource};
pub use typing_indicators::{
    AppleTypingEvent, CreateMessagingV2TypingIndicatorRequest,
    CreateMessagingV3TypingIndicatorRequest, TwilioTypingIndicator, TypingEvent,
};
#[cfg(feature = "sync")]
pub use typing_indicators::{
    BlockingMessagingV2TypingIndicatorsResource, BlockingMessagingV3TypingIndicatorsResource,
};
#[cfg(feature = "async")]
pub use typing_indicators::{
    MessagingV2TypingIndicatorsResource, MessagingV3TypingIndicatorsResource,
};
