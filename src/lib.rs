//! `twilio2` is a thin `reqwest` client for Twilio's Programmable Messaging
//! Messages REST API.
//!
//! Account SID + Auth Token credentials are passed per call using HTTP basic
//! auth and are never stored on the client. Request structs borrow caller-owned
//! values for the same reason: the client should not retain auth tokens, phone
//! numbers, callback URLs, or message bodies after a request completes.
//!
//! The crate intentionally covers the Messages resource and its Message
//! subresources only: Message create/fetch/list/update/delete, Message Media
//! metadata/list/download/delete, and Message Feedback creation. It stays close
//! to Twilio's form/query parameter names while using small enums only where
//! request values are intentionally constrained.
//!
//! # Example
//!
//! ```rust,no_run
//! use twilio2::{CreateMessageRequest, DEFAULT_BASE_URL, TwilioClient, TwilioCreds};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TwilioClient::try_new(reqwest::Client::new(), DEFAULT_BASE_URL)?;
//! let creds = TwilioCreds {
//!     account_sid: "ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
//!     auth_token: "secret",
//! };
//!
//! let mut request = CreateMessageRequest::new("+15551234567");
//! request.from = Some("+15557654321");
//! request.body = Some("hello");
//!
//! let message = client.create_message(creds, request).await?;
//! # let _ = message;
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

use std::collections::BTreeMap;
use std::error::Error as _;

use reqwest::Url;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use tracing::Instrument as _;

/// Default Twilio API root (no trailing slash).
pub const DEFAULT_BASE_URL: &str = "https://api.twilio.com";

const REDACTED: &str = "<redacted>";
const MAX_DIAGNOSTIC_BODY_BYTES: usize = 2048;

/// Error type for request validation, transport, API, decode, and response
/// metadata failures.
///
/// Diagnostic strings stored in these variants are sanitized first because these
/// errors commonly flow into application logs.
#[derive(Debug, thiserror::Error)]
pub enum TwilioError {
    #[error("invalid twilio base url: {0}")]
    InvalidBaseUrl(String),
    #[error("invalid twilio request: {0}")]
    InvalidRequest(String),
    #[error("invalid twilio response metadata: {0}")]
    InvalidResponseMetadata(String),
    #[error("http transport error: {0}")]
    Transport(String),
    /// Non-2xx response. `status` is the HTTP code; `body` is truncated diagnostic
    /// text and sanitized before being returned.
    #[error("twilio api error: status {status}")]
    Api { status: u16, body: String },
    #[error("malformed twilio response: {0}")]
    Decode(String),
}

/// Credentials for one call. Borrowed and never stored on the client.
#[derive(Clone, Copy)]
pub struct TwilioCreds<'a> {
    pub account_sid: &'a str,
    pub auth_token: &'a str,
}

impl std::fmt::Debug for TwilioCreds<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioCreds")
            .field("account_sid", &redacted_str(self.account_sid))
            .field("auth_token", &REDACTED)
            .finish()
    }
}

// --- request types ---------------------------------------------------------

/// Whether Twilio should retain or discard message content after processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentRetention {
    Retain,
    Discard,
}

impl ContentRetention {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Retain => "retain",
            Self::Discard => "discard",
        }
    }
}

/// Whether Twilio should retain or obfuscate message addresses after processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressRetention {
    Retain,
    Obfuscate,
}

impl AddressRetention {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Retain => "retain",
            Self::Obfuscate => "obfuscate",
        }
    }
}

/// Twilio traffic classification for messages that support it.
///
/// This is an enum even though Twilio currently documents one supported value;
/// keeping it constrained prevents callers from depending on undocumented
/// request strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficType {
    Free,
}

impl TrafficType {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Free => "free",
        }
    }
}

/// Scheduling mode for scheduled messages.
///
/// This is intentionally constrained to documented request values. Response
/// fields remain strings elsewhere so newly-added Twilio response values do not
/// break deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleType {
    Fixed,
}

impl ScheduleType {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
        }
    }
}

/// Whether Twilio's risk check should run for this message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskCheck {
    Enable,
    Disable,
}

impl RiskCheck {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }
}

/// Status values this crate allows callers to send in a Message update request.
///
/// Twilio exposes many message statuses in responses, but the update endpoint is
/// intentionally constrained here to the documented scheduled-message cancel
/// operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMessageStatus {
    Canceled,
}

impl UpdateMessageStatus {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Canceled => "canceled",
        }
    }
}

/// Feedback outcome values accepted by Twilio's Message Feedback endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageFeedbackOutcome {
    Confirmed,
    Unconfirmed,
}

impl MessageFeedbackOutcome {
    fn as_form_value(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

/// Request body for `POST /Messages.json`.
///
/// Twilio supports several sender and content combinations. Local validation
/// only checks invariants that are true for all create requests: `to`, one
/// sender (`from` or `messaging_service_sid`), and one content source (`body`,
/// `media_urls`, or `content_sid`). Twilio-specific combinations are left to
/// Twilio so this crate does not become stale when the API evolves.
///
/// `content_variables_json` and `tags_json` are raw JSON strings because Twilio
/// receives them as form fields, not as nested JSON in the HTTP body.
pub struct CreateMessageRequest<'a> {
    pub to: &'a str,
    pub from: Option<&'a str>,
    pub messaging_service_sid: Option<&'a str>,
    pub body: Option<&'a str>,
    pub media_urls: &'a [&'a str],
    pub content_sid: Option<&'a str>,
    pub status_callback: Option<&'a str>,
    pub application_sid: Option<&'a str>,
    pub provide_feedback: Option<bool>,
    pub attempt: Option<u32>,
    pub validity_period: Option<u32>,
    pub content_retention: Option<ContentRetention>,
    pub address_retention: Option<AddressRetention>,
    pub smart_encoded: Option<bool>,
    pub persistent_actions: &'a [&'a str],
    pub traffic_type: Option<TrafficType>,
    pub shorten_urls: Option<bool>,
    pub schedule_type: Option<ScheduleType>,
    pub send_at: Option<&'a str>,
    pub send_as_mms: Option<bool>,
    pub content_variables_json: Option<&'a str>,
    pub risk_check: Option<RiskCheck>,
    pub fallback_from: Option<&'a str>,
    pub tags_json: Option<&'a str>,
}

impl<'a> CreateMessageRequest<'a> {
    #[must_use]
    pub fn new(to: &'a str) -> Self {
        Self {
            to,
            from: None,
            messaging_service_sid: None,
            body: None,
            media_urls: &[],
            content_sid: None,
            status_callback: None,
            application_sid: None,
            provide_feedback: None,
            attempt: None,
            validity_period: None,
            content_retention: None,
            address_retention: None,
            smart_encoded: None,
            persistent_actions: &[],
            traffic_type: None,
            shorten_urls: None,
            schedule_type: None,
            send_at: None,
            send_as_mms: None,
            content_variables_json: None,
            risk_check: None,
            fallback_from: None,
            tags_json: None,
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.to.trim().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "To must not be empty".to_owned(),
            ));
        }
        if !has_non_empty(self.from) && !has_non_empty(self.messaging_service_sid) {
            return Err(TwilioError::InvalidRequest(
                "either From or MessagingServiceSid is required".to_owned(),
            ));
        }
        if self
            .media_urls
            .iter()
            .any(|media_url| media_url.trim().is_empty())
        {
            return Err(TwilioError::InvalidRequest(
                "MediaUrl values must not be empty".to_owned(),
            ));
        }
        if !has_non_empty(self.body)
            && self.media_urls.is_empty()
            && !has_non_empty(self.content_sid)
        {
            return Err(TwilioError::InvalidRequest(
                "at least one of Body, MediaUrl, or ContentSid is required".to_owned(),
            ));
        }
        if self.media_urls.len() > 10 {
            return Err(TwilioError::InvalidRequest(
                "MediaUrl can include at most 10 values".to_owned(),
            ));
        }
        Ok(())
    }

    fn form_params(&self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "To", Some(self.to));
        push_str(&mut params, "From", self.from);
        push_str(
            &mut params,
            "MessagingServiceSid",
            self.messaging_service_sid,
        );
        push_str(&mut params, "Body", self.body);
        for value in self.media_urls {
            push_str(&mut params, "MediaUrl", Some(value));
        }
        push_str(&mut params, "ContentSid", self.content_sid);
        push_str(&mut params, "StatusCallback", self.status_callback);
        push_str(&mut params, "ApplicationSid", self.application_sid);
        push_bool(&mut params, "ProvideFeedback", self.provide_feedback);
        push_u32(&mut params, "Attempt", self.attempt);
        push_u32(&mut params, "ValidityPeriod", self.validity_period);
        push_enum(&mut params, "ContentRetention", self.content_retention);
        push_enum(&mut params, "AddressRetention", self.address_retention);
        push_bool(&mut params, "SmartEncoded", self.smart_encoded);
        for value in self.persistent_actions {
            push_str(&mut params, "PersistentAction", Some(value));
        }
        push_enum(&mut params, "TrafficType", self.traffic_type);
        push_bool(&mut params, "ShortenUrls", self.shorten_urls);
        push_enum(&mut params, "ScheduleType", self.schedule_type);
        push_str(&mut params, "SendAt", self.send_at);
        push_bool(&mut params, "SendAsMms", self.send_as_mms);
        push_str(&mut params, "ContentVariables", self.content_variables_json);
        push_enum(&mut params, "RiskCheck", self.risk_check);
        push_str(&mut params, "FallbackFrom", self.fallback_from);
        push_str(&mut params, "Tags", self.tags_json);
        params
    }

    fn sensitive_values(&self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token, self.to];
        push_sensitive(&mut values, self.from);
        push_sensitive(&mut values, self.messaging_service_sid);
        push_sensitive(&mut values, self.body);
        values.extend(self.media_urls.iter().copied());
        push_sensitive(&mut values, self.content_sid);
        push_sensitive(&mut values, self.status_callback);
        push_sensitive(&mut values, self.application_sid);
        values.extend(self.persistent_actions.iter().copied());
        push_sensitive(&mut values, self.send_at);
        push_sensitive(&mut values, self.content_variables_json);
        push_sensitive(&mut values, self.fallback_from);
        push_sensitive(&mut values, self.tags_json);
        values
    }
}

/// Query parameters for the first page of `GET /Messages.json`.
///
/// Use [`TwilioClient::list_messages_page_uri`] for subsequent pages. Twilio
/// supplies opaque page tokens in `next_page_uri`; reusing that URI avoids
/// callers reconstructing pagination state incorrectly.
#[derive(Clone, Copy, Default)]
pub struct ListMessagesRequest<'a> {
    pub to: Option<&'a str>,
    pub from: Option<&'a str>,
    pub date_sent: Option<&'a str>,
    pub date_sent_before: Option<&'a str>,
    pub date_sent_after: Option<&'a str>,
    pub page_size: Option<u32>,
    pub page: Option<u32>,
    pub page_token: Option<&'a str>,
}

impl<'a> ListMessagesRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn validate(&self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)
    }

    fn apply_query(&self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        if let Some(value) = self.to {
            query.append_pair("To", value);
        }
        if let Some(value) = self.from {
            query.append_pair("From", value);
        }
        if let Some(value) = self.date_sent {
            query.append_pair("DateSent", value);
        }
        if let Some(value) = self.date_sent_before {
            query.append_pair("DateSent<", value);
        }
        if let Some(value) = self.date_sent_after {
            query.append_pair("DateSent>", value);
        }
        if let Some(value) = self.page_size {
            query.append_pair("PageSize", &value.to_string());
        }
        if let Some(value) = self.page {
            query.append_pair("Page", &value.to_string());
        }
        if let Some(value) = self.page_token {
            query.append_pair("PageToken", value);
        }
    }

    fn sensitive_values(&self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token];
        push_sensitive(&mut values, self.to);
        push_sensitive(&mut values, self.from);
        push_sensitive(&mut values, self.date_sent);
        push_sensitive(&mut values, self.date_sent_before);
        push_sensitive(&mut values, self.date_sent_after);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Request body for updating a Message.
///
/// The two common update operations are redacting a body and canceling a
/// scheduled message, so constructors are provided for both.
#[derive(Clone, Copy, Default)]
pub struct UpdateMessageRequest<'a> {
    pub sid: &'a str,
    pub body: Option<&'a str>,
    pub status: Option<UpdateMessageStatus>,
}

impl<'a> UpdateMessageRequest<'a> {
    #[must_use]
    pub fn new(sid: &'a str) -> Self {
        Self {
            sid,
            body: None,
            status: None,
        }
    }

    #[must_use]
    pub fn redact_body(sid: &'a str) -> Self {
        Self {
            sid,
            body: Some(""),
            status: None,
        }
    }

    #[must_use]
    pub fn cancel(sid: &'a str) -> Self {
        Self {
            sid,
            body: None,
            status: Some(UpdateMessageStatus::Canceled),
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.body.is_none() && self.status.is_none() {
            return Err(TwilioError::InvalidRequest(
                "update requires Body or Status".to_owned(),
            ));
        }
        Ok(())
    }

    fn form_params(&self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "Body", self.body);
        push_enum(&mut params, "Status", self.status);
        params
    }

    fn sensitive_values(&self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token, self.sid];
        push_sensitive(&mut values, self.body);
        values
    }
}

/// Query parameters for the first page of a Message's Media list.
///
/// The `message_sid` is part of the URL path, not a query parameter. It lives in
/// the request struct so the list method has a single borrowed request value.
pub struct ListMediaRequest<'a> {
    pub message_sid: &'a str,
    pub date_created: Option<&'a str>,
    pub date_created_before: Option<&'a str>,
    pub date_created_after: Option<&'a str>,
    pub page_size: Option<u32>,
    pub page: Option<u32>,
    pub page_token: Option<&'a str>,
}

impl<'a> ListMediaRequest<'a> {
    #[must_use]
    pub fn new(message_sid: &'a str) -> Self {
        Self {
            message_sid,
            date_created: None,
            date_created_before: None,
            date_created_after: None,
            page_size: None,
            page: None,
            page_token: None,
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.message_sid.trim().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "MessageSid must not be empty".to_owned(),
            ));
        }
        validate_page_size(self.page_size)
    }

    fn apply_query(&self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        if let Some(value) = self.date_created {
            query.append_pair("DateCreated", value);
        }
        if let Some(value) = self.date_created_before {
            query.append_pair("DateCreated<", value);
        }
        if let Some(value) = self.date_created_after {
            query.append_pair("DateCreated>", value);
        }
        if let Some(value) = self.page_size {
            query.append_pair("PageSize", &value.to_string());
        }
        if let Some(value) = self.page {
            query.append_pair("Page", &value.to_string());
        }
        if let Some(value) = self.page_token {
            query.append_pair("PageToken", value);
        }
    }

    fn sensitive_values(&self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token, self.message_sid];
        push_sensitive(&mut values, self.date_created);
        push_sensitive(&mut values, self.date_created_before);
        push_sensitive(&mut values, self.date_created_after);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Request for `POST /Messages/{MessageSid}/Feedback.json`.
///
/// The message SID is path data; `outcome` is the only form field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateMessageFeedbackRequest<'a> {
    pub message_sid: &'a str,
    pub outcome: MessageFeedbackOutcome,
}

impl<'a> CreateMessageFeedbackRequest<'a> {
    #[must_use]
    pub fn new(message_sid: &'a str, outcome: MessageFeedbackOutcome) -> Self {
        Self {
            message_sid,
            outcome,
        }
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_enum(&mut params, "Outcome", Some(self.outcome));
        params
    }
}

#[derive(Clone)]
struct FormParam {
    key: &'static str,
    value: String,
}

fn push_str(params: &mut Vec<FormParam>, key: &'static str, value: Option<&str>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_owned(),
        });
    }
}

fn push_bool(params: &mut Vec<FormParam>, key: &'static str, value: Option<bool>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_string(),
        });
    }
}

fn push_u32(params: &mut Vec<FormParam>, key: &'static str, value: Option<u32>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_string(),
        });
    }
}

trait FormEnum {
    fn form_value(self) -> &'static str;
}

impl FormEnum for ContentRetention {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for AddressRetention {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for TrafficType {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for ScheduleType {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for RiskCheck {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for UpdateMessageStatus {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

impl FormEnum for MessageFeedbackOutcome {
    fn form_value(self) -> &'static str {
        self.as_form_value()
    }
}

fn push_enum<T: FormEnum + Copy>(params: &mut Vec<FormParam>, key: &'static str, value: Option<T>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.form_value().to_owned(),
        });
    }
}

fn form_pairs(params: &[FormParam]) -> Vec<(&str, &str)> {
    params
        .iter()
        .map(|param| (param.key, param.value.as_str()))
        .collect()
}

fn push_sensitive<'a>(values: &mut Vec<&'a str>, value: Option<&'a str>) {
    if let Some(value) = value {
        values.push(value);
    }
}

fn has_non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn validate_page_size(page_size: Option<u32>) -> Result<(), TwilioError> {
    if let Some(page_size) = page_size {
        if !(1..=1000).contains(&page_size) {
            return Err(TwilioError::InvalidRequest(
                "PageSize must be in 1..=1000".to_owned(),
            ));
        }
    }
    Ok(())
}

// --- response types --------------------------------------------------------

/// A Twilio Message resource.
///
/// Fields are optional to keep decoding tolerant of Twilio returning `null` or
/// omitting fields in different message states. Twilio date strings are parsed
/// as RFC 2822; an unparsable date becomes `None` instead of failing the whole
/// response.
///
/// `Debug` redacts message bodies, phone numbers, SIDs, and URIs because these
/// values commonly end up in application logs during error handling.
#[derive(Clone)]
pub struct TwilioMessage {
    pub body: Option<String>,
    pub num_segments: Option<String>,
    pub direction: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub date_updated: Option<OffsetDateTime>,
    pub price: Option<String>,
    pub error_message: Option<String>,
    pub uri: Option<String>,
    pub account_sid: Option<String>,
    pub num_media: Option<String>,
    pub status: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub sid: Option<String>,
    pub date_sent: Option<OffsetDateTime>,
    pub date_created: Option<OffsetDateTime>,
    pub error_code: Option<i64>,
    pub price_unit: Option<String>,
    pub api_version: Option<String>,
    pub subresource_uris: Option<BTreeMap<String, String>>,
}

impl std::fmt::Debug for TwilioMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMessage")
            .field("body", &redacted_option(&self.body))
            .field("num_segments", &self.num_segments)
            .field("direction", &self.direction)
            .field("from", &redacted_option(&self.from))
            .field("to", &redacted_option(&self.to))
            .field("date_updated", &self.date_updated)
            .field("price", &self.price)
            .field("error_message", &self.error_message)
            .field("uri", &redacted_option(&self.uri))
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("num_media", &self.num_media)
            .field("status", &self.status)
            .field(
                "messaging_service_sid",
                &redacted_option(&self.messaging_service_sid),
            )
            .field("sid", &redacted_option(&self.sid))
            .field("date_sent", &self.date_sent)
            .field("date_created", &self.date_created)
            .field("error_code", &self.error_code)
            .field("price_unit", &self.price_unit)
            .field("api_version", &self.api_version)
            .field(
                "subresource_uris",
                &self.subresource_uris.as_ref().map(|_| REDACTED),
            )
            .finish()
    }
}

/// One page of Message resources plus Twilio's pagination metadata.
///
/// Page URIs are treated as sensitive because they can include phone numbers,
/// filters, and page tokens.
#[derive(Clone)]
pub struct TwilioMessagePage {
    pub messages: Vec<TwilioMessage>,
    pub next_page_uri: Option<String>,
    pub first_page_uri: Option<String>,
    pub previous_page_uri: Option<String>,
    pub uri: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

impl std::fmt::Debug for TwilioMessagePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMessagePage")
            .field("messages", &self.messages)
            .field("next_page_uri", &redacted_option(&self.next_page_uri))
            .field("first_page_uri", &redacted_option(&self.first_page_uri))
            .field(
                "previous_page_uri",
                &redacted_option(&self.previous_page_uri),
            )
            .field("uri", &redacted_option(&self.uri))
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

/// Metadata for one Media resource attached to a Message.
///
/// This is the JSON metadata endpoint. Use [`TwilioClient::download_media`] for
/// the extensionless endpoint that returns the actual media bytes.
#[derive(Clone)]
pub struct TwilioMedia {
    pub account_sid: Option<String>,
    pub content_type: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub parent_sid: Option<String>,
    pub sid: Option<String>,
    pub uri: Option<String>,
}

impl std::fmt::Debug for TwilioMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMedia")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("content_type", &self.content_type)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("parent_sid", &redacted_option(&self.parent_sid))
            .field("sid", &redacted_option(&self.sid))
            .field("uri", &redacted_option(&self.uri))
            .finish()
    }
}

/// One page of Message Media metadata plus Twilio's pagination metadata.
///
/// Page URIs are redacted in `Debug` because they can include message SIDs,
/// filters, and page tokens.
#[derive(Clone)]
pub struct TwilioMediaPage {
    pub media: Vec<TwilioMedia>,
    pub next_page_uri: Option<String>,
    pub first_page_uri: Option<String>,
    pub previous_page_uri: Option<String>,
    pub uri: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

impl std::fmt::Debug for TwilioMediaPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMediaPage")
            .field("media", &self.media)
            .field("next_page_uri", &redacted_option(&self.next_page_uri))
            .field("first_page_uri", &redacted_option(&self.first_page_uri))
            .field(
                "previous_page_uri",
                &redacted_option(&self.previous_page_uri),
            )
            .field("uri", &redacted_option(&self.uri))
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

/// Raw bytes returned by the extensionless Media endpoint.
///
/// The content type is copied from the HTTP response header when Twilio sends
/// one. The bytes are not logged by `Debug`; only their length is shown.
#[derive(Clone)]
pub struct TwilioMediaContent {
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for TwilioMediaContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMediaContent")
            .field("content_type", &self.content_type)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

/// Message Feedback resource returned after reporting delivery feedback.
///
/// `outcome` remains a string in responses so newly-added Twilio values do not
/// break deserialization.
#[derive(Clone)]
pub struct TwilioMessageFeedback {
    pub account_sid: Option<String>,
    pub message_sid: Option<String>,
    pub outcome: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub uri: Option<String>,
}

impl std::fmt::Debug for TwilioMessageFeedback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMessageFeedback")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("message_sid", &redacted_option(&self.message_sid))
            .field("outcome", &self.outcome)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("uri", &redacted_option(&self.uri))
            .finish()
    }
}

// --- wire types ------------------------------------------------------------

// Wire types mirror Twilio's JSON shape and stay private so public response
// types can normalize dates, tolerate missing fields, and enforce redacted
// Debug output consistently.
#[derive(Deserialize)]
struct WireMessage {
    body: Option<String>,
    num_segments: Option<String>,
    direction: Option<String>,
    from: Option<String>,
    to: Option<String>,
    date_updated: Option<String>,
    price: Option<String>,
    error_message: Option<String>,
    uri: Option<String>,
    account_sid: Option<String>,
    num_media: Option<String>,
    status: Option<String>,
    messaging_service_sid: Option<String>,
    sid: Option<String>,
    date_sent: Option<String>,
    date_created: Option<String>,
    error_code: Option<i64>,
    price_unit: Option<String>,
    api_version: Option<String>,
    subresource_uris: Option<BTreeMap<String, String>>,
}

impl WireMessage {
    fn into_message(self) -> TwilioMessage {
        TwilioMessage {
            body: self.body,
            num_segments: self.num_segments,
            direction: self.direction,
            from: self.from,
            to: self.to,
            date_updated: parse_rfc2822(self.date_updated),
            price: self.price,
            error_message: self.error_message,
            uri: self.uri,
            account_sid: self.account_sid,
            num_media: self.num_media,
            status: self.status,
            messaging_service_sid: self.messaging_service_sid,
            sid: self.sid,
            date_sent: parse_rfc2822(self.date_sent),
            date_created: parse_rfc2822(self.date_created),
            error_code: self.error_code,
            price_unit: self.price_unit,
            api_version: self.api_version,
            subresource_uris: self.subresource_uris,
        }
    }
}

#[derive(Deserialize)]
struct WireMessagePage {
    #[serde(default)]
    messages: Vec<WireMessage>,
    #[serde(default)]
    next_page_uri: Option<String>,
    #[serde(default)]
    first_page_uri: Option<String>,
    #[serde(default)]
    previous_page_uri: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    start: Option<i64>,
    #[serde(default)]
    end: Option<i64>,
}

impl WireMessagePage {
    fn into_page(self) -> TwilioMessagePage {
        TwilioMessagePage {
            messages: self
                .messages
                .into_iter()
                .map(WireMessage::into_message)
                .collect(),
            next_page_uri: non_empty(self.next_page_uri),
            first_page_uri: non_empty(self.first_page_uri),
            previous_page_uri: non_empty(self.previous_page_uri),
            uri: non_empty(self.uri),
            page: self.page,
            page_size: self.page_size,
            start: self.start,
            end: self.end,
        }
    }
}

#[derive(Deserialize)]
struct WireMedia {
    account_sid: Option<String>,
    content_type: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    parent_sid: Option<String>,
    sid: Option<String>,
    uri: Option<String>,
}

impl WireMedia {
    fn into_media(self) -> TwilioMedia {
        TwilioMedia {
            account_sid: self.account_sid,
            content_type: self.content_type,
            date_created: parse_rfc2822(self.date_created),
            date_updated: parse_rfc2822(self.date_updated),
            parent_sid: self.parent_sid,
            sid: self.sid,
            uri: self.uri,
        }
    }
}

#[derive(Deserialize)]
struct WireMediaPage {
    #[serde(default)]
    media_list: Vec<WireMedia>,
    #[serde(default)]
    next_page_uri: Option<String>,
    #[serde(default)]
    first_page_uri: Option<String>,
    #[serde(default)]
    previous_page_uri: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    start: Option<i64>,
    #[serde(default)]
    end: Option<i64>,
}

impl WireMediaPage {
    fn into_page(self) -> TwilioMediaPage {
        TwilioMediaPage {
            media: self
                .media_list
                .into_iter()
                .map(WireMedia::into_media)
                .collect(),
            next_page_uri: non_empty(self.next_page_uri),
            first_page_uri: non_empty(self.first_page_uri),
            previous_page_uri: non_empty(self.previous_page_uri),
            uri: non_empty(self.uri),
            page: self.page,
            page_size: self.page_size,
            start: self.start,
            end: self.end,
        }
    }
}

#[derive(Deserialize)]
struct WireMessageFeedback {
    account_sid: Option<String>,
    message_sid: Option<String>,
    outcome: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    uri: Option<String>,
}

impl WireMessageFeedback {
    fn into_feedback(self) -> TwilioMessageFeedback {
        TwilioMessageFeedback {
            account_sid: self.account_sid,
            message_sid: self.message_sid,
            outcome: self.outcome,
            date_created: parse_rfc2822(self.date_created),
            date_updated: parse_rfc2822(self.date_updated),
            uri: self.uri,
        }
    }
}

fn parse_rfc2822(value: Option<String>) -> Option<OffsetDateTime> {
    value.and_then(|value| OffsetDateTime::parse(&value, &Rfc2822).ok())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

// --- client ---------------------------------------------------------------

pub struct TwilioClient {
    http: reqwest::Client,
    base_url: Url,
}

impl TwilioClient {
    /// Build over an injected `reqwest::Client` so callers keep ownership of
    /// timeouts, TLS, proxies, connection pooling, and middleware.
    ///
    /// # Panics
    ///
    /// Panics when `base_url` is not a valid HTTPS base URL. Use
    /// [`Self::try_new`] to return a typed configuration error.
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self::try_new(http, base_url).expect("invalid Twilio base URL")
    }

    /// Fallible constructor for configuration paths that should not panic.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when `base_url` is empty, not
    /// HTTPS, lacks a host, includes embedded credentials, or includes a query
    /// string or fragment.
    pub fn try_new(
        http: reqwest::Client,
        base_url: impl Into<String>,
    ) -> Result<Self, TwilioError> {
        Ok(Self {
            http,
            base_url: {
                let base_url = base_url.into();
                normalize_base_url(&base_url).map_err(TwilioError::InvalidBaseUrl)?
            },
        })
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create_message(
        &self,
        creds: TwilioCreds<'_>,
        request: CreateMessageRequest<'_>,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(creds);
            let form_params = request.form_params();
            let form = form_pairs(&form_params);
            let url =
                self.endpoint_url(&["2010-04-01", "Accounts", creds.account_sid, "Messages.json"])?;
            let resp = self
                .http
                .post(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .form(&form)
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            let msg: WireMessage = decode_2xx(resp, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(&self.base_url, "create_message", "POST"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch_message(
        &self,
        creds: TwilioCreds<'_>,
        sid: &str,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            let sensitive_values = vec![creds.account_sid, creds.auth_token, sid];
            let url = self.message_url(creds.account_sid, sid)?;
            let resp = self
                .http
                .get(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            let msg: WireMessage = decode_2xx(resp, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(&self.base_url, "fetch_message", "GET"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list_messages(
        &self,
        creds: TwilioCreds<'_>,
        request: ListMessagesRequest<'_>,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(creds);
            let mut url =
                self.endpoint_url(&["2010-04-01", "Accounts", creds.account_sid, "Messages.json"])?;
            request.apply_query(&mut url);
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            self.read_message_page(resp, &sensitive_values, Some(&url), creds.account_sid)
                .await
        }
        .instrument(request_span(&self.base_url, "list_messages", "GET"))
        .await
    }

    /// Fetch a subsequent Messages page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Messages page for this account, or the HTTP
    /// request/response fails.
    pub async fn list_messages_page_uri(
        &self,
        creds: TwilioCreds<'_>,
        next_page_uri: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            let sensitive_values = vec![creds.account_sid, creds.auth_token, next_page_uri];
            let url =
                self.page_uri_url(next_page_uri, creds.account_sid, PageResource::Messages)?;
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            self.read_message_page(resp, &sensitive_values, Some(&url), creds.account_sid)
                .await
        }
        .instrument(request_span(
            &self.base_url,
            "list_messages_page_uri",
            "GET",
        ))
        .await
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update_message(
        &self,
        creds: TwilioCreds<'_>,
        request: UpdateMessageRequest<'_>,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(creds);
            let form_params = request.form_params();
            let form = form_pairs(&form_params);
            let url = self.message_url(creds.account_sid, request.sid)?;
            let resp = self
                .http
                .post(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .form(&form)
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            let msg: WireMessage = decode_2xx(resp, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(&self.base_url, "update_message", "POST"))
        .await
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete_message(
        &self,
        creds: TwilioCreds<'_>,
        sid: &str,
    ) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = vec![creds.account_sid, creds.auth_token, sid];
            let url = self.message_url(creds.account_sid, sid)?;
            let resp = self
                .http
                .delete(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            decode_empty_2xx(resp, &sensitive_values).await
        }
        .instrument(request_span(&self.base_url, "delete_message", "DELETE"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch_media(
        &self,
        creds: TwilioCreds<'_>,
        message_sid: &str,
        media_sid: &str,
    ) -> Result<TwilioMedia, TwilioError> {
        async move {
            let sensitive_values =
                vec![creds.account_sid, creds.auth_token, message_sid, media_sid];
            let url = self.media_url(creds.account_sid, message_sid, media_sid, true)?;
            let resp = self
                .http
                .get(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            let media: WireMedia = decode_2xx(resp, &sensitive_values).await?;
            Ok(media.into_media())
        }
        .instrument(request_span(&self.base_url, "fetch_media", "GET"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn download_media(
        &self,
        creds: TwilioCreds<'_>,
        message_sid: &str,
        media_sid: &str,
    ) -> Result<TwilioMediaContent, TwilioError> {
        async move {
            let sensitive_values =
                vec![creds.account_sid, creds.auth_token, message_sid, media_sid];
            let url = self.media_url(creds.account_sid, message_sid, media_sid, false)?;
            let resp = self
                .http
                .get(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            decode_media_content(resp, &sensitive_values).await
        }
        .instrument(request_span(&self.base_url, "download_media", "GET"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list_media(
        &self,
        creds: TwilioCreds<'_>,
        request: ListMediaRequest<'_>,
    ) -> Result<TwilioMediaPage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(creds);
            let mut url = self.endpoint_url(&[
                "2010-04-01",
                "Accounts",
                creds.account_sid,
                "Messages",
                request.message_sid,
                "Media.json",
            ])?;
            request.apply_query(&mut url);
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            self.read_media_page(resp, &sensitive_values, Some(&url), creds.account_sid)
                .await
        }
        .instrument(request_span(&self.base_url, "list_media", "GET"))
        .await
    }

    /// Fetch a subsequent Media page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Media page for this account, or the HTTP
    /// request/response fails.
    pub async fn list_media_page_uri(
        &self,
        creds: TwilioCreds<'_>,
        next_page_uri: &str,
    ) -> Result<TwilioMediaPage, TwilioError> {
        async move {
            let sensitive_values = vec![creds.account_sid, creds.auth_token, next_page_uri];
            let url = self.page_uri_url(next_page_uri, creds.account_sid, PageResource::Media)?;
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            self.read_media_page(resp, &sensitive_values, Some(&url), creds.account_sid)
                .await
        }
        .instrument(request_span(&self.base_url, "list_media_page_uri", "GET"))
        .await
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete_media(
        &self,
        creds: TwilioCreds<'_>,
        message_sid: &str,
        media_sid: &str,
    ) -> Result<(), TwilioError> {
        async move {
            let sensitive_values =
                vec![creds.account_sid, creds.auth_token, message_sid, media_sid];
            let url = self.media_url(creds.account_sid, message_sid, media_sid, true)?;
            let resp = self
                .http
                .delete(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            decode_empty_2xx(resp, &sensitive_values).await
        }
        .instrument(request_span(&self.base_url, "delete_media", "DELETE"))
        .await
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Feedback.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn create_message_feedback(
        &self,
        creds: TwilioCreds<'_>,
        request: CreateMessageFeedbackRequest<'_>,
    ) -> Result<TwilioMessageFeedback, TwilioError> {
        async move {
            let sensitive_values = vec![creds.account_sid, creds.auth_token, request.message_sid];
            let form_params = request.form_params();
            let form = form_pairs(&form_params);
            let url = self.endpoint_url(&[
                "2010-04-01",
                "Accounts",
                creds.account_sid,
                "Messages",
                request.message_sid,
                "Feedback.json",
            ])?;
            let resp = self
                .http
                .post(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .form(&form)
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            let feedback: WireMessageFeedback = decode_2xx(resp, &sensitive_values).await?;
            Ok(feedback.into_feedback())
        }
        .instrument(request_span(
            &self.base_url,
            "create_message_feedback",
            "POST",
        ))
        .await
    }

    async fn read_message_page(
        &self,
        resp: reqwest::Response,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
        account_sid: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireMessagePage = decode_2xx(resp, sensitive_values).await?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.page_uri_url(next_page_uri, account_sid, PageResource::Messages)?;
            if let Some(current_url) = current_url {
                validate_next_page_continuation(current_url, &next_url, PageResource::Messages)?;
            }
        }
        Ok(page)
    }

    async fn read_media_page(
        &self,
        resp: reqwest::Response,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
        account_sid: &str,
    ) -> Result<TwilioMediaPage, TwilioError> {
        let parsed: WireMediaPage = decode_2xx(resp, sensitive_values).await?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.page_uri_url(next_page_uri, account_sid, PageResource::Media)?;
            if let Some(current_url) = current_url {
                validate_next_page_continuation(current_url, &next_url, PageResource::Media)?;
            }
        }
        Ok(page)
    }

    fn message_url(&self, account_sid: &str, sid: &str) -> Result<Url, TwilioError> {
        self.endpoint_url(&[
            "2010-04-01",
            "Accounts",
            account_sid,
            "Messages",
            &format!("{sid}.json"),
        ])
    }

    fn media_url(
        &self,
        account_sid: &str,
        message_sid: &str,
        media_sid: &str,
        json: bool,
    ) -> Result<Url, TwilioError> {
        let media_segment = if json {
            format!("{media_sid}.json")
        } else {
            media_sid.to_owned()
        };
        self.endpoint_url(&[
            "2010-04-01",
            "Accounts",
            account_sid,
            "Messages",
            message_sid,
            "Media",
            &media_segment,
        ])
    }

    fn endpoint_url(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.base_url, segments)
    }

    fn page_uri_url(
        &self,
        next_page_uri: &str,
        account_sid: &str,
        resource: PageResource,
    ) -> Result<Url, TwilioError> {
        page_uri_url_from_base(&self.base_url, next_page_uri, account_sid, resource)
    }
}

// --- response handling -----------------------------------------------------

async fn decode_2xx<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    sensitive_values: &[&str],
) -> Result<T, TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        return api_error(resp, status.as_u16(), sensitive_values).await;
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| transport_error(&e, sensitive_values))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        let message = sanitize_diagnostic(e.to_string(), sensitive_values);
        tracing::warn!(error = %message, "failed to decode twilio response");
        TwilioError::Decode(message)
    })
}

async fn decode_empty_2xx(
    resp: reqwest::Response,
    sensitive_values: &[&str],
) -> Result<(), TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        return api_error(resp, status.as_u16(), sensitive_values).await;
    }
    Ok(())
}

async fn decode_media_content(
    resp: reqwest::Response,
    sensitive_values: &[&str],
) -> Result<TwilioMediaContent, TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        return api_error(resp, status.as_u16(), sensitive_values).await;
    }
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| transport_error(&e, sensitive_values))?;
    Ok(TwilioMediaContent {
        content_type,
        bytes: bytes.to_vec(),
    })
}

async fn api_error<T>(
    resp: reqwest::Response,
    status: u16,
    sensitive_values: &[&str],
) -> Result<T, TwilioError> {
    let body = match read_limited_response_text(resp).await {
        Ok(body) => body,
        Err(e) => reqwest_error_message(&e),
    };
    let body = sanitize_diagnostic(body, sensitive_values);
    tracing::warn!(
        http.status_code = status,
        response.body_len = body.len(),
        "twilio api error"
    );
    Err(TwilioError::Api {
        status,
        body: truncate(body),
    })
}

async fn read_limited_response_text(mut resp: reqwest::Response) -> Result<String, reqwest::Error> {
    let mut body = Vec::new();
    while body.len() <= MAX_DIAGNOSTIC_BODY_BYTES {
        let Some(chunk) = resp.chunk().await? else {
            break;
        };
        let remaining = MAX_DIAGNOSTIC_BODY_BYTES + 1 - body.len();
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn transport_error(e: &reqwest::Error, sensitive_values: &[&str]) -> TwilioError {
    let message = sanitize_diagnostic(reqwest_error_message(e), sensitive_values);
    tracing::warn!(error = %message, "twilio transport error");
    TwilioError::Transport(message)
}

fn reqwest_error_message(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if let Some(status) = e.status() {
        if !msg.contains(status.as_str()) {
            msg = format!("status {status}: {msg}");
        }
    }
    if let Some(source) = e.source() {
        let source = source.to_string();
        if !source.is_empty() && !msg.contains(&source) {
            msg = format!("{msg}: {source}");
        }
    }
    msg
}

fn truncate(s: String) -> String {
    if s.len() > MAX_DIAGNOSTIC_BODY_BYTES {
        let mut end = MAX_DIAGNOSTIC_BODY_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut t = s;
        t.truncate(end);
        t.push('…');
        t
    } else {
        s
    }
}

fn request_span(base_url: &Url, operation: &'static str, method: &'static str) -> tracing::Span {
    let peer_name = base_url.host_str().unwrap_or("<unknown>");
    tracing::debug_span!(
        "twilio2.request",
        operation,
        http.method = method,
        net.peer.name = %peer_name
    )
}

// --- URL and pagination validation ----------------------------------------

#[derive(Clone, Copy)]
enum PageResource {
    Messages,
    Media,
}

fn normalize_base_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("base URL is empty".to_owned());
    }

    let mut url = Url::parse(trimmed).map_err(|e| e.to_string())?;
    match url.scheme() {
        "https" => {}
        scheme => {
            return Err(format!("unsupported scheme {scheme:?}; expected https"));
        }
    }
    if url.host_str().is_none() {
        return Err("base URL must include a host".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("base URL must not include embedded credentials".to_owned());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("base URL must not include a query string or fragment".to_owned());
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn endpoint_url_from_base(base_url: &Url, segments: &[&str]) -> Result<Url, TwilioError> {
    let mut url = base_url.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|()| TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned()))?;
        path.pop_if_empty();
        path.extend(segments);
    }
    Ok(url)
}

fn page_uri_url_from_base(
    base_url: &Url,
    next_page_uri: &str,
    account_sid: &str,
    resource: PageResource,
) -> Result<Url, TwilioError> {
    // Treat Twilio pagination metadata as untrusted input. It can contain
    // sensitive filters, and callers may use custom base URLs for proxies, so we
    // constrain origin, base path, account SID, resource path, and query keys
    // before following it.
    let url = if next_page_uri.starts_with('/') && !next_page_uri.starts_with("//") {
        let mut url = base_url.clone();
        let base_prefix: Vec<String> = base_url
            .path_segments()
            .map(|segments| {
                segments
                    .filter(|segment| !segment.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let path_and_query = next_page_uri.strip_prefix('/').unwrap_or(next_page_uri);
        if path_and_query.contains('#') {
            return Err(TwilioError::InvalidResponseMetadata(
                "next_page_uri included a fragment".to_owned(),
            ));
        }
        let (path_part, query) = path_and_query
            .split_once('?')
            .map_or((path_and_query, None), |(path, query)| (path, Some(query)));
        {
            let mut path = url.path_segments_mut().map_err(|()| {
                TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned())
            })?;
            path.clear();
            path.extend(base_prefix.iter().map(String::as_str));
            path.extend(path_part.split('/').filter(|segment| !segment.is_empty()));
        }
        url.set_query(query);
        url.set_fragment(None);
        url
    } else {
        base_url
            .join(next_page_uri)
            .map_err(|e| TwilioError::InvalidResponseMetadata(e.to_string()))?
    };
    if url.scheme() != base_url.scheme()
        || url.host_str() != base_url.host_str()
        || url.port_or_known_default() != base_url.port_or_known_default()
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri changed API origin".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri included embedded credentials".to_owned(),
        ));
    }
    if url.fragment().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri included a fragment".to_owned(),
        ));
    }
    validate_page_uri(base_url, &url, account_sid, resource)?;
    Ok(url)
}

fn validate_page_uri(
    base_url: &Url,
    page_url: &Url,
    account_sid: &str,
    resource: PageResource,
) -> Result<(), TwilioError> {
    let base_prefix: Vec<&str> = base_url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    let page_segments: Vec<&str> = page_url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    let Some(api_segments) = page_segments.strip_prefix(base_prefix.as_slice()) else {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri left the configured API base path".to_owned(),
        ));
    };

    match resource {
        PageResource::Messages => {
            if api_segments.len() != 4
                || api_segments[0] != "2010-04-01"
                || api_segments[1] != "Accounts"
                || api_segments[2] != account_sid
                || api_segments[3] != "Messages.json"
            {
                return Err(TwilioError::InvalidResponseMetadata(
                    "next_page_uri is not a Messages page for this account".to_owned(),
                ));
            }
        }
        PageResource::Media => {
            if api_segments.len() != 6
                || api_segments[0] != "2010-04-01"
                || api_segments[1] != "Accounts"
                || api_segments[2] != account_sid
                || api_segments[3] != "Messages"
                || api_segments[5] != "Media.json"
            {
                return Err(TwilioError::InvalidResponseMetadata(
                    "next_page_uri is not a Media page for this account".to_owned(),
                ));
            }
        }
    }
    validate_page_query_keys(page_url, resource)?;
    Ok(())
}

fn validate_page_query_keys(page_url: &Url, resource: PageResource) -> Result<(), TwilioError> {
    let mut seen = Vec::new();
    for (key, _) in page_url.query_pairs() {
        if !allowed_page_query_key(key.as_ref(), resource) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri has unsupported query parameter {key:?}"
            )));
        }
        if seen.iter().any(|candidate| candidate == key.as_ref()) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri repeated query parameter {key:?}"
            )));
        }
        seen.push(key.into_owned());
    }
    Ok(())
}

fn allowed_page_query_key(key: &str, resource: PageResource) -> bool {
    match resource {
        PageResource::Messages => matches!(
            key,
            "To" | "From"
                | "DateSent"
                | "DateSent<"
                | "DateSent>"
                | "PageSize"
                | "Page"
                | "PageToken"
        ),
        PageResource::Media => matches!(
            key,
            "DateCreated" | "DateCreated<" | "DateCreated>" | "PageSize" | "Page" | "PageToken"
        ),
    }
}

fn validate_next_page_continuation(
    current_url: &Url,
    next_url: &Url,
    resource: PageResource,
) -> Result<(), TwilioError> {
    if current_url.path() != next_url.path() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri changed resource path".to_owned(),
        ));
    }
    for key in stable_page_query_keys(resource) {
        if query_values(current_url, key) != query_values(next_url, key) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri changed {key} query parameter"
            )));
        }
    }
    Ok(())
}

fn stable_page_query_keys(resource: PageResource) -> &'static [&'static str] {
    match resource {
        PageResource::Messages => &[
            "To",
            "From",
            "DateSent",
            "DateSent<",
            "DateSent>",
            "PageSize",
        ],
        PageResource::Media => &["DateCreated", "DateCreated<", "DateCreated>", "PageSize"],
    }
}

fn query_values(url: &Url, key: &str) -> Vec<String> {
    url.query_pairs()
        .filter_map(|(candidate, value)| {
            if candidate == key {
                Some(value.into_owned())
            } else {
                None
            }
        })
        .collect()
}

// --- redaction -------------------------------------------------------------

// Diagnostics can include lower-level request URLs, form bodies, and auth
// headers. Redaction combines operation-specific known values with key-based
// and URL-based rules so unexpected transport/decode messages do not leak
// tokens, phone numbers, SIDs, callback URLs, or message content.
fn redacted_str(value: &str) -> &str {
    if value.is_empty() { "" } else { REDACTED }
}

#[allow(
    clippy::ref_option,
    reason = "Debug impls pass struct fields directly; Option<&str> would move the noise to every call site."
)]
fn redacted_option(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(redacted_str)
}

fn sanitize_diagnostic(s: String, sensitive_values: &[&str]) -> String {
    let s = redact_known_values(s, sensitive_values);
    let s = redact_sensitive_key_values(&s);
    let s = redact_authorization_schemes(&s);
    redact_urls(&s)
}

fn redact_known_values(mut s: String, sensitive_values: &[&str]) -> String {
    for value in sensitive_values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        s = s.replace(value, REDACTED);
    }
    s
}

fn redact_sensitive_key_values(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if is_key_start(ch) || ch == '"' {
            let quoted_key = ch == '"';
            let key_start = if quoted_key { i + ch.len_utf8() } else { i };
            let Some(first_key_ch) = s[key_start..].chars().next() else {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            };
            if !is_key_start(first_key_ch) {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }

            let mut key_end = key_start + first_key_ch.len_utf8();
            while key_end < s.len() {
                let Some(ch) = s[key_end..].chars().next() else {
                    break;
                };
                if !is_key_continue(ch) {
                    break;
                }
                key_end += ch.len_utf8();
            }

            let mut cursor = if quoted_key {
                if !s[key_end..].starts_with('"') {
                    out.push(ch);
                    i += ch.len_utf8();
                    continue;
                }
                key_end + '"'.len_utf8()
            } else {
                key_end
            };
            while cursor < s.len() {
                let Some(ch) = s[cursor..].chars().next() else {
                    break;
                };
                if !ch.is_ascii_whitespace() {
                    break;
                }
                cursor += ch.len_utf8();
            }

            let sep = s[cursor..].chars().next();
            if matches!(sep, Some('=' | ':')) && is_sensitive_key(&s[key_start..key_end]) {
                let sensitive_key = normalized_key(&s[key_start..key_end]);
                cursor += sep.map_or(0, char::len_utf8);
                while cursor < s.len() {
                    let Some(ch) = s[cursor..].chars().next() else {
                        break;
                    };
                    if !ch.is_ascii_whitespace() {
                        break;
                    }
                    cursor += ch.len_utf8();
                }

                out.push_str(&s[i..cursor]);
                let value_end = if sensitive_key == "authorization" {
                    if s[cursor..].starts_with('"') {
                        out.push('"');
                        let (end, had_quote) = quoted_value_end(s, cursor + 1);
                        out.push_str(REDACTED);
                        if had_quote {
                            out.push('"');
                            end + 1
                        } else {
                            end
                        }
                    } else {
                        out.push_str(REDACTED);
                        authorization_value_end(s, cursor)
                    }
                } else if s[cursor..].starts_with('"') {
                    out.push('"');
                    let (end, had_quote) = quoted_value_end(s, cursor + 1);
                    out.push_str(REDACTED);
                    if had_quote {
                        out.push('"');
                        end + 1
                    } else {
                        end
                    }
                } else {
                    out.push_str(REDACTED);
                    delimited_value_end(s, cursor)
                };
                i = value_end;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_authorization_schemes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some(scheme_len) = auth_scheme_at(s, i) {
            out.push_str(&s[i..i + scheme_len]);
            let value_start = i + scheme_len;
            let value_end = delimited_value_end(s, value_start);
            if value_end > value_start {
                out.push_str(REDACTED);
            }
            i = value_end;
            continue;
        }
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if url_scheme_at(s, i).is_some() {
            out.push_str(REDACTED);
            i = url_value_end(s, i);
            continue;
        }
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn url_scheme_at(s: &str, i: usize) -> Option<usize> {
    const HTTP: &str = "http://";
    const HTTPS: &str = "https://";
    if !is_url_scheme_boundary(s, i) {
        return None;
    }
    let rest = &s[i..];
    if rest.len() >= HTTPS.len() && rest[..HTTPS.len()].eq_ignore_ascii_case(HTTPS) {
        Some(HTTPS.len())
    } else if rest.len() >= HTTP.len() && rest[..HTTP.len()].eq_ignore_ascii_case(HTTP) {
        Some(HTTP.len())
    } else {
        None
    }
}

fn is_url_scheme_boundary(s: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    s[..i]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

fn url_value_end(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if ch.is_ascii_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']' | '}') {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        normalized_key(key).as_str(),
        "authorization"
            | "authtoken"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "token"
            | "password"
            | "passwd"
            | "secret"
            | "apikey"
            | "key"
            | "sid"
            | "accountsid"
            | "messagesid"
            | "message_sid"
            | "mediasid"
            | "parentsid"
            | "body"
            | "to"
            | "from"
            | "fallbackfrom"
            | "url"
            | "uri"
            | "mediaurl"
            | "mediaurls"
            | "persistentaction"
            | "persistentactions"
            | "contentvariables"
            | "contentvariablesjson"
            | "tags"
            | "tagsjson"
            | "statuscallback"
            | "callbackurl"
            | "subresourceuris"
            | "nextpageuri"
            | "firstpageuri"
            | "previouspageuri"
    )
}

fn normalized_key(key: &str) -> String {
    key.to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn is_key_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_key_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn quoted_value_end(s: &str, mut i: usize) -> (usize, bool) {
    let mut escaped = false;
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
            i += ch.len_utf8();
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                i += ch.len_utf8();
            }
            '"' => return (i, true),
            _ => i += ch.len_utf8(),
        }
    }
    (i, false)
}

fn delimited_value_end(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if ch.is_ascii_whitespace() || ch == '&' {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn authorization_value_end(s: &str, i: usize) -> usize {
    if let Some(scheme_len) = auth_scheme_at(s, i) {
        return delimited_value_end(s, i + scheme_len);
    }
    delimited_value_end(s, i)
}

fn auth_scheme_at(s: &str, i: usize) -> Option<usize> {
    const BASIC: &str = "basic ";
    const BEARER: &str = "bearer ";
    if !is_auth_scheme_boundary(s, i) {
        return None;
    }
    let rest = &s[i..];
    if rest.len() >= BASIC.len() && rest[..BASIC.len()].eq_ignore_ascii_case(BASIC) {
        Some(BASIC.len())
    } else if rest.len() >= BEARER.len() && rest[..BEARER.len()].eq_ignore_ascii_case(BEARER) {
        Some(BEARER.len())
    } else {
        None
    }
}

fn is_auth_scheme_boundary(s: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    s[..i]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeMap;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use rcgen::CertifiedKey;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::{
        AddressRetention, ContentRetention, CreateMessageFeedbackRequest, CreateMessageRequest,
        ListMediaRequest, ListMessagesRequest, MessageFeedbackOutcome, PageResource, RiskCheck,
        ScheduleType, TrafficType, TwilioError, UpdateMessageRequest, endpoint_url_from_base,
        normalize_base_url, page_uri_url_from_base, sanitize_diagnostic,
    };

    #[derive(Clone)]
    struct MockResponse {
        status: u16,
        body: Vec<u8>,
        content_type: String,
        content_length: Option<usize>,
    }

    impl MockResponse {
        fn json(body: impl Into<String>) -> Self {
            Self {
                status: 200,
                body: body.into().into_bytes(),
                content_type: "application/json".to_owned(),
                content_length: None,
            }
        }

        fn created_json(body: impl Into<String>) -> Self {
            Self {
                status: 201,
                body: body.into().into_bytes(),
                content_type: "application/json".to_owned(),
                content_length: None,
            }
        }

        fn bytes(content_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
            Self {
                status: 200,
                body: body.into(),
                content_type: content_type.into(),
                content_length: None,
            }
        }

        fn no_content() -> Self {
            Self {
                status: 204,
                body: Vec::new(),
                content_type: "application/json".to_owned(),
                content_length: None,
            }
        }

        fn truncated(status: u16, body: impl Into<String>, content_length: usize) -> Self {
            Self {
                status,
                body: body.into().into_bytes(),
                content_type: "application/json".to_owned(),
                content_length: Some(content_length),
            }
        }
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl RecordedRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    struct HttpsMockServer {
        base_url: String,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
    }

    impl HttpsMockServer {
        async fn start(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let acceptor = tls_acceptor();
            let expected_requests = responses.len();
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let task_responses = Arc::clone(&responses);
            let task_requests = Arc::clone(&requests);

            tokio::spawn(async move {
                for _ in 0..expected_requests {
                    let (stream, _) = listener.accept().await.unwrap();
                    let acceptor = acceptor.clone();
                    let responses = Arc::clone(&task_responses);
                    let requests = Arc::clone(&task_requests);

                    tokio::spawn(async move {
                        let mut stream = acceptor.accept(stream).await.unwrap();
                        let request = read_http_request(&mut stream).await.unwrap();
                        let response = {
                            let mut responses = responses.lock().unwrap();
                            responses.pop_front().unwrap()
                        };
                        {
                            let mut requests = requests.lock().unwrap();
                            requests.push(request);
                        }
                        write_http_response(&mut stream, response).await.unwrap();
                    });
                }
            });

            Self {
                base_url: format!("https://{addr}"),
                requests,
            }
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn tls_acceptor() -> TlsAcceptor {
        let CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec![
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
        ])
        .unwrap();
        let cert_chain = vec![cert.der().clone()];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .unwrap();

        TlsAcceptor::from(Arc::new(config))
    }

    async fn read_http_request<S: AsyncRead + Unpin>(
        stream: &mut S,
    ) -> std::io::Result<RecordedRequest> {
        let mut raw = Vec::new();
        let mut chunk = [0; 1024];
        while header_end(&raw).is_none() {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
        }

        let header_end = header_end(&raw).unwrap();
        let header_text = String::from_utf8_lossy(&raw[..header_end]);
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().unwrap();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap().to_owned();
        let path = request_parts.next().unwrap().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
            })
            .collect();
        let content_length = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
        let mut body = raw[header_end + 4..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(content_length);

        Ok(RecordedRequest {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    fn header_end(raw: &[u8]) -> Option<usize> {
        raw.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response<S: AsyncWrite + Unpin>(
        stream: &mut S,
        response: MockResponse,
    ) -> std::io::Result<()> {
        let reason = match response.status {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            _ => "Error",
        };
        let content_length = response.content_length.unwrap_or(response.body.len());
        let headers = format!(
            "HTTP/1.1 {} {reason}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            response.status, response.content_type, content_length
        );
        stream.write_all(headers.as_bytes()).await?;
        stream.write_all(&response.body).await?;
        stream.shutdown().await
    }

    fn test_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .unwrap()
    }

    fn test_creds() -> super::TwilioCreds<'static> {
        super::TwilioCreds {
            account_sid: "AC123",
            auth_token: "token",
        }
    }

    fn assert_basic_auth(request: &RecordedRequest) {
        assert_eq!(
            request.header("authorization"),
            Some("Basic QUMxMjM6dG9rZW4=")
        );
    }

    #[tokio::test]
    async fn create_fetch_update_delete_message_send_expected_requests() {
        let server = HttpsMockServer::start(vec![
            MockResponse::created_json(full_message_json("SMcreated", "queued", "hello")),
            MockResponse::json(full_message_json("SMfetched", "delivered", "hello")),
            MockResponse::json(full_message_json("SMredacted", "sent", "")),
            MockResponse::json(full_message_json("SMcanceled", "canceled", "hello")),
            MockResponse::no_content(),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.body = Some("hello");
        create.media_urls = &["https://example.test/a.png", "https://example.test/b.png"];
        create.persistent_actions = &["mailto:test@example.test"];
        create.status_callback = Some("https://example.test/status");
        create.application_sid = Some("AP123");
        create.provide_feedback = Some(true);
        create.attempt = Some(2);
        create.validity_period = Some(3600);
        create.content_retention = Some(ContentRetention::Retain);
        create.address_retention = Some(AddressRetention::Obfuscate);
        create.smart_encoded = Some(true);
        create.traffic_type = Some(TrafficType::Free);
        create.shorten_urls = Some(false);
        create.schedule_type = Some(ScheduleType::Fixed);
        create.send_at = Some("2026-07-03T12:00:00Z");
        create.send_as_mms = Some(true);
        create.content_variables_json = Some(r#"{"name":"Ada"}"#);
        create.risk_check = Some(RiskCheck::Disable);
        create.fallback_from = Some("+15550000000");
        create.tags_json = Some(r#"{"campaign":"spring"}"#);

        let created = client.create_message(creds, create).await.unwrap();
        let fetched = client.fetch_message(creds, "SM fetch/123").await.unwrap();
        let redacted = client
            .update_message(creds, UpdateMessageRequest::redact_body("SMredact"))
            .await
            .unwrap();
        let canceled = client
            .update_message(creds, UpdateMessageRequest::cancel("SMcancel"))
            .await
            .unwrap();
        client.delete_message(creds, "SMdelete").await.unwrap();

        assert_eq!(created.sid.as_deref(), Some("SMcreated"));
        assert_eq!(fetched.status.as_deref(), Some("delivered"));
        assert_eq!(redacted.body.as_deref(), Some(""));
        assert_eq!(canceled.status.as_deref(), Some("canceled"));
        let requests = server.requests();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
        assert!(requests[0].body.contains("To=%2B15551234567"));
        assert!(requests[0].body.contains("From=%2B15557654321"));
        assert!(requests[0].body.contains("Body=hello"));
        assert!(
            requests[0]
                .body
                .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fa.png")
        );
        assert!(
            requests[0]
                .body
                .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fb.png")
        );
        assert!(
            requests[0]
                .body
                .contains("PersistentAction=mailto%3Atest%40example.test")
        );
        assert!(
            requests[0]
                .body
                .contains("ContentVariables=%7B%22name%22%3A%22Ada%22%7D")
        );
        assert!(requests[0].body.contains("RiskCheck=disable"));
        assert!(
            requests[0]
                .body
                .contains("Tags=%7B%22campaign%22%3A%22spring%22%7D")
        );
        assert_basic_auth(&requests[0]);
        assert_eq!(requests[1].method, "GET");
        assert_eq!(
            requests[1].path,
            "/2010-04-01/Accounts/AC123/Messages/SM%20fetch%2F123.json"
        );
        assert_eq!(requests[2].body, "Body=");
        assert_eq!(requests[3].body, "Status=canceled");
        assert_eq!(requests[4].method, "DELETE");
        assert_eq!(
            requests[4].path,
            "/2010-04-01/Accounts/AC123/Messages/SMdelete.json"
        );
    }

    #[tokio::test]
    async fn create_message_supports_messaging_service_and_content_sid() {
        let server = HttpsMockServer::start(vec![MockResponse::created_json(full_message_json(
            "SMcontent",
            "queued",
            "",
        ))])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();
        let mut create = CreateMessageRequest::new("+15551234567");
        create.messaging_service_sid = Some("MG123");
        create.content_sid = Some("HX123");

        let message = client.create_message(creds, create).await.unwrap();

        assert_eq!(message.sid.as_deref(), Some("SMcontent"));
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
        assert_eq!(
            requests[0].body,
            "To=%2B15551234567&MessagingServiceSid=MG123&ContentSid=HX123"
        );
        assert_basic_auth(&requests[0]);
    }

    #[tokio::test]
    async fn create_message_supports_media_only_content() {
        let server = HttpsMockServer::start(vec![MockResponse::created_json(full_message_json(
            "SMmedia", "queued", "",
        ))])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();
        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.media_urls = &["https://example.test/a.png", "https://example.test/b.png"];

        let message = client.create_message(creds, create).await.unwrap();

        assert_eq!(message.sid.as_deref(), Some("SMmedia"));
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert!(requests[0].body.contains("To=%2B15551234567"));
        assert!(requests[0].body.contains("From=%2B15557654321"));
        assert!(!requests[0].body.contains("Body="));
        assert!(!requests[0].body.contains("ContentSid="));
        assert!(
            requests[0]
                .body
                .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fa.png")
        );
        assert!(
            requests[0]
                .body
                .contains("MediaUrl=https%3A%2F%2Fexample.test%2Fb.png")
        );
        assert_basic_auth(&requests[0]);
    }

    #[tokio::test]
    async fn list_messages_and_page_uri_support_date_filters() {
        let next_page_uri = "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&DateSent=2026-07-01&DateSent%3C=2026-07-31&DateSent%3E=2026-06-01&PageSize=2&Page=1&PageToken=next";
        let first_body = format!(
            r#"{{
                "messages": [{full}],
                "next_page_uri": "{next_page_uri}",
                "first_page_uri": "/first",
                "previous_page_uri": null,
                "uri": "/current",
                "page": 0,
                "page_size": 2,
                "start": 0,
                "end": 0
            }}"#,
            full = full_message_json("SMfirst", "sent", "one")
        );
        let server = HttpsMockServer::start(vec![
            MockResponse::json(first_body),
            MockResponse::json(r#"{"messages":[],"next_page_uri":null}"#),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();
        let mut request = ListMessagesRequest::new();
        request.to = Some("+15551234567");
        request.from = Some("+15557654321");
        request.date_sent = Some("2026-07-01");
        request.date_sent_before = Some("2026-07-31");
        request.date_sent_after = Some("2026-06-01");
        request.page_size = Some(2);
        request.page = Some(0);
        request.page_token = Some("start");

        let first = client.list_messages(creds, request).await.unwrap();
        let second = client
            .list_messages_page_uri(creds, first.next_page_uri.as_deref().unwrap())
            .await
            .unwrap();

        assert_eq!(first.messages[0].sid.as_deref(), Some("SMfirst"));
        assert_eq!(first.page_size, Some(2));
        assert!(second.messages.is_empty());
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&DateSent=2026-07-01&DateSent%3C=2026-07-31&DateSent%3E=2026-06-01&PageSize=2&Page=0&PageToken=start"
        );
        assert_eq!(requests[1].path, next_page_uri);
    }

    #[tokio::test]
    async fn media_metadata_download_list_and_delete_work() {
        let next_page_uri = "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=1&Page=1&PageToken=next";
        let list_body = format!(
            r#"{{
                "media_list": [{media}],
                "next_page_uri": "{next_page_uri}",
                "page": 0,
                "page_size": 1
            }}"#,
            media = media_json("MElist")
        );
        let server = HttpsMockServer::start(vec![
            MockResponse::json(media_json("MEmeta")),
            MockResponse::bytes("image/png", vec![1, 2, 3, 4]),
            MockResponse::json(list_body),
            MockResponse::json(r#"{"media_list":[],"next_page_uri":null}"#),
            MockResponse::no_content(),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();

        let meta = client.fetch_media(creds, "SM123", "MEmeta").await.unwrap();
        let content = client
            .download_media(creds, "SM123", "MEraw")
            .await
            .unwrap();
        let mut list = ListMediaRequest::new("SM123");
        list.date_created = Some("2026-07-01");
        list.date_created_before = Some("2026-07-31");
        list.date_created_after = Some("2026-06-01");
        list.page_size = Some(1);
        list.page = Some(0);
        list.page_token = Some("start");
        let first = client.list_media(creds, list).await.unwrap();
        let second = client
            .list_media_page_uri(creds, first.next_page_uri.as_deref().unwrap())
            .await
            .unwrap();
        client
            .delete_media(creds, "SM123", "MEdelete")
            .await
            .unwrap();

        assert_eq!(meta.sid.as_deref(), Some("MEmeta"));
        assert_eq!(content.content_type.as_deref(), Some("image/png"));
        assert_eq!(content.bytes, vec![1, 2, 3, 4]);
        assert_eq!(first.media[0].sid.as_deref(), Some("MElist"));
        assert!(second.media.is_empty());
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEmeta.json"
        );
        assert_eq!(
            requests[1].path,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEraw"
        );
        assert_eq!(
            requests[2].path,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=1&Page=0&PageToken=start"
        );
        assert_eq!(requests[3].path, next_page_uri);
        assert_eq!(requests[4].method, "DELETE");
        assert_eq!(
            requests[4].path,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media/MEdelete.json"
        );
    }

    #[tokio::test]
    async fn list_media_rejects_next_page_uri_for_different_message() {
        let server = HttpsMockServer::start(vec![MockResponse::json(
            r#"{
                "media_list": [],
                "next_page_uri": "/2010-04-01/Accounts/AC123/Messages/SM999/Media.json?Page=1"
            }"#,
        )])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();

        let err = client
            .list_media(test_creds(), ListMediaRequest::new("SM123"))
            .await
            .expect_err("cross-message media pagination should be rejected");

        assert!(matches!(err, TwilioError::InvalidResponseMetadata(_)));
    }

    #[tokio::test]
    async fn create_feedback_sends_outcome() {
        let server = HttpsMockServer::start(vec![MockResponse::json(
            r#"{"account_sid":"AC123","message_sid":"SM123","outcome":"confirmed","date_created":"Fri, 24 May 2019 17:44:46 +0000","uri":"/feedback"}"#,
        )])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();

        let feedback = client
            .create_message_feedback(
                test_creds(),
                CreateMessageFeedbackRequest::new("SM123", MessageFeedbackOutcome::Confirmed),
            )
            .await
            .unwrap();

        assert_eq!(feedback.outcome.as_deref(), Some("confirmed"));
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Feedback.json"
        );
        assert_eq!(requests[0].body, "Outcome=confirmed");
    }

    #[test]
    fn request_validation_catches_local_errors() {
        let mut create = CreateMessageRequest::new("");
        create.from = Some("+15557654321");
        create.body = Some("hello");
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.body = Some("hello");
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("   ");
        create.messaging_service_sid = Some("\t");
        create.body = Some("hello");
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.body = Some(" ");
        create.content_sid = Some("\t");
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.media_urls = &["https://example.test/a.png", " "];
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut create = CreateMessageRequest::new("+15551234567");
        create.from = Some("+15557654321");
        create.body = Some("hello");
        create.media_urls = &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
        assert!(matches!(
            create.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        let mut list = ListMessagesRequest::new();
        list.page_size = Some(1001);
        assert!(matches!(
            list.validate(),
            Err(TwilioError::InvalidRequest(_))
        ));

        assert!(matches!(
            UpdateMessageRequest::new("SM123").validate(),
            Err(TwilioError::InvalidRequest(_))
        ));
    }

    #[test]
    fn wire_decoding_tolerates_nullable_unknown_and_bad_dates() {
        let parsed = serde_json::from_str::<super::WireMessage>(
            r#"{
                "sid": "SM123",
                "status": "sent",
                "body": null,
                "direction": "outbound-api",
                "date_created": "not a date",
                "date_sent": "Fri, 24 May 2019 17:44:50 +0000",
                "subresource_uris": {"media": "/media"},
                "unknown": "ignored"
            }"#,
        )
        .unwrap()
        .into_message();

        assert_eq!(parsed.sid.as_deref(), Some("SM123"));
        assert!(parsed.body.is_none());
        assert!(parsed.date_created.is_none());
        assert!(parsed.date_sent.is_some());
        assert_eq!(
            parsed
                .subresource_uris
                .as_ref()
                .unwrap()
                .get("media")
                .map(String::as_str),
            Some("/media")
        );

        let media = serde_json::from_str::<super::WireMedia>(&media_json("ME123"))
            .unwrap()
            .into_media();
        assert_eq!(media.sid.as_deref(), Some("ME123"));

        let page = serde_json::from_str::<super::WireMediaPage>(
            r#"{"media_list":[],"page":0,"page_size":50,"next_page_uri":null}"#,
        )
        .unwrap()
        .into_page();
        assert_eq!(page.page_size, Some(50));
    }

    #[tokio::test]
    async fn api_and_transport_errors_are_classified_and_sanitized() {
        let server = HttpsMockServer::start(vec![
            MockResponse::truncated(429, r#"{"message":"rate","To":"+15551234567"}"#, 128),
            MockResponse::truncated(200, r#"{"sid":"SM"#, 128),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();

        let api_err = client
            .fetch_message(test_creds(), "SM123")
            .await
            .expect_err("429 should be API error");
        assert!(matches!(api_err, TwilioError::Api { status: 429, .. }));
        let rendered = format!("{api_err:?}");
        assert!(!rendered.contains("+15551234567"));

        let transport_err = client
            .fetch_message(test_creds(), "SM456")
            .await
            .expect_err("truncated 200 should be transport error");
        assert!(matches!(transport_err, TwilioError::Transport(_)));
    }

    #[test]
    fn normalizes_and_rejects_base_urls() {
        let url = normalize_base_url(" https://api.twilio.com/ ").unwrap();
        assert_eq!(url.as_str(), "https://api.twilio.com/");

        let url = normalize_base_url("https://api.twilio.com/proxy").unwrap();
        assert_eq!(url.as_str(), "https://api.twilio.com/proxy/");

        for bad in [
            "",
            "http://api.twilio.com",
            "file:///tmp/twilio",
            "https://sid:token@api.twilio.com",
            "https://api.twilio.com?x=1",
            "https://api.twilio.com#frag",
        ] {
            assert!(
                normalize_base_url(bad).is_err(),
                "accepted bad base URL {bad:?}"
            );
        }
    }

    #[test]
    fn url_join_and_page_uri_validation_cover_messages_and_media() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy").unwrap();
        assert_eq!(
            endpoint_url_from_base(
                &base_url,
                &[
                    "2010-04-01",
                    "Accounts",
                    "AC/123",
                    "Messages",
                    "SM 123.json"
                ],
            )
            .unwrap()
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC%2F123/Messages/SM%20123.json"
        );
        assert_eq!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?DateSent%3E=2026-07-01&Page=1",
                "AC123",
                PageResource::Messages,
            )
            .unwrap()
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages.json?DateSent%3E=2026-07-01&Page=1"
        );
        assert_eq!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated%3C=2026-07-01&Page=1",
                "AC123",
                PageResource::Media,
            )
            .unwrap()
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated%3C=2026-07-01&Page=1"
        );

        for bad in [
            "https://example.test/2010-04-01/Accounts/AC123/Messages.json",
            "/2010-04-01/Accounts/AC123/Calls.json?Page=1",
            "/2010-04-01/Accounts/AC999/Messages.json?Page=1",
            "/2010-04-01/Accounts/AC123/Messages.json?Unexpected=1",
            "https://user:pass@api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1",
            "https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1#frag",
            "/2010-04-01/Accounts/AC123/Messages.json?Page=1&Page=2",
        ] {
            assert!(
                page_uri_url_from_base(&base_url, bad, "AC123", PageResource::Messages).is_err(),
                "accepted bad uri {bad}"
            );
        }

        for bad in [
            "https://example.test/2010-04-01/Accounts/AC123/Messages/SM123/Media.json",
            "/2010-04-01/Accounts/AC999/Messages/SM123/Media.json?Page=1",
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media/ME123.json?Page=1",
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Unexpected=1",
            "https://user:pass@api.twilio.com/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=1",
            "https://api.twilio.com/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=1#frag",
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=1#frag",
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?Page=1&Page=2",
        ] {
            assert!(
                page_uri_url_from_base(&base_url, bad, "AC123", PageResource::Media).is_err(),
                "accepted bad media uri {bad}"
            );
        }
    }

    #[test]
    fn next_page_continuation_preserves_stable_filters_and_resource_path() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy").unwrap();
        let current_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&DateSent%3E=2026-07-01&PageSize=50&Page=0",
            "AC123",
            PageResource::Messages,
        )
        .unwrap();
        let next_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&DateSent%3E=2026-07-01&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            PageResource::Messages,
        )
        .unwrap();
        assert!(
            super::validate_next_page_continuation(&current_url, &next_url, PageResource::Messages)
                .is_ok()
        );
        let changed = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15550000000&DateSent%3E=2026-07-01&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            PageResource::Messages,
        )
        .unwrap();
        assert!(
            super::validate_next_page_continuation(&current_url, &changed, PageResource::Messages)
                .is_err()
        );

        let current_media_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=50&Page=0",
            "AC123",
            PageResource::Media,
        )
        .unwrap();
        let next_media_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            PageResource::Media,
        )
        .unwrap();
        let changed_media_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-02&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            PageResource::Media,
        )
        .unwrap();
        assert!(
            super::validate_next_page_continuation(
                &current_media_url,
                &next_media_url,
                PageResource::Media
            )
            .is_ok()
        );

        let changed_media_path = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages/SM999/Media.json?DateCreated=2026-07-01&DateCreated%3C=2026-07-31&DateCreated%3E=2026-06-01&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            PageResource::Media,
        )
        .unwrap();
        assert!(
            super::validate_next_page_continuation(
                &current_media_url,
                &changed_media_path,
                PageResource::Media
            )
            .is_err()
        );

        assert!(
            super::validate_next_page_continuation(
                &current_media_url,
                &changed_media_url,
                PageResource::Media
            )
            .is_err()
        );
    }

    #[test]
    fn debug_redacts_sensitive_response_values() {
        let msg = super::TwilioMessage {
            body: Some("secret body".to_owned()),
            num_segments: Some("1".to_owned()),
            direction: Some("outbound-api".to_owned()),
            from: Some("+15557654321".to_owned()),
            to: Some("+15551234567".to_owned()),
            date_updated: None,
            price: None,
            error_message: None,
            uri: Some("/2010-04-01/Accounts/AC123/Messages/SM123.json".to_owned()),
            account_sid: Some("AC123".to_owned()),
            num_media: Some("0".to_owned()),
            status: Some("sent".to_owned()),
            messaging_service_sid: Some("MG123".to_owned()),
            sid: Some("SM123".to_owned()),
            date_sent: None,
            date_created: None,
            error_code: None,
            price_unit: None,
            api_version: Some("2010-04-01".to_owned()),
            subresource_uris: Some(BTreeMap::from([("media".to_owned(), "/media".to_owned())])),
        };
        let rendered = format!("{msg:?}");
        for leaked in [
            "secret body",
            "+15557654321",
            "+15551234567",
            "SM123",
            "/2010-04-01",
            "/media",
        ] {
            assert!(
                !rendered.contains(leaked),
                "debug leaked {leaked}: {rendered}"
            );
        }
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_known_values_auth_and_sensitive_keys() {
        let diagnostic = concat!(
            "url=https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?",
            "To=%2B15551234567&From=%2B15557654321&Body=hello&AuthToken=abc123 ",
            "Authorization: Basic dXNlcjpwYXNz ",
            r#"Authorization: "Basic cXVvdGVkOnNlY3JldA==" "#,
            "Bearer abc.def.ghi ",
            r#"json={"password":"pw","api_key":"key123","body":"secret text"} "#,
            "raw=super-secret-token"
        );

        let redacted = sanitize_diagnostic(diagnostic.to_owned(), &["super-secret-token"]);

        for leaked in [
            "api.twilio.com",
            "2010-04-01",
            "Accounts",
            "%2B15551234567",
            "%2B15557654321",
            "hello",
            "abc123",
            "dXNlcjpwYXNz",
            "cXVvdGVkOnNlY3JldA",
            "abc.def.ghi",
            "pw",
            "key123",
            "secret text",
            "super-secret-token",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_urls_without_known_values() {
        let redacted = sanitize_diagnostic(
            "transport failed for https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321".to_owned(),
            &[],
        );

        for leaked in [
            "https://api.twilio.com",
            "2010-04-01",
            "Accounts",
            "AC123",
            "%2B15551234567",
            "%2B15557654321",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_operation_specific_request_values() {
        let diagnostic = concat!(
            "request failed for https://api.twilio.com/2010-04-01/Accounts/ACsecret/Messages/",
            "SMsecret.json?To=%2B15551234567&From=%2B15557654321 with body hello+world"
        );

        let redacted = sanitize_diagnostic(
            diagnostic.to_owned(),
            &[
                "ACsecret",
                "SMsecret",
                "%2B15551234567",
                "%2B15557654321",
                "hello+world",
            ],
        );

        for leaked in [
            "ACsecret",
            "SMsecret",
            "%2B15551234567",
            "%2B15557654321",
            "hello+world",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_new_request_and_response_keys() {
        let diagnostic = concat!(
            "MediaUrls=https://example.test/a.png&",
            "PersistentActions=mailto:test@example.test&",
            r#"ContentVariablesJson={"name":"Ada"}&"#,
            r#"TagsJson={"campaign":"spring"}&"#,
            "StatusCallback=https://example.test/status&",
            "SubresourceUris=/2010-04-01/Accounts/AC123/Messages/SM123/Media.json"
        );

        let redacted = sanitize_diagnostic(diagnostic.to_owned(), &[]);

        for leaked in [
            "https://example.test/a.png",
            "mailto:test@example.test",
            "Ada",
            "campaign",
            "spring",
            "https://example.test/status",
            "/2010-04-01",
            "SM123",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    fn full_message_json(sid: &str, status: &str, body: &str) -> String {
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

    fn media_json(sid: &str) -> String {
        format!(
            r#"{{
                "account_sid": "AC123",
                "content_type": "image/jpeg",
                "date_created": "Sun, 16 Aug 2015 15:53:54 +0000",
                "date_updated": "Sun, 16 Aug 2015 15:53:55 +0000",
                "parent_sid": "SM123",
                "sid": "{sid}",
                "uri": "/2010-04-01/Accounts/AC123/Messages/SM123/Media/{sid}.json"
            }}"#
        )
    }
}
