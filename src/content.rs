#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::collections::BTreeMap;
use std::fmt;

use http::Method;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
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
    ApiFamily, DEFAULT_PAGE_SIZE, REDACTED, RequestSpec, TwilioError, V1PageMeta, WireV1PageMeta,
    decode_json_response, validate_content_v1_next_page_continuation,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator, request_span};

/// A `twilio/text` template payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentText {
    pub body: String,
}

impl ContentText {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

impl fmt::Debug for ContentText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentText")
            .field("body", &REDACTED)
            .finish()
    }
}

/// A `twilio/media` template payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentMedia {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub media: Vec<String>,
}

impl ContentMedia {
    #[must_use]
    pub fn new(media: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            body: None,
            media: media.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

impl fmt::Debug for ContentMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentMedia")
            .field("body", &self.body.as_ref().map(|_| REDACTED))
            .field("media", &format_args!("[{REDACTED}; {}]", self.media.len()))
            .finish()
    }
}

/// An action used by quick-reply and card templates.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentAction {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub action_type: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
}

impl ContentAction {
    #[must_use]
    pub fn quick_reply(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            action_type: Some("QUICK_REPLY".to_owned()),
            title: title.into(),
            id: Some(id.into()),
            url: None,
            phone: None,
        }
    }

    #[must_use]
    pub fn url(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            action_type: Some("URL".to_owned()),
            title: title.into(),
            id: None,
            url: Some(url.into()),
            phone: None,
        }
    }

    #[must_use]
    pub fn phone(title: impl Into<String>, phone: impl Into<String>) -> Self {
        Self {
            action_type: Some("PHONE_NUMBER".to_owned()),
            title: title.into(),
            id: None,
            url: None,
            phone: Some(phone.into()),
        }
    }
}

impl fmt::Debug for ContentAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentAction")
            .field("action_type", &self.action_type)
            .field("title", &REDACTED)
            .field("id", &self.id.as_ref().map(|_| REDACTED))
            .field("url", &self.url.as_ref().map(|_| REDACTED))
            .field("phone", &self.phone.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// A `twilio/quick-reply` template payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentQuickReply {
    pub body: String,
    pub actions: Vec<ContentAction>,
}

impl ContentQuickReply {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            actions: Vec::new(),
        }
    }

    #[must_use]
    pub fn action(mut self, action: ContentAction) -> Self {
        self.actions.push(action);
        self
    }
}

impl fmt::Debug for ContentQuickReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentQuickReply")
            .field("body", &REDACTED)
            .field(
                "actions",
                &format_args!("[{REDACTED}; {}]", self.actions.len()),
            )
            .finish()
    }
}

/// A `twilio/card` template payload.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ContentCard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ContentAction>,
}

impl ContentCard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }
    #[must_use]
    pub fn subtitle(mut self, value: impl Into<String>) -> Self {
        self.subtitle = Some(value.into());
        self
    }
    #[must_use]
    pub fn body(mut self, value: impl Into<String>) -> Self {
        self.body = Some(value.into());
        self
    }
    #[must_use]
    pub fn media(mut self, value: impl Into<String>) -> Self {
        self.media.push(value.into());
        self
    }
    #[must_use]
    pub fn action(mut self, value: ContentAction) -> Self {
        self.actions.push(value);
        self
    }
}

impl fmt::Debug for ContentCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentCard")
            .field("title", &self.title.as_ref().map(|_| REDACTED))
            .field("subtitle", &self.subtitle.as_ref().map(|_| REDACTED))
            .field("body", &self.body.as_ref().map(|_| REDACTED))
            .field("media", &format_args!("[{REDACTED}; {}]", self.media.len()))
            .field(
                "actions",
                &format_args!("[{REDACTED}; {}]", self.actions.len()),
            )
            .finish()
    }
}

/// Request-side collection of Content template type payloads.
#[derive(Clone, Default)]
pub struct ContentTypes<'a> {
    text: Option<ContentText>,
    media: Option<ContentMedia>,
    quick_reply: Option<ContentQuickReply>,
    card: Option<ContentCard>,
    custom: BTreeMap<String, &'a Value>,
}

impl<'a> ContentTypes<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn text(mut self, value: ContentText) -> Self {
        self.text = Some(value);
        self
    }
    #[must_use]
    pub fn media(mut self, value: ContentMedia) -> Self {
        self.media = Some(value);
        self
    }
    #[must_use]
    pub fn quick_reply(mut self, value: ContentQuickReply) -> Self {
        self.quick_reply = Some(value);
        self
    }
    #[must_use]
    pub fn card(mut self, value: ContentCard) -> Self {
        self.card = Some(value);
        self
    }

    /// Add an unmodeled Content type.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for an empty, duplicate, or built-in type name.
    pub fn custom(
        mut self,
        name: impl Into<String>,
        value: &'a Value,
    ) -> Result<Self, TwilioError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "content type name must not be empty".to_owned(),
            ));
        }
        if matches!(
            name.as_str(),
            "twilio/text" | "twilio/media" | "twilio/quick-reply" | "twilio/card"
        ) || self.custom.contains_key(&name)
        {
            return Err(TwilioError::InvalidRequest(
                "content type name is duplicated".to_owned(),
            ));
        }
        self.custom.insert(name, value);
        Ok(self)
    }

    fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.media.is_none()
            && self.quick_reply.is_none()
            && self.card.is_none()
            && self.custom.is_empty()
    }
}

impl Serialize for ContentTypes<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap as _;
        let mut map = serializer.serialize_map(None)?;
        if let Some(value) = &self.text {
            map.serialize_entry("twilio/text", value)?;
        }
        if let Some(value) = &self.media {
            map.serialize_entry("twilio/media", value)?;
        }
        if let Some(value) = &self.quick_reply {
            map.serialize_entry("twilio/quick-reply", value)?;
        }
        if let Some(value) = &self.card {
            map.serialize_entry("twilio/card", value)?;
        }
        for (name, value) in &self.custom {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl fmt::Debug for ContentTypes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentTypes")
            .field("types", &REDACTED)
            .finish()
    }
}

/// Lossless response-side Content type map.
#[derive(Clone, Default)]
pub struct TwilioContentTypes(pub BTreeMap<String, Value>);

impl TwilioContentTypes {
    #[must_use]
    pub fn raw(&self) -> &BTreeMap<String, Value> {
        &self.0
    }
    /// Decode the `twilio/text` entry.
    ///
    /// # Errors
    /// Returns an error when the entry has an incompatible shape.
    pub fn text(&self) -> Result<Option<ContentText>, serde_json::Error> {
        decode_type(&self.0, "twilio/text")
    }
    /// Decode the `twilio/media` entry.
    ///
    /// # Errors
    /// Returns an error when the entry has an incompatible shape.
    pub fn media(&self) -> Result<Option<ContentMedia>, serde_json::Error> {
        decode_type(&self.0, "twilio/media")
    }
    /// Decode the `twilio/quick-reply` entry.
    ///
    /// # Errors
    /// Returns an error when the entry has an incompatible shape.
    pub fn quick_reply(&self) -> Result<Option<ContentQuickReply>, serde_json::Error> {
        decode_type(&self.0, "twilio/quick-reply")
    }
    /// Decode the `twilio/card` entry.
    ///
    /// # Errors
    /// Returns an error when the entry has an incompatible shape.
    pub fn card(&self) -> Result<Option<ContentCard>, serde_json::Error> {
        decode_type(&self.0, "twilio/card")
    }
}

fn decode_type<T: serde::de::DeserializeOwned>(
    types: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<T>, serde_json::Error> {
    types
        .get(key)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
}

impl fmt::Debug for TwilioContentTypes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TwilioContentTypes")
            .field(&format_args!("{REDACTED}; {} types", self.0.len()))
            .finish()
    }
}

/// Request to create a Content template.
#[derive(Clone, Serialize)]
pub struct CreateContentRequest<'a> {
    friendly_name: &'a str,
    language: &'a str,
    types: ContentTypes<'a>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    variables: BTreeMap<&'a str, &'a str>,
}

impl<'a> CreateContentRequest<'a> {
    #[must_use]
    pub fn new(friendly_name: &'a str, language: &'a str, types: ContentTypes<'a>) -> Self {
        Self {
            friendly_name,
            language,
            types,
            variables: BTreeMap::new(),
        }
    }
    #[must_use]
    pub fn variable(mut self, key: &'a str, value: &'a str) -> Self {
        self.variables.insert(key, value);
        self
    }
    #[must_use]
    pub fn variables(mut self, values: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        self.variables.extend(values);
        self
    }
    fn validate(&self) -> Result<(), TwilioError> {
        validate_nonempty("FriendlyName", self.friendly_name)?;
        validate_nonempty("Language", self.language)?;
        if self.types.is_empty() {
            return Err(TwilioError::InvalidRequest(
                "Types must not be empty".to_owned(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for CreateContentRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateContentRequest")
            .field("friendly_name", &REDACTED)
            .field("language", &self.language)
            .field("types", &REDACTED)
            .field("variables", &REDACTED)
            .finish()
    }
}

/// Request to update a Content template.
#[derive(Clone, Serialize, Default)]
pub struct UpdateContentRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    friendly_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<ContentTypes<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variables: Option<BTreeMap<&'a str, &'a str>>,
}

impl<'a> UpdateContentRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn friendly_name(mut self, value: &'a str) -> Self {
        self.friendly_name = Some(value);
        self
    }
    #[must_use]
    pub fn types(mut self, value: ContentTypes<'a>) -> Self {
        self.types = Some(value);
        self
    }
    #[must_use]
    pub fn variables(mut self, values: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        self.variables = Some(values.into_iter().collect());
        self
    }
    fn validate(&self) -> Result<(), TwilioError> {
        if let Some(name) = self.friendly_name {
            validate_nonempty("FriendlyName", name)?;
        }
        if self.types.as_ref().is_some_and(ContentTypes::is_empty) {
            return Err(TwilioError::InvalidRequest(
                "Types must not be empty".to_owned(),
            ));
        }
        if self.friendly_name.is_none() && self.types.is_none() && self.variables.is_none() {
            return Err(TwilioError::InvalidRequest(
                "update must contain at least one field".to_owned(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for UpdateContentRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateContentRequest")
            .field("friendly_name", &self.friendly_name.map(|_| REDACTED))
            .field("types", &self.types.as_ref().map(|_| REDACTED))
            .field("variables", &self.variables.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// Request to list Content templates.
#[derive(Clone, Copy, Default)]
pub struct ListContentRequest<'a> {
    page_size: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListContentRequest<'a> {
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
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        if self.page_size.is_some_and(|v| !(1..=500).contains(&v)) {
            return Err(TwilioError::InvalidRequest(
                "PageSize must be between 1 and 500".to_owned(),
            ));
        }
        if self.page_token.is_some_and(|v| v.trim().is_empty()) {
            return Err(TwilioError::InvalidRequest(
                "PageToken must not be empty".to_owned(),
            ));
        }
        Ok(())
    }
    fn pairs(self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        if let Some(v) = self.page_size {
            p.push(("PageSize".to_owned(), v.to_string()));
        }
        if let Some(v) = self.page_token {
            p.push(("PageToken".to_owned(), v.to_owned()));
        }
        p
    }
}

impl fmt::Debug for ListContentRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListContentRequest")
            .field("page_size", &self.page_size)
            .field("page_token", &self.page_token.map(|_| REDACTED))
            .finish()
    }
}

/// Options for deleting a Content template.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeleteContentRequest {
    delete_in_waba: Option<bool>,
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

/// `WhatsApp` template category.
#[derive(Clone, Copy, Debug)]
pub enum WhatsAppTemplateCategory {
    Utility,
    Marketing,
    Authentication,
}
impl Serialize for WhatsAppTemplateCategory {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(match self {
            Self::Utility => "UTILITY",
            Self::Marketing => "MARKETING",
            Self::Authentication => "AUTHENTICATION",
        })
    }
}

/// Request to submit a template for `WhatsApp` approval.
#[derive(Clone, Serialize)]
pub struct SubmitWhatsAppApprovalRequest<'a> {
    name: &'a str,
    category: WhatsAppTemplateCategory,
}
impl<'a> SubmitWhatsAppApprovalRequest<'a> {
    #[must_use]
    pub fn new(name: &'a str, category: WhatsAppTemplateCategory) -> Self {
        Self { name, category }
    }
    fn validate(&self) -> Result<(), TwilioError> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(TwilioError::InvalidRequest("WhatsApp approval name must contain only lowercase ASCII letters, digits, and underscores".to_owned()));
        }
        Ok(())
    }
}
impl fmt::Debug for SubmitWhatsAppApprovalRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubmitWhatsAppApprovalRequest")
            .field("name", &REDACTED)
            .field("category", &self.category)
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
    pub date_created: Option<String>,
    pub date_updated: Option<String>,
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
            .finish()
    }
}

#[derive(Deserialize)]
struct WireContent {
    sid: Option<String>,
    account_sid: Option<String>,
    friendly_name: Option<String>,
    language: Option<String>,
    variables: Option<BTreeMap<String, String>>,
    types: Option<BTreeMap<String, Value>>,
    url: Option<String>,
    links: Option<BTreeMap<String, String>>,
    date_created: Option<String>,
    date_updated: Option<String>,
}
impl WireContent {
    fn into_content(self) -> TwilioContent {
        TwilioContent {
            sid: self.sid,
            account_sid: self.account_sid,
            friendly_name: self.friendly_name,
            language: self.language,
            variables: self.variables.unwrap_or_default(),
            types: TwilioContentTypes(self.types.unwrap_or_default()),
            url: self.url,
            links: self.links.unwrap_or_default(),
            date_created: self.date_created,
            date_updated: self.date_updated,
        }
    }
}

/// One page of Content templates.
#[derive(Clone)]
pub struct TwilioContentPage {
    pub contents: Vec<TwilioContent>,
    pub meta: V1PageMeta,
}
impl fmt::Debug for TwilioContentPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentPage")
            .field(
                "contents",
                &format_args!("[{REDACTED}; {}]", self.contents.len()),
            )
            .field("meta", &self.meta)
            .finish()
    }
}
#[derive(Deserialize)]
struct WireContentPage {
    #[serde(default)]
    contents: Vec<WireContent>,
    #[serde(default)]
    meta: WireV1PageMeta,
}
impl WireContentPage {
    fn into_page(self) -> TwilioContentPage {
        TwilioContentPage {
            contents: self
                .contents
                .into_iter()
                .map(WireContent::into_content)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

/// Response from a `WhatsApp` approval submission.
#[derive(Clone)]
pub struct TwilioWhatsAppApprovalSubmission {
    pub category: Option<String>,
    pub status: Option<String>,
    pub rejection_reason: Option<String>,
    pub name: Option<String>,
    pub content_type: Option<String>,
    pub extra: BTreeMap<String, Value>,
}
impl fmt::Debug for TwilioWhatsAppApprovalSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioWhatsAppApprovalSubmission")
            .field("approval", &REDACTED)
            .finish()
    }
}

/// Fetched approval status for a Content template.
#[derive(Clone)]
pub struct TwilioContentApprovalStatus {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub whatsapp: Option<TwilioWhatsAppApprovalSubmission>,
    pub url: Option<String>,
    pub extra: BTreeMap<String, Value>,
}
impl fmt::Debug for TwilioContentApprovalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioContentApprovalStatus")
            .field("approval", &REDACTED)
            .finish()
    }
}

#[derive(Deserialize)]
struct WireApproval {
    category: Option<String>,
    status: Option<String>,
    rejection_reason: Option<String>,
    name: Option<String>,
    content_type: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
impl WireApproval {
    fn into_submission(self) -> TwilioWhatsAppApprovalSubmission {
        TwilioWhatsAppApprovalSubmission {
            category: self.category,
            status: self.status,
            rejection_reason: self.rejection_reason,
            name: self.name,
            content_type: self.content_type,
            extra: self.extra,
        }
    }
}
#[derive(Deserialize)]
struct WireApprovalStatus {
    sid: Option<String>,
    account_sid: Option<String>,
    whatsapp: Option<WireApproval>,
    url: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn validate_nonempty(name: &str, value: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )))
    } else if name == "ContentSid"
        && !(value.len() == 34
            && value.starts_with("HX")
            && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Err(TwilioError::InvalidRequest(
            "ContentSid must be an HX SID".to_owned(),
        ))
    } else {
        Ok(())
    }
}
fn sensitive<'a>(account: &'a crate::TwilioAuth, sid: Option<&'a str>) -> Vec<&'a str> {
    let mut v = vec![account.account_sid(), account.auth_secret()];
    if let Some(sid) = sid {
        v.push(sid);
    }
    v
}

macro_rules! content_resources {
    ($account:ty, $root:ident, $v1:ident, $contents:ident, $content:ident, $approvals:ident $(, $async:tt)?) => {
        #[derive(Clone, Copy)]
        pub struct $root<'a> {
            account: $account,
        }
        impl<'a> $root<'a> {
            pub(crate) fn new(account: $account) -> Self {
                Self { account }
            }
            #[must_use]
            pub fn v1(self) -> $v1<'a> {
                $v1 {
                    account: self.account,
                }
            }
        }
        #[derive(Clone, Copy)]
        pub struct $v1<'a> {
            account: $account,
        }
        impl<'a> $v1<'a> {
            #[must_use]
            pub fn contents(self) -> $contents<'a> {
                $contents {
                    account: self.account,
                }
            }
            #[must_use]
            pub fn content(self, sid: &'a str) -> $content<'a> {
                $content {
                    account: self.account,
                    sid,
                }
            }
        }
        #[derive(Clone, Copy)]
        pub struct $contents<'a> {
            account: $account,
        }
        #[derive(Clone, Copy)]
        pub struct $content<'a> {
            account: $account,
            sid: &'a str,
        }
        #[derive(Clone, Copy)]
        pub struct $approvals<'a> {
            account: $account,
            sid: &'a str,
        }
    };
}

#[cfg(feature = "async")]
content_resources!(
    TwilioAccount<'a>,
    ContentResource,
    ContentV1Resource,
    ContentsResource,
    ContentTemplateResource,
    ContentApprovalRequestsResource,
    async
);
#[cfg(feature = "sync")]
content_resources!(
    BlockingTwilioAccount<'a>,
    BlockingContentResource,
    BlockingContentV1Resource,
    BlockingContentsResource,
    BlockingContentTemplateResource,
    BlockingContentApprovalRequestsResource
);

#[cfg(feature = "async")]
impl<'a> ContentsResource<'a> {
    /// Create a Content template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn create(
        self,
        request: CreateContentRequest<'a>,
    ) -> Result<TwilioContent, TwilioError> {
        async move {
            request.validate()?;
            let values = sensitive(self.account.creds, None);
            let spec = RequestSpec::new(ApiFamily::ContentV1, Method::POST, ["Content"])
                .operation("content.v1.contents.create")
                .json_body(&request)?;
            let wire: WireContent = self.account.send_spec_json(spec, &values).await?;
            Ok(wire.into_content())
        }
        .instrument(request_span(
            &self.account.client.config.content,
            "content.v1.contents.create",
            "POST",
        ))
        .await
    }
    /// List Content templates.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, decode, or pagination failures.
    pub async fn list(
        self,
        request: ListContentRequest<'a>,
    ) -> Result<TwilioContentPage, TwilioError> {
        request.validate()?;
        let values = sensitive(self.account.creds, request.page_token);
        let mut current = self.account.client.content_endpoint(&["Content"])?;
        for (k, v) in request.pairs() {
            current.query_pairs_mut().append_pair(&k, &v);
        }
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content"])
            .operation("content.v1.contents.list")
            .query_pairs(request.pairs());
        let raw = self.account.send_spec_raw(spec, &values).await?;
        self.read_page(&raw.output, &values, Some(&current))
    }
    /// Follow a validated Content continuation URL.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for invalid metadata or request failures.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioContentPage, TwilioError> {
        let mut values = sensitive(self.account.creds, None);
        values.push(next_page_url);
        let url = self.account.client.content_page_url(next_page_url)?;
        let spec = RequestSpec::from_url(
            ApiFamily::ContentV1,
            Method::GET,
            url.clone(),
            "content.v1.contents.list_page_url",
        );
        let raw = self.account.send_spec_raw(spec, &values).await?;
        self.read_page(&raw.output, &values, Some(&url))
    }
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioContentPage, TwilioContent> {
        let request = ListContentRequest::new().page_size(DEFAULT_PAGE_SIZE);
        TwilioPaginator::new(
            move |next| -> PageFuture<'a, TwilioContentPage> {
                Box::pin(async move {
                    match next {
                        Some(url) => self.list_page_url(&url).await,
                        None => self.list(request).await,
                    }
                })
            },
            |page| (page.contents, page.meta.next_page_url),
        )
    }
    fn read_page(
        self,
        raw: &crate::RawResponse,
        values: &[&str],
        current: Option<&Url>,
    ) -> Result<TwilioContentPage, TwilioError> {
        let page = decode_json_response::<WireContentPage>(raw, values)?.into_page();
        if page.meta.key.as_deref().is_some_and(|k| k != "contents") {
            return Err(TwilioError::InvalidResponseMetadata(
                "pagination metadata key is not contents".to_owned(),
            ));
        }
        if let Some(next) = page.meta.next_page_url.as_deref() {
            let next_url = self.account.client.content_page_url(next)?;
            if let Some(current) = current {
                validate_content_v1_next_page_continuation(current, &next_url)?;
            }
        }
        Ok(page)
    }
}

#[cfg(feature = "async")]
impl<'a> ContentTemplateResource<'a> {
    #[must_use]
    pub fn approval_requests(self) -> ContentApprovalRequestsResource<'a> {
        ContentApprovalRequestsResource {
            account: self.account,
            sid: self.sid,
        }
    }
    /// Fetch the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn fetch(self) -> Result<TwilioContent, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content", self.sid])
            .operation("content.v1.content.fetch");
        let wire: WireContent = self.account.send_spec_json(spec, &values).await?;
        Ok(wire.into_content())
    }
    /// Update the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn update(
        self,
        request: UpdateContentRequest<'a>,
    ) -> Result<TwilioContent, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        request.validate()?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::PUT, ["Content", self.sid])
            .operation("content.v1.content.update")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &values).await?;
        Ok(wire.into_content())
    }
    /// Delete the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, or API failures.
    pub async fn delete(self, request: DeleteContentRequest) -> Result<(), TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let mut spec =
            RequestSpec::new(ApiFamily::ContentV1, Method::DELETE, ["Content", self.sid])
                .operation("content.v1.content.delete");
        if let Some(value) = request.delete_in_waba {
            spec = spec.query("deleteInWaba", value.to_string());
        }
        self.account.send_spec_empty(spec, &values).await
    }
}

#[cfg(feature = "async")]
impl<'a> ContentApprovalRequestsResource<'a> {
    /// Submit the template for `WhatsApp` approval.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn submit_whatsapp(
        self,
        request: SubmitWhatsAppApprovalRequest<'a>,
    ) -> Result<TwilioWhatsAppApprovalSubmission, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        request.validate()?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::POST,
            ["Content", self.sid, "ApprovalRequests", "whatsapp"],
        )
        .operation("content.v1.approvals.submit_whatsapp")
        .json_body(&request)?;
        let wire: WireApproval = self.account.send_spec_json(spec, &values).await?;
        Ok(wire.into_submission())
    }
    /// Fetch the template approval status.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn fetch(self) -> Result<TwilioContentApprovalStatus, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::GET,
            ["Content", self.sid, "ApprovalRequests"],
        )
        .operation("content.v1.approvals.fetch");
        let wire: WireApprovalStatus = self.account.send_spec_json(spec, &values).await?;
        Ok(TwilioContentApprovalStatus {
            sid: wire.sid,
            account_sid: wire.account_sid,
            whatsapp: wire.whatsapp.map(WireApproval::into_submission),
            url: wire.url,
            extra: wire.extra,
        })
    }
}

#[cfg(feature = "sync")]
impl<'a> BlockingContentsResource<'a> {
    /// Create a Content template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn create(self, request: CreateContentRequest<'a>) -> Result<TwilioContent, TwilioError> {
        request.validate()?;
        let values = sensitive(self.account.creds, None);
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::POST, ["Content"])
            .operation("content.v1.contents.create")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &values)?;
        Ok(wire.into_content())
    }
    /// List Content templates.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, decode, or pagination failures.
    pub fn list(self, request: ListContentRequest<'a>) -> Result<TwilioContentPage, TwilioError> {
        request.validate()?;
        let values = sensitive(self.account.creds, request.page_token);
        let mut current = self.account.client.content_endpoint(&["Content"])?;
        for (k, v) in request.pairs() {
            current.query_pairs_mut().append_pair(&k, &v);
        }
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content"])
            .operation("content.v1.contents.list")
            .query_pairs(request.pairs());
        let raw = self.account.send_spec_raw(spec, &values)?;
        self.read_page(&raw.output, &values, Some(&current))
    }
    /// Follow a validated Content continuation URL.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for invalid metadata or request failures.
    pub fn list_page_url(self, next_page_url: &str) -> Result<TwilioContentPage, TwilioError> {
        let mut values = sensitive(self.account.creds, None);
        values.push(next_page_url);
        let url = self.account.client.content_page_url(next_page_url)?;
        let spec = RequestSpec::from_url(
            ApiFamily::ContentV1,
            Method::GET,
            url.clone(),
            "content.v1.contents.list_page_url",
        );
        let raw = self.account.send_spec_raw(spec, &values)?;
        self.read_page(&raw.output, &values, Some(&url))
    }
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, TwilioContentPage, TwilioContent> {
        let request = ListContentRequest::new().page_size(DEFAULT_PAGE_SIZE);
        BlockingTwilioPaginator::new(
            move |next| match next {
                Some(url) => self.list_page_url(&url),
                None => self.list(request),
            },
            |page| (page.contents, page.meta.next_page_url),
        )
    }
    fn read_page(
        self,
        raw: &crate::RawResponse,
        values: &[&str],
        current: Option<&Url>,
    ) -> Result<TwilioContentPage, TwilioError> {
        let page = decode_json_response::<WireContentPage>(raw, values)?.into_page();
        if page.meta.key.as_deref().is_some_and(|k| k != "contents") {
            return Err(TwilioError::InvalidResponseMetadata(
                "pagination metadata key is not contents".to_owned(),
            ));
        }
        if let Some(next) = page.meta.next_page_url.as_deref() {
            let next_url = self.account.client.content_page_url(next)?;
            if let Some(current) = current {
                validate_content_v1_next_page_continuation(current, &next_url)?;
            }
        }
        Ok(page)
    }
}

#[cfg(feature = "sync")]
impl<'a> BlockingContentTemplateResource<'a> {
    #[must_use]
    pub fn approval_requests(self) -> BlockingContentApprovalRequestsResource<'a> {
        BlockingContentApprovalRequestsResource {
            account: self.account,
            sid: self.sid,
        }
    }
    /// Fetch the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn fetch(self) -> Result<TwilioContent, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content", self.sid])
            .operation("content.v1.content.fetch");
        let wire: WireContent = self.account.send_spec_json(spec, &values)?;
        Ok(wire.into_content())
    }
    /// Update the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn update(self, request: UpdateContentRequest<'a>) -> Result<TwilioContent, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        request.validate()?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::PUT, ["Content", self.sid])
            .operation("content.v1.content.update")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &values)?;
        Ok(wire.into_content())
    }
    /// Delete the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, or API failures.
    pub fn delete(self, request: DeleteContentRequest) -> Result<(), TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let mut spec =
            RequestSpec::new(ApiFamily::ContentV1, Method::DELETE, ["Content", self.sid])
                .operation("content.v1.content.delete");
        if let Some(value) = request.delete_in_waba {
            spec = spec.query("deleteInWaba", value.to_string());
        }
        self.account.send_spec_empty(spec, &values)
    }
}

#[cfg(feature = "sync")]
impl<'a> BlockingContentApprovalRequestsResource<'a> {
    /// Submit the template for `WhatsApp` approval.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn submit_whatsapp(
        self,
        request: SubmitWhatsAppApprovalRequest<'a>,
    ) -> Result<TwilioWhatsAppApprovalSubmission, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        request.validate()?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::POST,
            ["Content", self.sid, "ApprovalRequests", "whatsapp"],
        )
        .operation("content.v1.approvals.submit_whatsapp")
        .json_body(&request)?;
        let wire: WireApproval = self.account.send_spec_json(spec, &values)?;
        Ok(wire.into_submission())
    }
    /// Fetch the template approval status.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn fetch(self) -> Result<TwilioContentApprovalStatus, TwilioError> {
        validate_nonempty("ContentSid", self.sid)?;
        let values = sensitive(self.account.creds, Some(self.sid));
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::GET,
            ["Content", self.sid, "ApprovalRequests"],
        )
        .operation("content.v1.approvals.fetch");
        let wire: WireApprovalStatus = self.account.send_spec_json(spec, &values)?;
        Ok(TwilioContentApprovalStatus {
            sid: wire.sid,
            account_sid: wire.account_sid,
            whatsapp: wire.whatsapp.map(WireApproval::into_submission),
            url: wire.url,
            extra: wire.extra,
        })
    }
}
