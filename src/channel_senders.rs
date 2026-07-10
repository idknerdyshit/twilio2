#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::fmt;

use http::Method;
use serde::{Deserialize, Serialize, Serializer};
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
    ApiFamily, DEFAULT_PAGE_SIZE, MessagingV2PageResource, RequestSpec, TwilioAuth, TwilioError,
    V1PageMeta, WireV1PageMeta, decode_json_response, non_empty, push_sensitive, redacted_option,
    validate_messaging_v2_meta_key, validate_messaging_v2_next_page_continuation,
    validate_page_size,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator, request_span};

/// Messaging channel filter for v2 standalone senders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagingV2Channel {
    Whatsapp,
    Rcs,
}

impl MessagingV2Channel {
    fn query_value(self) -> &'static str {
        match self {
            Self::Whatsapp => "whatsapp",
            Self::Rcs => "rcs",
        }
    }
}

/// HTTP method values accepted by standalone channel sender webhook fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelSenderHttpMethod {
    Post,
    Put,
}

impl Serialize for ChannelSenderHttpMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Post => "POST",
            Self::Put => "PUT",
        })
    }
}

#[derive(Clone, Serialize, Default)]
pub struct ChannelSenderConfiguration<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    waba_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_method: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_application_sid: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_type: Option<&'a str>,
}

impl<'a> ChannelSenderConfiguration<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn waba_id(mut self, value: &'a str) -> Self {
        self.waba_id = Some(value);
        self
    }

    #[must_use]
    pub fn verification_method(mut self, value: &'a str) -> Self {
        self.verification_method = Some(value);
        self
    }

    #[must_use]
    pub fn verification_code(mut self, value: &'a str) -> Self {
        self.verification_code = Some(value);
        self
    }

    #[must_use]
    pub fn voice_application_sid(mut self, value: &'a str) -> Self {
        self.voice_application_sid = Some(value);
        self
    }

    #[must_use]
    pub fn account_type(mut self, value: &'a str) -> Self {
        self.account_type = Some(value);
        self
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.waba_id);
        push_sensitive(&mut values, self.verification_code);
        push_sensitive(&mut values, self.voice_application_sid);
        values
    }
}

impl fmt::Debug for ChannelSenderConfiguration<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderConfiguration")
            .field("waba_id", &self.waba_id.map(|_| crate::common::REDACTED))
            .field("verification_method", &self.verification_method)
            .field(
                "verification_code",
                &self.verification_code.map(|_| crate::common::REDACTED),
            )
            .field(
                "voice_application_sid",
                &self.voice_application_sid.map(|_| crate::common::REDACTED),
            )
            .field("account_type", &self.account_type)
            .finish()
    }
}

#[derive(Clone, Serialize, Default)]
pub struct ChannelSenderWebhook<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_method: Option<ChannelSenderHttpMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_method: Option<ChannelSenderHttpMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_callback_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_callback_method: Option<ChannelSenderHttpMethod>,
}

impl<'a> ChannelSenderWebhook<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn callback_url(mut self, value: &'a str) -> Self {
        self.callback_url = Some(value);
        self
    }

    #[must_use]
    pub fn callback_method(mut self, value: ChannelSenderHttpMethod) -> Self {
        self.callback_method = Some(value);
        self
    }

    #[must_use]
    pub fn fallback_url(mut self, value: &'a str) -> Self {
        self.fallback_url = Some(value);
        self
    }

    #[must_use]
    pub fn fallback_method(mut self, value: ChannelSenderHttpMethod) -> Self {
        self.fallback_method = Some(value);
        self
    }

    #[must_use]
    pub fn status_callback_url(mut self, value: &'a str) -> Self {
        self.status_callback_url = Some(value);
        self
    }

    #[must_use]
    pub fn status_callback_method(mut self, value: ChannelSenderHttpMethod) -> Self {
        self.status_callback_method = Some(value);
        self
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.callback_url);
        push_sensitive(&mut values, self.fallback_url);
        push_sensitive(&mut values, self.status_callback_url);
        values
    }
}

impl fmt::Debug for ChannelSenderWebhook<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderWebhook")
            .field(
                "callback_url",
                &self.callback_url.map(|_| crate::common::REDACTED),
            )
            .field("callback_method", &self.callback_method)
            .field(
                "fallback_url",
                &self.fallback_url.map(|_| crate::common::REDACTED),
            )
            .field("fallback_method", &self.fallback_method)
            .field(
                "status_callback_url",
                &self.status_callback_url.map(|_| crate::common::REDACTED),
            )
            .field("status_callback_method", &self.status_callback_method)
            .finish()
    }
}

#[derive(Clone, Serialize)]
pub struct ChannelSenderProfileEmail<'a> {
    email: &'a str,
    label: &'a str,
}

impl<'a> ChannelSenderProfileEmail<'a> {
    #[must_use]
    pub fn new(email: &'a str, label: &'a str) -> Self {
        Self { email, label }
    }
}

impl fmt::Debug for ChannelSenderProfileEmail<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderProfileEmail")
            .field("email", &crate::common::REDACTED)
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone, Serialize)]
pub struct ChannelSenderProfileWebsite<'a> {
    website: &'a str,
    label: &'a str,
}

impl<'a> ChannelSenderProfileWebsite<'a> {
    #[must_use]
    pub fn new(website: &'a str, label: &'a str) -> Self {
        Self { website, label }
    }
}

impl fmt::Debug for ChannelSenderProfileWebsite<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderProfileWebsite")
            .field("website", &crate::common::REDACTED)
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone, Serialize)]
pub struct ChannelSenderProfilePhoneNumber<'a> {
    phone_number: &'a str,
    label: &'a str,
}

impl<'a> ChannelSenderProfilePhoneNumber<'a> {
    #[must_use]
    pub fn new(phone_number: &'a str, label: &'a str) -> Self {
        Self {
            phone_number,
            label,
        }
    }
}

impl fmt::Debug for ChannelSenderProfilePhoneNumber<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderProfilePhoneNumber")
            .field("phone_number", &crate::common::REDACTED)
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone, Serialize, Default)]
pub struct ChannelSenderProfile<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    emails: Vec<ChannelSenderProfileEmail<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logo_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vertical: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    websites: Vec<ChannelSenderProfileWebsite<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    banner_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    privacy_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terms_of_service_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accent_color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_case: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    phone_numbers: Vec<ChannelSenderProfilePhoneNumber<'a>>,
}

impl<'a> ChannelSenderProfile<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn name(mut self, value: &'a str) -> Self {
        self.name = Some(value);
        self
    }

    #[must_use]
    pub fn about(mut self, value: &'a str) -> Self {
        self.about = Some(value);
        self
    }

    #[must_use]
    pub fn address(mut self, value: &'a str) -> Self {
        self.address = Some(value);
        self
    }

    #[must_use]
    pub fn description(mut self, value: &'a str) -> Self {
        self.description = Some(value);
        self
    }

    #[must_use]
    pub fn email(mut self, email: ChannelSenderProfileEmail<'a>) -> Self {
        self.emails.push(email);
        self
    }

    #[must_use]
    pub fn logo_url(mut self, value: &'a str) -> Self {
        self.logo_url = Some(value);
        self
    }

    #[must_use]
    pub fn vertical(mut self, value: &'a str) -> Self {
        self.vertical = Some(value);
        self
    }

    #[must_use]
    pub fn website(mut self, website: ChannelSenderProfileWebsite<'a>) -> Self {
        self.websites.push(website);
        self
    }

    #[must_use]
    pub fn banner_url(mut self, value: &'a str) -> Self {
        self.banner_url = Some(value);
        self
    }

    #[must_use]
    pub fn privacy_url(mut self, value: &'a str) -> Self {
        self.privacy_url = Some(value);
        self
    }

    #[must_use]
    pub fn terms_of_service_url(mut self, value: &'a str) -> Self {
        self.terms_of_service_url = Some(value);
        self
    }

    #[must_use]
    pub fn accent_color(mut self, value: &'a str) -> Self {
        self.accent_color = Some(value);
        self
    }

    #[must_use]
    pub fn use_case(mut self, value: &'a str) -> Self {
        self.use_case = Some(value);
        self
    }

    #[must_use]
    pub fn phone_number(mut self, value: ChannelSenderProfilePhoneNumber<'a>) -> Self {
        self.phone_numbers.push(value);
        self
    }

    fn sensitive_values(&self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.name);
        push_sensitive(&mut values, self.about);
        push_sensitive(&mut values, self.address);
        push_sensitive(&mut values, self.description);
        for email in &self.emails {
            values.push(email.email);
        }
        push_sensitive(&mut values, self.logo_url);
        for website in &self.websites {
            values.push(website.website);
        }
        push_sensitive(&mut values, self.banner_url);
        push_sensitive(&mut values, self.privacy_url);
        push_sensitive(&mut values, self.terms_of_service_url);
        for phone_number in &self.phone_numbers {
            values.push(phone_number.phone_number);
        }
        values
    }

    fn has_non_empty_name(&self) -> bool {
        self.name.is_some_and(|name| !name.trim().is_empty())
    }
}

impl fmt::Debug for ChannelSenderProfile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSenderProfile")
            .field("name", &self.name.map(|_| crate::common::REDACTED))
            .field("about", &self.about.map(|_| crate::common::REDACTED))
            .field("address", &self.address.map(|_| crate::common::REDACTED))
            .field(
                "description",
                &self.description.map(|_| crate::common::REDACTED),
            )
            .field("emails", &format_args!("[{}]", self.emails.len()))
            .field("logo_url", &self.logo_url.map(|_| crate::common::REDACTED))
            .field("vertical", &self.vertical)
            .field("websites", &format_args!("[{}]", self.websites.len()))
            .field(
                "banner_url",
                &self.banner_url.map(|_| crate::common::REDACTED),
            )
            .field(
                "privacy_url",
                &self.privacy_url.map(|_| crate::common::REDACTED),
            )
            .field(
                "terms_of_service_url",
                &self.terms_of_service_url.map(|_| crate::common::REDACTED),
            )
            .field("accent_color", &self.accent_color)
            .field("use_case", &self.use_case)
            .field(
                "phone_numbers",
                &format_args!("[{}]", self.phone_numbers.len()),
            )
            .finish()
    }
}

/// JSON body for `POST /v2/Channels/Senders`.
#[derive(Clone, Serialize)]
pub struct CreateMessagingV2ChannelSenderRequest<'a> {
    sender_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    configuration: Option<ChannelSenderConfiguration<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook: Option<ChannelSenderWebhook<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<ChannelSenderProfile<'a>>,
}

impl<'a> CreateMessagingV2ChannelSenderRequest<'a> {
    #[must_use]
    pub fn new(sender_id: &'a str) -> Self {
        Self {
            sender_id,
            configuration: None,
            webhook: None,
            profile: None,
        }
    }

    #[must_use]
    pub fn configuration(mut self, value: ChannelSenderConfiguration<'a>) -> Self {
        self.configuration = Some(value);
        self
    }

    #[must_use]
    pub fn webhook(mut self, value: ChannelSenderWebhook<'a>) -> Self {
        self.webhook = Some(value);
        self
    }

    #[must_use]
    pub fn profile(mut self, value: ChannelSenderProfile<'a>) -> Self {
        self.profile = Some(value);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        validate_required("sender_id", self.sender_id)?;
        if self
            .sender_id
            .get(.."whatsapp:".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("whatsapp:"))
            && !self
                .profile
                .as_ref()
                .is_some_and(ChannelSenderProfile::has_non_empty_name)
        {
            return Err(TwilioError::InvalidRequest(
                "profile.name is required for WhatsApp senders".to_owned(),
            ));
        }
        Ok(())
    }

    fn sensitive_values(&self) -> Vec<&'a str> {
        let mut values = vec![self.sender_id];
        if let Some(configuration) = self.configuration.clone() {
            values.extend(configuration.sensitive_values());
        }
        if let Some(webhook) = self.webhook.clone() {
            values.extend(webhook.sensitive_values());
        }
        if let Some(profile) = &self.profile {
            values.extend(profile.sensitive_values());
        }
        values
    }
}

impl fmt::Debug for CreateMessagingV2ChannelSenderRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateMessagingV2ChannelSenderRequest")
            .field("sender_id", &crate::common::REDACTED)
            .field("configuration", &self.configuration)
            .field("webhook", &self.webhook)
            .field("profile", &self.profile)
            .finish()
    }
}

/// JSON body for `POST /v2/Channels/Senders/{Sid}`.
#[derive(Clone, Serialize, Default)]
pub struct UpdateMessagingV2ChannelSenderRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    configuration: Option<ChannelSenderConfiguration<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook: Option<ChannelSenderWebhook<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<ChannelSenderProfile<'a>>,
}

impl<'a> UpdateMessagingV2ChannelSenderRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn configuration(mut self, value: ChannelSenderConfiguration<'a>) -> Self {
        self.configuration = Some(value);
        self
    }

    #[must_use]
    pub fn webhook(mut self, value: ChannelSenderWebhook<'a>) -> Self {
        self.webhook = Some(value);
        self
    }

    #[must_use]
    pub fn profile(mut self, value: ChannelSenderProfile<'a>) -> Self {
        self.profile = Some(value);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.configuration.is_none() && self.webhook.is_none() && self.profile.is_none() {
            return Err(TwilioError::InvalidRequest(
                "at least one channel sender field must be set".to_owned(),
            ));
        }
        Ok(())
    }

    fn sensitive_values(&self) -> Vec<&'a str> {
        let mut values = Vec::new();
        if let Some(configuration) = self.configuration.clone() {
            values.extend(configuration.sensitive_values());
        }
        if let Some(webhook) = self.webhook.clone() {
            values.extend(webhook.sensitive_values());
        }
        if let Some(profile) = &self.profile {
            values.extend(profile.sensitive_values());
        }
        values
    }
}

impl fmt::Debug for UpdateMessagingV2ChannelSenderRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateMessagingV2ChannelSenderRequest")
            .field("configuration", &self.configuration)
            .field("webhook", &self.webhook)
            .field("profile", &self.profile)
            .finish()
    }
}

#[derive(Clone, Copy)]
pub struct ListMessagingV2ChannelSendersRequest<'a> {
    channel: MessagingV2Channel,
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListMessagingV2ChannelSendersRequest<'a> {
    #[must_use]
    pub fn new(channel: MessagingV2Channel) -> Self {
        Self {
            channel,
            page_size: None,
            page: None,
            page_token: None,
        }
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

    fn validate(self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)?;
        if self.page_token.is_some_and(|value| value.trim().is_empty()) {
            return Err(TwilioError::InvalidRequest(
                "PageToken must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    fn apply_query(self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        query.append_pair("Channel", self.channel.query_value());
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

    fn query_pairs(self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        pairs.push(("Channel".to_owned(), self.channel.query_value().to_owned()));
        if let Some(value) = self.page_size {
            pairs.push(("PageSize".to_owned(), value.to_string()));
        }
        if let Some(value) = self.page {
            pairs.push(("Page".to_owned(), value.to_string()));
        }
        if let Some(value) = self.page_token {
            pairs.push(("PageToken".to_owned(), value.to_owned()));
        }
        pairs
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.page_token);
        values
    }
}

impl fmt::Debug for ListMessagingV2ChannelSendersRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListMessagingV2ChannelSendersRequest")
            .field("channel", &self.channel)
            .field("page_size", &self.page_size)
            .field("page", &self.page)
            .field(
                "page_token",
                &self.page_token.map(|_| crate::common::REDACTED),
            )
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioMessagingV2ChannelSender {
    pub sid: Option<String>,
    pub sender_id: Option<String>,
    pub status: Option<String>,
    pub configuration: Option<TwilioChannelSenderConfiguration>,
    pub webhook: Option<TwilioChannelSenderWebhook>,
    pub profile: Option<TwilioChannelSenderProfile>,
    pub compliance: Option<TwilioChannelSenderCompliance>,
    pub properties: Option<TwilioChannelSenderProperties>,
    pub offline_reasons: Option<Vec<TwilioChannelSenderOfflineReason>>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioMessagingV2ChannelSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioMessagingV2ChannelSender")
            .field("sid", &redacted_option(&self.sid))
            .field("sender_id", &redacted_option(&self.sender_id))
            .field("status", &self.status)
            .field("configuration", &self.configuration)
            .field("webhook", &self.webhook)
            .field("profile", &self.profile)
            .field("compliance", &self.compliance)
            .field("properties", &self.properties)
            .field("offline_reasons", &self.offline_reasons)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderConfiguration {
    pub waba_id: Option<String>,
    pub verification_method: Option<String>,
    pub verification_code: Option<String>,
    pub voice_application_sid: Option<String>,
    pub account_type: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderConfiguration")
            .field("waba_id", &redacted_option(&self.waba_id))
            .field("verification_method", &self.verification_method)
            .field(
                "verification_code",
                &redacted_option(&self.verification_code),
            )
            .field(
                "voice_application_sid",
                &redacted_option(&self.voice_application_sid),
            )
            .field("account_type", &self.account_type)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderWebhook {
    pub callback_url: Option<String>,
    pub callback_method: Option<String>,
    pub fallback_url: Option<String>,
    pub fallback_method: Option<String>,
    pub status_callback_url: Option<String>,
    pub status_callback_method: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderWebhook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderWebhook")
            .field("callback_url", &redacted_option(&self.callback_url))
            .field("callback_method", &self.callback_method)
            .field("fallback_url", &redacted_option(&self.fallback_url))
            .field("fallback_method", &self.fallback_method)
            .field(
                "status_callback_url",
                &redacted_option(&self.status_callback_url),
            )
            .field("status_callback_method", &self.status_callback_method)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderProfile {
    pub name: Option<String>,
    pub about: Option<String>,
    pub address: Option<String>,
    pub description: Option<String>,
    pub emails: Option<Vec<TwilioChannelSenderProfileEmail>>,
    pub logo_url: Option<String>,
    pub vertical: Option<String>,
    pub websites: Option<Vec<TwilioChannelSenderProfileWebsite>>,
    pub banner_url: Option<String>,
    pub privacy_url: Option<String>,
    pub terms_of_service_url: Option<String>,
    pub accent_color: Option<String>,
    pub use_case: Option<String>,
    pub phone_numbers: Option<Vec<TwilioChannelSenderProfilePhoneNumber>>,
}

impl fmt::Debug for TwilioChannelSenderProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderProfile")
            .field("name", &redacted_option(&self.name))
            .field("about", &redacted_option(&self.about))
            .field("address", &redacted_option(&self.address))
            .field("description", &redacted_option(&self.description))
            .field("emails", &self.emails.as_ref().map(Vec::len))
            .field("logo_url", &redacted_option(&self.logo_url))
            .field("vertical", &self.vertical)
            .field("websites", &self.websites.as_ref().map(Vec::len))
            .field("banner_url", &redacted_option(&self.banner_url))
            .field("privacy_url", &redacted_option(&self.privacy_url))
            .field(
                "terms_of_service_url",
                &redacted_option(&self.terms_of_service_url),
            )
            .field("accent_color", &self.accent_color)
            .field("use_case", &self.use_case)
            .field("phone_numbers", &self.phone_numbers.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderProfileEmail {
    pub email: Option<String>,
    pub label: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderProfileEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderProfileEmail")
            .field("email", &redacted_option(&self.email))
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderProfileWebsite {
    pub website: Option<String>,
    pub label: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderProfileWebsite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderProfileWebsite")
            .field("website", &redacted_option(&self.website))
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderProfilePhoneNumber {
    pub phone_number: Option<String>,
    pub label: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderProfilePhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderProfilePhoneNumber")
            .field("phone_number", &redacted_option(&self.phone_number))
            .field("label", &self.label)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioChannelSenderProperties {
    pub quality_rating: Option<String>,
    pub messaging_limit: Option<String>,
}

#[derive(Clone)]
pub struct TwilioChannelSenderCompliance {
    pub registration_sid: Option<String>,
    pub countries: Option<Vec<TwilioChannelSenderComplianceCountry>>,
}

impl fmt::Debug for TwilioChannelSenderCompliance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderCompliance")
            .field("registration_sid", &redacted_option(&self.registration_sid))
            .field("countries", &self.countries.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderComplianceCountry {
    pub country: Option<String>,
    pub registration_sid: Option<String>,
    pub status: Option<String>,
    pub carriers: Option<Vec<TwilioChannelSenderComplianceCarrier>>,
}

impl fmt::Debug for TwilioChannelSenderComplianceCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderComplianceCountry")
            .field("country", &self.country)
            .field("registration_sid", &redacted_option(&self.registration_sid))
            .field("status", &self.status)
            .field("carriers", &self.carriers.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderComplianceCarrier {
    pub name: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderComplianceCarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderComplianceCarrier")
            .field("name", &self.name)
            .field("status", &self.status)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioChannelSenderOfflineReason {
    pub code: Option<String>,
    pub message: Option<String>,
    pub more_info: Option<String>,
}

impl fmt::Debug for TwilioChannelSenderOfflineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioChannelSenderOfflineReason")
            .field("code", &self.code)
            .field("message", &redacted_option(&self.message))
            .field("more_info", &redacted_option(&self.more_info))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioMessagingV2ChannelSenderPage {
    pub senders: Vec<TwilioMessagingV2ChannelSender>,
    pub meta: V1PageMeta,
}

#[derive(Deserialize)]
struct WireChannelSender {
    sid: Option<String>,
    sender_id: Option<String>,
    status: Option<String>,
    configuration: Option<WireChannelSenderConfiguration>,
    webhook: Option<WireChannelSenderWebhook>,
    profile: Option<WireChannelSenderProfile>,
    compliance: Option<WireChannelSenderCompliance>,
    properties: Option<WireChannelSenderProperties>,
    offline_reasons: Option<Vec<WireChannelSenderOfflineReason>>,
    url: Option<String>,
}

impl WireChannelSender {
    fn into_sender(self) -> TwilioMessagingV2ChannelSender {
        TwilioMessagingV2ChannelSender {
            sid: non_empty(self.sid),
            sender_id: non_empty(self.sender_id),
            status: non_empty(self.status),
            configuration: self
                .configuration
                .map(WireChannelSenderConfiguration::into_config),
            webhook: self.webhook.map(WireChannelSenderWebhook::into_webhook),
            profile: self.profile.map(WireChannelSenderProfile::into_profile),
            compliance: self
                .compliance
                .map(WireChannelSenderCompliance::into_compliance),
            properties: self
                .properties
                .map(WireChannelSenderProperties::into_properties),
            offline_reasons: self.offline_reasons.map(|reasons| {
                reasons
                    .into_iter()
                    .map(WireChannelSenderOfflineReason::into_reason)
                    .collect()
            }),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderConfiguration {
    waba_id: Option<String>,
    verification_method: Option<String>,
    verification_code: Option<String>,
    voice_application_sid: Option<String>,
    account_type: Option<String>,
}

impl WireChannelSenderConfiguration {
    fn into_config(self) -> TwilioChannelSenderConfiguration {
        TwilioChannelSenderConfiguration {
            waba_id: non_empty(self.waba_id),
            verification_method: non_empty(self.verification_method),
            verification_code: non_empty(self.verification_code),
            voice_application_sid: non_empty(self.voice_application_sid),
            account_type: non_empty(self.account_type),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderWebhook {
    callback_url: Option<String>,
    callback_method: Option<String>,
    fallback_url: Option<String>,
    fallback_method: Option<String>,
    status_callback_url: Option<String>,
    status_callback_method: Option<String>,
}

impl WireChannelSenderWebhook {
    fn into_webhook(self) -> TwilioChannelSenderWebhook {
        TwilioChannelSenderWebhook {
            callback_url: non_empty(self.callback_url),
            callback_method: non_empty(self.callback_method),
            fallback_url: non_empty(self.fallback_url),
            fallback_method: non_empty(self.fallback_method),
            status_callback_url: non_empty(self.status_callback_url),
            status_callback_method: non_empty(self.status_callback_method),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderProfile {
    name: Option<String>,
    about: Option<String>,
    address: Option<String>,
    description: Option<String>,
    emails: Option<Vec<WireChannelSenderProfileEmail>>,
    logo_url: Option<String>,
    vertical: Option<String>,
    websites: Option<Vec<WireChannelSenderProfileWebsite>>,
    banner_url: Option<String>,
    privacy_url: Option<String>,
    terms_of_service_url: Option<String>,
    accent_color: Option<String>,
    use_case: Option<String>,
    phone_numbers: Option<Vec<WireChannelSenderProfilePhoneNumber>>,
}

impl WireChannelSenderProfile {
    fn into_profile(self) -> TwilioChannelSenderProfile {
        TwilioChannelSenderProfile {
            name: non_empty(self.name),
            about: non_empty(self.about),
            address: non_empty(self.address),
            description: non_empty(self.description),
            emails: self.emails.map(|items| {
                items
                    .into_iter()
                    .map(WireChannelSenderProfileEmail::into_email)
                    .collect()
            }),
            logo_url: non_empty(self.logo_url),
            vertical: non_empty(self.vertical),
            websites: self.websites.map(|items| {
                items
                    .into_iter()
                    .map(WireChannelSenderProfileWebsite::into_website)
                    .collect()
            }),
            banner_url: non_empty(self.banner_url),
            privacy_url: non_empty(self.privacy_url),
            terms_of_service_url: non_empty(self.terms_of_service_url),
            accent_color: non_empty(self.accent_color),
            use_case: non_empty(self.use_case),
            phone_numbers: self.phone_numbers.map(|items| {
                items
                    .into_iter()
                    .map(WireChannelSenderProfilePhoneNumber::into_phone_number)
                    .collect()
            }),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderProfileEmail {
    email: Option<String>,
    label: Option<String>,
}

impl WireChannelSenderProfileEmail {
    fn into_email(self) -> TwilioChannelSenderProfileEmail {
        TwilioChannelSenderProfileEmail {
            email: non_empty(self.email),
            label: non_empty(self.label),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderProfileWebsite {
    website: Option<String>,
    label: Option<String>,
}

impl WireChannelSenderProfileWebsite {
    fn into_website(self) -> TwilioChannelSenderProfileWebsite {
        TwilioChannelSenderProfileWebsite {
            website: non_empty(self.website),
            label: non_empty(self.label),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderProfilePhoneNumber {
    phone_number: Option<String>,
    label: Option<String>,
}

impl WireChannelSenderProfilePhoneNumber {
    fn into_phone_number(self) -> TwilioChannelSenderProfilePhoneNumber {
        TwilioChannelSenderProfilePhoneNumber {
            phone_number: non_empty(self.phone_number),
            label: non_empty(self.label),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderProperties {
    quality_rating: Option<String>,
    messaging_limit: Option<String>,
}

impl WireChannelSenderProperties {
    fn into_properties(self) -> TwilioChannelSenderProperties {
        TwilioChannelSenderProperties {
            quality_rating: non_empty(self.quality_rating),
            messaging_limit: non_empty(self.messaging_limit),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderCompliance {
    registration_sid: Option<String>,
    countries: Option<Vec<WireChannelSenderComplianceCountry>>,
}

impl WireChannelSenderCompliance {
    fn into_compliance(self) -> TwilioChannelSenderCompliance {
        TwilioChannelSenderCompliance {
            registration_sid: non_empty(self.registration_sid),
            countries: self.countries.map(|countries| {
                countries
                    .into_iter()
                    .map(WireChannelSenderComplianceCountry::into_country)
                    .collect()
            }),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderComplianceCountry {
    country: Option<String>,
    registration_sid: Option<String>,
    status: Option<String>,
    carriers: Option<Vec<WireChannelSenderComplianceCarrier>>,
}

impl WireChannelSenderComplianceCountry {
    fn into_country(self) -> TwilioChannelSenderComplianceCountry {
        TwilioChannelSenderComplianceCountry {
            country: non_empty(self.country),
            registration_sid: non_empty(self.registration_sid),
            status: non_empty(self.status),
            carriers: self.carriers.map(|carriers| {
                carriers
                    .into_iter()
                    .map(WireChannelSenderComplianceCarrier::into_carrier)
                    .collect()
            }),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderComplianceCarrier {
    name: Option<String>,
    status: Option<String>,
    url: Option<String>,
}

impl WireChannelSenderComplianceCarrier {
    fn into_carrier(self) -> TwilioChannelSenderComplianceCarrier {
        TwilioChannelSenderComplianceCarrier {
            name: non_empty(self.name),
            status: non_empty(self.status),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderOfflineReason {
    code: Option<String>,
    message: Option<String>,
    more_info: Option<String>,
}

impl WireChannelSenderOfflineReason {
    fn into_reason(self) -> TwilioChannelSenderOfflineReason {
        TwilioChannelSenderOfflineReason {
            code: non_empty(self.code),
            message: non_empty(self.message),
            more_info: non_empty(self.more_info),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSenderPage {
    #[serde(default)]
    senders: Vec<WireChannelSender>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireChannelSenderPage {
    fn into_page(self) -> TwilioMessagingV2ChannelSenderPage {
        TwilioMessagingV2ChannelSenderPage {
            senders: self
                .senders
                .into_iter()
                .map(WireChannelSender::into_sender)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

fn validate_required(name: &str, value: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}

fn sensitive_values<'a>(creds: &'a TwilioAuth, sid: Option<&'a str>) -> Vec<&'a str> {
    let mut values = vec![creds.account_sid(), creds.auth_secret()];
    push_sensitive(&mut values, sid);
    values
}

#[cfg(feature = "async")]
fn validate_async_next_url(
    account: TwilioAccount<'_>,
    next_page_url: Option<&str>,
    current_url: Option<&Url>,
) -> Result<(), TwilioError> {
    let Some(next_page_url) = next_page_url else {
        return Ok(());
    };
    let resource = MessagingV2PageResource::ChannelSenders;
    let next_url = account
        .client
        .messaging_v2_page_url(next_page_url, resource)?;
    if let Some(current_url) = current_url {
        validate_messaging_v2_next_page_continuation(current_url, &next_url, resource)?;
    }
    Ok(())
}

#[cfg(feature = "sync")]
fn validate_blocking_next_url(
    account: BlockingTwilioAccount<'_>,
    next_page_url: Option<&str>,
    current_url: Option<&Url>,
) -> Result<(), TwilioError> {
    let Some(next_page_url) = next_page_url else {
        return Ok(());
    };
    let resource = MessagingV2PageResource::ChannelSenders;
    let next_url = account
        .client
        .messaging_v2_page_url(next_page_url, resource)?;
    if let Some(current_url) = current_url {
        validate_messaging_v2_next_page_continuation(current_url, &next_url, resource)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2ChannelSendersResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2ChannelSendersResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Channels/Senders`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub async fn create(
        self,
        request: CreateMessagingV2ChannelSenderRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        async move {
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account.creds, None);
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV2,
                Method::POST,
                ["Channels", "Senders"],
            )
            .operation("messaging.v2.channel_senders.create")
            .json_body(&request)?;
            let parsed: WireChannelSender =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_sender())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.create",
            "POST",
        ))
        .await
    }

    /// `GET /Channels/Senders`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListMessagingV2ChannelSendersRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        async move {
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account.creds, None);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self
                .account
                .client
                .messaging_v2_endpoint(&["Channels", "Senders"])?;
            request.apply_query(&mut url);
            let spec =
                RequestSpec::new(ApiFamily::MessagingV2, Method::GET, ["Channels", "Senders"])
                    .operation("messaging.v2.channel_senders.list")
                    .query_pairs(request.query_pairs());
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent Senders page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if the page URL leaves the configured Messaging
    /// API base, changes stable filters, or the HTTP request/response fails.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        async move {
            let mut sensitive_values = sensitive_values(self.account.creds, None);
            sensitive_values.push(next_page_url);
            let resource = MessagingV2PageResource::ChannelSenders;
            let url = self
                .account
                .client
                .messaging_v2_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV2,
                Method::GET,
                url.clone(),
                "messaging.v2.channel_senders.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.list_page_url",
            "GET",
        ))
        .await
    }

    /// Lazily list all standalone v2 channel senders using a default page size
    /// of 50.
    #[must_use]
    pub fn list_all(
        self,
        channel: MessagingV2Channel,
    ) -> TwilioPaginator<'a, TwilioMessagingV2ChannelSenderPage, TwilioMessagingV2ChannelSender>
    {
        let request =
            ListMessagingV2ChannelSendersRequest::new(channel).page_size(DEFAULT_PAGE_SIZE);
        TwilioPaginator::new(
            move |next_page_url| -> PageFuture<'a, TwilioMessagingV2ChannelSenderPage> {
                Box::pin(async move {
                    match next_page_url {
                        Some(url) => self.list_page_url(&url).await,
                        None => self.list(request).await,
                    }
                })
            },
            |page| (page.senders, page.meta.next_page_url),
        )
    }

    #[must_use]
    pub fn sender(self, sid: &'a str) -> MessagingV2ChannelSenderResource<'a> {
        MessagingV2ChannelSenderResource {
            account: self.account,
            sid,
        }
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        let parsed: WireChannelSenderPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = MessagingV2PageResource::ChannelSenders;
        validate_messaging_v2_meta_key(&page.meta, resource)?;
        validate_async_next_url(
            self.account,
            page.meta.next_page_url.as_deref(),
            current_url,
        )?;
        Ok(page)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2ChannelSenderResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2ChannelSenderResource<'a> {
    /// `GET /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        async move {
            validate_required("Sid", self.sid)?;
            let sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
            let spec = RequestSpec::new(
                ApiFamily::MessagingV2,
                Method::GET,
                ["Channels", "Senders", self.sid],
            )
            .operation("messaging.v2.channel_senders.fetch");
            let parsed: WireChannelSender =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_sender())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.fetch",
            "GET",
        ))
        .await
    }

    /// `POST /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub async fn update(
        self,
        request: UpdateMessagingV2ChannelSenderRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        async move {
            validate_required("Sid", self.sid)?;
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV2,
                Method::POST,
                ["Channels", "Senders", self.sid],
            )
            .operation("messaging.v2.channel_senders.update")
            .json_body(&request)?;
            let parsed: WireChannelSender =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_sender())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.update",
            "POST",
        ))
        .await
    }

    /// `DELETE /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            validate_required("Sid", self.sid)?;
            let sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
            let spec = RequestSpec::new(
                ApiFamily::MessagingV2,
                Method::DELETE,
                ["Channels", "Senders", self.sid],
            )
            .operation("messaging.v2.channel_senders.delete");
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.channel_senders.delete",
            "DELETE",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2ChannelSendersResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2ChannelSendersResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Channels/Senders`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub fn create(
        self,
        request: CreateMessagingV2ChannelSenderRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        request.validate()?;
        let mut sensitive_values = sensitive_values(self.account.creds, None);
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV2,
            Method::POST,
            ["Channels", "Senders"],
        )
        .operation("messaging.v2.channel_senders.create")
        .json_body(&request)?;
        let parsed: WireChannelSender = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_sender())
    }

    /// `GET /Channels/Senders`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(
        self,
        request: ListMessagingV2ChannelSendersRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        request.validate()?;
        let mut sensitive_values = sensitive_values(self.account.creds, None);
        sensitive_values.extend(request.sensitive_values());
        let mut url = self
            .account
            .client
            .messaging_v2_endpoint(&["Channels", "Senders"])?;
        request.apply_query(&mut url);
        let spec = RequestSpec::new(ApiFamily::MessagingV2, Method::GET, ["Channels", "Senders"])
            .operation("messaging.v2.channel_senders.list")
            .query_pairs(request.query_pairs());
        let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
        self.read_page(&raw.output, &sensitive_values, Some(&url))
    }

    /// Fetch a subsequent Senders page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if the page URL leaves the configured Messaging
    /// API base, changes stable filters, or the HTTP request/response fails.
    pub fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        let mut sensitive_values = sensitive_values(self.account.creds, None);
        sensitive_values.push(next_page_url);
        let resource = MessagingV2PageResource::ChannelSenders;
        let url = self
            .account
            .client
            .messaging_v2_page_url(next_page_url, resource)?;
        let spec = RequestSpec::from_url(
            ApiFamily::MessagingV2,
            Method::GET,
            url.clone(),
            "messaging.v2.channel_senders.list_page_url",
        );
        let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
        self.read_page(&raw.output, &sensitive_values, Some(&url))
    }

    /// Lazily list all standalone v2 channel senders using a default page size
    /// of 50.
    #[must_use]
    pub fn list_all(
        self,
        channel: MessagingV2Channel,
    ) -> BlockingTwilioPaginator<
        'a,
        TwilioMessagingV2ChannelSenderPage,
        TwilioMessagingV2ChannelSender,
    > {
        let request =
            ListMessagingV2ChannelSendersRequest::new(channel).page_size(DEFAULT_PAGE_SIZE);
        BlockingTwilioPaginator::new(
            move |next_page_url| match next_page_url {
                Some(url) => self.list_page_url(&url),
                None => self.list(request),
            },
            |page| (page.senders, page.meta.next_page_url),
        )
    }

    #[must_use]
    pub fn sender(self, sid: &'a str) -> BlockingMessagingV2ChannelSenderResource<'a> {
        BlockingMessagingV2ChannelSenderResource {
            account: self.account,
            sid,
        }
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioMessagingV2ChannelSenderPage, TwilioError> {
        let parsed: WireChannelSenderPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = MessagingV2PageResource::ChannelSenders;
        validate_messaging_v2_meta_key(&page.meta, resource)?;
        validate_blocking_next_url(
            self.account,
            page.meta.next_page_url.as_deref(),
            current_url,
        )?;
        Ok(page)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2ChannelSenderResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2ChannelSenderResource<'a> {
    /// `GET /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        validate_required("Sid", self.sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::MessagingV2,
            Method::GET,
            ["Channels", "Senders", self.sid],
        )
        .operation("messaging.v2.channel_senders.fetch");
        let parsed: WireChannelSender = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_sender())
    }

    /// `POST /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub fn update(
        self,
        request: UpdateMessagingV2ChannelSenderRequest<'a>,
    ) -> Result<TwilioMessagingV2ChannelSender, TwilioError> {
        validate_required("Sid", self.sid)?;
        request.validate()?;
        let mut sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV2,
            Method::POST,
            ["Channels", "Senders", self.sid],
        )
        .operation("messaging.v2.channel_senders.update")
        .json_body(&request)?;
        let parsed: WireChannelSender = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_sender())
    }

    /// `DELETE /Channels/Senders/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub fn delete(self) -> Result<(), TwilioError> {
        validate_required("Sid", self.sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::MessagingV2,
            Method::DELETE,
            ["Channels", "Senders", self.sid],
        )
        .operation("messaging.v2.channel_senders.delete");
        self.account.send_spec_empty(spec, &sensitive_values)
    }
}
