#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::fmt;

use http::Method;
use serde::{Deserialize, Serialize, Serializer};
#[cfg(feature = "async")]
use tracing::Instrument as _;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "async")]
use crate::common::request_span;
use crate::common::{ApiFamily, FormParam, RequestSpec, TwilioAuth, TwilioError, push_str};

#[derive(Clone, Copy)]
pub struct CreateMessagingV2TypingIndicatorRequest<'a> {
    message_id: &'a str,
}

impl<'a> CreateMessagingV2TypingIndicatorRequest<'a> {
    #[must_use]
    pub fn whatsapp(message_id: &'a str) -> Self {
        Self { message_id }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("messageId", self.message_id)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "channel", Some("whatsapp"));
        push_str(&mut params, "messageId", Some(self.message_id));
        params
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![self.message_id]
    }
}

impl fmt::Debug for CreateMessagingV2TypingIndicatorRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateMessagingV2TypingIndicatorRequest")
            .field("channel", &"whatsapp")
            .field("message_id", &crate::common::REDACTED)
            .finish()
    }
}

/// Typing-indicator event accepted by Apple and RCS channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypingEvent {
    Start,
    End,
}

impl Serialize for TypingEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Start => "START",
            Self::End => "END",
        })
    }
}

/// Backwards-compatible name for [`TypingEvent`].
pub type AppleTypingEvent = TypingEvent;

#[derive(Clone, Copy)]
pub struct CreateMessagingV3TypingIndicatorRequest<'a> {
    kind: MessagingV3TypingIndicatorKind<'a>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(untagged)]
enum MessagingV3TypingIndicatorKind<'a> {
    WhatsApp {
        channel: &'static str,
        #[serde(rename = "messageId")]
        message_id: &'a str,
    },
    Apple {
        channel: &'static str,
        from: &'a str,
        to: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        event: Option<TypingEvent>,
    },
    Rcs {
        channel: &'static str,
        from: &'a str,
        to: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        event: Option<TypingEvent>,
    },
}

impl<'a> CreateMessagingV3TypingIndicatorRequest<'a> {
    #[must_use]
    pub fn whatsapp(message_id: &'a str) -> Self {
        Self {
            kind: MessagingV3TypingIndicatorKind::WhatsApp {
                channel: "WHATSAPP",
                message_id,
            },
        }
    }

    #[must_use]
    pub fn apple(from: &'a str, to: &'a str) -> Self {
        Self {
            kind: MessagingV3TypingIndicatorKind::Apple {
                channel: "APPLE",
                from,
                to,
                event: None,
            },
        }
    }

    /// Create an RCS typing indicator request.
    #[must_use]
    pub fn rcs(from: &'a str, to: &'a str) -> Self {
        Self {
            kind: MessagingV3TypingIndicatorKind::Rcs {
                channel: "RCS",
                from,
                to,
                event: None,
            },
        }
    }

    #[must_use]
    pub fn event(mut self, value: TypingEvent) -> Self {
        if let MessagingV3TypingIndicatorKind::Apple { event, .. }
        | MessagingV3TypingIndicatorKind::Rcs { event, .. } = &mut self.kind
        {
            *event = Some(value);
        }
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        match self.kind {
            MessagingV3TypingIndicatorKind::WhatsApp { message_id, .. } => {
                validate_required("messageId", message_id)
            }
            MessagingV3TypingIndicatorKind::Apple { from, to, .. } => {
                validate_required("from", from)?;
                validate_required("to", to)
            }
            MessagingV3TypingIndicatorKind::Rcs { from, to, .. } => {
                validate_required("from", from)?;
                validate_required("to", to)
            }
        }
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        match self.kind {
            MessagingV3TypingIndicatorKind::WhatsApp { message_id, .. } => vec![message_id],
            MessagingV3TypingIndicatorKind::Apple { from, to, .. }
            | MessagingV3TypingIndicatorKind::Rcs { from, to, .. } => vec![from, to],
        }
    }
}

impl Serialize for CreateMessagingV3TypingIndicatorRequest<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.kind.serialize(serializer)
    }
}

impl fmt::Debug for CreateMessagingV3TypingIndicatorRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            MessagingV3TypingIndicatorKind::WhatsApp { .. } => f
                .debug_struct("CreateMessagingV3TypingIndicatorRequest")
                .field("channel", &"WHATSAPP")
                .field("message_id", &crate::common::REDACTED)
                .finish(),
            MessagingV3TypingIndicatorKind::Apple { event, .. } => f
                .debug_struct("CreateMessagingV3TypingIndicatorRequest")
                .field("channel", &"APPLE")
                .field("from", &crate::common::REDACTED)
                .field("to", &crate::common::REDACTED)
                .field("event", &event)
                .finish(),
            MessagingV3TypingIndicatorKind::Rcs { event, .. } => f
                .debug_struct("CreateMessagingV3TypingIndicatorRequest")
                .field("channel", &"RCS")
                .field("from", &crate::common::REDACTED)
                .field("to", &crate::common::REDACTED)
                .field("event", &event)
                .finish(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TwilioTypingIndicator {
    pub success: Option<bool>,
}

#[derive(Deserialize)]
struct WireTypingIndicator {
    success: Option<bool>,
}

impl WireTypingIndicator {
    fn into_indicator(self) -> TwilioTypingIndicator {
        TwilioTypingIndicator {
            success: self.success,
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

fn sensitive_values(creds: &TwilioAuth) -> Vec<&str> {
    vec![creds.account_sid(), creds.auth_secret()]
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2TypingIndicatorsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2TypingIndicatorsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Indicators/Typing.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateMessagingV2TypingIndicatorRequest<'a>,
    ) -> Result<TwilioTypingIndicator, TwilioError> {
        async move {
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account.creds);
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV2,
                Method::POST,
                ["Indicators", "Typing.json"],
            )
            .operation("messaging.v2.typing_indicators.create")
            .form_params(request.form_params());
            let parsed: WireTypingIndicator =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_indicator())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v2.typing_indicators.create",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV3TypingIndicatorsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV3TypingIndicatorsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Indicators/Typing.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub async fn create(
        self,
        request: CreateMessagingV3TypingIndicatorRequest<'a>,
    ) -> Result<TwilioTypingIndicator, TwilioError> {
        async move {
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account.creds);
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV3,
                Method::POST,
                ["Indicators", "Typing.json"],
            )
            .operation("messaging.v3.typing_indicators.create")
            .json_body(&request)?;
            let parsed: WireTypingIndicator =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_indicator())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v3.typing_indicators.create",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2TypingIndicatorsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2TypingIndicatorsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Indicators/Typing.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn create(
        self,
        request: CreateMessagingV2TypingIndicatorRequest<'a>,
    ) -> Result<TwilioTypingIndicator, TwilioError> {
        request.validate()?;
        let mut sensitive_values = sensitive_values(self.account.creds);
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV2,
            Method::POST,
            ["Indicators", "Typing.json"],
        )
        .operation("messaging.v2.typing_indicators.create")
        .form_params(request.form_params());
        let parsed: WireTypingIndicator = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_indicator())
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV3TypingIndicatorsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV3TypingIndicatorsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Indicators/Typing.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, JSON serialization
    /// failures, transport failures, non-2xx API responses, or malformed JSON
    /// responses.
    pub fn create(
        self,
        request: CreateMessagingV3TypingIndicatorRequest<'a>,
    ) -> Result<TwilioTypingIndicator, TwilioError> {
        request.validate()?;
        let mut sensitive_values = sensitive_values(self.account.creds);
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV3,
            Method::POST,
            ["Indicators", "Typing.json"],
        )
        .operation("messaging.v3.typing_indicators.create")
        .json_body(&request)?;
        let parsed: WireTypingIndicator = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_indicator())
    }
}
