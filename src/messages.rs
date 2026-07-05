#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::collections::BTreeMap;

use http::Method;
use serde::Deserialize;
use time::OffsetDateTime;
#[cfg(feature = "async")]
use tracing::Instrument as _;
use url::Url;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "sync")]
use crate::common::BlockingTwilioPaginator;
use crate::common::{
    ApiFamily, DEFAULT_PAGE_SIZE, FormEnum, FormParam, LegacyPageResource, RequestSpec,
    TwilioCreds, TwilioError, TwilioMediaContent, decode_json_response, has_non_empty, non_empty,
    parse_rfc2822, push_bool, push_enum, push_sensitive, push_str, push_u32, redacted_option,
    request_span, validate_legacy_next_page_continuation, validate_page_size,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};

const MESSAGE_BODY_MAX_CHARS: usize = 1_600;
const MESSAGE_VALIDITY_PERIOD_MIN_SECONDS: u32 = 1;
const MESSAGE_VALIDITY_PERIOD_MAX_SECONDS: u32 = 36_000;

/// Whether Twilio should retain or discard message content after processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentRetention {
    Retain,
    Discard,
}

impl FormEnum for ContentRetention {
    fn form_value(self) -> &'static str {
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

impl FormEnum for AddressRetention {
    fn form_value(self) -> &'static str {
        match self {
            Self::Retain => "retain",
            Self::Obfuscate => "obfuscate",
        }
    }
}

/// Twilio traffic classification for messages that support it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficType {
    Free,
}

impl FormEnum for TrafficType {
    fn form_value(self) -> &'static str {
        match self {
            Self::Free => "free",
        }
    }
}

/// Scheduling mode for scheduled messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleType {
    Fixed,
}

impl FormEnum for ScheduleType {
    fn form_value(self) -> &'static str {
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

impl FormEnum for RiskCheck {
    fn form_value(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }
}

/// Status values this crate allows callers to send in a Message update request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMessageStatus {
    Canceled,
}

impl FormEnum for UpdateMessageStatus {
    fn form_value(self) -> &'static str {
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

impl FormEnum for MessageFeedbackOutcome {
    fn form_value(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

/// Request body for `POST /Messages.json`.
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

    #[must_use]
    pub fn from(mut self, value: &'a str) -> Self {
        self.from = Some(value);
        self
    }

    #[must_use]
    pub fn messaging_service_sid(mut self, value: &'a str) -> Self {
        self.messaging_service_sid = Some(value);
        self
    }

    #[must_use]
    pub fn body(mut self, value: &'a str) -> Self {
        self.body = Some(value);
        self
    }

    #[must_use]
    pub fn media_urls(mut self, value: &'a [&'a str]) -> Self {
        self.media_urls = value;
        self
    }

    #[must_use]
    pub fn content_sid(mut self, value: &'a str) -> Self {
        self.content_sid = Some(value);
        self
    }

    #[must_use]
    pub fn status_callback(mut self, value: &'a str) -> Self {
        self.status_callback = Some(value);
        self
    }

    #[must_use]
    pub fn application_sid(mut self, value: &'a str) -> Self {
        self.application_sid = Some(value);
        self
    }

    #[must_use]
    pub fn provide_feedback(mut self, value: bool) -> Self {
        self.provide_feedback = Some(value);
        self
    }

    #[must_use]
    pub fn attempt(mut self, value: u32) -> Self {
        self.attempt = Some(value);
        self
    }

    #[must_use]
    pub fn validity_period(mut self, value: u32) -> Self {
        self.validity_period = Some(value);
        self
    }

    #[must_use]
    pub fn content_retention(mut self, value: ContentRetention) -> Self {
        self.content_retention = Some(value);
        self
    }

    #[must_use]
    pub fn address_retention(mut self, value: AddressRetention) -> Self {
        self.address_retention = Some(value);
        self
    }

    #[must_use]
    pub fn smart_encoded(mut self, value: bool) -> Self {
        self.smart_encoded = Some(value);
        self
    }

    #[must_use]
    pub fn persistent_actions(mut self, value: &'a [&'a str]) -> Self {
        self.persistent_actions = value;
        self
    }

    #[must_use]
    pub fn traffic_type(mut self, value: TrafficType) -> Self {
        self.traffic_type = Some(value);
        self
    }

    #[must_use]
    pub fn shorten_urls(mut self, value: bool) -> Self {
        self.shorten_urls = Some(value);
        self
    }

    #[must_use]
    pub fn schedule_type(mut self, value: ScheduleType) -> Self {
        self.schedule_type = Some(value);
        self
    }

    #[must_use]
    pub fn send_at(mut self, value: &'a str) -> Self {
        self.send_at = Some(value);
        self
    }

    #[must_use]
    pub fn send_as_mms(mut self, value: bool) -> Self {
        self.send_as_mms = Some(value);
        self
    }

    #[must_use]
    pub fn content_variables_json(mut self, value: &'a str) -> Self {
        self.content_variables_json = Some(value);
        self
    }

    #[must_use]
    pub fn risk_check(mut self, value: RiskCheck) -> Self {
        self.risk_check = Some(value);
        self
    }

    #[must_use]
    pub fn fallback_from(mut self, value: &'a str) -> Self {
        self.fallback_from = Some(value);
        self
    }

    #[must_use]
    pub fn tags_json(mut self, value: &'a str) -> Self {
        self.tags_json = Some(value);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
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
        // These are Twilio API contract checks, not Rust/framework safeguards.
        // Keeping them local gives callers deterministic InvalidRequest errors
        // for simple documented hard limits before any network request is made.
        if let Some(body) = self.body {
            if body.chars().count() > MESSAGE_BODY_MAX_CHARS {
                return Err(TwilioError::InvalidRequest(format!(
                    "Body must be at most {MESSAGE_BODY_MAX_CHARS} characters"
                )));
            }
        }
        if let Some(validity_period) = self.validity_period {
            if !(MESSAGE_VALIDITY_PERIOD_MIN_SECONDS..=MESSAGE_VALIDITY_PERIOD_MAX_SECONDS)
                .contains(&validity_period)
            {
                return Err(TwilioError::InvalidRequest(format!(
                    "ValidityPeriod must be in {MESSAGE_VALIDITY_PERIOD_MIN_SECONDS}..={MESSAGE_VALIDITY_PERIOD_MAX_SECONDS}"
                )));
            }
        }
        if self.shorten_urls == Some(true) && !has_non_empty(self.messaging_service_sid) {
            return Err(TwilioError::InvalidRequest(
                "ShortenUrls requires MessagingServiceSid".to_owned(),
            ));
        }
        if self.content_variables_json.is_some() && !has_non_empty(self.content_sid) {
            return Err(TwilioError::InvalidRequest(
                "ContentVariables requires ContentSid".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn form_params(&self) -> Vec<FormParam> {
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

    pub(crate) fn sensitive_values(&self, creds: &'a TwilioCreds) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_token(), self.to];
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

    #[must_use]
    pub fn to(mut self, value: &'a str) -> Self {
        self.to = Some(value);
        self
    }

    #[must_use]
    pub fn from(mut self, value: &'a str) -> Self {
        self.from = Some(value);
        self
    }

    #[must_use]
    pub fn date_sent(mut self, value: &'a str) -> Self {
        self.date_sent = Some(value);
        self
    }

    #[must_use]
    pub fn date_sent_before(mut self, value: &'a str) -> Self {
        self.date_sent_before = Some(value);
        self
    }

    #[must_use]
    pub fn date_sent_after(mut self, value: &'a str) -> Self {
        self.date_sent_after = Some(value);
        self
    }

    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }

    #[must_use]
    pub fn page(mut self, value: u32) -> Self {
        self.page = Some(value);
        self
    }

    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)
    }

    pub(crate) fn apply_query(&self, url: &mut Url) {
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

    pub(crate) fn sensitive_values(&self, creds: &'a TwilioCreds) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_token()];
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
#[derive(Clone, Copy, Default)]
pub struct UpdateMessageRequest<'a> {
    pub body: Option<&'a str>,
    pub status: Option<UpdateMessageStatus>,
}

impl<'a> UpdateMessageRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            body: None,
            status: None,
        }
    }

    #[must_use]
    pub fn body(mut self, value: &'a str) -> Self {
        self.body = Some(value);
        self
    }

    #[must_use]
    pub fn status(mut self, value: UpdateMessageStatus) -> Self {
        self.status = Some(value);
        self
    }

    #[must_use]
    pub fn redact_body() -> Self {
        Self {
            body: Some(""),
            status: None,
        }
    }

    #[must_use]
    pub fn cancel() -> Self {
        Self {
            body: None,
            status: Some(UpdateMessageStatus::Canceled),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        if self.body.is_none() && self.status.is_none() {
            return Err(TwilioError::InvalidRequest(
                "update requires Body or Status".to_owned(),
            ));
        }
        // Twilio only accepts Body updates for redaction, represented by an
        // exactly empty string.
        if let Some(body) = self.body {
            if !body.is_empty() {
                return Err(TwilioError::InvalidRequest(
                    "Body must be empty when updating a Message".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn form_params(&self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "Body", self.body);
        push_enum(&mut params, "Status", self.status);
        params
    }

    pub(crate) fn sensitive_values(&self, creds: &'a TwilioCreds, sid: &'a str) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_token(), sid];
        push_sensitive(&mut values, self.body);
        values
    }
}

/// Query parameters for the first page of a Message's Media list.
#[derive(Clone, Copy, Default)]
pub struct ListMediaRequest<'a> {
    pub date_created: Option<&'a str>,
    pub date_created_before: Option<&'a str>,
    pub date_created_after: Option<&'a str>,
    pub page_size: Option<u32>,
    pub page: Option<u32>,
    pub page_token: Option<&'a str>,
}

impl<'a> ListMediaRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn date_created(mut self, value: &'a str) -> Self {
        self.date_created = Some(value);
        self
    }

    #[must_use]
    pub fn date_created_before(mut self, value: &'a str) -> Self {
        self.date_created_before = Some(value);
        self
    }

    #[must_use]
    pub fn date_created_after(mut self, value: &'a str) -> Self {
        self.date_created_after = Some(value);
        self
    }

    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }

    #[must_use]
    pub fn page(mut self, value: u32) -> Self {
        self.page = Some(value);
        self
    }

    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)
    }

    pub(crate) fn apply_query(&self, url: &mut Url) {
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

    pub(crate) fn sensitive_values(
        &self,
        creds: &'a TwilioCreds,
        message_sid: &'a str,
    ) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_token(), message_sid];
        push_sensitive(&mut values, self.date_created);
        push_sensitive(&mut values, self.date_created_before);
        push_sensitive(&mut values, self.date_created_after);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Request for `POST /Messages/{MessageSid}/Feedback.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateMessageFeedbackRequest {
    pub outcome: MessageFeedbackOutcome,
}

impl CreateMessageFeedbackRequest {
    #[must_use]
    pub fn new(outcome: MessageFeedbackOutcome) -> Self {
        Self { outcome }
    }

    pub(crate) fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_enum(&mut params, "Outcome", Some(self.outcome));
        params
    }
}

/// A Twilio Message resource.
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
                &self
                    .subresource_uris
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .finish()
    }
}

/// One page of Message resources plus Twilio's pagination metadata.
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

/// Message Feedback resource returned after reporting delivery feedback.
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

#[derive(Deserialize)]
pub(crate) struct WireMessage {
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
    pub(crate) fn into_message(self) -> TwilioMessage {
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
pub(crate) struct WireMedia {
    account_sid: Option<String>,
    content_type: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    parent_sid: Option<String>,
    sid: Option<String>,
    uri: Option<String>,
}

impl WireMedia {
    pub(crate) fn into_media(self) -> TwilioMedia {
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

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagesResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagesResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateMessageRequest<'a>,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let form_params = request.form_params();
            let spec = RequestSpec::new(
                ApiFamily::Rest,
                Method::POST,
                [
                    "2010-04-01",
                    "Accounts",
                    self.account.creds.account_sid(),
                    "Messages.json",
                ],
            )
            .operation("messages.create")
            .form_params(form_params);
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "messages.create",
            "POST",
        ))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListMessagesRequest<'a>,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.account.creds.account_sid(),
                "Messages.json",
            ])?;
            request.apply_query(&mut url);
            let spec =
                RequestSpec::from_url(ApiFamily::Rest, Method::GET, url.clone(), "messages.list");
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "messages.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent Messages page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Messages page for this account, or the HTTP
    /// request/response fails.
    pub async fn list_page_uri(
        self,
        next_page_uri: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                next_page_uri,
            ];
            let url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid(),
                LegacyPageResource::Messages,
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "messages.list_page_uri",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "messages.list_page_uri",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireMessagePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid(),
                LegacyPageResource::Messages,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(
                    current_url,
                    &next_url,
                    LegacyPageResource::Messages,
                )?;
            }
        }
        Ok(page)
    }

    /// Lazily list all Messages using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioMessagePage, TwilioMessage> {
        self.list_all_with(ListMessagesRequest::new())
    }

    /// Lazily list all Messages using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListMessagesRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioMessagePage, TwilioMessage> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_uri(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioMessagePage>
            },
            split_message_page,
        )
    }
}

fn split_message_page(page: TwilioMessagePage) -> (Vec<TwilioMessage>, Option<String>) {
    (page.messages, page.next_page_uri)
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessageResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> MessageResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioMessage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                self.sid,
            ];
            let spec = self.message_spec(Method::GET, "message.fetch")?;
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "message.fetch",
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
    pub async fn update(
        self,
        request: UpdateMessageRequest<'a>,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let form_params = request.form_params();
            let spec = self
                .message_spec(Method::POST, "message.update")?
                .form_params(form_params);
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(msg.into_message())
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "message.update",
            "POST",
        ))
        .await
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                self.sid,
            ];
            let spec = self.message_spec(Method::DELETE, "message.delete")?;
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "message.delete",
            "DELETE",
        ))
        .await
    }

    /// Message Media subresource collection.
    #[must_use]
    pub fn media(self) -> MessageMediaResource<'a> {
        MessageMediaResource { message: self }
    }

    /// Message Feedback subresource.
    #[must_use]
    pub fn feedback(self) -> MessageFeedbackResource<'a> {
        MessageFeedbackResource { message: self }
    }

    fn message_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid(),
            "Messages",
            &format!("{}.json", self.sid),
        ])
    }

    fn message_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.message_url()?,
            operation,
        ))
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessageMediaResource<'a> {
    message: MessageResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessageMediaResource<'a> {
    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self, media_sid: &'a str) -> Result<TwilioMedia, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, true, Method::GET, "message.media.fetch")?;
            let media: WireMedia = self
                .message
                .account
                .send_spec_json(spec, &sensitive_values)
                .await?;
            Ok(media.into_media())
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.fetch",
            "GET",
        ))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn download(self, media_sid: &'a str) -> Result<TwilioMediaContent, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, false, Method::GET, "message.media.download")?;
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            Ok(TwilioMediaContent {
                content_type: raw
                    .raw
                    .headers
                    .get(http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                bytes: raw.raw.body,
            })
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.download",
            "GET",
        ))
        .await
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(self, request: ListMediaRequest<'a>) -> Result<TwilioMediaPage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.message.account.creds, self.message.sid);
            let mut url = self.message.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.message.account.creds.account_sid(),
                "Messages",
                self.message.sid,
                "Media.json",
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "message.media.list",
            );
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent Media page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Media page for this message, or the HTTP
    /// request/response fails.
    pub async fn list_page_uri(self, next_page_uri: &str) -> Result<TwilioMediaPage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.message.account.creds.account_sid(),
                self.message.account.creds.auth_token(),
                self.message.sid,
                next_page_uri,
            ];
            let url = self.message.account.client.legacy_page_url(
                next_page_uri,
                self.message.account.creds.account_sid(),
                LegacyPageResource::Media {
                    message_sid: self.message.sid,
                },
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "message.media.list_page_uri",
            );
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.list_page_uri",
            "GET",
        ))
        .await
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete(self, media_sid: &'a str) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, true, Method::DELETE, "message.media.delete")?;
            self.message
                .account
                .send_spec_empty(spec, &sensitive_values)
                .await
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.delete",
            "DELETE",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMediaPage, TwilioError> {
        let parsed: WireMediaPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let resource = LegacyPageResource::Media {
                message_sid: self.message.sid,
            };
            let next_url = self.message.account.client.legacy_page_url(
                next_page_uri,
                self.message.account.creds.account_sid(),
                resource,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(current_url, &next_url, resource)?;
            }
        }
        Ok(page)
    }

    fn sensitive_values(self, media_sid: &'a str) -> Vec<&'a str> {
        vec![
            self.message.account.creds.account_sid(),
            self.message.account.creds.auth_token(),
            self.message.sid,
            media_sid,
        ]
    }

    fn media_url(self, media_sid: &str, json: bool) -> Result<Url, TwilioError> {
        let media_segment = if json {
            format!("{media_sid}.json")
        } else {
            media_sid.to_owned()
        };
        self.message.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.message.account.creds.account_sid(),
            "Messages",
            self.message.sid,
            "Media",
            &media_segment,
        ])
    }

    fn media_spec(
        self,
        media_sid: &str,
        json: bool,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.media_url(media_sid, json)?,
            operation,
        ))
    }

    /// Lazily list all Media records using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioMediaPage, TwilioMedia> {
        self.list_all_with(ListMediaRequest::new())
    }

    /// Lazily list all Media records using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListMediaRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioMediaPage, TwilioMedia> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_uri(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioMediaPage>
            },
            split_media_page,
        )
    }
}

fn split_media_page(page: TwilioMediaPage) -> (Vec<TwilioMedia>, Option<String>) {
    (page.media, page.next_page_uri)
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessageFeedbackResource<'a> {
    message: MessageResource<'a>,
}

#[cfg(feature = "async")]
impl MessageFeedbackResource<'_> {
    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Feedback.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateMessageFeedbackRequest,
    ) -> Result<TwilioMessageFeedback, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.message.account.creds.account_sid(),
                self.message.account.creds.auth_token(),
                self.message.sid,
            ];
            let form_params = request.form_params();
            let url = self.message.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.message.account.creds.account_sid(),
                "Messages",
                self.message.sid,
                "Feedback.json",
            ])?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::POST,
                url,
                "message.feedback.create",
            )
            .form_params(form_params);
            let feedback: WireMessageFeedback = self
                .message
                .account
                .send_spec_json(spec, &sensitive_values)
                .await?;
            Ok(feedback.into_feedback())
        }
        .instrument(request_span(
            &self.message.account.client.config.rest_base_url,
            "message.feedback.create",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagesResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagesResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn create(self, request: CreateMessageRequest<'a>) -> Result<TwilioMessage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "messages.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(
                ApiFamily::Rest,
                Method::POST,
                [
                    "2010-04-01",
                    "Accounts",
                    self.account.creds.account_sid(),
                    "Messages.json",
                ],
            )
            .operation("messages.create")
            .form_params(request.form_params());
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(msg.into_message())
        })
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(self, request: ListMessagesRequest<'a>) -> Result<TwilioMessagePage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "messages.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.account.creds.account_sid(),
                "Messages.json",
            ])?;
            request.apply_query(&mut url);
            let spec =
                RequestSpec::from_url(ApiFamily::Rest, Method::GET, url.clone(), "messages.list");
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent Messages page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Messages page for this account, or the HTTP
    /// request/response fails.
    pub fn list_page_uri(self, next_page_uri: &str) -> Result<TwilioMessagePage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "messages.list_page_uri",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                next_page_uri,
            ];
            let url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid(),
                LegacyPageResource::Messages,
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "messages.list_page_uri",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireMessagePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid(),
                LegacyPageResource::Messages,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(
                    current_url,
                    &next_url,
                    LegacyPageResource::Messages,
                )?;
            }
        }
        Ok(page)
    }

    /// Lazily list all Messages using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, TwilioMessagePage, TwilioMessage> {
        self.list_all_with(ListMessagesRequest::new())
    }

    /// Lazily list all Messages using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListMessagesRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioMessagePage, TwilioMessage> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_uri(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_message_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessageResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessageResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioMessage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "message.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                self.sid,
            ];
            let spec = self.message_spec(Method::GET, "message.fetch")?;
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(msg.into_message())
        })
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn update(self, request: UpdateMessageRequest<'a>) -> Result<TwilioMessage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "message.update",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let spec = self
                .message_spec(Method::POST, "message.update")?
                .form_params(request.form_params());
            let msg: WireMessage = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(msg.into_message())
        })
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub fn delete(self) -> Result<(), TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "message.delete",
            "DELETE",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_token(),
                self.sid,
            ];
            let spec = self.message_spec(Method::DELETE, "message.delete")?;
            self.account.send_spec_empty(spec, &sensitive_values)
        })
    }

    /// Message Media subresource collection.
    #[must_use]
    pub fn media(self) -> BlockingMessageMediaResource<'a> {
        BlockingMessageMediaResource { message: self }
    }

    /// Message Feedback subresource.
    #[must_use]
    pub fn feedback(self) -> BlockingMessageFeedbackResource<'a> {
        BlockingMessageFeedbackResource { message: self }
    }

    fn message_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid(),
            "Messages",
            &format!("{}.json", self.sid),
        ])
    }

    fn message_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.message_url()?,
            operation,
        ))
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessageMediaResource<'a> {
    message: BlockingMessageResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessageMediaResource<'a> {
    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self, media_sid: &'a str) -> Result<TwilioMedia, TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, true, Method::GET, "message.media.fetch")?;
            let media: WireMedia = self
                .message
                .account
                .send_spec_json(spec, &sensitive_values)?;
            Ok(media.into_media())
        })
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub fn download(self, media_sid: &'a str) -> Result<TwilioMediaContent, TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.download",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, false, Method::GET, "message.media.download")?;
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)?;
            Ok(TwilioMediaContent {
                content_type: raw
                    .raw
                    .headers
                    .get(http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                bytes: raw.raw.body,
            })
        })
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(self, request: ListMediaRequest<'a>) -> Result<TwilioMediaPage, TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.message.account.creds, self.message.sid);
            let mut url = self.message.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.message.account.creds.account_sid(),
                "Messages",
                self.message.sid,
                "Media.json",
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "message.media.list",
            );
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent Media page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, is not a Media page for this message, or the HTTP
    /// request/response fails.
    pub fn list_page_uri(self, next_page_uri: &str) -> Result<TwilioMediaPage, TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.list_page_uri",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.message.account.creds.account_sid(),
                self.message.account.creds.auth_token(),
                self.message.sid,
                next_page_uri,
            ];
            let url = self.message.account.client.legacy_page_url(
                next_page_uri,
                self.message.account.creds.account_sid(),
                LegacyPageResource::Media {
                    message_sid: self.message.sid,
                },
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "message.media.list_page_uri",
            );
            let raw = self
                .message
                .account
                .send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// `DELETE /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Media/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub fn delete(self, media_sid: &'a str) -> Result<(), TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.media.delete",
            "DELETE",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values(media_sid);
            let spec = self.media_spec(media_sid, true, Method::DELETE, "message.media.delete")?;
            self.message
                .account
                .send_spec_empty(spec, &sensitive_values)
        })
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMediaPage, TwilioError> {
        let parsed: WireMediaPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let resource = LegacyPageResource::Media {
                message_sid: self.message.sid,
            };
            let next_url = self.message.account.client.legacy_page_url(
                next_page_uri,
                self.message.account.creds.account_sid(),
                resource,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(current_url, &next_url, resource)?;
            }
        }
        Ok(page)
    }

    fn sensitive_values(self, media_sid: &'a str) -> Vec<&'a str> {
        vec![
            self.message.account.creds.account_sid(),
            self.message.account.creds.auth_token(),
            self.message.sid,
            media_sid,
        ]
    }

    fn media_url(self, media_sid: &str, json: bool) -> Result<Url, TwilioError> {
        let media_segment = if json {
            format!("{media_sid}.json")
        } else {
            media_sid.to_owned()
        };
        self.message.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.message.account.creds.account_sid(),
            "Messages",
            self.message.sid,
            "Media",
            &media_segment,
        ])
    }

    fn media_spec(
        self,
        media_sid: &str,
        json: bool,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.media_url(media_sid, json)?,
            operation,
        ))
    }

    /// Lazily list all Media records using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, TwilioMediaPage, TwilioMedia> {
        self.list_all_with(ListMediaRequest::new())
    }

    /// Lazily list all Media records using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListMediaRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioMediaPage, TwilioMedia> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_uri(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_media_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessageFeedbackResource<'a> {
    message: BlockingMessageResource<'a>,
}

#[cfg(feature = "sync")]
impl BlockingMessageFeedbackResource<'_> {
    /// `POST /2010-04-01/Accounts/{AccountSid}/Messages/{MessageSid}/Feedback.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn create(
        self,
        request: CreateMessageFeedbackRequest,
    ) -> Result<TwilioMessageFeedback, TwilioError> {
        request_span(
            &self.message.account.client.config.rest_base_url,
            "message.feedback.create",
            "POST",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.message.account.creds.account_sid(),
                self.message.account.creds.auth_token(),
                self.message.sid,
            ];
            let url = self.message.account.client.rest_endpoint(&[
                "2010-04-01",
                "Accounts",
                self.message.account.creds.account_sid(),
                "Messages",
                self.message.sid,
                "Feedback.json",
            ])?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::POST,
                url,
                "message.feedback.create",
            )
            .form_params(request.form_params());
            let feedback: WireMessageFeedback = self
                .message
                .account
                .send_spec_json(spec, &sensitive_values)?;
            Ok(feedback.into_feedback())
        })
    }
}
