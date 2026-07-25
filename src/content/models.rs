use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::{Iso8601, Rfc2822};

use super::types::ContentTypes;
use crate::common::{REDACTED, TwilioError, V1PageMeta, WireV1PageMeta};

fn invalid(message: impl Into<String>) -> TwilioError {
    TwilioError::InvalidRequest(message.into())
}

fn require_nonempty(field: &str, value: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        Err(invalid(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

fn validate_page_size(value: Option<u32>) -> Result<(), TwilioError> {
    if value.is_some_and(|value| !(1..=1000).contains(&value)) {
        return Err(invalid("PageSize must be between 1 and 1000"));
    }
    Ok(())
}

fn parse_timestamp(value: Option<String>) -> Option<OffsetDateTime> {
    value.and_then(|value| {
        OffsetDateTime::parse(&value, &Iso8601::DEFAULT)
            .or_else(|_| OffsetDateTime::parse(&value, &Rfc2822))
            .ok()
    })
}

fn validate_date(field: &str, value: &str) -> Result<(), TwilioError> {
    if OffsetDateTime::parse(value, &Iso8601::DEFAULT).is_err()
        && OffsetDateTime::parse(value, &Rfc2822).is_err()
    {
        return Err(invalid(format!(
            "{field} must be an ISO-8601 or RFC-2822 timestamp"
        )));
    }
    Ok(())
}

/// Request to create a Content template.
#[derive(Clone, Serialize)]
pub struct CreateContentRequest {
    language: String,
    types: ContentTypes,
    #[serde(skip_serializing_if = "Option::is_none")]
    friendly_name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    variables: BTreeMap<String, String>,
}

impl CreateContentRequest {
    #[must_use]
    pub fn new(language: impl Into<String>, types: ContentTypes) -> Self {
        Self {
            language: language.into(),
            types,
            friendly_name: None,
            variables: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn friendly_name(mut self, value: impl Into<String>) -> Self {
        self.friendly_name = Some(value.into());
        self
    }

    #[must_use]
    pub fn variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn variables(
        mut self,
        values: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.variables.extend(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        require_nonempty("Language", &self.language)?;
        if self
            .friendly_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid("FriendlyName must not be empty"));
        }
        if self.types.raw().is_empty() {
            return Err(invalid("Types must not be empty"));
        }
        if self.variables.keys().any(|key| key.trim().is_empty()) {
            return Err(invalid("variable names must not be empty"));
        }
        self.types.validate()
    }
}

impl fmt::Debug for CreateContentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateContentRequest")
            .field("language", &self.language)
            .field(
                "friendly_name",
                &self.friendly_name.as_ref().map(|_| REDACTED),
            )
            .field("types", &REDACTED)
            .field("variables", &REDACTED)
            .finish()
    }
}

/// Request to replace the mutable fields of a Content template.
///
/// Twilio's schema also declares an optional `language` property, but Twilio
/// documents a template's language as immutable after creation. This request
/// intentionally does not expose that property.
#[derive(Clone, Serialize)]
pub struct UpdateContentRequest {
    types: ContentTypes,
    #[serde(skip_serializing_if = "Option::is_none")]
    friendly_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<BTreeMap<String, String>>,
}

impl UpdateContentRequest {
    #[must_use]
    pub fn new(types: ContentTypes) -> Self {
        Self {
            types,
            friendly_name: None,
            variables: None,
        }
    }

    #[must_use]
    pub fn friendly_name(mut self, value: impl Into<String>) -> Self {
        self.friendly_name = Some(value.into());
        self
    }

    #[must_use]
    pub fn variables(
        mut self,
        values: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.variables = Some(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        );
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        if self.types.raw().is_empty() {
            return Err(invalid("Types must not be empty"));
        }
        if self
            .friendly_name
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid("FriendlyName must not be empty"));
        }
        if self
            .variables
            .as_ref()
            .is_some_and(|variables| variables.keys().any(|key| key.trim().is_empty()))
        {
            return Err(invalid("variable names must not be empty"));
        }
        self.types.validate()
    }
}

impl fmt::Debug for UpdateContentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateContentRequest")
            .field("types", &REDACTED)
            .field(
                "friendly_name",
                &self.friendly_name.as_ref().map(|_| REDACTED),
            )
            .field("variables", &self.variables.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// Common v1 Content list parameters.
#[derive(Clone, Default)]
pub struct ListContentRequest {
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl ListContentRequest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }

    #[must_use]
    pub fn page_token(mut self, value: impl Into<String>) -> Self {
        self.page_token = Some(value.into());
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)?;
        if self
            .page_token
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid("PageToken must not be empty"));
        }
        Ok(())
    }

    pub(crate) fn pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(value) = self.page_size {
            pairs.push(("PageSize".to_owned(), value.to_string()));
        }
        if let Some(value) = &self.page_token {
            pairs.push(("PageToken".to_owned(), value.clone()));
        }
        pairs
    }

    pub(crate) fn with_default_page_size(mut self) -> Self {
        if self.page_size.is_none() {
            self.page_size = Some(50);
        }
        self
    }
}

impl fmt::Debug for ListContentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListContentRequest")
            .field("page_size", &self.page_size)
            .field("page_token", &self.page_token.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// Filters accepted by the Content v2 search endpoints.
#[derive(Clone, Default)]
pub struct ContentSearchRequest {
    languages: Vec<String>,
    content_types: Vec<String>,
    channel_eligibilities: Vec<String>,
    content: Option<String>,
    content_name: Option<String>,
    sort_by_date: Option<String>,
    sort_by_content_name: Option<String>,
    date_created_before: Option<String>,
    date_created_after: Option<String>,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl ContentSearchRequest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn language(mut self, value: impl Into<String>) -> Self {
        self.languages.push(value.into());
        self
    }
    #[must_use]
    pub fn content_type(mut self, value: impl Into<String>) -> Self {
        self.content_types.push(value.into());
        self
    }
    #[must_use]
    pub fn channel_eligibility(mut self, value: impl Into<String>) -> Self {
        self.channel_eligibilities.push(value.into());
        self
    }
    #[must_use]
    pub fn content(mut self, value: impl Into<String>) -> Self {
        self.content = Some(value.into());
        self
    }
    #[must_use]
    pub fn content_name(mut self, value: impl Into<String>) -> Self {
        self.content_name = Some(value.into());
        self
    }
    /// Sort by the date the Content resource was updated.
    ///
    /// Twilio currently accepts a string value but does not publish a closed
    /// set of allowed values.
    #[must_use]
    pub fn sort_by_date(mut self, value: impl Into<String>) -> Self {
        self.sort_by_date = Some(value.into());
        self
    }
    /// Sort by the Content resource name.
    ///
    /// Twilio currently accepts a string value but does not publish a closed
    /// set of allowed values.
    #[must_use]
    pub fn sort_by_content_name(mut self, value: impl Into<String>) -> Self {
        self.sort_by_content_name = Some(value.into());
        self
    }
    #[must_use]
    pub fn date_created_before(mut self, value: impl Into<String>) -> Self {
        self.date_created_before = Some(value.into());
        self
    }
    #[must_use]
    pub fn date_created_after(mut self, value: impl Into<String>) -> Self {
        self.date_created_after = Some(value.into());
        self
    }
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }
    #[must_use]
    pub fn page_token(mut self, value: impl Into<String>) -> Self {
        self.page_token = Some(value.into());
        self
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)?;
        for (field, values) in [
            ("Language", &self.languages),
            ("ContentType", &self.content_types),
            ("ChannelEligibility", &self.channel_eligibilities),
        ] {
            if values.iter().any(|value| value.trim().is_empty()) {
                return Err(invalid(format!("{field} must not be empty")));
            }
        }
        if self.channel_eligibilities.iter().any(|value| {
            value
                .split_once(':')
                .is_none_or(|(channel, status)| channel.is_empty() || status.is_empty())
        }) {
            return Err(invalid(
                "ChannelEligibility must use the channel:status format",
            ));
        }
        for (field, value) in [
            ("Content", self.content.as_deref()),
            ("ContentName", self.content_name.as_deref()),
            ("SortByDate", self.sort_by_date.as_deref()),
            ("SortByContentName", self.sort_by_content_name.as_deref()),
            ("PageToken", self.page_token.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(invalid(format!("{field} must not be empty")));
            }
        }
        if let Some(value) = &self.date_created_before {
            validate_date("DateCreatedBefore", value)?;
        }
        if let Some(value) = &self.date_created_after {
            validate_date("DateCreatedAfter", value)?;
        }
        if self.content.as_ref().is_some_and(|value| {
            value.chars().count() > 1024 || value.split_whitespace().count() > 30
        }) {
            return Err(invalid(
                "Content must not exceed 1024 characters or 30 words",
            ));
        }
        if self.content_name.as_ref().is_some_and(|value| {
            value.chars().count() > 450 || value.split_whitespace().count() > 30
        }) {
            return Err(invalid(
                "ContentName must not exceed 450 characters or 30 words",
            ));
        }
        Ok(())
    }

    pub(crate) fn pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        pairs.extend(
            self.languages
                .iter()
                .cloned()
                .map(|v| ("Language".to_owned(), v)),
        );
        pairs.extend(
            self.content_types
                .iter()
                .cloned()
                .map(|v| ("ContentType".to_owned(), v)),
        );
        pairs.extend(
            self.channel_eligibilities
                .iter()
                .cloned()
                .map(|v| ("ChannelEligibility".to_owned(), v)),
        );
        for (key, value) in [
            ("Content", self.content.as_ref()),
            ("ContentName", self.content_name.as_ref()),
            ("SortByDate", self.sort_by_date.as_ref()),
            ("SortByContentName", self.sort_by_content_name.as_ref()),
            ("DateCreatedBefore", self.date_created_before.as_ref()),
            ("DateCreatedAfter", self.date_created_after.as_ref()),
        ] {
            if let Some(value) = value {
                pairs.push((key.to_owned(), value.clone()));
            }
        }
        if let Some(value) = self.page_size {
            pairs.push(("PageSize".to_owned(), value.to_string()));
        }
        if let Some(value) = &self.page_token {
            pairs.push(("PageToken".to_owned(), value.clone()));
        }
        pairs
    }

    pub(crate) fn with_default_page_size(mut self) -> Self {
        if self.page_size.is_none() {
            self.page_size = Some(50);
        }
        self
    }
}

impl fmt::Debug for ContentSearchRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentSearchRequest")
            .field("languages", &self.languages)
            .field("content_types", &self.content_types)
            .field("channel_eligibilities", &self.channel_eligibilities)
            .field("content", &self.content.as_ref().map(|_| REDACTED))
            .field(
                "content_name",
                &self.content_name.as_ref().map(|_| REDACTED),
            )
            .field("sort_by_date", &self.sort_by_date)
            .field("sort_by_content_name", &self.sort_by_content_name)
            .field("date_created_before", &self.date_created_before)
            .field("date_created_after", &self.date_created_after)
            .field("page_size", &self.page_size)
            .field("page_token", &self.page_token.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// Lossless response-side Content type map.
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct TwilioContentTypes(pub BTreeMap<String, Value>);

impl TwilioContentTypes {
    #[must_use]
    pub fn raw(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    /// Decode a named type without losing unknown types.
    ///
    /// # Errors
    /// Returns an error when the named payload has an incompatible JSON shape.
    pub fn decode<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, serde_json::Error> {
        self.0
            .get(name)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
    }

    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn text(&self) -> Result<Option<super::types::ContentText>, serde_json::Error> {
        self.decode("twilio/text")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn media(&self) -> Result<Option<super::types::ContentMedia>, serde_json::Error> {
        self.decode("twilio/media")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn location(&self) -> Result<Option<super::types::ContentLocation>, serde_json::Error> {
        self.decode("twilio/location")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn list_picker(
        &self,
    ) -> Result<Option<super::types::ContentListPicker>, serde_json::Error> {
        self.decode("twilio/list-picker")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn call_to_action(
        &self,
    ) -> Result<Option<super::types::ContentCallToAction>, serde_json::Error> {
        self.decode("twilio/call-to-action")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn quick_reply(
        &self,
    ) -> Result<Option<super::types::ContentQuickReply>, serde_json::Error> {
        self.decode("twilio/quick-reply")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn card(&self) -> Result<Option<super::types::ContentCard>, serde_json::Error> {
        self.decode("twilio/card")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn carousel(&self) -> Result<Option<super::types::ContentCarousel>, serde_json::Error> {
        self.decode("twilio/carousel")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn catalog(&self) -> Result<Option<super::types::ContentCatalog>, serde_json::Error> {
        self.decode("twilio/catalog")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn pay(&self) -> Result<Option<super::types::ContentPay>, serde_json::Error> {
        self.decode("twilio/pay")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn flows(&self) -> Result<Option<super::types::ContentFlows>, serde_json::Error> {
        self.decode("twilio/flows")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn schedule(&self) -> Result<Option<super::types::ContentSchedule>, serde_json::Error> {
        self.decode("twilio/schedule")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn whatsapp_card(&self) -> Result<Option<super::types::WhatsappCard>, serde_json::Error> {
        self.decode("whatsapp/card")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn whatsapp_authentication(
        &self,
    ) -> Result<Option<super::types::WhatsappAuthentication>, serde_json::Error> {
        self.decode("whatsapp/authentication")
    }
    /// # Errors
    /// Returns an error when the payload has an incompatible shape.
    pub fn whatsapp_flows(&self) -> Result<Option<super::types::WhatsappFlows>, serde_json::Error> {
        self.decode("whatsapp/flows")
    }
}

impl fmt::Debug for TwilioContentTypes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentTypes")
            .field("type_count", &self.0.len())
            .finish()
    }
}

/// A Content template returned by Twilio.
#[derive(Clone)]
pub struct TwilioContent {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub friendly_name: Option<String>,
    pub language: Option<String>,
    pub variables: BTreeMap<String, String>,
    pub types: TwilioContentTypes,
    pub url: Option<String>,
    pub links: BTreeMap<String, String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for TwilioContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContent")
            .field("sid", &self.sid.as_ref().map(|_| REDACTED))
            .field("account_sid", &self.account_sid.as_ref().map(|_| REDACTED))
            .field(
                "friendly_name",
                &self.friendly_name.as_ref().map(|_| REDACTED),
            )
            .field("language", &self.language)
            .field("variables", &REDACTED)
            .field("types", &REDACTED)
            .field("url", &self.url.as_ref().map(|_| REDACTED))
            .field("links", &REDACTED)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("extra", &REDACTED)
            .finish()
    }
}

#[derive(Deserialize)]
pub(crate) struct WireContent {
    sid: Option<String>,
    account_sid: Option<String>,
    friendly_name: Option<String>,
    language: Option<String>,
    variables: Option<BTreeMap<String, String>>,
    types: Option<TwilioContentTypes>,
    url: Option<String>,
    links: Option<BTreeMap<String, String>>,
    date_created: Option<String>,
    date_updated: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl WireContent {
    pub(crate) fn into_content(self) -> TwilioContent {
        TwilioContent {
            sid: self.sid,
            account_sid: self.account_sid,
            friendly_name: self.friendly_name,
            language: self.language,
            variables: self.variables.unwrap_or_default(),
            types: self.types.unwrap_or_default(),
            url: self.url,
            links: self.links.unwrap_or_default(),
            date_created: parse_timestamp(self.date_created),
            date_updated: parse_timestamp(self.date_updated),
            extra: self.extra,
        }
    }
}

macro_rules! content_page {
    ($name:ident, $wire:ident, $field:ident) => {
        #[derive(Clone)]
        pub struct $name {
            pub $field: Vec<TwilioContent>,
            pub meta: V1PageMeta,
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field(
                        stringify!($field),
                        &format_args!("[{REDACTED}; {}]", self.$field.len()),
                    )
                    .field("meta", &self.meta)
                    .finish()
            }
        }
        #[derive(Deserialize)]
        pub(crate) struct $wire {
            #[serde(default)]
            $field: Vec<WireContent>,
            #[serde(default)]
            meta: WireV1PageMeta,
        }
        impl $wire {
            pub(crate) fn into_page(self) -> $name {
                $name {
                    $field: self
                        .$field
                        .into_iter()
                        .map(WireContent::into_content)
                        .collect(),
                    meta: self.meta.into_meta(),
                }
            }
        }
    };
}

content_page!(TwilioContentPage, WireContentPage, contents);

/// `WhatsApp` approval details. String fields are deliberately forward-compatible.
#[derive(Clone, Default, Deserialize)]
pub struct TwilioWhatsAppApproval {
    pub category: Option<String>,
    pub status: Option<String>,
    pub rejection_reason: Option<String>,
    pub name: Option<String>,
    pub content_type: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for TwilioWhatsAppApproval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioWhatsAppApproval")
            .field("approval", &REDACTED)
            .finish()
    }
}

/// Content template with its channel approval data.
#[derive(Clone)]
pub struct TwilioContentAndApprovals {
    pub content: TwilioContent,
    pub approval_requests: BTreeMap<String, TwilioWhatsAppApproval>,
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for TwilioContentAndApprovals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentAndApprovals")
            .field("content", &REDACTED)
            .field("approval_requests", &REDACTED)
            .field("extra", &REDACTED)
            .finish()
    }
}

#[derive(Deserialize)]
pub(crate) struct WireContentAndApprovals {
    #[serde(flatten)]
    content: WireContent,
    #[serde(default)]
    approval_requests: BTreeMap<String, TwilioWhatsAppApproval>,
}

impl WireContentAndApprovals {
    pub(crate) fn into_item(self) -> TwilioContentAndApprovals {
        let mut content = self.content.into_content();
        let extra = std::mem::take(&mut content.extra);
        TwilioContentAndApprovals {
            content,
            approval_requests: self.approval_requests,
            extra,
        }
    }
}

#[derive(Clone)]
pub struct TwilioContentAndApprovalsPage {
    pub contents: Vec<TwilioContentAndApprovals>,
    pub meta: V1PageMeta,
}
impl fmt::Debug for TwilioContentAndApprovalsPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentAndApprovalsPage")
            .field(
                "contents",
                &format_args!("[{REDACTED}; {}]", self.contents.len()),
            )
            .field("meta", &self.meta)
            .finish()
    }
}
#[derive(Deserialize)]
pub(crate) struct WireContentAndApprovalsPage {
    #[serde(default)]
    contents: Vec<WireContentAndApprovals>,
    #[serde(default)]
    meta: WireV1PageMeta,
}
impl WireContentAndApprovalsPage {
    pub(crate) fn into_page(self) -> TwilioContentAndApprovalsPage {
        TwilioContentAndApprovalsPage {
            contents: self
                .contents
                .into_iter()
                .map(WireContentAndApprovals::into_item)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

/// A mapping from a legacy `WhatsApp` template to Content.
#[derive(Clone)]
pub struct TwilioLegacyContent {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub friendly_name: Option<String>,
    pub language: Option<String>,
    pub variables: BTreeMap<String, String>,
    pub types: TwilioContentTypes,
    pub legacy_template_name: Option<String>,
    pub legacy_body: Option<String>,
    pub url: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub extra: BTreeMap<String, Value>,
}
impl fmt::Debug for TwilioLegacyContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLegacyContent")
            .field("legacy_content", &REDACTED)
            .finish()
    }
}

#[derive(Deserialize)]
pub(crate) struct WireLegacyContent {
    sid: Option<String>,
    account_sid: Option<String>,
    friendly_name: Option<String>,
    language: Option<String>,
    variables: Option<BTreeMap<String, String>>,
    types: Option<TwilioContentTypes>,
    legacy_template_name: Option<String>,
    legacy_body: Option<String>,
    url: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl WireLegacyContent {
    pub(crate) fn into_content(self) -> TwilioLegacyContent {
        TwilioLegacyContent {
            sid: self.sid,
            account_sid: self.account_sid,
            friendly_name: self.friendly_name,
            language: self.language,
            variables: self.variables.unwrap_or_default(),
            types: self.types.unwrap_or_default(),
            legacy_template_name: self.legacy_template_name,
            legacy_body: self.legacy_body,
            url: self.url,
            date_created: parse_timestamp(self.date_created),
            date_updated: parse_timestamp(self.date_updated),
            extra: self.extra,
        }
    }
}

#[derive(Clone)]
pub struct TwilioLegacyContentPage {
    pub contents: Vec<TwilioLegacyContent>,
    pub meta: V1PageMeta,
}
impl fmt::Debug for TwilioLegacyContentPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLegacyContentPage")
            .field(
                "contents",
                &format_args!("[{REDACTED}; {}]", self.contents.len()),
            )
            .field("meta", &self.meta)
            .finish()
    }
}
#[derive(Deserialize)]
pub(crate) struct WireLegacyContentPage {
    #[serde(default, alias = "legacy_contents")]
    contents: Vec<WireLegacyContent>,
    #[serde(default)]
    meta: WireV1PageMeta,
}
impl WireLegacyContentPage {
    pub(crate) fn into_page(self) -> TwilioLegacyContentPage {
        TwilioLegacyContentPage {
            contents: self
                .contents
                .into_iter()
                .map(WireLegacyContent::into_content)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

/// `WhatsApp` template category accepted by approval submission.
#[derive(Clone, Copy, Debug)]
pub enum WhatsAppTemplateCategory {
    Utility,
    Marketing,
    Authentication,
}
impl Serialize for WhatsAppTemplateCategory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Utility => "UTILITY",
            Self::Marketing => "MARKETING",
            Self::Authentication => "AUTHENTICATION",
        })
    }
}

#[derive(Clone, Serialize)]
pub struct SubmitWhatsAppApprovalRequest {
    name: String,
    category: WhatsAppTemplateCategory,
}
impl SubmitWhatsAppApprovalRequest {
    #[must_use]
    pub fn new(name: impl Into<String>, category: WhatsAppTemplateCategory) -> Self {
        Self {
            name: name.into(),
            category,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(invalid(
                "WhatsApp approval name must contain only lowercase ASCII letters, digits, and underscores",
            ));
        }
        Ok(())
    }
}
impl fmt::Debug for SubmitWhatsAppApprovalRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubmitWhatsAppApprovalRequest")
            .field("name", &REDACTED)
            .field("category", &self.category)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeleteContentRequest {
    pub(crate) delete_in_waba: Option<bool>,
}
impl DeleteContentRequest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn delete_in_waba(mut self, value: bool) -> Self {
        self.delete_in_waba = Some(value);
        self
    }
}

/// Response from a `WhatsApp` approval submission.
pub type TwilioWhatsAppApprovalSubmission = TwilioWhatsAppApproval;

#[derive(Clone, Default, Deserialize)]
pub struct TwilioContentApprovalStatus {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub whatsapp: Option<TwilioWhatsAppApproval>,
    pub url: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl fmt::Debug for TwilioContentApprovalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentApprovalStatus")
            .field("approval", &REDACTED)
            .finish()
    }
}
