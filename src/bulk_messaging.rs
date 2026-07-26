//! Twilio Bulk Messaging v1 Messages and Operations resources.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use http::Method;
use serde::{Deserialize, Deserializer, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "sync")]
use crate::common::BlockingTwilioPaginator;
#[cfg(feature = "async")]
use crate::common::TwilioPaginator;
use crate::common::{ApiFamily, RequestSpec, TwilioError};

const MAX_PAGE_SIZE: u32 = 1_000;
const MAX_RECIPIENTS: usize = 10_000;
const MAX_TAGS: usize = 10;

fn redacted_debug(name: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct(name).field("data", &"<redacted>").finish()
}

fn non_empty(value: &str, field: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        Err(TwilioError::InvalidRequest(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return false;
    };
    (26..=35).contains(&suffix.len())
        && matches!(suffix.as_bytes().first(), Some(b'0'..=b'7'))
        && suffix.bytes().all(|byte| {
            byte.is_ascii_digit()
                || byte.is_ascii_lowercase() && !matches!(byte, b'i' | b'l' | b'o' | b'u')
        })
}

fn validate_id(value: &str, prefix: &str, field: &str) -> Result<(), TwilioError> {
    if valid_prefixed_id(value, prefix) {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(format!(
            "{field} has an invalid format"
        )))
    }
}

/// A Bulk Messaging channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkMessageChannel {
    Sms,
    Rcs,
    Whatsapp,
}

impl BulkMessageChannel {
    fn wire(self) -> &'static str {
        match self {
            Self::Sms => "SMS",
            Self::Rcs => "RCS",
            Self::Whatsapp => "WHATSAPP",
        }
    }
}

/// The address space used for a recipient.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkMessageRecipientChannel {
    Phone,
    Whatsapp,
}

/// One messaging address offered for a recipient.
#[derive(Clone, Serialize)]
pub struct BulkMessageAddress {
    address: String,
    channel: BulkMessageRecipientChannel,
}

impl BulkMessageAddress {
    /// Construct a recipient messaging address.
    #[must_use]
    pub fn new(address: impl Into<String>, channel: BulkMessageRecipientChannel) -> Self {
        Self {
            address: address.into(),
            channel,
        }
    }
}

impl fmt::Debug for BulkMessageAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageAddress", f)
    }
}

/// Per-recipient personalization variables.
#[derive(Clone, Default, Serialize)]
#[serde(transparent)]
pub struct BulkMessageVariables(BTreeMap<String, String>);

impl BulkMessageVariables {
    /// Construct empty variables.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one personalization value.
    #[must_use]
    pub fn variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        for (key, value) in &self.0 {
            non_empty(key, "recipient variable key")?;
            non_empty(value, "recipient variable value")?;
        }
        Ok(())
    }
}

impl fmt::Debug for BulkMessageVariables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageVariables", f)
    }
}

/// One recipient. Each variant maps to one Twilio schema `oneOf` arm.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageRecipient {
    Addresses {
        addresses: Vec<BulkMessageAddress>,
        #[serde(skip_serializing_if = "Option::is_none")]
        variables: Option<BulkMessageVariables>,
    },
    Profile {
        #[serde(rename = "profileId")]
        profile_id: String,
        #[serde(rename = "memoryStoreId")]
        memory_store_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        variables: Option<BulkMessageVariables>,
    },
}

impl BulkMessageRecipient {
    /// Construct an address recipient.
    #[must_use]
    pub fn addresses(addresses: impl IntoIterator<Item = BulkMessageAddress>) -> Self {
        Self::Addresses {
            addresses: addresses.into_iter().collect(),
            variables: None,
        }
    }

    /// Construct a recipient with one address.
    #[must_use]
    pub fn address(address: impl Into<String>, channel: BulkMessageRecipientChannel) -> Self {
        Self::addresses([BulkMessageAddress::new(address, channel)])
    }

    /// Construct a Twilio Profile recipient.
    #[must_use]
    pub fn profile(profile_id: impl Into<String>, memory_store_id: impl Into<String>) -> Self {
        Self::Profile {
            profile_id: profile_id.into(),
            memory_store_id: memory_store_id.into(),
            variables: None,
        }
    }

    /// Attach personalization variables.
    #[must_use]
    pub fn variables(mut self, value: BulkMessageVariables) -> Self {
        match &mut self {
            Self::Addresses { variables, .. } | Self::Profile { variables, .. } => {
                *variables = Some(value);
            }
        }
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Addresses {
                addresses,
                variables,
            } => {
                if !(1..=10).contains(&addresses.len()) {
                    return Err(TwilioError::InvalidRequest(
                        "recipient addresses must contain between 1 and 10 items".to_owned(),
                    ));
                }
                for address in addresses {
                    non_empty(&address.address, "recipient address")?;
                }
                if let Some(variables) = variables {
                    variables.validate()?;
                }
            }
            Self::Profile {
                profile_id,
                memory_store_id,
                variables,
            } => {
                validate_id(profile_id, "mem_profile_", "profile ID")?;
                validate_id(memory_store_id, "mem_store_", "memory store ID")?;
                if let Some(variables) = variables {
                    variables.validate()?;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for BulkMessageRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageRecipient", f)
    }
}

/// Sender selection for a Bulk Messaging submission.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageSender {
    Address {
        address: String,
        channel: BulkMessageChannel,
    },
    SenderId {
        #[serde(rename = "senderId")]
        sender_id: String,
    },
    SenderPool {
        #[serde(rename = "senderPoolId")]
        sender_pool_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        channels: Option<crate::BulkSenderChannelControls>,
    },
}

impl BulkMessageSender {
    /// Select a sender address and channel.
    #[must_use]
    pub fn address(address: impl Into<String>, channel: BulkMessageChannel) -> Self {
        Self::Address {
            address: address.into(),
            channel,
        }
    }

    /// Select a Sender resource.
    #[must_use]
    pub fn sender_id(sender_id: impl Into<String>) -> Self {
        Self::SenderId {
            sender_id: sender_id.into(),
        }
    }

    /// Select a Sender Pool.
    #[must_use]
    pub fn sender_pool_id(sender_pool_id: impl Into<String>) -> Self {
        Self::SenderPool {
            sender_pool_id: sender_pool_id.into(),
            channels: None,
        }
    }

    /// Apply channel filtering and priority controls to a Sender Pool.
    #[must_use]
    pub fn channels(mut self, value: crate::BulkSenderChannelControls) -> Self {
        if let Self::SenderPool { channels, .. } = &mut self {
            *channels = Some(value);
        }
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Address { address, .. } => non_empty(address, "sender address"),
            Self::SenderId { sender_id } => validate_id(sender_id, "comms_sender_", "sender ID"),
            Self::SenderPool {
                sender_pool_id,
                channels,
            } => {
                validate_id(sender_pool_id, "comms_senderpool_", "sender pool ID")?;
                if let Some(channels) = channels {
                    channels.validate()?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for BulkMessageSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageSender", f)
    }
}

/// A media URL in simple text/media content.
#[derive(Clone, Serialize)]
pub struct BulkMessageMedia {
    url: String,
}

impl BulkMessageMedia {
    /// Construct a media reference.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

impl fmt::Debug for BulkMessageMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageMedia", f)
    }
}

/// A fully typed inline channel module.
///
/// The channel-specific payload is held beneath its mandatory channel key,
/// preventing ambiguous cross-channel unions.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageInlineModule {
    Sms { sms: BulkMessageSmsModule },
    Rcs { rcs: BulkMessageRcsModule },
}

impl BulkMessageInlineModule {
    /// Wrap an SMS module.
    #[must_use]
    pub fn sms(module: BulkMessageSmsModule) -> Self {
        Self::Sms { sms: module }
    }

    /// Wrap an RCS module.
    #[must_use]
    pub fn rcs(module: BulkMessageRcsModule) -> Self {
        Self::Rcs { rcs: module }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Sms { sms } => sms.validate(),
            Self::Rcs { rcs } => rcs.validate(),
        }
    }
}

impl fmt::Debug for BulkMessageInlineModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageInlineModule", f)
    }
}

/// Inline SMS content.
#[derive(Clone, Serialize)]
pub struct BulkMessageSmsModule {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    media: Vec<BulkMessageMedia>,
}

impl BulkMessageSmsModule {
    /// Construct inline SMS text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            media: Vec::new(),
        }
    }

    /// Construct inline MMS media.
    #[must_use]
    pub fn media(media: impl IntoIterator<Item = BulkMessageMedia>) -> Self {
        Self {
            text: None,
            media: media.into_iter().collect(),
        }
    }

    /// Add MMS media to inline SMS text.
    #[must_use]
    pub fn with_media(mut self, media: impl IntoIterator<Item = BulkMessageMedia>) -> Self {
        self.media = media.into_iter().collect();
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.text.is_none() && self.media.is_empty() {
            return Err(TwilioError::InvalidRequest(
                "SMS content must include text or media".to_owned(),
            ));
        }
        if let Some(text) = &self.text {
            non_empty(text, "SMS text")?;
        }
        for media in &self.media {
            non_empty(&media.url, "SMS media URL")?;
        }
        Ok(())
    }
}

impl fmt::Debug for BulkMessageSmsModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageSmsModule", f)
    }
}

/// A typed RCS rich-card or text module.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageRcsModule {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        suggestions: Vec<BulkMessageSuggestion>,
    },
    Media {
        media: BulkMessageRcsMedia,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        suggestions: Vec<BulkMessageSuggestion>,
    },
    RichCard {
        #[serde(rename = "richCard")]
        rich_card: BulkMessageRichCard,
    },
}

impl BulkMessageRcsModule {
    /// Construct RCS text.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            suggestions: Vec::new(),
        }
    }

    /// Construct RCS media.
    #[must_use]
    pub fn media(media: BulkMessageRcsMedia) -> Self {
        Self::Media {
            media,
            suggestions: Vec::new(),
        }
    }

    /// Add an RCS suggestion to text or media content.
    #[must_use]
    pub fn suggestion(mut self, value: BulkMessageSuggestion) -> Self {
        match &mut self {
            Self::Text { suggestions, .. } | Self::Media { suggestions, .. } => {
                suggestions.push(value);
            }
            Self::RichCard { .. } => {}
        }
        self
    }

    /// Construct an RCS rich card.
    #[must_use]
    pub fn rich_card(card: BulkMessageRichCard) -> Self {
        Self::RichCard { rich_card: card }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Text { text, suggestions } => {
                non_empty(text, "RCS text")?;
                validate_rcs_suggestions(suggestions, 11)
            }
            Self::Media { media, suggestions } => {
                media.validate()?;
                validate_rcs_suggestions(suggestions, 11)
            }
            Self::RichCard { rich_card } => rich_card.validate(),
        }
    }
}

/// Inline RCS media.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageRcsMedia {
    content_info: BulkMessageRcsContentInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<BulkMessageRcsMediaHeight>,
}

impl BulkMessageRcsMedia {
    /// Construct RCS media from a public file URL.
    #[must_use]
    pub fn new(file_url: impl Into<String>) -> Self {
        Self {
            content_info: BulkMessageRcsContentInfo {
                file_url: file_url.into(),
                thumbnail_url: None,
                force_refresh: None,
            },
            height: None,
        }
    }

    /// Set a thumbnail URL.
    #[must_use]
    pub fn thumbnail_url(mut self, value: impl Into<String>) -> Self {
        self.content_info.thumbnail_url = Some(value.into());
        self
    }

    /// Force Twilio to refresh cached media.
    #[must_use]
    pub fn force_refresh(mut self, value: bool) -> Self {
        self.content_info.force_refresh = Some(value);
        self
    }

    /// Set the displayed media height.
    #[must_use]
    pub fn height(mut self, value: BulkMessageRcsMediaHeight) -> Self {
        self.height = Some(value);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        non_empty(&self.content_info.file_url, "RCS media file URL")?;
        if let Some(value) = &self.content_info.thumbnail_url {
            non_empty(value, "RCS media thumbnail URL")?;
        }
        Ok(())
    }
}

impl fmt::Debug for BulkMessageRcsMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageRcsMedia", f)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BulkMessageRcsContentInfo {
    file_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    force_refresh: Option<bool>,
}

/// RCS media display height.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkMessageRcsMediaHeight {
    Tall,
    Medium,
    Short,
}

impl fmt::Debug for BulkMessageRcsModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageRcsModule", f)
    }
}

/// An RCS rich card.
#[derive(Clone, Serialize)]
pub struct BulkMessageRichCard {
    #[serde(rename = "standaloneCard")]
    standalone_card: BulkMessageStandaloneCard,
}

impl BulkMessageRichCard {
    /// Construct a rich card.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            standalone_card: BulkMessageStandaloneCard {
                card_orientation: None,
                thumbnail_image_alignment: None,
                card_content: BulkMessageCardContent {
                    title: Some(title.into()),
                    description: None,
                    media: None,
                    suggestions: Vec::new(),
                },
            },
        }
    }

    /// Set the card description.
    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.standalone_card.card_content.description = Some(value.into());
        self
    }

    /// Set card media.
    #[must_use]
    pub fn media(mut self, value: BulkMessageRcsMedia) -> Self {
        self.standalone_card.card_content.media = Some(value);
        self
    }

    /// Add an interaction suggestion.
    #[must_use]
    pub fn suggestion(mut self, value: BulkMessageSuggestion) -> Self {
        self.standalone_card.card_content.suggestions.push(value);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        let content = &self.standalone_card.card_content;
        if let Some(title) = &content.title {
            non_empty(title, "RCS rich card title")?;
        }
        if let Some(description) = &content.description {
            non_empty(description, "RCS rich card description")?;
        }
        if let Some(media) = &content.media {
            media.validate()?;
        }
        validate_rcs_suggestions(&content.suggestions, 4)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BulkMessageStandaloneCard {
    #[serde(skip_serializing_if = "Option::is_none")]
    card_orientation: Option<BulkMessageCardOrientation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail_image_alignment: Option<BulkMessageThumbnailAlignment>,
    card_content: BulkMessageCardContent,
}

#[derive(Clone, Serialize)]
struct BulkMessageCardContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    media: Option<BulkMessageRcsMedia>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: Vec<BulkMessageSuggestion>,
}

/// Standalone RCS card orientation.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkMessageCardOrientation {
    Horizontal,
    Vertical,
}

/// Thumbnail alignment for a horizontal standalone RCS card.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkMessageThumbnailAlignment {
    Left,
    Right,
}

impl fmt::Debug for BulkMessageRichCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageRichCard", f)
    }
}

/// One mutually exclusive rich-message suggestion.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageSuggestion {
    Reply { reply: BulkMessageReply },
    Action { action: BulkMessageSuggestedAction },
}

impl BulkMessageSuggestion {
    /// Construct a suggested reply.
    #[must_use]
    pub fn reply(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Reply {
            reply: BulkMessageReply {
                text: label.into(),
                postback_data: value.into(),
            },
        }
    }

    /// Construct an open-URL action.
    #[must_use]
    pub fn open_url(label: impl Into<String>, url: impl Into<String>) -> Self {
        let url = url.into();
        Self::open_url_action(label, url.clone(), url)
    }

    /// Construct an open-URL action with explicit postback data.
    #[must_use]
    pub fn open_url_action(
        text: impl Into<String>,
        postback_data: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self::Action {
            action: BulkMessageSuggestedAction::OpenUrl {
                text: text.into(),
                postback_data: postback_data.into(),
                open_url_action: BulkMessageOpenUrlAction { url: url.into() },
            },
        }
    }

    /// Construct a dial action.
    #[must_use]
    pub fn dial(label: impl Into<String>, number: impl Into<String>) -> Self {
        let number = number.into();
        Self::Action {
            action: BulkMessageSuggestedAction::Dial {
                text: label.into(),
                postback_data: number.clone(),
                dial_action: BulkMessageDialAction {
                    phone_number: number,
                },
            },
        }
    }

    /// Construct a view-location action.
    #[must_use]
    pub fn view_location(
        text: impl Into<String>,
        postback_data: impl Into<String>,
        action: BulkMessageViewLocationAction,
    ) -> Self {
        Self::Action {
            action: BulkMessageSuggestedAction::ViewLocation {
                text: text.into(),
                postback_data: postback_data.into(),
                view_location_action: action,
            },
        }
    }

    /// Construct a create-calendar-event action.
    #[must_use]
    pub fn create_calendar_event(
        text: impl Into<String>,
        postback_data: impl Into<String>,
        action: BulkMessageCalendarAction,
    ) -> Self {
        Self::Action {
            action: BulkMessageSuggestedAction::CreateCalendarEvent {
                text: text.into(),
                postback_data: postback_data.into(),
                create_calendar_event_action: action,
            },
        }
    }

    /// Construct a share-location action.
    #[must_use]
    pub fn share_location(text: impl Into<String>, postback_data: impl Into<String>) -> Self {
        Self::Action {
            action: BulkMessageSuggestedAction::ShareLocation {
                text: text.into(),
                postback_data: postback_data.into(),
                share_location_action: BulkMessageShareLocationAction {},
            },
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Reply { reply } => {
                non_empty(&reply.text, "reply text")?;
                non_empty(&reply.postback_data, "reply postback data")
            }
            Self::Action { action } => action.validate(),
        }
    }
}

impl fmt::Debug for BulkMessageSuggestion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageSuggestion", f)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageReply {
    text: String,
    postback_data: String,
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageSuggestedAction {
    OpenUrl {
        text: String,
        #[serde(rename = "postbackData")]
        postback_data: String,
        #[serde(rename = "openUrlAction")]
        open_url_action: BulkMessageOpenUrlAction,
    },
    Dial {
        text: String,
        #[serde(rename = "postbackData")]
        postback_data: String,
        #[serde(rename = "dialAction")]
        dial_action: BulkMessageDialAction,
    },
    ViewLocation {
        text: String,
        #[serde(rename = "postbackData")]
        postback_data: String,
        #[serde(rename = "viewLocationAction")]
        view_location_action: BulkMessageViewLocationAction,
    },
    CreateCalendarEvent {
        text: String,
        #[serde(rename = "postbackData")]
        postback_data: String,
        #[serde(rename = "createCalendarEventAction")]
        create_calendar_event_action: BulkMessageCalendarAction,
    },
    ShareLocation {
        text: String,
        #[serde(rename = "postbackData")]
        postback_data: String,
        #[serde(rename = "shareLocationAction")]
        share_location_action: BulkMessageShareLocationAction,
    },
}

impl BulkMessageSuggestedAction {
    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::OpenUrl {
                text,
                postback_data,
                open_url_action,
            } => {
                non_empty(text, "action text")?;
                non_empty(postback_data, "action postback data")?;
                non_empty(&open_url_action.url, "action URL")
            }
            Self::Dial {
                text,
                postback_data,
                dial_action,
            } => {
                non_empty(text, "action text")?;
                non_empty(postback_data, "action postback data")?;
                non_empty(&dial_action.phone_number, "action phone number")
            }
            Self::ViewLocation {
                text,
                postback_data,
                view_location_action,
            } => {
                non_empty(text, "action text")?;
                non_empty(postback_data, "action postback data")?;
                view_location_action.validate()
            }
            Self::CreateCalendarEvent {
                text,
                postback_data,
                create_calendar_event_action,
            } => {
                non_empty(text, "action text")?;
                non_empty(postback_data, "action postback data")?;
                create_calendar_event_action.validate()
            }
            Self::ShareLocation {
                text,
                postback_data,
                ..
            } => {
                non_empty(text, "action text")?;
                non_empty(postback_data, "action postback data")
            }
        }
    }
}

#[derive(Clone, Serialize)]
pub struct BulkMessageOpenUrlAction {
    url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageDialAction {
    phone_number: String,
}

/// Coordinates used by an RCS view-location action.
#[derive(Clone, Copy, Serialize)]
pub struct BulkMessageCoordinates {
    latitude: f64,
    longitude: f64,
}

impl BulkMessageCoordinates {
    /// Construct geographic coordinates.
    #[must_use]
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }
}

/// A view-location action, using coordinates or a search query.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageViewLocationAction {
    #[serde(skip_serializing_if = "Option::is_none")]
    lat_long: Option<BulkMessageCoordinates>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
}

impl BulkMessageViewLocationAction {
    /// Construct a coordinate-based location.
    #[must_use]
    pub fn coordinates(latitude: f64, longitude: f64) -> Self {
        Self {
            lat_long: Some(BulkMessageCoordinates::new(latitude, longitude)),
            label: None,
            query: None,
        }
    }

    /// Construct a location search query.
    #[must_use]
    pub fn query(value: impl Into<String>) -> Self {
        Self {
            lat_long: None,
            label: None,
            query: Some(value.into()),
        }
    }

    /// Set the coordinate pin label.
    #[must_use]
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.lat_long.is_some() == self.query.is_some() {
            return Err(TwilioError::InvalidRequest(
                "view-location action requires exactly one of coordinates or query".to_owned(),
            ));
        }
        if let Some(coordinates) = self.lat_long
            && (!(-90.0..=90.0).contains(&coordinates.latitude)
                || !(-180.0..=180.0).contains(&coordinates.longitude))
        {
            return Err(TwilioError::InvalidRequest(
                "view-location coordinates are out of range".to_owned(),
            ));
        }
        if let Some(query) = &self.query {
            non_empty(query, "view-location query")?;
        }
        Ok(())
    }
}

/// An RCS create-calendar-event action.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageCalendarAction {
    start_time: String,
    end_time: String,
    title: String,
    description: String,
}

impl BulkMessageCalendarAction {
    /// Construct a calendar event.
    #[must_use]
    pub fn new(
        start_time: impl Into<String>,
        end_time: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            start_time: start_time.into(),
            end_time: end_time.into(),
            title: title.into(),
            description: description.into(),
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        let start = OffsetDateTime::parse(&self.start_time, &Rfc3339).map_err(|_| {
            TwilioError::InvalidRequest("calendar start time must be RFC 3339".to_owned())
        })?;
        let end = OffsetDateTime::parse(&self.end_time, &Rfc3339).map_err(|_| {
            TwilioError::InvalidRequest("calendar end time must be RFC 3339".to_owned())
        })?;
        if end <= start {
            return Err(TwilioError::InvalidRequest(
                "calendar end time must be later than start time".to_owned(),
            ));
        }
        non_empty(&self.title, "calendar title")?;
        non_empty(&self.description, "calendar description")
    }
}

/// Empty payload required by the RCS share-location action.
#[derive(Clone, Copy, Default, Serialize)]
pub struct BulkMessageShareLocationAction {}

fn validate_rcs_suggestions(
    suggestions: &[BulkMessageSuggestion],
    maximum: usize,
) -> Result<(), TwilioError> {
    if suggestions.len() > maximum {
        return Err(TwilioError::InvalidRequest(format!(
            "RCS content supports at most {maximum} suggestions"
        )));
    }
    suggestions
        .iter()
        .try_for_each(BulkMessageSuggestion::validate)
}

/// Message content. Variants cannot be combined on the wire.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkMessageContent {
    TextMedia {
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        media: Vec<BulkMessageMedia>,
    },
    ContentId {
        #[serde(rename = "contentId")]
        content_id: String,
    },
    Inline {
        modules: Vec<BulkMessageInlineModule>,
    },
}

impl BulkMessageContent {
    /// Construct text content.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::TextMedia {
            text: Some(text.into()),
            media: Vec::new(),
        }
    }

    /// Construct media content.
    #[must_use]
    pub fn media(media: impl IntoIterator<Item = BulkMessageMedia>) -> Self {
        Self::TextMedia {
            text: None,
            media: media.into_iter().collect(),
        }
    }

    /// Construct text plus media content.
    #[must_use]
    pub fn text_and_media(
        text: impl Into<String>,
        media: impl IntoIterator<Item = BulkMessageMedia>,
    ) -> Self {
        Self::TextMedia {
            text: Some(text.into()),
            media: media.into_iter().collect(),
        }
    }

    /// Select a Content API template SID.
    #[must_use]
    pub fn content_id(value: impl Into<String>) -> Self {
        Self::ContentId {
            content_id: value.into(),
        }
    }

    /// Construct inline channel modules.
    #[must_use]
    pub fn inline(modules: impl IntoIterator<Item = BulkMessageInlineModule>) -> Self {
        Self::Inline {
            modules: modules.into_iter().collect(),
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::TextMedia { text, media } => {
                if text.is_none() && media.is_empty() {
                    return Err(TwilioError::InvalidRequest(
                        "message content must not be empty".to_owned(),
                    ));
                }
                if let Some(text) = text {
                    non_empty(text, "message text")?;
                }
                if media.len() > 10 {
                    return Err(TwilioError::InvalidRequest(
                        "message content supports at most 10 media items".to_owned(),
                    ));
                }
                for item in media {
                    non_empty(&item.url, "media URL")?;
                }
                Ok(())
            }
            Self::ContentId { content_id } => {
                if (28..=36).contains(&content_id.len())
                    && content_id.starts_with("HX")
                    && content_id[2..]
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric())
                {
                    Ok(())
                } else {
                    Err(TwilioError::InvalidRequest(
                        "content SID has an invalid format".to_owned(),
                    ))
                }
            }
            Self::Inline { modules } => {
                if !(1..=4).contains(&modules.len()) {
                    return Err(TwilioError::InvalidRequest(
                        "inline modules must contain between 1 and 4 items".to_owned(),
                    ));
                }
                let mut kinds = BTreeSet::new();
                for module in modules {
                    let kind = match module {
                        BulkMessageInlineModule::Sms { .. } => "SMS",
                        BulkMessageInlineModule::Rcs { .. } => "RCS",
                    };
                    if !kinds.insert(kind) {
                        return Err(TwilioError::InvalidRequest(
                            "inline channel modules must have unique channels".to_owned(),
                        ));
                    }
                    module.validate()?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for BulkMessageContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageContent", f)
    }
}

/// Borrowed request for sending Bulk Messages.
#[derive(Clone, Copy, Serialize)]
pub struct SendBulkMessagesRequest<'a> {
    to: &'a [BulkMessageRecipient],
    content: &'a BulkMessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<&'a BulkMessageSender>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schedule: Option<BulkMessageSchedule<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<&'a BTreeMap<String, String>>,
}

#[derive(Clone, Copy, Serialize)]
struct BulkMessageSchedule<'a> {
    #[serde(rename = "sendAt")]
    send_at: [&'a str; 1],
}

impl<'a> SendBulkMessagesRequest<'a> {
    /// Construct a send request.
    #[must_use]
    pub fn new(to: &'a [BulkMessageRecipient], content: &'a BulkMessageContent) -> Self {
        Self {
            to,
            content,
            from: None,
            schedule: None,
            tags: None,
        }
    }

    /// Select a sender.
    #[must_use]
    pub fn from(mut self, value: &'a BulkMessageSender) -> Self {
        self.from = Some(value);
        self
    }

    /// Schedule the message at one RFC 3339 expression.
    #[must_use]
    pub fn send_at(mut self, value: &'a str) -> Self {
        self.schedule = Some(BulkMessageSchedule { send_at: [value] });
        self
    }

    /// Attach tags.
    #[must_use]
    pub fn tags(mut self, value: &'a BTreeMap<String, String>) -> Self {
        self.tags = Some(value);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        if self.to.is_empty() || self.to.len() > MAX_RECIPIENTS {
            return Err(TwilioError::InvalidRequest(
                "bulk message recipients must contain between 1 and 10000 items".to_owned(),
            ));
        }
        for recipient in self.to {
            recipient.validate()?;
        }
        self.content.validate()?;
        if let Some(from) = self.from {
            from.validate()?;
        }
        if let Some(schedule) = self.schedule {
            OffsetDateTime::parse(schedule.send_at[0], &Rfc3339).map_err(|_| {
                TwilioError::InvalidRequest("schedule must be an RFC 3339 date-time".to_owned())
            })?;
        }
        if let Some(tags) = self.tags {
            validate_tags(tags)?;
        }
        Ok(())
    }
}

impl fmt::Debug for SendBulkMessagesRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("SendBulkMessagesRequest", f)
    }
}

fn validate_tags(tags: &BTreeMap<String, String>) -> Result<(), TwilioError> {
    if tags.len() > MAX_TAGS {
        return Err(TwilioError::InvalidRequest(
            "at most 10 tags are allowed".to_owned(),
        ));
    }
    for (key, value) in tags {
        if key.is_empty()
            || key.len() > 128
            || value.is_empty()
            || value.len() > 256
            || !key.bytes().all(tag_byte)
            || !value.bytes().all(tag_byte)
        {
            return Err(TwilioError::InvalidRequest(
                "tag keys and values must use the documented character set and limits".to_owned(),
            ));
        }
    }
    Ok(())
}

fn tag_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
}

/// Accepted Bulk Messaging submission metadata.
#[derive(Clone)]
pub struct BulkMessageSubmission {
    pub operation_id: String,
    pub operation_location: Option<Url>,
}

impl fmt::Debug for BulkMessageSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageSubmission", f)
    }
}

/// Borrowed filters for listing Bulk Messages.
#[derive(Clone, Copy, Default)]
pub struct ListBulkMessagesRequest<'a> {
    operation_id: Option<&'a str>,
    session_id: Option<&'a str>,
    start_date: Option<&'a str>,
    end_date: Option<&'a str>,
    profile: Option<&'a str>,
    channel: Option<BulkMessageChannel>,
    status: Option<&'a str>,
    tags: Option<&'a str>,
    page_token: Option<&'a str>,
    page_size: Option<u32>,
}

macro_rules! list_builder {
    ($name:ident, $field:ident, $ty:ty) => {
        #[must_use]
        pub fn $name(mut self, value: $ty) -> Self {
            self.$field = Some(value);
            self
        }
    };
}

impl<'a> ListBulkMessagesRequest<'a> {
    /// Construct unfiltered list options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    list_builder!(operation_id, operation_id, &'a str);
    list_builder!(session_id, session_id, &'a str);
    list_builder!(start_date, start_date, &'a str);
    list_builder!(end_date, end_date, &'a str);
    list_builder!(profile, profile, &'a str);
    list_builder!(channel, channel, BulkMessageChannel);
    list_builder!(status, status, &'a str);
    list_builder!(tags, tags, &'a str);
    list_builder!(page_token, page_token, &'a str);
    list_builder!(page_size, page_size, u32);

    fn validate(&self) -> Result<(), TwilioError> {
        if let Some(value) = self.operation_id {
            validate_id(value, "comms_operation_", "operation ID")?;
        }
        if let Some(value) = self.profile {
            validate_id(value, "mem_profile_", "profile ID")?;
        }
        validate_dates(self.start_date, self.end_date)?;
        validate_page_size(self.page_size)
    }

    fn query(&self, token_override: Option<&str>) -> Vec<(String, String)> {
        let mut query = Vec::new();
        push_query(&mut query, "operationId", self.operation_id);
        push_query(&mut query, "sessionId", self.session_id);
        push_query(&mut query, "startDate", self.start_date);
        push_query(&mut query, "endDate", self.end_date);
        push_query(&mut query, "profile", self.profile);
        if let Some(channel) = self.channel {
            query.push(("channel".to_owned(), channel.wire().to_owned()));
        }
        push_query(&mut query, "status", self.status);
        push_query(&mut query, "tags", self.tags);
        push_query(&mut query, "pageToken", token_override.or(self.page_token));
        if let Some(page_size) = self.page_size {
            query.push(("pageSize".to_owned(), page_size.to_string()));
        }
        query
    }
}

impl fmt::Debug for ListBulkMessagesRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("ListBulkMessagesRequest", f)
    }
}

/// Borrowed filters for listing Bulk Message Operations.
#[derive(Clone, Copy, Default)]
pub struct ListBulkMessageOperationsRequest<'a> {
    start_date: Option<&'a str>,
    end_date: Option<&'a str>,
    status: Option<&'a str>,
    page_token: Option<&'a str>,
    page_size: Option<u32>,
}

impl<'a> ListBulkMessageOperationsRequest<'a> {
    /// Construct unfiltered operation list options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    list_builder!(start_date, start_date, &'a str);
    list_builder!(end_date, end_date, &'a str);
    list_builder!(status, status, &'a str);
    list_builder!(page_token, page_token, &'a str);
    list_builder!(page_size, page_size, u32);

    fn validate(&self) -> Result<(), TwilioError> {
        validate_dates(self.start_date, self.end_date)?;
        validate_page_size(self.page_size)
    }

    fn query(&self, token_override: Option<&str>) -> Vec<(String, String)> {
        let mut query = Vec::new();
        push_query(&mut query, "startDate", self.start_date);
        push_query(&mut query, "endDate", self.end_date);
        push_query(&mut query, "status", self.status);
        push_query(&mut query, "pageToken", token_override.or(self.page_token));
        if let Some(page_size) = self.page_size {
            query.push(("pageSize".to_owned(), page_size.to_string()));
        }
        query
    }
}

impl fmt::Debug for ListBulkMessageOperationsRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("ListBulkMessageOperationsRequest", f)
    }
}

fn validate_dates(start: Option<&str>, end: Option<&str>) -> Result<(), TwilioError> {
    let start = start
        .map(|value| OffsetDateTime::parse(value, &Rfc3339))
        .transpose()
        .map_err(|_| TwilioError::InvalidRequest("start date must be RFC 3339".to_owned()))?;
    let end = end
        .map(|value| OffsetDateTime::parse(value, &Rfc3339))
        .transpose()
        .map_err(|_| TwilioError::InvalidRequest("end date must be RFC 3339".to_owned()))?;
    if start.zip(end).is_some_and(|(start, end)| end <= start) {
        return Err(TwilioError::InvalidRequest(
            "end date must be later than start date".to_owned(),
        ));
    }
    Ok(())
}

fn validate_page_size(value: Option<u32>) -> Result<(), TwilioError> {
    if value.is_some_and(|value| !(1..=MAX_PAGE_SIZE).contains(&value)) {
        Err(TwilioError::InvalidRequest(
            "page size must be between 1 and 1000".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn push_query(query: &mut Vec<(String, String)>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        query.push((key.to_owned(), value.to_owned()));
    }
}

fn deserialize_optional_timestamp<'de, D>(
    deserializer: D,
) -> Result<Option<OffsetDateTime>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|value| OffsetDateTime::parse(&value, &Rfc3339).map_err(serde::de::Error::custom))
        .transpose()
}

fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    OffsetDateTime::parse(&value, &Rfc3339).map_err(serde::de::Error::custom)
}

/// Metadata returned by message list and fetch endpoints.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessage {
    pub id: Option<String>,
    pub from: Option<BulkMessageResponseSender>,
    #[serde(default)]
    pub to: Vec<BulkMessageResponseRecipient>,
    pub status: Option<crate::BulkMessagingValue>,
    #[serde(default)]
    pub attempts: Vec<BulkMessageAttempt>,
    #[serde(default)]
    pub related: Vec<BulkMessageRelatedResource>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub created_at: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub updated_at: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub scheduled_for: Option<OffsetDateTime>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

impl fmt::Debug for BulkMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessage", f)
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum BulkMessageResponseSender {
    Address {
        address: String,
        channel: crate::BulkMessagingValue,
        #[serde(rename = "senderId")]
        sender_id: String,
        #[serde(rename = "senderPoolId")]
        sender_pool_id: Option<String>,
    },
    Sender {
        #[serde(rename = "senderId")]
        sender_id: String,
    },
    SenderPool {
        #[serde(rename = "senderPoolId")]
        sender_pool_id: String,
    },
    Profile {
        #[serde(rename = "profileId")]
        profile_id: String,
        #[serde(rename = "memoryStoreId")]
        memory_store_id: String,
    },
}

impl fmt::Debug for BulkMessageResponseSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageResponseSender", f)
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum BulkMessageResponseRecipient {
    Address {
        address: String,
        channel: crate::BulkMessagingValue,
    },
    Addresses {
        addresses: Vec<BulkMessageResponseAddress>,
    },
    Profile {
        #[serde(rename = "profileId")]
        profile_id: String,
        #[serde(rename = "memoryStoreId")]
        memory_store_id: String,
    },
}

/// One address in a recipient response.
#[derive(Clone, Deserialize)]
pub struct BulkMessageResponseAddress {
    pub address: String,
    pub channel: crate::BulkMessagingValue,
}

impl fmt::Debug for BulkMessageResponseRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageResponseRecipient", f)
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageAttempt {
    pub id: Option<String>,
    pub status: Option<String>,
    pub channel: Option<String>,
    pub sender_id: Option<String>,
    #[serde(default)]
    pub to: Vec<BulkMessageResponseRecipient>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub created_at: Option<OffsetDateTime>,
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
    pub updated_at: Option<OffsetDateTime>,
}

impl fmt::Debug for BulkMessageAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageAttempt", f)
    }
}

#[derive(Clone, Deserialize)]
pub struct BulkMessageRelatedResource {
    pub name: Option<String>,
    pub id: Option<String>,
    pub uri: Option<String>,
}

impl fmt::Debug for BulkMessageRelatedResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageRelatedResource", f)
    }
}

/// Aggregate operation statistics.
#[derive(Clone, Default, Deserialize)]
pub struct BulkMessageOperationStats {
    pub total: u64,
    pub recipients: u64,
    pub attempts: u64,
    pub scheduled: u64,
    pub queued: u64,
    pub sent: u64,
    pub delivered: u64,
    pub read: u64,
    pub undelivered: u64,
    pub unaddressable: u64,
    pub failed: u64,
    pub canceled: u64,
}

impl fmt::Debug for BulkMessageOperationStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkMessageOperationStats")
            .field("total", &self.total)
            .field("recipients", &self.recipients)
            .finish_non_exhaustive()
    }
}

/// One Bulk Messaging asynchronous operation.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageOperation {
    pub id: String,
    pub status: crate::BulkMessagingValue,
    pub stats: BulkMessageOperationStats,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub updated_at: OffsetDateTime,
}

impl BulkMessageOperation {
    fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "COMPLETED" | "CANCELED")
    }
}

impl fmt::Debug for BulkMessageOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted_debug("BulkMessageOperation", f)
    }
}

/// One token-paginated page of messages.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessagePage {
    #[serde(default)]
    pub messages: Vec<BulkMessage>,
    #[serde(default, alias = "next_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub pagination: crate::BulkMessagingPagination,
}

impl fmt::Debug for BulkMessagePage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkMessagePage")
            .field("messages", &self.messages.len())
            .field(
                "next_page_token",
                &self.next_page_token.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

/// One token-paginated page of operations.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessageOperationPage {
    #[serde(default)]
    pub operations: Vec<BulkMessageOperation>,
    #[serde(default, alias = "next_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub pagination: crate::BulkMessagingPagination,
}

impl fmt::Debug for BulkMessageOperationPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkMessageOperationPage")
            .field("operations", &self.operations.len())
            .field(
                "next_page_token",
                &self.next_page_token.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

fn messages_spec(query: Vec<(String, String)>) -> RequestSpec {
    RequestSpec::new(ApiFamily::BulkMessagingV1, Method::GET, ["Messages"])
        .query_pairs(query)
        .operation("bulk_messages.list")
}

fn operations_spec(query: Vec<(String, String)>) -> RequestSpec {
    RequestSpec::new(
        ApiFamily::BulkMessagingV1,
        Method::GET,
        ["Messages", "Operations"],
    )
    .query_pairs(query)
    .operation("bulk_messages.operations.list")
}

fn submission_from_raw(
    raw: &crate::common::RawResponse,
    base: &Url,
) -> Result<BulkMessageSubmission, TwilioError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        operation_id: Option<String>,
        operation_location: Option<String>,
    }
    let body = if raw.body.iter().all(u8::is_ascii_whitespace) {
        Body {
            operation_id: None,
            operation_location: None,
        }
    } else {
        serde_json::from_slice(&raw.body).map_err(|_| {
            TwilioError::InvalidResponseMetadata("bulk submission body is malformed".to_owned())
        })?
    };
    let header_id = raw
        .headers
        .get("operationId")
        .or_else(|| raw.headers.get("operation-id"))
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| {
            TwilioError::InvalidResponseMetadata(
                "bulk submission operation header is malformed".to_owned(),
            )
        })?;
    if body
        .operation_id
        .as_deref()
        .zip(header_id)
        .is_some_and(|(body, header)| body != header)
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "bulk submission metadata conflicts".to_owned(),
        ));
    }
    let operation_id = body.operation_id.as_deref().or(header_id).ok_or_else(|| {
        TwilioError::InvalidResponseMetadata("bulk submission operation ID is missing".to_owned())
    })?;
    validate_id(operation_id, "comms_operation_", "operation ID").map_err(|_| {
        TwilioError::InvalidResponseMetadata("bulk submission operation ID is malformed".to_owned())
    })?;
    let header_location = raw
        .headers
        .get("operationLocation")
        .or_else(|| raw.headers.get("operation-location"))
        .or_else(|| raw.headers.get("location"))
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| {
            TwilioError::InvalidResponseMetadata(
                "bulk submission location header is malformed".to_owned(),
            )
        })?;
    if body
        .operation_location
        .as_deref()
        .zip(header_location)
        .is_some_and(|(body, header)| body != header)
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "bulk submission metadata conflicts".to_owned(),
        ));
    }
    let location = body.operation_location.as_deref().or(header_location);
    let operation_location = location
        .map(|location| validate_operation_location(base, location, operation_id))
        .transpose()?;
    Ok(BulkMessageSubmission {
        operation_id: operation_id.to_owned(),
        operation_location,
    })
}

fn validate_operation_location(
    base: &Url,
    location: &str,
    operation_id: &str,
) -> Result<Url, TwilioError> {
    let url = Url::parse(location).map_err(|_| {
        TwilioError::InvalidResponseMetadata("bulk submission location is malformed".to_owned())
    })?;
    let same_origin = url.scheme() == base.scheme()
        && url.host_str() == base.host_str()
        && url.port_or_known_default() == base.port_or_known_default();
    let expected = format!("{}v1/Messages/Operations/{operation_id}", base.path());
    if !same_origin || url.path() != expected || url.query().is_some() || url.fragment().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(
            "bulk submission location is outside the configured operation resource".to_owned(),
        ));
    }
    Ok(url)
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessagingResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkMessagingResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// Select Bulk Messaging v1.
    #[must_use]
    pub fn v1(self) -> BulkMessagingV1Resource<'a> {
        BulkMessagingV1Resource {
            account: self.account,
        }
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessagingV1Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkMessagingV1Resource<'a> {
    /// Select the Messages collection.
    #[must_use]
    pub fn messages(self) -> BulkMessagesResource<'a> {
        BulkMessagesResource {
            account: self.account,
        }
    }

    /// Select the Senders collection.
    #[must_use]
    pub fn senders(self) -> crate::BulkSendersResource<'a> {
        crate::BulkSendersResource::new(self.account)
    }

    /// Select the Sender Pools collection.
    #[must_use]
    pub fn sender_pools(self) -> crate::BulkSenderPoolsResource<'a> {
        crate::BulkSenderPoolsResource::new(self.account)
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessagesResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkMessagesResource<'a> {
    /// Submit Bulk Messages.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, transport/API failures, or malformed
    /// 202 response metadata.
    pub async fn send(
        self,
        request: SendBulkMessagesRequest<'_>,
    ) -> Result<BulkMessageSubmission, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::BulkMessagingV1, Method::POST, ["Messages"])
            .operation("bulk_messages.send")
            .accept_status(202)
            .json_body(&request)?;
        let raw = self.account.send_spec_raw(spec, &[]).await?;
        submission_from_raw(&raw.output, &self.account.client.config.bulk_messaging)
    }

    /// Seek a Bulk Message by a downstream `SM` or `MM` SID.
    ///
    /// The 301 response is never followed automatically. Its target is validated
    /// against the configured Bulk Messaging origin before credentials are used.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid SID, unsafe redirect metadata, request
    /// failure, or response decoding failure.
    pub async fn seek(self, sid: &str) -> Result<BulkMessage, TwilioError> {
        validate_message_sid(sid)?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", "Seek", sid],
        )
        .operation("bulk_messages.seek")
        .accept_status(301);
        let response = self.account.send_spec_raw(spec, &[sid]).await?;
        let message_id =
            seek_message_id(&response.output, &self.account.client.config.bulk_messaging)?;
        let fetch = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", &message_id],
        )
        .operation("bulk_messages.seek.fetch");
        self.account.send_spec_json(fetch, &[&message_id]).await
    }

    /// List one page.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid filters, transport/API failures, or decoding.
    pub async fn list(
        self,
        request: ListBulkMessagesRequest<'_>,
    ) -> Result<BulkMessagePage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(messages_spec(request.query(None)), &[])
            .await
    }

    /// Fetch one message by ID.
    #[must_use]
    pub fn message(self, message_id: &'a str) -> BulkMessageResource<'a> {
        BulkMessageResource {
            account: self.account,
            message_id,
        }
    }

    /// Select Operations.
    #[must_use]
    pub fn operations(self) -> BulkMessageOperationsResource<'a> {
        BulkMessageOperationsResource {
            account: self.account,
        }
    }

    /// Select one Operation.
    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BulkMessageOperationResource<'a> {
        BulkMessageOperationResource {
            account: self.account,
            operation_id,
        }
    }

    /// Paginate all messages.
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, BulkMessagePage, BulkMessage> {
        self.list_all_with(ListBulkMessagesRequest::new())
    }

    /// Paginate all messages while preserving initial filters.
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkMessagesRequest<'a>,
    ) -> TwilioPaginator<'a, BulkMessagePage, BulkMessage> {
        TwilioPaginator::new(
            move |token| {
                let request = request;
                Box::pin(async move {
                    request.validate()?;
                    self.account
                        .send_spec_json(messages_spec(request.query(token.as_deref())), &[])
                        .await
                })
            },
            |page| (page.messages, page.pagination.next.or(page.next_page_token)),
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessageResource<'a> {
    account: TwilioAccount<'a>,
    message_id: &'a str,
}

#[cfg(feature = "async")]
impl BulkMessageResource<'_> {
    /// Fetch this message.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid ID, transport/API failures, or decoding.
    pub async fn fetch(self) -> Result<BulkMessage, TwilioError> {
        validate_id(self.message_id, "comms_message_", "message ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", self.message_id],
        )
        .operation("bulk_messages.fetch");
        self.account.send_spec_json(spec, &[self.message_id]).await
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessageOperationsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkMessageOperationsResource<'a> {
    /// List one operation page.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid filters, transport/API failures, or decoding.
    pub async fn list(
        self,
        request: ListBulkMessageOperationsRequest<'_>,
    ) -> Result<BulkMessageOperationPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(operations_spec(request.query(None)), &[])
            .await
    }

    /// Select one operation.
    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BulkMessageOperationResource<'a> {
        BulkMessageOperationResource {
            account: self.account,
            operation_id,
        }
    }

    /// Paginate every operation.
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, BulkMessageOperationPage, BulkMessageOperation> {
        self.list_all_with(ListBulkMessageOperationsRequest::new())
    }

    /// Paginate operations while preserving initial filters.
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkMessageOperationsRequest<'a>,
    ) -> TwilioPaginator<'a, BulkMessageOperationPage, BulkMessageOperation> {
        TwilioPaginator::new(
            move |token| {
                let request = request;
                Box::pin(async move {
                    request.validate()?;
                    self.account
                        .send_spec_json(operations_spec(request.query(token.as_deref())), &[])
                        .await
                })
            },
            |page| {
                (
                    page.operations,
                    page.pagination.next.or(page.next_page_token),
                )
            },
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkMessageOperationResource<'a> {
    account: TwilioAccount<'a>,
    operation_id: &'a str,
}

#[cfg(feature = "async")]
impl BulkMessageOperationResource<'_> {
    /// Fetch this operation.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid ID, transport/API failures, or decoding.
    pub async fn fetch(self) -> Result<BulkMessageOperation, TwilioError> {
        validate_id(self.operation_id, "comms_operation_", "operation ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", "Operations", self.operation_id],
        )
        .operation("bulk_messages.operations.fetch");
        self.account
            .send_spec_json(spec, &[self.operation_id])
            .await
    }

    /// Poll until completion/cancellation or the timeout deadline.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, request failures, or timeout.
    pub async fn wait(
        self,
        interval: Duration,
        timeout: Duration,
    ) -> Result<BulkMessageOperation, TwilioError> {
        validate_wait(interval, timeout)?;
        let deadline = Instant::now() + timeout;
        loop {
            let operation = self.fetch().await?;
            if operation.terminal() {
                return Ok(operation);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(TwilioError::OperationTimeout);
            }
            tokio::time::sleep(interval.min(deadline.saturating_duration_since(now))).await;
        }
    }
}

fn validate_wait(interval: Duration, timeout: Duration) -> Result<(), TwilioError> {
    if interval.is_zero() || timeout.is_zero() {
        Err(TwilioError::InvalidRequest(
            "operation wait interval and timeout must be non-zero".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn validate_message_sid(value: &str) -> Result<(), TwilioError> {
    if value.len() == 34
        && (value.starts_with("SM") || value.starts_with("MM"))
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(
            "message SID has an invalid format".to_owned(),
        ))
    }
}

fn seek_message_id(raw: &crate::common::RawResponse, base: &Url) -> Result<String, TwilioError> {
    let location = raw
        .headers
        .get(http::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            TwilioError::InvalidResponseMetadata("seek location is missing or malformed".to_owned())
        })?;
    let url = Url::parse(location).map_err(|_| {
        TwilioError::InvalidResponseMetadata("seek location is malformed".to_owned())
    })?;
    let same_origin = url.scheme() == base.scheme()
        && url.host_str() == base.host_str()
        && url.port_or_known_default() == base.port_or_known_default();
    let prefix = format!("{}v1/Messages/", base.path());
    let message_id = url.path().strip_prefix(&prefix).ok_or_else(|| {
        TwilioError::InvalidResponseMetadata(
            "seek location is outside the configured message resource".to_owned(),
        )
    })?;
    if !same_origin
        || message_id.contains('/')
        || url.query().is_some()
        || url.fragment().is_some()
        || validate_id(message_id, "comms_message_", "message ID").is_err()
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "seek location is outside the configured message resource".to_owned(),
        ));
    }
    Ok(message_id.to_owned())
}

// The blocking resource graph intentionally mirrors the async graph.
#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessagingResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkMessagingResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }
    #[must_use]
    pub fn v1(self) -> BlockingBulkMessagingV1Resource<'a> {
        BlockingBulkMessagingV1Resource {
            account: self.account,
        }
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessagingV1Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkMessagingV1Resource<'a> {
    #[must_use]
    pub fn messages(self) -> BlockingBulkMessagesResource<'a> {
        BlockingBulkMessagesResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn senders(self) -> crate::BlockingBulkSendersResource<'a> {
        crate::BlockingBulkSendersResource::new(self.account)
    }

    #[must_use]
    pub fn sender_pools(self) -> crate::BlockingBulkSenderPoolsResource<'a> {
        crate::BlockingBulkSenderPoolsResource::new(self.account)
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessagesResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkMessagesResource<'a> {
    /// Submit Bulk Messages.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, request failures, or malformed metadata.
    pub fn send(
        self,
        request: SendBulkMessagesRequest<'_>,
    ) -> Result<BulkMessageSubmission, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::BulkMessagingV1, Method::POST, ["Messages"])
            .operation("bulk_messages.send")
            .accept_status(202)
            .json_body(&request)?;
        let raw = self.account.send_spec_raw(spec, &[])?;
        submission_from_raw(&raw.output, &self.account.client.config.bulk_messaging)
    }

    /// Seek a message by downstream `SM` or `MM` SID.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid SID, unsafe redirect metadata, request
    /// failure, or response decoding failure.
    pub fn seek(self, sid: &str) -> Result<BulkMessage, TwilioError> {
        validate_message_sid(sid)?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", "Seek", sid],
        )
        .operation("bulk_messages.seek")
        .accept_status(301);
        let response = self.account.send_spec_raw(spec, &[sid])?;
        let message_id =
            seek_message_id(&response.output, &self.account.client.config.bulk_messaging)?;
        let fetch = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", &message_id],
        )
        .operation("bulk_messages.seek.fetch");
        self.account.send_spec_json(fetch, &[&message_id])
    }

    /// List one page.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid filters, request failures, or decoding.
    pub fn list(
        self,
        request: ListBulkMessagesRequest<'_>,
    ) -> Result<BulkMessagePage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(messages_spec(request.query(None)), &[])
    }

    #[must_use]
    pub fn message(self, message_id: &'a str) -> BlockingBulkMessageResource<'a> {
        BlockingBulkMessageResource {
            account: self.account,
            message_id,
        }
    }

    #[must_use]
    pub fn operations(self) -> BlockingBulkMessageOperationsResource<'a> {
        BlockingBulkMessageOperationsResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BlockingBulkMessageOperationResource<'a> {
        BlockingBulkMessageOperationResource {
            account: self.account,
            operation_id,
        }
    }

    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, BulkMessagePage, BulkMessage> {
        self.list_all_with(ListBulkMessagesRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkMessagesRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkMessagePage, BulkMessage> {
        BlockingTwilioPaginator::new(
            move |token| {
                request.validate()?;
                self.account
                    .send_spec_json(messages_spec(request.query(token.as_deref())), &[])
            },
            |page| (page.messages, page.pagination.next.or(page.next_page_token)),
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessageResource<'a> {
    account: BlockingTwilioAccount<'a>,
    message_id: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingBulkMessageResource<'_> {
    /// Fetch this message.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid ID, request failures, or decoding.
    pub fn fetch(self) -> Result<BulkMessage, TwilioError> {
        validate_id(self.message_id, "comms_message_", "message ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", self.message_id],
        )
        .operation("bulk_messages.fetch");
        self.account.send_spec_json(spec, &[self.message_id])
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessageOperationsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkMessageOperationsResource<'a> {
    /// List one operation page.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid filters, request failures, or decoding.
    pub fn list(
        self,
        request: ListBulkMessageOperationsRequest<'_>,
    ) -> Result<BulkMessageOperationPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(operations_spec(request.query(None)), &[])
    }

    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BlockingBulkMessageOperationResource<'a> {
        BlockingBulkMessageOperationResource {
            account: self.account,
            operation_id,
        }
    }

    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, BulkMessageOperationPage, BulkMessageOperation> {
        self.list_all_with(ListBulkMessageOperationsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkMessageOperationsRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkMessageOperationPage, BulkMessageOperation> {
        BlockingTwilioPaginator::new(
            move |token| {
                request.validate()?;
                self.account
                    .send_spec_json(operations_spec(request.query(token.as_deref())), &[])
            },
            |page| {
                (
                    page.operations,
                    page.pagination.next.or(page.next_page_token),
                )
            },
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkMessageOperationResource<'a> {
    account: BlockingTwilioAccount<'a>,
    operation_id: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingBulkMessageOperationResource<'_> {
    /// Fetch this operation.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid ID, request failures, or decoding.
    pub fn fetch(self) -> Result<BulkMessageOperation, TwilioError> {
        validate_id(self.operation_id, "comms_operation_", "operation ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Messages", "Operations", self.operation_id],
        )
        .operation("bulk_messages.operations.fetch");
        self.account.send_spec_json(spec, &[self.operation_id])
    }

    /// Poll until completion/cancellation or the timeout deadline.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, request failures, or timeout.
    pub fn wait(
        self,
        interval: Duration,
        timeout: Duration,
    ) -> Result<BulkMessageOperation, TwilioError> {
        validate_wait(interval, timeout)?;
        let deadline = Instant::now() + timeout;
        loop {
            let operation = self.fetch()?;
            if operation.terminal() {
                return Ok(operation);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(TwilioError::OperationTimeout);
            }
            std::thread::sleep(interval.min(deadline.saturating_duration_since(now)));
        }
    }
}
