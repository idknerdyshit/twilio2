//! Bulk Messaging v1 Senders, Sender Pools, and Event Streams models.
//!
//! Individual resource methods share the same validation, transport, API, and
//! response-decoding error contract.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use http::Method;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

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
const MAX_RESOLVE_RECIPIENTS: usize = 100;
const MAX_CREATE_POOL_SENDERS: usize = 10_000;
const MAX_ADD_POOL_SENDERS: usize = 1_000;
const REDACTED_EVENT_VALUE: &str = "<redacted>";

fn redacted(name: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        (26..=35).contains(&suffix.len())
            && matches!(suffix.as_bytes().first(), Some(b'0'..=b'7'))
            && suffix.bytes().all(|byte| {
                byte.is_ascii_digit()
                    || byte.is_ascii_lowercase() && !matches!(byte, b'i' | b'l' | b'o' | b'u')
            })
    })
}

fn validate_id(value: &str, prefix: &str, field: &str) -> Result<(), TwilioError> {
    if valid_id(value, prefix) {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(format!(
            "{field} has an invalid format"
        )))
    }
}

fn validate_page_size(value: Option<u32>) -> Result<(), TwilioError> {
    if value.is_some_and(|size| !(1..=MAX_PAGE_SIZE).contains(&size)) {
        Err(TwilioError::InvalidRequest(
            "page size must be between 1 and 1000".to_owned(),
        ))
    } else {
        Ok(())
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

fn validate_tags(tags: &BTreeMap<String, String>) -> Result<(), TwilioError> {
    if tags.len() > 10
        || tags.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 128
                || value.is_empty()
                || value.len() > 256
                || !key.bytes().all(tag_byte)
                || !value.bytes().all(tag_byte)
        })
    {
        return Err(TwilioError::InvalidRequest(
            "tags exceed the documented character or size limits".to_owned(),
        ));
    }
    Ok(())
}

fn tag_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
}

fn timestamp<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    OffsetDateTime::parse(&value, &Rfc3339).map_err(serde::de::Error::custom)
}

/// An open, string-backed Bulk Messaging wire value.
///
/// Unknown values are retained so additions made by Twilio do not break decoding.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BulkMessagingValue(String);

impl BulkMessagingValue {
    /// Construct a value without restricting future wire values.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the wire value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub const SMS: &'static str = "SMS";
    pub const RCS: &'static str = "RCS";
    pub const EMAIL: &'static str = "EMAIL";
    pub const WHATSAPP: &'static str = "WHATSAPP";
    pub const PUSH: &'static str = "PUSH";
    pub const PHONE: &'static str = "PHONE";
    pub const ACTIVATED: &'static str = "ACTIVATED";
    pub const DEACTIVATED: &'static str = "DEACTIVATED";
    pub const PROCESSING: &'static str = "PROCESSING";
    pub const COMPLETED: &'static str = "COMPLETED";
    pub const SCHEDULED: &'static str = "SCHEDULED";
    pub const FAILED: &'static str = "FAILED";
    pub const CANCELED: &'static str = "CANCELED";
}

impl From<&str> for BulkMessagingValue {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for BulkMessagingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BulkMessagingValue(<redacted>)")
    }
}

impl fmt::Display for BulkMessagingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Token pagination metadata returned by Bulk Messaging v1.
#[derive(Clone, Default, Deserialize)]
pub struct BulkMessagingPagination {
    pub next: Option<String>,
    #[serde(rename = "self")]
    pub self_token: Option<String>,
}

impl fmt::Debug for BulkMessagingPagination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkMessagingPagination", f)
    }
}

/// A Sender available to Bulk Messaging.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSender {
    pub id: String,
    pub display_name: Option<String>,
    pub address: String,
    pub channel: BulkMessagingValue,
    pub status: BulkMessagingValue,
    pub tags: BTreeMap<String, String>,
    #[serde(deserialize_with = "timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(deserialize_with = "timestamp")]
    pub updated_at: OffsetDateTime,
}

impl fmt::Debug for BulkSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSender", f)
    }
}

/// One Sender page.
#[derive(Clone, Default, Deserialize)]
pub struct BulkSenderPage {
    #[serde(default)]
    pub senders: Vec<BulkSender>,
    #[serde(default)]
    pub pagination: BulkMessagingPagination,
}

impl fmt::Debug for BulkSenderPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkSenderPage")
            .field("senders", &self.senders.len())
            .field("pagination", &self.pagination)
            .finish()
    }
}

/// Filters for listing Senders.
#[derive(Clone, Copy, Default)]
pub struct ListBulkSendersRequest<'a> {
    channel: Option<BulkSenderChannel>,
    status: Option<BulkSenderStatus>,
    start_date: Option<&'a str>,
    end_date: Option<&'a str>,
    page_token: Option<&'a str>,
    page_size: Option<u32>,
}

impl<'a> ListBulkSendersRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn channel(mut self, value: BulkSenderChannel) -> Self {
        self.channel = Some(value);
        self
    }
    #[must_use]
    pub fn status(mut self, value: BulkSenderStatus) -> Self {
        self.status = Some(value);
        self
    }
    #[must_use]
    pub fn start_date(mut self, value: &'a str) -> Self {
        self.start_date = Some(value);
        self
    }
    #[must_use]
    pub fn end_date(mut self, value: &'a str) -> Self {
        self.end_date = Some(value);
        self
    }
    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        validate_dates(self.start_date, self.end_date)?;
        validate_page_size(self.page_size)
    }
    fn query(self, token: Option<&str>) -> Vec<(String, String)> {
        let mut query = [
            ("startDate", self.start_date),
            ("endDate", self.end_date),
            ("pageToken", token.or(self.page_token)),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
        .chain(
            self.page_size
                .map(|value| ("pageSize".to_owned(), value.to_string())),
        )
        .collect::<Vec<_>>();
        if let Some(channel) = self.channel {
            query.push(("channel".to_owned(), channel.wire().to_owned()));
        }
        if let Some(status) = self.status {
            query.push(("status".to_owned(), status.wire().to_owned()));
        }
        query
    }
}

impl fmt::Debug for ListBulkSendersRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("ListBulkSendersRequest", f)
    }
}

/// Request body for address/channel Sender search.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBulkSendersRequest<'a> {
    address: &'a str,
    channel: BulkSenderChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<BulkSenderStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_size: Option<u32>,
}

impl<'a> SearchBulkSendersRequest<'a> {
    #[must_use]
    pub fn new(address: &'a str, channel: BulkSenderChannel) -> Self {
        Self {
            address,
            channel,
            status: None,
            page_token: None,
            page_size: None,
        }
    }
    #[must_use]
    pub fn status(mut self, value: BulkSenderStatus) -> Self {
        self.status = Some(value);
        self
    }
    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        non_empty(self.address, "sender search address")?;
        validate_page_size(self.page_size)
    }
}

impl fmt::Debug for SearchBulkSendersRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("SearchBulkSendersRequest", f)
    }
}

/// A recipient endpoint used for Sender resolution.
#[derive(Clone, Serialize)]
pub struct BulkSenderResolveRecipient {
    address: String,
    channel: String,
}

impl BulkSenderResolveRecipient {
    #[must_use]
    pub fn new(address: impl Into<String>, channel: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            channel: channel.into(),
        }
    }
    fn validate(&self) -> Result<(), TwilioError> {
        non_empty(&self.address, "resolve recipient address")?;
        non_empty(&self.channel, "resolve recipient channel")
    }
}

impl fmt::Debug for BulkSenderResolveRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolveRecipient", f)
    }
}

/// One channel priority entry for Sender resolution.
#[derive(Clone, Serialize)]
pub struct BulkSenderChannelPriority {
    channel: BulkSenderChannel,
    priority: u32,
}

impl BulkSenderChannelPriority {
    #[must_use]
    pub fn new(channel: BulkSenderChannel, priority: u32) -> Self {
        Self { channel, priority }
    }
}

/// A closed channel value accepted by Sender requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkSenderChannel {
    Sms,
    Rcs,
    Email,
    Whatsapp,
    Push,
}

impl BulkSenderChannel {
    const fn wire(self) -> &'static str {
        match self {
            Self::Sms => "SMS",
            Self::Rcs => "RCS",
            Self::Email => "EMAIL",
            Self::Whatsapp => "WHATSAPP",
            Self::Push => "PUSH",
        }
    }
}

/// A closed Sender status accepted by Sender requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkSenderStatus {
    Activated,
    Deactivated,
}

impl BulkSenderStatus {
    const fn wire(self) -> &'static str {
        match self {
            Self::Activated => "ACTIVATED",
            Self::Deactivated => "DEACTIVATED",
        }
    }
}

/// Channel controls for Sender resolution.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderChannelControls {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    filter_in: Vec<BulkSenderChannel>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    priority: Vec<BulkSenderChannelPriority>,
}

impl BulkSenderChannelControls {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn filter_in(mut self, values: impl IntoIterator<Item = BulkSenderChannel>) -> Self {
        self.filter_in = values.into_iter().collect();
        self
    }
    #[must_use]
    pub fn priority(mut self, values: impl IntoIterator<Item = BulkSenderChannelPriority>) -> Self {
        self.priority = values.into_iter().collect();
        self
    }
    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        let filters: BTreeSet<_> = self.filter_in.iter().collect();
        let priorities: BTreeSet<_> = self.priority.iter().map(|item| &item.channel).collect();
        if filters.len() != self.filter_in.len() || priorities.len() != self.priority.len() {
            return Err(TwilioError::InvalidRequest(
                "resolve channel controls must contain unique channels".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Request body for resolving Sender/recipient pairs.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveBulkSendersRequest<'a> {
    recipient_addresses: &'a [BulkSenderResolveRecipient],
    #[serde(skip_serializing_if = "Option::is_none")]
    sender_pool_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sender_addresses: Option<&'a [BulkSenderResolveAddress]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channels: Option<&'a BulkSenderChannelControls>,
}

impl<'a> ResolveBulkSendersRequest<'a> {
    #[must_use]
    pub fn new(recipient_addresses: &'a [BulkSenderResolveRecipient]) -> Self {
        Self {
            recipient_addresses,
            sender_pool_id: None,
            sender_addresses: None,
            channels: None,
        }
    }
    /// Resolve against explicit sender addresses instead of account inventory or a pool.
    #[must_use]
    pub fn sender_addresses(mut self, value: &'a [BulkSenderResolveAddress]) -> Self {
        self.sender_addresses = Some(value);
        self
    }
    #[must_use]
    pub fn sender_pool_id(mut self, value: &'a str) -> Self {
        self.sender_pool_id = Some(value);
        self
    }
    #[must_use]
    pub fn channels(mut self, value: &'a BulkSenderChannelControls) -> Self {
        self.channels = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        if !(1..=MAX_RESOLVE_RECIPIENTS).contains(&self.recipient_addresses.len()) {
            return Err(TwilioError::InvalidRequest(
                "resolve recipients must contain between 1 and 100 items".to_owned(),
            ));
        }
        self.recipient_addresses
            .iter()
            .try_for_each(BulkSenderResolveRecipient::validate)?;
        if let Some(id) = self.sender_pool_id {
            validate_id(id, "comms_senderpool_", "sender pool ID")?;
        }
        if self.sender_pool_id.is_some() && self.sender_addresses.is_some() {
            return Err(TwilioError::InvalidRequest(
                "sender pool and sender addresses are mutually exclusive".to_owned(),
            ));
        }
        if let Some(addresses) = self.sender_addresses {
            if addresses.is_empty() || addresses.len() > 100 {
                return Err(TwilioError::InvalidRequest(
                    "sender addresses must contain between 1 and 100 items".to_owned(),
                ));
            }
            addresses
                .iter()
                .try_for_each(BulkSenderResolveAddress::validate)?;
        }
        if let Some(channels) = self.channels {
            channels.validate()?;
        }
        Ok(())
    }
}

/// An explicit sender identity used during Sender resolution.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum BulkSenderResolveAddress {
    Messaging {
        address: String,
        channel: BulkSenderMessagingChannel,
    },
    Email {
        address: String,
        name: String,
    },
    Push {
        #[serde(skip_serializing_if = "Option::is_none")]
        fcm: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        apn: Option<String>,
    },
}

/// A messaging-address channel accepted by explicit Sender resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BulkSenderMessagingChannel {
    Sms,
    Rcs,
    Whatsapp,
}

impl BulkSenderResolveAddress {
    /// Construct an SMS, RCS, or `WhatsApp` sender address.
    #[must_use]
    pub fn messaging(address: impl Into<String>, channel: BulkSenderMessagingChannel) -> Self {
        Self::Messaging {
            address: address.into(),
            channel,
        }
    }

    /// Construct an email sender address and display name.
    #[must_use]
    pub fn email(address: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Email {
            address: address.into(),
            name: name.into(),
        }
    }

    /// Construct a push sender from optional FCM and APN Credential IDs.
    #[must_use]
    pub fn push(fcm: Option<impl Into<String>>, apn: Option<impl Into<String>>) -> Self {
        Self::Push {
            fcm: fcm.map(Into::into),
            apn: apn.map(Into::into),
        }
    }

    fn validate(&self) -> Result<(), TwilioError> {
        match self {
            Self::Messaging { address, .. } => non_empty(address, "sender address"),
            Self::Email { address, name } => {
                non_empty(address, "sender email address")?;
                non_empty(name, "sender email name")
            }
            Self::Push { fcm, apn } => {
                if fcm.is_none() && apn.is_none() {
                    return Err(TwilioError::InvalidRequest(
                        "push sender requires an FCM or APN credential".to_owned(),
                    ));
                }
                for credential in [fcm.as_deref(), apn.as_deref()].into_iter().flatten() {
                    validate_id(credential, "comms_credential_", "credential ID")?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for BulkSenderResolveAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolveAddress", f)
    }
}

impl fmt::Debug for ResolveBulkSendersRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("ResolveBulkSendersRequest", f)
    }
}

/// One resolved sender-recipient pair.
#[derive(Clone, Deserialize)]
pub struct BulkSenderResolution {
    pub from: BulkSenderResolutionFrom,
    pub to: BulkSenderResolutionRecipient,
    pub priority: u32,
}

/// The resolved Sender eligible to reach a recipient.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderResolutionFrom {
    pub sender_id: String,
    pub display_name: Option<String>,
    pub address: Option<String>,
    pub channel: BulkMessagingValue,
}

impl fmt::Debug for BulkSenderResolutionFrom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolutionFrom", f)
    }
}

impl fmt::Debug for BulkSenderResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolution", f)
    }
}

#[derive(Clone, Deserialize)]
pub struct BulkSenderResolutionRecipient {
    pub address: String,
    pub channel: BulkMessagingValue,
}

impl fmt::Debug for BulkSenderResolutionRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolutionRecipient", f)
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct BulkSenderResolutionPage {
    #[serde(default)]
    pub results: Vec<BulkSenderResolution>,
}

impl fmt::Debug for BulkSenderResolutionPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderResolutionPage", f)
    }
}

/// A Bulk Messaging Sender Pool.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderPool {
    pub id: String,
    pub name: String,
    pub tags: BTreeMap<String, String>,
    #[serde(deserialize_with = "timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(deserialize_with = "timestamp")]
    pub updated_at: OffsetDateTime,
}

impl fmt::Debug for BulkSenderPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderPool", f)
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderPoolPage {
    #[serde(default)]
    pub sender_pools: Vec<BulkSenderPool>,
    #[serde(default)]
    pub pagination: BulkMessagingPagination,
}

impl fmt::Debug for BulkSenderPoolPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderPoolPage", f)
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct BulkSenderPoolMemberPage {
    #[serde(default)]
    pub senders: Vec<BulkSender>,
    #[serde(default)]
    pub pagination: BulkMessagingPagination,
}

impl fmt::Debug for BulkSenderPoolMemberPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderPoolMemberPage", f)
    }
}

/// A Sender Pool asynchronous operation.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderPoolOperation {
    pub id: String,
    pub status: BulkMessagingValue,
    pub stats: BulkSenderPoolOperationStats,
    #[serde(deserialize_with = "timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(deserialize_with = "timestamp")]
    pub updated_at: OffsetDateTime,
}

/// Aggregate statistics for a Sender Pool operation.
#[derive(Clone, Deserialize)]
pub struct BulkSenderPoolOperationStats {
    pub total: u64,
    pub queued: u64,
    pub created: u64,
    pub failed: u64,
}

impl fmt::Debug for BulkSenderPoolOperationStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkSenderPoolOperationStats")
            .field("total", &self.total)
            .field("queued", &self.queued)
            .field("created", &self.created)
            .field("failed", &self.failed)
            .finish()
    }
}

impl BulkSenderPoolOperation {
    fn terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            BulkMessagingValue::COMPLETED
                | BulkMessagingValue::FAILED
                | BulkMessagingValue::CANCELED
        )
    }
}

impl fmt::Debug for BulkSenderPoolOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderPoolOperation", f)
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct BulkSenderPoolOperationPage {
    #[serde(default)]
    pub operations: Vec<BulkSenderPoolOperation>,
    #[serde(default)]
    pub pagination: BulkMessagingPagination,
}

impl fmt::Debug for BulkSenderPoolOperationPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkSenderPoolOperationPage", f)
    }
}

/// Create a Sender Pool.
#[derive(Clone, Copy, Serialize)]
pub struct CreateBulkSenderPoolRequest<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<&'a BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    senders: Option<&'a [BulkSenderPoolSenderRequest<'a>]>,
}

impl<'a> CreateBulkSenderPoolRequest<'a> {
    #[must_use]
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            tags: None,
            senders: None,
        }
    }
    #[must_use]
    pub fn tags(mut self, value: &'a BTreeMap<String, String>) -> Self {
        self.tags = Some(value);
        self
    }
    #[must_use]
    pub fn senders(mut self, value: &'a [BulkSenderPoolSenderRequest<'a>]) -> Self {
        self.senders = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        non_empty(self.name, "sender pool name")?;
        if let Some(tags) = self.tags {
            validate_tags(tags)?;
        }
        if let Some(senders) = self.senders {
            validate_create_sender_members(senders)?;
        }
        Ok(())
    }
}

impl fmt::Debug for CreateBulkSenderPoolRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("CreateBulkSenderPoolRequest", f)
    }
}

/// One Sender reference in a pool mutation.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSenderPoolSenderRequest<'a> {
    sender_id: &'a str,
}

impl<'a> BulkSenderPoolSenderRequest<'a> {
    #[must_use]
    pub fn new(sender_id: &'a str) -> Self {
        Self { sender_id }
    }
}

fn validate_create_sender_members(
    values: &[BulkSenderPoolSenderRequest<'_>],
) -> Result<(), TwilioError> {
    if values.len() > MAX_CREATE_POOL_SENDERS {
        return Err(TwilioError::InvalidRequest(
            "initial sender membership must contain at most 10000 items".to_owned(),
        ));
    }
    validate_unique_sender_members(values)
}

fn validate_add_sender_members(
    values: &[BulkSenderPoolSenderRequest<'_>],
) -> Result<(), TwilioError> {
    if values.is_empty() || values.len() > MAX_ADD_POOL_SENDERS {
        return Err(TwilioError::InvalidRequest(
            "sender membership must contain between 1 and 1000 items".to_owned(),
        ));
    }
    validate_unique_sender_members(values)
}

fn validate_unique_sender_members(
    values: &[BulkSenderPoolSenderRequest<'_>],
) -> Result<(), TwilioError> {
    let unique: BTreeSet<_> = values.iter().map(|value| value.sender_id).collect();
    if unique.len() != values.len() {
        return Err(TwilioError::InvalidRequest(
            "sender membership IDs must be unique".to_owned(),
        ));
    }
    values
        .iter()
        .try_for_each(|value| validate_id(value.sender_id, "comms_sender_", "sender ID"))
}

/// Update a Sender Pool. At least one field is required.
#[derive(Clone, Copy, Default, Serialize)]
pub struct UpdateBulkSenderPoolRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<&'a BTreeMap<String, String>>,
}

impl<'a> UpdateBulkSenderPoolRequest<'a> {
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
    pub fn tags(mut self, value: &'a BTreeMap<String, String>) -> Self {
        self.tags = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        if self.name.is_none() && self.tags.is_none() {
            return Err(TwilioError::InvalidRequest(
                "sender pool update must contain at least one field".to_owned(),
            ));
        }
        if let Some(name) = self.name {
            non_empty(name, "sender pool name")?;
        }
        if let Some(tags) = self.tags {
            validate_tags(tags)?;
        }
        Ok(())
    }
}

impl fmt::Debug for UpdateBulkSenderPoolRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("UpdateBulkSenderPoolRequest", f)
    }
}

/// Common filters for Sender Pool collections.
#[derive(Clone, Copy, Default)]
pub struct ListBulkSenderPoolsRequest<'a> {
    start_date: Option<&'a str>,
    end_date: Option<&'a str>,
    operation_id: Option<&'a str>,
    page_token: Option<&'a str>,
    page_size: Option<u32>,
}

impl<'a> ListBulkSenderPoolsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn start_date(mut self, value: &'a str) -> Self {
        self.start_date = Some(value);
        self
    }
    #[must_use]
    pub fn end_date(mut self, value: &'a str) -> Self {
        self.end_date = Some(value);
        self
    }
    #[must_use]
    pub fn operation_id(mut self, value: &'a str) -> Self {
        self.operation_id = Some(value);
        self
    }
    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        validate_dates(self.start_date, self.end_date)?;
        if let Some(operation_id) = self.operation_id {
            validate_id(operation_id, "comms_operation_", "operation ID")?;
        }
        validate_page_size(self.page_size)
    }
    fn query(self, token: Option<&str>) -> Vec<(String, String)> {
        [
            ("startDate", self.start_date),
            ("endDate", self.end_date),
            ("operationId", self.operation_id),
            ("pageToken", token.or(self.page_token)),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
        .chain(
            self.page_size
                .map(|value| ("pageSize".to_owned(), value.to_string())),
        )
        .collect()
    }
}

impl fmt::Debug for ListBulkSenderPoolsRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("ListBulkSenderPoolsRequest", f)
    }
}

/// Accepted operation reference returned by a 202 response.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessagingOperationReference {
    pub operation_id: String,
    pub operation_location: String,
}

impl fmt::Debug for BulkMessagingOperationReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkMessagingOperationReference", f)
    }
}

/// Resource reference returned by pool mutation endpoints.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMessagingResourceReference {
    pub resource_id: String,
    pub resource_location: String,
}

impl fmt::Debug for BulkMessagingResourceReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkMessagingResourceReference", f)
    }
}

fn senders_spec(request: ListBulkSendersRequest<'_>, token: Option<&str>) -> RequestSpec {
    RequestSpec::new(ApiFamily::BulkMessagingV1, Method::GET, ["Senders"])
        .query_pairs(request.query(token))
        .operation("bulk_senders.list")
}

fn pools_spec(request: ListBulkSenderPoolsRequest<'_>, token: Option<&str>) -> RequestSpec {
    RequestSpec::new(ApiFamily::BulkMessagingV1, Method::GET, ["SenderPools"])
        .query_pairs(request.query(token))
        .operation("bulk_sender_pools.list")
}

fn member_spec(
    pool_id: &str,
    request: ListBulkSendersRequest<'_>,
    token: Option<&str>,
) -> RequestSpec {
    RequestSpec::new(
        ApiFamily::BulkMessagingV1,
        Method::GET,
        ["SenderPools", pool_id, "Senders"],
    )
    .query_pairs(request.query(token))
    .operation("bulk_sender_pools.senders.list")
}

/// Filters for listing Sender Pool operations.
#[derive(Clone, Copy, Default)]
pub struct ListBulkSenderPoolOperationsRequest<'a> {
    start_date: Option<&'a str>,
    end_date: Option<&'a str>,
    status: Option<&'a str>,
    page_token: Option<&'a str>,
    page_size: Option<u32>,
}

impl<'a> ListBulkSenderPoolOperationsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn start_date(mut self, value: &'a str) -> Self {
        self.start_date = Some(value);
        self
    }
    #[must_use]
    pub fn end_date(mut self, value: &'a str) -> Self {
        self.end_date = Some(value);
        self
    }
    #[must_use]
    pub fn status(mut self, value: &'a str) -> Self {
        self.status = Some(value);
        self
    }
    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }
    fn validate(self) -> Result<(), TwilioError> {
        validate_dates(self.start_date, self.end_date)?;
        validate_page_size(self.page_size)
    }
    fn query(self, token: Option<&str>) -> Vec<(String, String)> {
        [
            ("startDate", self.start_date),
            ("endDate", self.end_date),
            ("status", self.status),
            ("pageToken", token.or(self.page_token)),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
        .chain(
            self.page_size
                .map(|value| ("pageSize".to_owned(), value.to_string())),
        )
        .collect()
    }
}

impl fmt::Debug for ListBulkSenderPoolOperationsRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("ListBulkSenderPoolOperationsRequest", f)
    }
}

fn pool_operations_spec(
    request: ListBulkSenderPoolOperationsRequest<'_>,
    token: Option<&str>,
) -> RequestSpec {
    RequestSpec::new(
        ApiFamily::BulkMessagingV1,
        Method::GET,
        ["SenderPools", "Operations"],
    )
    .query_pairs(request.query(token))
    .operation("bulk_sender_pools.operations.list")
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSendersResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkSendersResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn sender(self, sender_id: &'a str) -> BulkSenderResource<'a> {
        BulkSenderResource {
            account: self.account,
            sender_id,
        }
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn list(
        self,
        request: ListBulkSendersRequest<'_>,
    ) -> Result<BulkSenderPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(senders_spec(request, None), &[])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn search(
        self,
        request: SearchBulkSendersRequest<'_>,
    ) -> Result<BulkSenderPage, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["Senders", "Search"],
        )
        .operation("bulk_senders.search")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[]).await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn resolve(
        self,
        request: ResolveBulkSendersRequest<'_>,
    ) -> Result<BulkSenderResolutionPage, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["Senders", "Resolve"],
        )
        .operation("bulk_senders.resolve")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[]).await
    }

    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, BulkSenderPage, BulkSender> {
        self.list_all_with(ListBulkSendersRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSendersRequest<'a>,
    ) -> TwilioPaginator<'a, BulkSenderPage, BulkSender> {
        TwilioPaginator::new(
            move |token| {
                Box::pin(async move {
                    request.validate()?;
                    self.account
                        .send_spec_json(senders_spec(request, token.as_deref()), &[])
                        .await
                })
            },
            |page| (page.senders, page.pagination.next),
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSenderResource<'a> {
    account: TwilioAccount<'a>,
    sender_id: &'a str,
}

#[cfg(feature = "async")]
impl BulkSenderResource<'_> {
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn fetch(self) -> Result<BulkSender, TwilioError> {
        validate_id(self.sender_id, "comms_sender_", "sender ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Senders", self.sender_id],
        )
        .operation("bulk_senders.fetch");
        self.account.send_spec_json(spec, &[self.sender_id]).await
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSenderPoolsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkSenderPoolsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn sender_pool(self, sender_pool_id: &'a str) -> BulkSenderPoolResource<'a> {
        BulkSenderPoolResource {
            account: self.account,
            sender_pool_id,
        }
    }

    #[must_use]
    pub fn operations(self) -> BulkSenderPoolOperationsResource<'a> {
        BulkSenderPoolOperationsResource {
            account: self.account,
        }
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn create(
        self,
        request: CreateBulkSenderPoolRequest<'_>,
    ) -> Result<BulkMessagingOperationReference, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::BulkMessagingV1, Method::POST, ["SenderPools"])
            .operation("bulk_sender_pools.create")
            .accept_status(202)
            .json_body(&request)?;
        self.account.send_spec_json(spec, &[]).await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn list(
        self,
        request: ListBulkSenderPoolsRequest<'_>,
    ) -> Result<BulkSenderPoolPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(pools_spec(request, None), &[])
            .await
    }

    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, BulkSenderPoolPage, BulkSenderPool> {
        self.list_all_with(ListBulkSenderPoolsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSenderPoolsRequest<'a>,
    ) -> TwilioPaginator<'a, BulkSenderPoolPage, BulkSenderPool> {
        TwilioPaginator::new(
            move |token| {
                Box::pin(async move {
                    request.validate()?;
                    self.account
                        .send_spec_json(pools_spec(request, token.as_deref()), &[])
                        .await
                })
            },
            |page| (page.sender_pools, page.pagination.next),
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSenderPoolResource<'a> {
    account: TwilioAccount<'a>,
    sender_pool_id: &'a str,
}

#[cfg(feature = "async")]
impl BulkSenderPoolResource<'_> {
    fn validate(self) -> Result<(), TwilioError> {
        validate_id(self.sender_pool_id, "comms_senderpool_", "sender pool ID")
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn fetch(self) -> Result<BulkSenderPool, TwilioError> {
        self.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.fetch");
        self.account
            .send_spec_json(spec, &[self.sender_pool_id])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn update(
        self,
        request: UpdateBulkSenderPoolRequest<'_>,
    ) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::PATCH,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.update")
        .accept_status(202)
        .json_body(&request)?;
        self.account
            .send_spec_json(spec, &[self.sender_pool_id])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn delete(self) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::DELETE,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.delete")
        .accept_status(202);
        self.account
            .send_spec_json(spec, &[self.sender_pool_id])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn add_senders(
        self,
        senders: &[BulkSenderPoolSenderRequest<'_>],
    ) -> Result<BulkMessagingOperationReference, TwilioError> {
        self.validate()?;
        validate_add_sender_members(senders)?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["SenderPools", self.sender_pool_id, "Senders"],
        )
        .operation("bulk_sender_pools.senders.add")
        .accept_status(202)
        .json_body(&senders)?;
        self.account
            .send_spec_json(spec, &[self.sender_pool_id])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn list_senders(
        self,
        request: ListBulkSendersRequest<'_>,
    ) -> Result<BulkSenderPoolMemberPage, TwilioError> {
        self.validate()?;
        request.validate()?;
        self.account
            .send_spec_json(
                member_spec(self.sender_pool_id, request, None),
                &[self.sender_pool_id],
            )
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn remove_sender(
        self,
        sender_id: &str,
    ) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        validate_id(sender_id, "comms_sender_", "sender ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::DELETE,
            ["SenderPools", self.sender_pool_id, "Senders", sender_id],
        )
        .operation("bulk_sender_pools.senders.remove")
        .accept_status(202);
        self.account
            .send_spec_json(spec, &[self.sender_pool_id, sender_id])
            .await
    }
}

#[cfg(feature = "async")]
impl<'a> BulkSenderPoolResource<'a> {
    /// Lazily iterate every Sender assigned to this Sender Pool.
    #[must_use]
    pub fn list_all_senders(self) -> TwilioPaginator<'a, BulkSenderPoolMemberPage, BulkSender> {
        self.list_all_senders_with(ListBulkSendersRequest::new())
    }

    /// Lazily iterate every Sender assigned to this Sender Pool with filters.
    #[must_use]
    pub fn list_all_senders_with(
        self,
        request: ListBulkSendersRequest<'a>,
    ) -> TwilioPaginator<'a, BulkSenderPoolMemberPage, BulkSender> {
        TwilioPaginator::new(
            move |token| {
                Box::pin(async move {
                    self.validate()?;
                    request.validate()?;
                    self.account
                        .send_spec_json(
                            member_spec(self.sender_pool_id, request, token.as_deref()),
                            &[self.sender_pool_id],
                        )
                        .await
                })
            },
            |page| (page.senders, page.pagination.next),
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSenderPoolOperationsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> BulkSenderPoolOperationsResource<'a> {
    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BulkSenderPoolOperationResource<'a> {
        BulkSenderPoolOperationResource {
            account: self.account,
            operation_id,
        }
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn list(
        self,
        request: ListBulkSenderPoolOperationsRequest<'_>,
    ) -> Result<BulkSenderPoolOperationPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(pool_operations_spec(request, None), &[])
            .await
    }

    /// Lazily iterate every Sender Pool operation.
    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, BulkSenderPoolOperationPage, BulkSenderPoolOperation> {
        self.list_all_with(ListBulkSenderPoolOperationsRequest::new())
    }

    /// Lazily iterate every Sender Pool operation with filters.
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSenderPoolOperationsRequest<'a>,
    ) -> TwilioPaginator<'a, BulkSenderPoolOperationPage, BulkSenderPoolOperation> {
        TwilioPaginator::new(
            move |token| {
                Box::pin(async move {
                    request.validate()?;
                    self.account
                        .send_spec_json(pool_operations_spec(request, token.as_deref()), &[])
                        .await
                })
            },
            |page| (page.operations, page.pagination.next),
        )
    }
}

#[cfg(feature = "async")]
#[derive(Clone, Copy)]
pub struct BulkSenderPoolOperationResource<'a> {
    account: TwilioAccount<'a>,
    operation_id: &'a str,
}

#[cfg(feature = "async")]
impl BulkSenderPoolOperationResource<'_> {
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn fetch(self) -> Result<BulkSenderPoolOperation, TwilioError> {
        validate_id(self.operation_id, "comms_operation_", "operation ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["SenderPools", "Operations", self.operation_id],
        )
        .operation("bulk_sender_pools.operations.fetch");
        self.account
            .send_spec_json(spec, &[self.operation_id])
            .await
    }

    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub async fn wait(
        self,
        interval: Duration,
        timeout: Duration,
    ) -> Result<BulkSenderPoolOperation, TwilioError> {
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

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSendersResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkSendersResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }
    #[must_use]
    pub fn sender(self, sender_id: &'a str) -> BlockingBulkSenderResource<'a> {
        BlockingBulkSenderResource {
            account: self.account,
            sender_id,
        }
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn list(self, request: ListBulkSendersRequest<'_>) -> Result<BulkSenderPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(senders_spec(request, None), &[])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn search(
        self,
        request: SearchBulkSendersRequest<'_>,
    ) -> Result<BulkSenderPage, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["Senders", "Search"],
        )
        .operation("bulk_senders.search")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn resolve(
        self,
        request: ResolveBulkSendersRequest<'_>,
    ) -> Result<BulkSenderResolutionPage, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["Senders", "Resolve"],
        )
        .operation("bulk_senders.resolve")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[])
    }
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, BulkSenderPage, BulkSender> {
        self.list_all_with(ListBulkSendersRequest::new())
    }
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSendersRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPage, BulkSender> {
        BlockingTwilioPaginator::new(
            move |token| {
                request.validate()?;
                self.account
                    .send_spec_json(senders_spec(request, token.as_deref()), &[])
            },
            |page| (page.senders, page.pagination.next),
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSenderResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sender_id: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingBulkSenderResource<'_> {
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn fetch(self) -> Result<BulkSender, TwilioError> {
        validate_id(self.sender_id, "comms_sender_", "sender ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["Senders", self.sender_id],
        )
        .operation("bulk_senders.fetch");
        self.account.send_spec_json(spec, &[self.sender_id])
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSenderPoolsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkSenderPoolsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }
    #[must_use]
    pub fn sender_pool(self, sender_pool_id: &'a str) -> BlockingBulkSenderPoolResource<'a> {
        BlockingBulkSenderPoolResource {
            account: self.account,
            sender_pool_id,
        }
    }
    #[must_use]
    pub fn operations(self) -> BlockingBulkSenderPoolOperationsResource<'a> {
        BlockingBulkSenderPoolOperationsResource {
            account: self.account,
        }
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn create(
        self,
        request: CreateBulkSenderPoolRequest<'_>,
    ) -> Result<BulkMessagingOperationReference, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::BulkMessagingV1, Method::POST, ["SenderPools"])
            .operation("bulk_sender_pools.create")
            .accept_status(202)
            .json_body(&request)?;
        self.account.send_spec_json(spec, &[])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn list(
        self,
        request: ListBulkSenderPoolsRequest<'_>,
    ) -> Result<BulkSenderPoolPage, TwilioError> {
        request.validate()?;
        self.account.send_spec_json(pools_spec(request, None), &[])
    }
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, BulkSenderPoolPage, BulkSenderPool> {
        self.list_all_with(ListBulkSenderPoolsRequest::new())
    }
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSenderPoolsRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPoolPage, BulkSenderPool> {
        BlockingTwilioPaginator::new(
            move |token| {
                request.validate()?;
                self.account
                    .send_spec_json(pools_spec(request, token.as_deref()), &[])
            },
            |page| (page.sender_pools, page.pagination.next),
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSenderPoolResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sender_pool_id: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingBulkSenderPoolResource<'_> {
    fn validate(self) -> Result<(), TwilioError> {
        validate_id(self.sender_pool_id, "comms_senderpool_", "sender pool ID")
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn fetch(self) -> Result<BulkSenderPool, TwilioError> {
        self.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.fetch");
        self.account.send_spec_json(spec, &[self.sender_pool_id])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn update(
        self,
        request: UpdateBulkSenderPoolRequest<'_>,
    ) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::PATCH,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.update")
        .accept_status(202)
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[self.sender_pool_id])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn delete(self) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::DELETE,
            ["SenderPools", self.sender_pool_id],
        )
        .operation("bulk_sender_pools.delete")
        .accept_status(202);
        self.account.send_spec_json(spec, &[self.sender_pool_id])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn add_senders(
        self,
        senders: &[BulkSenderPoolSenderRequest<'_>],
    ) -> Result<BulkMessagingOperationReference, TwilioError> {
        self.validate()?;
        validate_add_sender_members(senders)?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::POST,
            ["SenderPools", self.sender_pool_id, "Senders"],
        )
        .operation("bulk_sender_pools.senders.add")
        .accept_status(202)
        .json_body(&senders)?;
        self.account.send_spec_json(spec, &[self.sender_pool_id])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn list_senders(
        self,
        request: ListBulkSendersRequest<'_>,
    ) -> Result<BulkSenderPoolMemberPage, TwilioError> {
        self.validate()?;
        request.validate()?;
        self.account.send_spec_json(
            member_spec(self.sender_pool_id, request, None),
            &[self.sender_pool_id],
        )
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn remove_sender(
        self,
        sender_id: &str,
    ) -> Result<BulkMessagingResourceReference, TwilioError> {
        self.validate()?;
        validate_id(sender_id, "comms_sender_", "sender ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::DELETE,
            ["SenderPools", self.sender_pool_id, "Senders", sender_id],
        )
        .operation("bulk_sender_pools.senders.remove")
        .accept_status(202);
        self.account
            .send_spec_json(spec, &[self.sender_pool_id, sender_id])
    }
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkSenderPoolResource<'a> {
    /// Lazily iterate every Sender assigned to this Sender Pool.
    #[must_use]
    pub fn list_all_senders(
        self,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPoolMemberPage, BulkSender> {
        self.list_all_senders_with(ListBulkSendersRequest::new())
    }

    /// Lazily iterate every Sender assigned to this Sender Pool with filters.
    #[must_use]
    pub fn list_all_senders_with(
        self,
        request: ListBulkSendersRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPoolMemberPage, BulkSender> {
        BlockingTwilioPaginator::new(
            move |token| {
                self.validate()?;
                request.validate()?;
                self.account.send_spec_json(
                    member_spec(self.sender_pool_id, request, token.as_deref()),
                    &[self.sender_pool_id],
                )
            },
            |page| (page.senders, page.pagination.next),
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSenderPoolOperationsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingBulkSenderPoolOperationsResource<'a> {
    #[must_use]
    pub fn operation(self, operation_id: &'a str) -> BlockingBulkSenderPoolOperationResource<'a> {
        BlockingBulkSenderPoolOperationResource {
            account: self.account,
            operation_id,
        }
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn list(
        self,
        request: ListBulkSenderPoolOperationsRequest<'_>,
    ) -> Result<BulkSenderPoolOperationPage, TwilioError> {
        request.validate()?;
        self.account
            .send_spec_json(pool_operations_spec(request, None), &[])
    }

    /// Lazily iterate every Sender Pool operation.
    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPoolOperationPage, BulkSenderPoolOperation> {
        self.list_all_with(ListBulkSenderPoolOperationsRequest::new())
    }

    /// Lazily iterate every Sender Pool operation with filters.
    #[must_use]
    pub fn list_all_with(
        self,
        request: ListBulkSenderPoolOperationsRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, BulkSenderPoolOperationPage, BulkSenderPoolOperation> {
        BlockingTwilioPaginator::new(
            move |token| {
                request.validate()?;
                self.account
                    .send_spec_json(pool_operations_spec(request, token.as_deref()), &[])
            },
            |page| (page.operations, page.pagination.next),
        )
    }
}

#[cfg(feature = "sync")]
#[derive(Clone, Copy)]
pub struct BlockingBulkSenderPoolOperationResource<'a> {
    account: BlockingTwilioAccount<'a>,
    operation_id: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingBulkSenderPoolOperationResource<'_> {
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn fetch(self) -> Result<BulkSenderPoolOperation, TwilioError> {
        validate_id(self.operation_id, "comms_operation_", "operation ID")?;
        let spec = RequestSpec::new(
            ApiFamily::BulkMessagingV1,
            Method::GET,
            ["SenderPools", "Operations", self.operation_id],
        )
        .operation("bulk_sender_pools.operations.fetch");
        self.account.send_spec_json(spec, &[self.operation_id])
    }
    /// # Errors
    ///
    /// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
    pub fn wait(
        self,
        interval: Duration,
        timeout: Duration,
    ) -> Result<BulkSenderPoolOperation, TwilioError> {
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

/// Subscription type for Bulk Messaging operation-processing events.
pub const BULK_EVENT_OPERATION_PROCESSING: &str = "com.twilio.comms-api.operation.processing";
pub const BULK_EVENT_OPERATION_COMPLETED: &str = "com.twilio.comms-api.operation.completed";
pub const BULK_EVENT_OPERATION_SCHEDULED: &str = "com.twilio.comms-api.operation.scheduled";
pub const BULK_EVENT_MESSAGE_QUEUED: &str = "com.twilio.comms-api.message.queued";
pub const BULK_EVENT_MESSAGE_SENT: &str = "com.twilio.comms-api.message.sent";
pub const BULK_EVENT_MESSAGE_FAILED: &str = "com.twilio.comms-api.message.failed";
pub const BULK_EVENT_MESSAGE_DELIVERED: &str = "com.twilio.comms-api.message.delivered";
pub const BULK_EVENT_MESSAGE_UNDELIVERED: &str = "com.twilio.comms-api.message.undelivered";
pub const BULK_EVENT_MESSAGE_READ: &str = "com.twilio.comms-api.message.read";
pub const BULK_EVENT_MESSAGE_INBOUND_RECEIVED: &str =
    "com.twilio.comms-api.message.inbound-received";

pub const BULK_SCHEMA_OPERATION_PROCESSING_V2: &str =
    "https://events-schemas.twilio.com/CommsApi.OperationProcessing/2";
pub const BULK_SCHEMA_OPERATION_COMPLETED_V2: &str =
    "https://events-schemas.twilio.com/CommsApi.OperationCompleted/2";
pub const BULK_SCHEMA_OPERATION_SCHEDULED_V2: &str =
    "https://events-schemas.twilio.com/CommsApi.OperationScheduled/2";
pub const BULK_SCHEMA_MESSAGE_QUEUED_V3: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageQueued/3";
pub const BULK_SCHEMA_MESSAGE_SENT_V4: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageSent/4";
pub const BULK_SCHEMA_MESSAGE_FAILED_V4: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageFailed/4";
pub const BULK_SCHEMA_MESSAGE_DELIVERED_V4: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageDelivered/4";
pub const BULK_SCHEMA_MESSAGE_UNDELIVERED_V4: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageUndelivered/4";
pub const BULK_SCHEMA_MESSAGE_READ_V4: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageRead/4";
pub const BULK_SCHEMA_MESSAGE_INBOUND_V2: &str =
    "https://events-schemas.twilio.com/CommsApi.MessageInbound/2";

/// Operation event payload, with future fields retained.
#[derive(Clone, Deserialize)]
pub struct BulkOperationEvent {
    pub operation_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for BulkOperationEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkOperationEvent", f)
    }
}

impl BulkOperationEvent {
    fn redact(&mut self) {
        redact_option(&mut self.operation_id);
        self.extra.clear();
    }
}

/// Outbound message event payload shared by queued and terminal events.
#[derive(Clone, Deserialize)]
pub struct BulkOutboundMessageEvent {
    pub operation_id: Option<String>,
    pub message_id: Option<String>,
    pub account_sid: Option<String>,
    pub downstream_id: Option<String>,
    pub attempt: Option<String>,
    pub error_code: Option<String>,
    pub from: Option<BulkMessagingEventAddress>,
    pub to: Option<BulkMessagingEventAddress>,
    pub tags: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for BulkOutboundMessageEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkOutboundMessageEvent", f)
    }
}

impl BulkOutboundMessageEvent {
    fn redact(&mut self) {
        redact_option(&mut self.operation_id);
        redact_option(&mut self.message_id);
        redact_option(&mut self.account_sid);
        redact_option(&mut self.downstream_id);
        redact_option(&mut self.attempt);
        redact_option(&mut self.error_code);
        if let Some(from) = &mut self.from {
            from.redact();
        }
        if let Some(to) = &mut self.to {
            to.redact();
        }
        if let Some(tags) = &mut self.tags {
            tags.clear();
        }
        self.extra.clear();
    }
}

/// One source or destination address in a Bulk Messaging event.
#[derive(Clone, Deserialize)]
pub struct BulkMessagingEventAddress {
    pub id: Option<String>,
    pub address: Option<String>,
    pub channel: Option<BulkMessagingValue>,
}

impl fmt::Debug for BulkMessagingEventAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkMessagingEventAddress", f)
    }
}

impl BulkMessagingEventAddress {
    fn redact(&mut self) {
        redact_option(&mut self.id);
        redact_option(&mut self.address);
        if self.channel.is_some() {
            self.channel = Some(BulkMessagingValue::new(REDACTED_EVENT_VALUE));
        }
    }
}

/// Inbound-received message event payload.
#[derive(Clone, Deserialize)]
pub struct BulkInboundMessageEvent {
    pub operation_id: Option<String>,
    pub message_id: Option<String>,
    pub downstream_id: Option<String>,
    pub session_id: Option<String>,
    pub to: Option<BulkMessagingEventAddress>,
    pub tags: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl fmt::Debug for BulkInboundMessageEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkInboundMessageEvent", f)
    }
}

impl BulkInboundMessageEvent {
    fn redact(&mut self) {
        redact_option(&mut self.operation_id);
        redact_option(&mut self.message_id);
        redact_option(&mut self.downstream_id);
        redact_option(&mut self.session_id);
        if let Some(to) = &mut self.to {
            to.redact();
        }
        if let Some(tags) = &mut self.tags {
            tags.clear();
        }
        self.extra.clear();
    }
}

fn redact_option(value: &mut Option<String>) {
    if value.is_some() {
        *value = Some(REDACTED_EVENT_VALUE.to_owned());
    }
}

/// Typed event payload dispatch. Event payload values are redacted before return.
#[derive(Clone)]
pub enum BulkMessagingEventData {
    OperationProcessing(BulkOperationEvent),
    OperationCompleted(BulkOperationEvent),
    OperationScheduled(BulkOperationEvent),
    MessageQueued(BulkOutboundMessageEvent),
    MessageSent(BulkOutboundMessageEvent),
    MessageFailed(BulkOutboundMessageEvent),
    MessageDelivered(BulkOutboundMessageEvent),
    MessageUndelivered(BulkOutboundMessageEvent),
    MessageRead(BulkOutboundMessageEvent),
    MessageInboundReceived(BulkInboundMessageEvent),
    Unknown,
}

impl fmt::Debug for BulkMessagingEventData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BulkMessagingEventData(<redacted>)")
    }
}

/// One `CloudEvents` 1.0 Bulk Messaging event.
#[derive(Clone)]
pub struct BulkMessagingEvent {
    pub specversion: String,
    pub id: String,
    pub source: String,
    pub event_type: BulkMessagingValue,
    pub dataschema: Option<String>,
    pub time: Option<OffsetDateTime>,
    pub data: BulkMessagingEventData,
}

impl fmt::Debug for BulkMessagingEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        redacted("BulkMessagingEvent", f)
    }
}

impl<'de> Deserialize<'de> for BulkMessagingEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawEvent {
            specversion: String,
            #[serde(rename = "id")]
            _id: String,
            #[serde(rename = "source")]
            _source: String,
            #[serde(rename = "type")]
            event_type: String,
            dataschema: Option<String>,
            time: Option<String>,
            data: Value,
        }

        let raw = RawEvent::deserialize(deserializer)?;
        if raw.specversion != "1.0" {
            return Err(serde::de::Error::custom(
                "unsupported CloudEvents specversion",
            ));
        }
        let data = match raw.data {
            Value::String(encoded) => serde_json::from_str(&encoded)
                .map_err(|_| serde::de::Error::custom("event data is not valid JSON"))?,
            Value::Object(_) => raw.data,
            _ => {
                return Err(serde::de::Error::custom(
                    "event data must be a JSON object or encoded object",
                ));
            }
        };
        let typed = match raw.event_type.as_str() {
            BULK_EVENT_OPERATION_PROCESSING => BulkMessagingEventData::OperationProcessing(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_OPERATION_COMPLETED => BulkMessagingEventData::OperationCompleted(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_OPERATION_SCHEDULED => BulkMessagingEventData::OperationScheduled(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_QUEUED => BulkMessagingEventData::MessageQueued(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_SENT => BulkMessagingEventData::MessageSent(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_FAILED => BulkMessagingEventData::MessageFailed(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_DELIVERED => BulkMessagingEventData::MessageDelivered(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_UNDELIVERED => BulkMessagingEventData::MessageUndelivered(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_READ => BulkMessagingEventData::MessageRead(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            BULK_EVENT_MESSAGE_INBOUND_RECEIVED => BulkMessagingEventData::MessageInboundReceived(
                serde_json::from_value(data).map_err(serde::de::Error::custom)?,
            ),
            _ => BulkMessagingEventData::Unknown,
        };
        let mut typed = typed;
        match &mut typed {
            BulkMessagingEventData::OperationProcessing(event)
            | BulkMessagingEventData::OperationCompleted(event)
            | BulkMessagingEventData::OperationScheduled(event) => event.redact(),
            BulkMessagingEventData::MessageQueued(event)
            | BulkMessagingEventData::MessageSent(event)
            | BulkMessagingEventData::MessageFailed(event)
            | BulkMessagingEventData::MessageDelivered(event)
            | BulkMessagingEventData::MessageUndelivered(event)
            | BulkMessagingEventData::MessageRead(event) => event.redact(),
            BulkMessagingEventData::MessageInboundReceived(event) => event.redact(),
            BulkMessagingEventData::Unknown => {}
        }
        let time = raw
            .time
            .map(|value| OffsetDateTime::parse(&value, &Rfc3339))
            .transpose()
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            specversion: raw.specversion,
            id: REDACTED_EVENT_VALUE.to_owned(),
            source: REDACTED_EVENT_VALUE.to_owned(),
            event_type: BulkMessagingValue::new(raw.event_type),
            dataschema: raw.dataschema.map(|_| REDACTED_EVENT_VALUE.to_owned()),
            time,
            data: typed,
        })
    }
}

/// Decode the array body delivered by a Twilio Event Streams webhook.
///
/// Duplicate and out-of-order events are returned unchanged.
/// # Errors
///
/// Returns [`TwilioError`] for request validation, transport, API, or response-decoding failures.
pub fn parse_bulk_messaging_events(body: &[u8]) -> Result<Vec<BulkMessagingEvent>, TwilioError> {
    serde_json::from_slice(body)
        .map_err(|_| TwilioError::Decode("malformed Bulk Messaging event batch".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{
        BulkSenderChannel, BulkSenderChannelControls, BulkSenderChannelPriority,
        BulkSenderPoolSenderRequest, CreateBulkSenderPoolRequest,
    };

    fn sender_id(index: usize) -> String {
        format!("comms_sender_{index:026x}")
    }

    #[test]
    fn pool_creation_accepts_empty_and_ten_thousand_initial_senders() {
        let empty: [BulkSenderPoolSenderRequest<'_>; 0] = [];
        assert!(
            CreateBulkSenderPoolRequest::new("pool")
                .senders(&empty)
                .validate()
                .is_ok()
        );

        let ids: Vec<_> = (0..10_000).map(sender_id).collect();
        let senders: Vec<_> = ids
            .iter()
            .map(|id| BulkSenderPoolSenderRequest::new(id))
            .collect();
        assert!(
            CreateBulkSenderPoolRequest::new("pool")
                .senders(&senders)
                .validate()
                .is_ok()
        );

        let extra_id = sender_id(10_000);
        let mut too_many = senders;
        too_many.push(BulkSenderPoolSenderRequest::new(&extra_id));
        assert!(
            CreateBulkSenderPoolRequest::new("pool")
                .senders(&too_many)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn channel_controls_allow_sparse_priorities_but_not_duplicate_channels() {
        assert!(
            BulkSenderChannelControls::new()
                .priority([BulkSenderChannelPriority::new(BulkSenderChannel::Sms, 10)])
                .validate()
                .is_ok()
        );
        assert!(
            BulkSenderChannelControls::new()
                .priority([
                    BulkSenderChannelPriority::new(BulkSenderChannel::Sms, 0),
                    BulkSenderChannelPriority::new(BulkSenderChannel::Sms, 10),
                ])
                .validate()
                .is_err()
        );
    }
}
