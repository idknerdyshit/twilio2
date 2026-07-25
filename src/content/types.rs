//! Typed payloads for Twilio Content templates.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{REDACTED, TwilioError};

fn invalid(message: &'static str) -> TwilioError {
    TwilioError::InvalidRequest(message.to_owned())
}

fn require(value: &str, message: &'static str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        Err(invalid(message))
    } else {
        Ok(())
    }
}

fn require_count<T>(
    values: &[T],
    min: usize,
    max: usize,
    message: &'static str,
) -> Result<(), TwilioError> {
    if (min..=max).contains(&values.len()) {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn validate_flow_component(component: &ContentFlowComponent) -> Result<(), TwilioError> {
    match component {
        ContentFlowComponent::ShortText { label, text, .. }
        | ContentFlowComponent::LongText { label, text, .. } => {
            require(label, "flow input label must not be empty")?;
            require(text, "flow input helper text must not be empty")
        }
        ContentFlowComponent::SingleSelect {
            label,
            text,
            options,
        }
        | ContentFlowComponent::MultiSelect {
            label,
            text,
            options,
        } => {
            require(label, "flow select label must not be empty")?;
            require(text, "flow select helper text must not be empty")?;
            require_count(
                options.options(),
                1,
                usize::MAX,
                "flow select options must not be empty",
            )
        }
        ContentFlowComponent::DatePicker {
            label,
            min_date,
            max_date,
            unavailable_dates,
            ..
        } => {
            require(label, "flow date picker label must not be empty")?;
            require(min_date, "flow minimum date must not be empty")?;
            require(max_date, "flow maximum date must not be empty")?;
            require(
                unavailable_dates,
                "flow unavailable dates must not be empty",
            )
        }
        ContentFlowComponent::List { label, options } => {
            require(label, "flow list label must not be empty")?;
            require_count(
                options.options(),
                1,
                usize::MAX,
                "flow list options must not be empty",
            )
        }
        ContentFlowComponent::TextHeading { text }
        | ContentFlowComponent::TextSubheading { text }
        | ContentFlowComponent::TextCaption { text }
        | ContentFlowComponent::TextBody { text } => {
            require(text, "flow text component must not be empty")
        }
        ContentFlowComponent::RichText { text_list } => require_count(
            text_list,
            1,
            usize::MAX,
            "flow rich text list must not be empty",
        ),
        ContentFlowComponent::Media { url } => require(url, "flow media URL must not be empty"),
        ContentFlowComponent::Footer { label } => {
            require(label, "flow footer label must not be empty")
        }
    }
}

macro_rules! redacted_debug {
    ($type:ty) => {
        impl fmt::Debug for $type {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($type))
                    .field("payload", &REDACTED)
                    .finish()
            }
        }
    };
}

/// A `twilio/text` payload.
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
redacted_debug!(ContentText);

/// A `twilio/media` payload.
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
redacted_debug!(ContentMedia);

/// A `twilio/location` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentLocation {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}
impl ContentLocation {
    #[must_use]
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            label: None,
            id: None,
            address: None,
        }
    }
    #[must_use]
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }
    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.id = Some(value.into());
        self
    }
    #[must_use]
    pub fn address(mut self, value: impl Into<String>) -> Self {
        self.address = Some(value.into());
        self
    }
}
redacted_debug!(ContentLocation);

/// One row in a `twilio/list-picker` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentListItem {
    pub id: String,
    pub item: String,
    pub description: String,
}
impl ContentListItem {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        item: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            item: item.into(),
            description: description.into(),
        }
    }
}
redacted_debug!(ContentListItem);

/// A `twilio/list-picker` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentListPicker {
    pub body: String,
    pub button: String,
    pub items: Vec<ContentListItem>,
}
impl ContentListPicker {
    #[must_use]
    pub fn new(body: impl Into<String>, button: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            button: button.into(),
            items: Vec::new(),
        }
    }
    #[must_use]
    pub fn item(mut self, value: ContentListItem) -> Self {
        self.items.push(value);
        self
    }
}
redacted_debug!(ContentListPicker);

/// A call-to-action button. The tagged variants prevent invalid field combinations.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentCallToActionAction {
    #[serde(rename = "URL")]
    Url { title: String, url: String },
    #[serde(rename = "PHONE_NUMBER")]
    Phone { title: String, phone: String },
    #[serde(rename = "COPY_CODE")]
    CopyCode { title: String, code: String },
    #[serde(rename = "VOICE_CALL")]
    VoiceCall { title: String, phone: String },
    #[serde(rename = "VOICE_CALL_REQUEST")]
    VoiceCallRequest { title: String, id: String },
}
impl ContentCallToActionAction {
    #[must_use]
    pub fn url(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self::Url {
            title: title.into(),
            url: url.into(),
        }
    }
    #[must_use]
    pub fn phone(title: impl Into<String>, phone: impl Into<String>) -> Self {
        Self::Phone {
            title: title.into(),
            phone: phone.into(),
        }
    }
    #[must_use]
    pub fn copy_code(title: impl Into<String>, code: impl Into<String>) -> Self {
        Self::CopyCode {
            title: title.into(),
            code: code.into(),
        }
    }
}
redacted_debug!(ContentCallToActionAction);

/// A `twilio/call-to-action` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentCallToAction {
    pub body: String,
    pub actions: Vec<ContentCallToActionAction>,
}
impl ContentCallToAction {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            actions: Vec::new(),
        }
    }
    #[must_use]
    pub fn action(mut self, action: ContentCallToActionAction) -> Self {
        self.actions.push(action);
        self
    }
}
redacted_debug!(ContentCallToAction);

/// A quick-reply button.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentQuickReplyAction {
    #[serde(rename = "type", default = "quick_reply_type")]
    action_type: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
fn quick_reply_type() -> String {
    "QUICK_REPLY".to_owned()
}
impl ContentQuickReplyAction {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            action_type: quick_reply_type(),
            title: title.into(),
            id: None,
        }
    }
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}
redacted_debug!(ContentQuickReplyAction);

/// A `twilio/quick-reply` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentQuickReply {
    pub body: String,
    pub actions: Vec<ContentQuickReplyAction>,
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
    pub fn action(mut self, action: ContentQuickReplyAction) -> Self {
        self.actions.push(action);
        self
    }
}
redacted_debug!(ContentQuickReply);

/// Size of a URL action's in-app web view.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentWebviewSize {
    Tall,
    Full,
    Half,
    None,
}

/// An action valid on `twilio/card` and `whatsapp/card`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentCardAction {
    #[serde(rename = "URL")]
    Url {
        title: String,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        webview_size: Option<ContentWebviewSize>,
    },
    #[serde(rename = "PHONE_NUMBER")]
    Phone { title: String, phone: String },
    #[serde(rename = "QUICK_REPLY")]
    QuickReply { title: String, id: String },
    #[serde(rename = "COPY_CODE")]
    CopyCode { title: String, code: String },
    #[serde(rename = "VOICE_CALL")]
    VoiceCall { title: String, phone: String },
}
impl ContentCardAction {
    #[must_use]
    pub fn url(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self::Url {
            title: title.into(),
            url: url.into(),
            webview_size: None,
        }
    }
    #[must_use]
    pub fn phone(title: impl Into<String>, phone: impl Into<String>) -> Self {
        Self::Phone {
            title: title.into(),
            phone: phone.into(),
        }
    }
    #[must_use]
    pub fn quick_reply(title: impl Into<String>, id: impl Into<String>) -> Self {
        Self::QuickReply {
            title: title.into(),
            id: id.into(),
        }
    }
}
redacted_debug!(ContentCardAction);

/// A `twilio/card` payload.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentCardOrientation {
    Horizontal,
    Vertical,
}

/// Alignment of a thumbnail in a horizontal RCS card.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentCardThumbnailAlignment {
    Left,
    Right,
}

/// Height of an RCS card.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentCardHeight {
    Short,
    Medium,
    Tall,
}

/// A `twilio/card` payload.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ContentCard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ContentCardOrientation>,
    #[serde(
        rename = "thumbnailImageAlignment",
        skip_serializing_if = "Option::is_none"
    )]
    pub thumbnail_image_alignment: Option<ContentCardThumbnailAlignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<ContentCardHeight>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ContentCardAction>,
}
impl ContentCard {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Construct a card using the documented body-only alternative.
    #[must_use]
    pub fn body_only(body: impl Into<String>) -> Self {
        Self {
            body: Some(body.into()),
            ..Self::default()
        }
    }
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }
    #[must_use]
    pub fn body(mut self, value: impl Into<String>) -> Self {
        self.body = Some(value.into());
        self
    }
    #[must_use]
    pub fn subtitle(mut self, value: impl Into<String>) -> Self {
        self.subtitle = Some(value.into());
        self
    }
    #[must_use]
    pub fn orientation(mut self, value: ContentCardOrientation) -> Self {
        self.orientation = Some(value);
        self
    }
    #[must_use]
    pub fn thumbnail_image_alignment(mut self, value: ContentCardThumbnailAlignment) -> Self {
        self.thumbnail_image_alignment = Some(value);
        self
    }
    #[must_use]
    pub fn height(mut self, value: ContentCardHeight) -> Self {
        self.height = Some(value);
        self
    }
    #[must_use]
    pub fn media(mut self, value: impl Into<String>) -> Self {
        self.media.push(value.into());
        self
    }
    #[must_use]
    pub fn action(mut self, value: ContentCardAction) -> Self {
        self.actions.push(value);
        self
    }
}
redacted_debug!(ContentCard);

/// A `whatsapp/card` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct WhatsappCard {
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ContentCardAction>,
}
impl WhatsappCard {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            footer: None,
            header_text: None,
            media: Vec::new(),
            actions: Vec::new(),
        }
    }
    #[must_use]
    pub fn footer(mut self, value: impl Into<String>) -> Self {
        self.footer = Some(value.into());
        self
    }
    #[must_use]
    pub fn header_text(mut self, value: impl Into<String>) -> Self {
        self.header_text = Some(value.into());
        self
    }
    #[must_use]
    pub fn media(mut self, value: impl Into<String>) -> Self {
        self.media.push(value.into());
        self
    }
    #[must_use]
    pub fn action(mut self, value: ContentCardAction) -> Self {
        self.actions.push(value);
        self
    }
}
redacted_debug!(WhatsappCard);

/// An action valid on a carousel card.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentCarouselAction {
    #[serde(rename = "URL")]
    Url { title: String, url: String },
    #[serde(rename = "PHONE_NUMBER")]
    Phone { title: String, phone: String },
    #[serde(rename = "QUICK_REPLY")]
    QuickReply { title: String, id: String },
}
redacted_debug!(ContentCarouselAction);

/// One card in a carousel.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentCarouselCard {
    pub body: String,
    pub media: String,
    pub actions: Vec<ContentCarouselAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
impl ContentCarouselCard {
    #[must_use]
    pub fn new(body: impl Into<String>, media: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            media: media.into(),
            actions: Vec::new(),
            title: None,
        }
    }
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }
    #[must_use]
    pub fn action(mut self, value: ContentCarouselAction) -> Self {
        self.actions.push(value);
        self
    }
}
redacted_debug!(ContentCarouselCard);

/// A `twilio/carousel` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentCarousel {
    pub body: String,
    pub cards: Vec<ContentCarouselCard>,
}
impl ContentCarousel {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            cards: Vec::new(),
        }
    }
    #[must_use]
    pub fn card(mut self, value: ContentCarouselCard) -> Self {
        self.cards.push(value);
        self
    }
}
redacted_debug!(ContentCarousel);

/// A static or dynamic catalog item descriptor.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentCatalogItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
impl ContentCatalogItem {
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: None,
            section_title: None,
            name: None,
            media_url: None,
            price: None,
            description: None,
        }
    }
    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.id = Some(value.into());
        self
    }
    #[must_use]
    pub fn section_title(mut self, value: impl Into<String>) -> Self {
        self.section_title = Some(value.into());
        self
    }
}
impl Default for ContentCatalogItem {
    fn default() -> Self {
        Self::new()
    }
}
redacted_debug!(ContentCatalogItem);

/// A `twilio/catalog` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentCatalog {
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ContentCatalogItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_items: Option<String>,
}
impl ContentCatalog {
    #[must_use]
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: body.into(),
            id: None,
            title: None,
            subtitle: None,
            items: Vec::new(),
            dynamic_items: None,
        }
    }
    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.id = Some(value.into());
        self
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
    pub fn item(mut self, value: ContentCatalogItem) -> Self {
        self.items.push(value);
        self
    }
    #[must_use]
    pub fn dynamic_items(mut self, value: impl Into<String>) -> Self {
        self.dynamic_items = Some(value.into());
        self
    }
}
redacted_debug!(ContentCatalog);

/// Pix key kind.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentPixKeyType {
    Cpf,
    Cnpj,
    Email,
    Phone,
    Evp,
}

/// Pix order status.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentPixStatus {
    Pending,
    Processing,
    PartiallyShipped,
    Shipped,
    Completed,
    Canceled,
}

/// Pix details for a payment message.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPix {
    #[serde(rename = "ORDER_DETAILS")]
    OrderDetails {
        code: String,
        key_type: ContentPixKeyType,
        key: String,
    },
    #[serde(rename = "ORDER_STATUS")]
    OrderStatus { status: ContentPixStatus },
}
redacted_debug!(ContentPix);

/// A `twilio/pay` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentPay {
    pub payment_id: String,
    pub body: String,
    pub merchant_name: String,
    pub country_code: String,
    pub currency_code: String,
    pub items: String,
    pub order_expiration: String,
    pub order_expiration_description: String,
    pub subtotal_amount: String,
    pub total_amount: String,
    pub pix: ContentPix,
}
impl ContentPay {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        payment_id: impl Into<String>,
        body: impl Into<String>,
        merchant_name: impl Into<String>,
        items: impl Into<String>,
        order_expiration: impl Into<String>,
        order_expiration_description: impl Into<String>,
        subtotal_amount: impl Into<String>,
        total_amount: impl Into<String>,
        pix: ContentPix,
    ) -> Self {
        Self {
            payment_id: payment_id.into(),
            body: body.into(),
            merchant_name: merchant_name.into(),
            country_code: "BR".to_owned(),
            currency_code: "BRL".to_owned(),
            items: items.into(),
            order_expiration: order_expiration.into(),
            order_expiration_description: order_expiration_description.into(),
            subtotal_amount: subtotal_amount.into(),
            total_amount: total_amount.into(),
            pix,
        }
    }
}
redacted_debug!(ContentPay);

/// Category of a Twilio-hosted Flow.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentFlowType {
    SignUp,
    SignIn,
    AppointmentBooking,
    LeadGeneration,
    ContactUs,
    CustomerSupport,
    Survey,
    Other,
}

/// Input keyboard for a Flow text field.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentFlowInputType {
    Text,
    Number,
    Email,
    Password,
    Passcode,
    Phone,
}

/// A selectable Flow option.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentFlowOption {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
impl ContentFlowOption {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
        }
    }
}
redacted_debug!(ContentFlowOption);

/// A Flow option list encoded in Twilio's required stringified-JSON form.
#[derive(Clone)]
pub struct ContentFlowOptions(Vec<ContentFlowOption>);
impl ContentFlowOptions {
    #[must_use]
    pub fn new(options: impl IntoIterator<Item = ContentFlowOption>) -> Self {
        Self(options.into_iter().collect())
    }

    #[must_use]
    pub fn options(&self) -> &[ContentFlowOption] {
        &self.0
    }
}
impl Serialize for ContentFlowOptions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let encoded = serde_json::to_string(&self.0).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&encoded)
    }
}
impl<'de> Deserialize<'de> for ContentFlowOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        serde_json::from_str(&encoded)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
redacted_debug!(ContentFlowOptions);

/// A component in a Twilio-hosted Flow page.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentFlowComponent {
    #[serde(rename = "SHORT_TEXT")]
    ShortText {
        label: String,
        text: String,
        #[serde(default)]
        required: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        input_type: Option<ContentFlowInputType>,
    },
    #[serde(rename = "LONG_TEXT")]
    LongText {
        label: String,
        text: String,
        #[serde(default)]
        required: bool,
    },
    #[serde(rename = "SINGLE_SELECT")]
    SingleSelect {
        label: String,
        text: String,
        options: ContentFlowOptions,
    },
    #[serde(rename = "MULTI_SELECT")]
    MultiSelect {
        label: String,
        text: String,
        options: ContentFlowOptions,
    },
    #[serde(rename = "DATE_PICKER")]
    DatePicker {
        label: String,
        min_date: String,
        max_date: String,
        unavailable_dates: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "LIST")]
    List {
        label: String,
        options: ContentFlowOptions,
    },
    #[serde(rename = "TEXT_HEADING")]
    TextHeading { text: String },
    #[serde(rename = "TEXT_SUBHEADING")]
    TextSubheading { text: String },
    #[serde(rename = "TEXT_CAPTION")]
    TextCaption { text: String },
    #[serde(rename = "TEXT_BODY")]
    TextBody { text: String },
    #[serde(rename = "RICH_TEXT")]
    RichText { text_list: Vec<String> },
    #[serde(rename = "MEDIA")]
    Media { url: String },
    #[serde(rename = "FOOTER")]
    Footer { label: String },
}
redacted_debug!(ContentFlowComponent);

/// One page of a Twilio-hosted Flow.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentFlowPage {
    pub id: String,
    pub layout: Vec<ContentFlowComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_id: Option<String>,
}
impl ContentFlowPage {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            layout: Vec::new(),
            title: None,
            subtitle: None,
            next_page_id: None,
        }
    }
    #[must_use]
    pub fn component(mut self, value: ContentFlowComponent) -> Self {
        self.layout.push(value);
        self
    }
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }
}
redacted_debug!(ContentFlowPage);

/// A `twilio/flows` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentFlows {
    pub body: String,
    pub button_text: String,
    #[serde(rename = "type")]
    pub flow_type: ContentFlowType,
    pub pages: Vec<ContentFlowPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
}
impl ContentFlows {
    #[must_use]
    pub fn new(
        body: impl Into<String>,
        button_text: impl Into<String>,
        flow_type: ContentFlowType,
    ) -> Self {
        Self {
            body: body.into(),
            button_text: button_text.into(),
            flow_type,
            pages: Vec::new(),
            subtitle: None,
            media_url: None,
        }
    }
    #[must_use]
    pub fn page(mut self, value: ContentFlowPage) -> Self {
        self.pages.push(value);
        self
    }
    #[must_use]
    pub fn subtitle(mut self, value: impl Into<String>) -> Self {
        self.subtitle = Some(value.into());
        self
    }
    #[must_use]
    pub fn media_url(mut self, value: impl Into<String>) -> Self {
        self.media_url = Some(value.into());
        self
    }
}
redacted_debug!(ContentFlows);

/// A `twilio/schedule` payload from the official Content `OpenAPI` schema.
#[derive(Clone, Serialize, Deserialize)]
pub struct ContentSchedule {
    pub id: String,
    pub title: String,
    #[serde(rename = "timeSlots")]
    pub time_slots: String,
}
impl ContentSchedule {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        time_slots: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            time_slots: time_slots.into(),
        }
    }
}
redacted_debug!(ContentSchedule);

/// The sole `WhatsApp` authentication action.
#[derive(Clone, Serialize, Deserialize)]
pub struct WhatsappAuthenticationAction {
    #[serde(rename = "type", default = "copy_code_type")]
    action_type: String,
    pub copy_code_text: String,
}
fn copy_code_type() -> String {
    "COPY_CODE".to_owned()
}
impl WhatsappAuthenticationAction {
    #[must_use]
    pub fn new(copy_code_text: impl Into<String>) -> Self {
        Self {
            action_type: copy_code_type(),
            copy_code_text: copy_code_text.into(),
        }
    }
}
redacted_debug!(WhatsappAuthenticationAction);

/// A `whatsapp/authentication` payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct WhatsappAuthentication {
    pub actions: Vec<WhatsappAuthenticationAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add_security_recommendation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_expiration_minutes: Option<u16>,
}
impl WhatsappAuthentication {
    #[must_use]
    pub fn new(action: WhatsappAuthenticationAction) -> Self {
        Self {
            actions: vec![action],
            add_security_recommendation: None,
            code_expiration_minutes: None,
        }
    }
    #[must_use]
    pub fn add_security_recommendation(mut self, value: bool) -> Self {
        self.add_security_recommendation = Some(value);
        self
    }
    #[must_use]
    pub fn code_expiration_minutes(mut self, value: u16) -> Self {
        self.code_expiration_minutes = Some(value);
        self
    }
}
redacted_debug!(WhatsappAuthentication);

/// A `whatsapp/flows` payload referencing a Meta-hosted Flow.
#[derive(Clone, Serialize, Deserialize)]
pub struct WhatsappFlows {
    pub body: String,
    pub button_text: String,
    pub flow_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_first_page_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_flow_first_page_endpoint: Option<bool>,
}
impl WhatsappFlows {
    #[must_use]
    pub fn new(
        body: impl Into<String>,
        button_text: impl Into<String>,
        flow_id: impl Into<String>,
    ) -> Self {
        Self {
            body: body.into(),
            button_text: button_text.into(),
            flow_id: flow_id.into(),
            subtitle: None,
            media_url: None,
            flow_token: None,
            flow_first_page_id: None,
            is_flow_first_page_endpoint: None,
        }
    }
    #[must_use]
    pub fn subtitle(mut self, value: impl Into<String>) -> Self {
        self.subtitle = Some(value.into());
        self
    }
    #[must_use]
    pub fn media_url(mut self, value: impl Into<String>) -> Self {
        self.media_url = Some(value.into());
        self
    }
    #[must_use]
    pub fn flow_token(mut self, value: impl Into<String>) -> Self {
        self.flow_token = Some(value.into());
        self
    }
    #[must_use]
    pub fn first_page_id(mut self, value: impl Into<String>) -> Self {
        self.flow_first_page_id = Some(value.into());
        self
    }
    #[must_use]
    pub fn first_page_endpoint(mut self, value: bool) -> Self {
        self.is_flow_first_page_endpoint = Some(value);
        self
    }
}
redacted_debug!(WhatsappFlows);

/// An owned, lossless map of Content type names to payloads.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentTypes(BTreeMap<String, Value>);

macro_rules! content_type_accessors {
    ($(($setter:ident, $getter:ident, $key:literal, $type:ty)),+ $(,)?) => {$ (
        #[must_use] pub fn $setter(mut self, value: $type) -> Self {
            let serialized = match serde_json::to_value(value) {
                Ok(serialized) => serialized,
                Err(error) => unreachable!("typed Content payload failed to serialize: {error}"),
            };
            self.0.insert($key.to_owned(), serialized); self
        }
        #[doc = "Decode this typed Content payload, preserving the raw map on failure."]
        #[doc = ""]
        #[doc = "# Errors"]
        #[doc = "Returns an error when the stored payload has an incompatible shape."]
        pub fn $getter(&self) -> Result<Option<$type>, serde_json::Error> { self.decode($key) }
    )+ };
}

impl ContentTypes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    content_type_accessors!(
        (text, get_text, "twilio/text", ContentText),
        (media, get_media, "twilio/media", ContentMedia),
        (location, get_location, "twilio/location", ContentLocation),
        (
            list_picker,
            get_list_picker,
            "twilio/list-picker",
            ContentListPicker
        ),
        (
            call_to_action,
            get_call_to_action,
            "twilio/call-to-action",
            ContentCallToAction
        ),
        (
            quick_reply,
            get_quick_reply,
            "twilio/quick-reply",
            ContentQuickReply
        ),
        (card, get_card, "twilio/card", ContentCard),
        (carousel, get_carousel, "twilio/carousel", ContentCarousel),
        (catalog, get_catalog, "twilio/catalog", ContentCatalog),
        (pay, get_pay, "twilio/pay", ContentPay),
        (flows, get_flows, "twilio/flows", ContentFlows),
        (schedule, get_schedule, "twilio/schedule", ContentSchedule),
        (
            whatsapp_card,
            get_whatsapp_card,
            "whatsapp/card",
            WhatsappCard
        ),
        (
            whatsapp_authentication,
            get_whatsapp_authentication,
            "whatsapp/authentication",
            WhatsappAuthentication
        ),
        (
            whatsapp_flows,
            get_whatsapp_flows,
            "whatsapp/flows",
            WhatsappFlows
        ),
    );

    /// Insert an unmodeled type as owned JSON.
    ///
    /// # Errors
    /// Returns an error for an empty, built-in, or duplicate type name.
    pub fn custom(mut self, name: impl Into<String>, value: Value) -> Result<Self, TwilioError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(invalid("content type name must not be empty"));
        }
        if matches!(
            name.as_str(),
            "twilio/text"
                | "twilio/media"
                | "twilio/location"
                | "twilio/list-picker"
                | "twilio/call-to-action"
                | "twilio/quick-reply"
                | "twilio/card"
                | "twilio/carousel"
                | "twilio/catalog"
                | "twilio/pay"
                | "twilio/flows"
                | "twilio/schedule"
                | "whatsapp/card"
                | "whatsapp/authentication"
                | "whatsapp/flows"
        ) || self.0.contains_key(&name)
        {
            return Err(invalid("content type name is reserved or duplicated"));
        }
        self.0.insert(name, value);
        Ok(self)
    }

    #[must_use]
    pub fn raw(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    /// Return whether no Content type payloads have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[must_use]
    pub fn into_raw(self) -> BTreeMap<String, Value> {
        self.0
    }

    fn decode<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, serde_json::Error> {
        self.0
            .get(key)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
    }

    /// Validate structural constraints documented by Twilio before sending.
    ///
    /// # Errors
    /// Returns [`TwilioError`] when a typed payload is empty, malformed, or exceeds a documented count.
    #[allow(clippy::too_many_lines)] // Keeping type-name dispatch together prevents validation drift.
    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        if self.0.is_empty() {
            return Err(invalid("Types must not be empty"));
        }
        for (name, value) in &self.0 {
            match name.as_str() {
                "twilio/text" => {
                    let v: ContentText = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/text payload"))?;
                    require(&v.body, "text body must not be empty")?;
                }
                "twilio/media" => {
                    let v: ContentMedia = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/media payload"))?;
                    require_count(&v.media, 1, 10, "media must contain 1 to 10 URLs")?;
                }
                "twilio/location" => {
                    let v: ContentLocation = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/location payload"))?;
                    if !v.latitude.is_finite()
                        || !v.longitude.is_finite()
                        || !(-90.0..=90.0).contains(&v.latitude)
                        || !(-180.0..=180.0).contains(&v.longitude)
                    {
                        return Err(invalid("location coordinates are out of range"));
                    }
                }
                "twilio/list-picker" => {
                    let v: ContentListPicker = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/list-picker payload"))?;
                    require_count(&v.items, 1, 10, "list picker must contain 1 to 10 items")?;
                }
                "twilio/call-to-action" => {
                    let v: ContentCallToAction = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/call-to-action payload"))?;
                    require_count(
                        &v.actions,
                        1,
                        10,
                        "call to action must contain at least one action",
                    )?;
                }
                "twilio/quick-reply" => {
                    let v: ContentQuickReply = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/quick-reply payload"))?;
                    require_count(
                        &v.actions,
                        1,
                        10,
                        "quick reply must contain 1 to 10 actions",
                    )?;
                }
                "twilio/card" => {
                    let v: ContentCard = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/card payload"))?;
                    if v.title
                        .as_deref()
                        .is_none_or(|title| title.trim().is_empty())
                        && v.body.as_deref().is_none_or(|body| body.trim().is_empty())
                    {
                        return Err(invalid("card must contain a title or body"));
                    }
                    if (v.title.is_none() || v.body.is_none())
                        && v.subtitle.is_none()
                        && v.media.is_empty()
                        && v.actions.is_empty()
                    {
                        return Err(invalid(
                            "card must contain at least one field in addition to title or body",
                        ));
                    }
                }
                "twilio/carousel" => {
                    let v: ContentCarousel = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/carousel payload"))?;
                    require_count(&v.cards, 2, 10, "carousel must contain 2 to 10 cards")?;
                    for card in v.cards {
                        require_count(
                            &card.actions,
                            1,
                            2,
                            "carousel card must contain 1 or 2 actions",
                        )?;
                    }
                }
                "twilio/catalog" => {
                    let v: ContentCatalog = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/catalog payload"))?;
                    require(&v.body, "catalog body must not be empty")?;
                    if v.items.is_empty() && v.dynamic_items.is_none() {
                        return Err(invalid("catalog must contain static or dynamic items"));
                    }
                }
                "twilio/pay" => {
                    let v: ContentPay = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/pay payload"))?;
                    require(&v.payment_id, "payment id must not be empty")?;
                    require(&v.body, "payment body must not be empty")?;
                    require(&v.merchant_name, "merchant name must not be empty")?;
                    require(&v.items, "payment items must not be empty")?;
                    if v.country_code != "BR" || v.currency_code != "BRL" {
                        return Err(invalid("twilio/pay requires BR and BRL"));
                    }
                }
                "twilio/flows" => {
                    let v: ContentFlows = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/flows payload"))?;
                    require_count(&v.pages, 1, 10, "flow must contain 1 to 10 pages")?;
                    for page in v.pages {
                        require(&page.id, "flow page id must not be empty")?;
                        if page.id.len() > 20 {
                            return Err(invalid("flow page id must not exceed 20 characters"));
                        }
                        require_count(
                            &page.layout,
                            1,
                            usize::MAX,
                            "flow page layout must not be empty",
                        )?;
                        for component in &page.layout {
                            validate_flow_component(component)?;
                        }
                    }
                }
                "whatsapp/authentication" => {
                    let v: WhatsappAuthentication = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid whatsapp/authentication payload"))?;
                    require_count(
                        &v.actions,
                        1,
                        1,
                        "authentication must contain exactly one action",
                    )?;
                    if v.code_expiration_minutes
                        .is_some_and(|minutes| !(1..=90).contains(&minutes))
                    {
                        return Err(invalid("authentication expiration must be 1 to 90 minutes"));
                    }
                }
                "twilio/schedule" => {
                    let v: ContentSchedule = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid twilio/schedule payload"))?;
                    require(&v.id, "schedule id must not be empty")?;
                    require(&v.title, "schedule title must not be empty")?;
                    require(&v.time_slots, "schedule time slots must not be empty")?;
                }
                "whatsapp/card" => {
                    let v: WhatsappCard = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid whatsapp/card payload"))?;
                    require(&v.body, "WhatsApp card body must not be empty")?;
                    require_count(
                        &v.actions,
                        1,
                        10,
                        "WhatsApp card must contain 1 to 10 actions",
                    )?;
                    if v.header_text.is_some() && !v.media.is_empty() {
                        return Err(invalid(
                            "WhatsApp card cannot contain both header text and media",
                        ));
                    }
                }
                "whatsapp/flows" => {
                    let v: WhatsappFlows = serde_json::from_value(value.clone())
                        .map_err(|_| invalid("invalid whatsapp/flows payload"))?;
                    require(&v.body, "WhatsApp flow body must not be empty")?;
                    require(
                        &v.button_text,
                        "WhatsApp flow button text must not be empty",
                    )?;
                    require(&v.flow_id, "WhatsApp flow id must not be empty")?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl fmt::Debug for ContentTypes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentTypes")
            .field(
                "payloads",
                &format_args!("{REDACTED}; {} types", self.0.len()),
            )
            .finish()
    }
}
