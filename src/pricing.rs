#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::collections::BTreeMap;
use std::fmt;

use http::Method;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
#[cfg(feature = "async")]
use tracing::Instrument as _;
use url::Url;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
use crate::common::{
    ApiFamily, DEFAULT_PAGE_SIZE, PricingPageResource, RequestSpec, TwilioError, V1PageMeta,
    WireV1PageMeta, decode_json_response, non_empty, pricing_page_url_from_base, push_sensitive,
    redacted_option, request_span, validate_page_size, validate_pricing_meta_key,
    validate_pricing_next_page_continuation,
};

#[cfg(feature = "sync")]
use crate::common::BlockingTwilioPaginator;
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};

/// Query parameters for `GET /Messaging/Countries`.
#[derive(Clone, Copy, Default)]
pub struct ListPricingMessagingCountriesRequest<'a> {
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListPricingMessagingCountriesRequest<'a> {
    /// Create an empty Pricing Messaging Countries list request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `PageSize`.
    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }

    /// Set `Page`.
    #[must_use]
    pub fn page(mut self, value: u32) -> Self {
        self.page = Some(value);
        self
    }

    /// Set `PageToken`.
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

    fn query_pairs(self) -> Vec<(String, String)> {
        let mut query = Vec::new();
        if let Some(value) = self.page_size {
            query.push(("PageSize".to_owned(), value.to_string()));
        }
        if let Some(value) = self.page {
            query.push(("Page".to_owned(), value.to_string()));
        }
        if let Some(value) = self.page_token {
            query.push(("PageToken".to_owned(), value.to_owned()));
        }
        query
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.page_token);
        values
    }
}

impl fmt::Debug for ListPricingMessagingCountriesRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListPricingMessagingCountriesRequest")
            .field("page_size", &self.page_size)
            .field("page", &self.page)
            .field(
                "page_token",
                &self.page_token.map(|_| crate::common::REDACTED),
            )
            .finish()
    }
}

#[derive(Clone, Copy, Default)]
pub struct FetchPricingOriginBasedNumberRequest<'a> {
    origination_number: Option<&'a str>,
}

impl<'a> FetchPricingOriginBasedNumberRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn origination_number(mut self, value: &'a str) -> Self {
        self.origination_number = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        if self
            .origination_number
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(TwilioError::InvalidRequest(
                "OriginationNumber must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    fn apply_query(self, url: &mut Url) {
        if let Some(value) = self.origination_number {
            url.query_pairs_mut()
                .append_pair("OriginationNumber", value);
        }
    }

    fn query_pairs(self) -> Vec<(String, String)> {
        self.origination_number
            .map(|value| vec![("OriginationNumber".to_owned(), value.to_owned())])
            .unwrap_or_default()
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.origination_number);
        values
    }
}

impl fmt::Debug for FetchPricingOriginBasedNumberRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FetchPricingOriginBasedNumberRequest")
            .field(
                "origination_number",
                &self.origination_number.map(|_| crate::common::REDACTED),
            )
            .finish()
    }
}

/// Twilio Pricing v1 Messaging root resource.
#[derive(Clone)]
pub struct TwilioPricingMessaging {
    pub name: Option<String>,
    pub url: Option<String>,
    pub links: Option<BTreeMap<String, String>>,
}

impl fmt::Debug for TwilioPricingMessaging {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingMessaging")
            .field("name", &self.name)
            .field("url", &redacted_option(&self.url))
            .field("links", &RedactedLinks(self.links.as_ref()))
            .finish()
    }
}

struct RedactedLinks<'a>(Option<&'a BTreeMap<String, String>>);

impl fmt::Debug for RedactedLinks<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(links) => write!(f, "Some([<redacted>; {}])", links.len()),
            None => f.write_str("None"),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingMessaging {
    name: Option<String>,
    url: Option<String>,
    links: Option<BTreeMap<String, String>>,
}

impl WirePricingMessaging {
    fn into_pricing_messaging(self) -> TwilioPricingMessaging {
        TwilioPricingMessaging {
            name: non_empty(self.name),
            url: non_empty(self.url),
            links: self.links.filter(|links| !links.is_empty()),
        }
    }
}

/// A summary item from `GET /Messaging/Countries`.
#[derive(Clone)]
pub struct TwilioPricingMessagingCountrySummary {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingMessagingCountrySummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingMessagingCountrySummary")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Deserialize)]
struct WirePricingMessagingCountrySummary {
    country: Option<String>,
    iso_country: Option<String>,
    url: Option<String>,
}

impl WirePricingMessagingCountrySummary {
    fn into_summary(self) -> TwilioPricingMessagingCountrySummary {
        TwilioPricingMessagingCountrySummary {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            url: non_empty(self.url),
        }
    }
}

/// One page of Pricing Messaging country summaries.
#[derive(Clone)]
pub struct TwilioPricingMessagingCountryPage {
    pub countries: Vec<TwilioPricingMessagingCountrySummary>,
    pub meta: V1PageMeta,
}

impl fmt::Debug for TwilioPricingMessagingCountryPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingMessagingCountryPage")
            .field("countries", &self.countries)
            .field("meta", &self.meta)
            .finish()
    }
}

#[derive(Deserialize)]
struct WirePricingMessagingCountryPage {
    #[serde(default)]
    countries: Vec<WirePricingMessagingCountrySummary>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WirePricingMessagingCountryPage {
    fn into_page(self) -> TwilioPricingMessagingCountryPage {
        TwilioPricingMessagingCountryPage {
            countries: self
                .countries
                .into_iter()
                .map(WirePricingMessagingCountrySummary::into_summary)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

/// Pricing for one Messaging country.
#[derive(Clone)]
pub struct TwilioPricingMessagingCountry {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub outbound_sms_prices: Vec<TwilioOutboundSmsPrice>,
    pub inbound_sms_prices: Vec<TwilioInboundSmsPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingMessagingCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingMessagingCountry")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("outbound_sms_prices", &self.outbound_sms_prices)
            .field("inbound_sms_prices", &self.inbound_sms_prices)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Deserialize)]
struct WirePricingMessagingCountry {
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    outbound_sms_prices: Vec<WireOutboundSmsPrice>,
    #[serde(default)]
    inbound_sms_prices: Vec<WireInboundSmsPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingMessagingCountry {
    fn into_country(self) -> TwilioPricingMessagingCountry {
        TwilioPricingMessagingCountry {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            outbound_sms_prices: self
                .outbound_sms_prices
                .into_iter()
                .map(WireOutboundSmsPrice::into_price)
                .collect(),
            inbound_sms_prices: self
                .inbound_sms_prices
                .into_iter()
                .map(WireInboundSmsPrice::into_price)
                .collect(),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

/// Outbound SMS price group for one carrier/MCC/MNC.
#[derive(Clone, Debug)]
pub struct TwilioOutboundSmsPrice {
    pub carrier: Option<String>,
    pub mcc: Option<String>,
    pub mnc: Option<String>,
    pub prices: Vec<TwilioSmsPrice>,
}

#[derive(Deserialize)]
struct WireOutboundSmsPrice {
    carrier: Option<String>,
    mcc: Option<String>,
    mnc: Option<String>,
    #[serde(default)]
    prices: Vec<WireSmsPrice>,
}

impl WireOutboundSmsPrice {
    fn into_price(self) -> TwilioOutboundSmsPrice {
        TwilioOutboundSmsPrice {
            carrier: non_empty(self.carrier),
            mcc: non_empty(self.mcc),
            mnc: non_empty(self.mnc),
            prices: self
                .prices
                .into_iter()
                .map(WireSmsPrice::into_price)
                .collect(),
        }
    }
}

/// Inbound SMS price for one Twilio number type.
#[derive(Clone, Debug)]
pub struct TwilioInboundSmsPrice {
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub number_type: Option<String>,
}

#[derive(Deserialize)]
struct WireInboundSmsPrice {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    number_type: Option<String>,
}

impl WireInboundSmsPrice {
    fn into_price(self) -> TwilioInboundSmsPrice {
        TwilioInboundSmsPrice {
            base_price: self.base_price,
            current_price: self.current_price,
            number_type: non_empty(self.number_type),
        }
    }
}

/// SMS price for one outbound Twilio number type.
#[derive(Clone, Debug)]
pub struct TwilioSmsPrice {
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub number_type: Option<String>,
}

#[derive(Deserialize)]
struct WireSmsPrice {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    number_type: Option<String>,
}

impl WireSmsPrice {
    fn into_price(self) -> TwilioSmsPrice {
        TwilioSmsPrice {
            base_price: self.base_price,
            current_price: self.current_price,
            number_type: non_empty(self.number_type),
        }
    }
}

pub type ListPricingCountriesRequest<'a> = ListPricingMessagingCountriesRequest<'a>;

#[derive(Clone)]
pub struct TwilioPricingCountrySummary {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingCountrySummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingCountrySummary")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioPricingCountryPage {
    pub countries: Vec<TwilioPricingCountrySummary>,
    pub meta: V1PageMeta,
}

#[derive(Clone, Debug)]
pub struct TwilioPhoneNumberPrice {
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub number_type: Option<String>,
}

#[derive(Clone)]
pub struct TwilioPricingPhoneNumberCountry {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub phone_number_prices: Vec<TwilioPhoneNumberPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingPhoneNumberCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingPhoneNumberCountry")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("phone_number_prices", &self.phone_number_prices)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioVoicePrefixPrice {
    pub prefixes: Vec<String>,
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub friendly_name: Option<String>,
}

impl fmt::Debug for TwilioVoicePrefixPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioVoicePrefixPrice")
            .field(
                "prefixes",
                &format_args!("[{} redacted]", self.prefixes.len()),
            )
            .field("base_price", &self.base_price)
            .field("current_price", &self.current_price)
            .field("friendly_name", &self.friendly_name)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioInboundCallPrice {
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub number_type: Option<String>,
}

#[derive(Clone)]
pub struct TwilioOriginBasedPrefixPrice {
    pub origination_prefixes: Vec<String>,
    pub destination_prefixes: Vec<String>,
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
    pub friendly_name: Option<String>,
}

impl fmt::Debug for TwilioOriginBasedPrefixPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioOriginBasedPrefixPrice")
            .field(
                "origination_prefixes",
                &format_args!("[{} redacted]", self.origination_prefixes.len()),
            )
            .field(
                "destination_prefixes",
                &format_args!("[{} redacted]", self.destination_prefixes.len()),
            )
            .field("base_price", &self.base_price)
            .field("current_price", &self.current_price)
            .field("friendly_name", &self.friendly_name)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioOriginBasedOutboundCallPrice {
    pub origination_prefixes: Vec<String>,
    pub base_price: Option<Decimal>,
    pub current_price: Option<Decimal>,
}

impl fmt::Debug for TwilioOriginBasedOutboundCallPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioOriginBasedOutboundCallPrice")
            .field(
                "origination_prefixes",
                &format_args!("[{} redacted]", self.origination_prefixes.len()),
            )
            .field("base_price", &self.base_price)
            .field("current_price", &self.current_price)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioPricingVoiceCountry {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub outbound_prefix_prices: Vec<TwilioVoicePrefixPrice>,
    pub inbound_call_prices: Vec<TwilioInboundCallPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingVoiceCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingVoiceCountry")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("outbound_prefix_prices", &self.outbound_prefix_prices)
            .field("inbound_call_prices", &self.inbound_call_prices)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioPricingOriginBasedVoiceCountry {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub outbound_prefix_prices: Vec<TwilioOriginBasedPrefixPrice>,
    pub inbound_call_prices: Vec<TwilioInboundCallPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingOriginBasedVoiceCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingOriginBasedVoiceCountry")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("outbound_prefix_prices", &self.outbound_prefix_prices)
            .field("inbound_call_prices", &self.inbound_call_prices)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioPricingTrunkingCountry {
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub terminating_prefix_prices: Vec<TwilioOriginBasedPrefixPrice>,
    pub originating_call_prices: Vec<TwilioInboundCallPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingTrunkingCountry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingTrunkingCountry")
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("terminating_prefix_prices", &self.terminating_prefix_prices)
            .field("originating_call_prices", &self.originating_call_prices)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioPricingOriginBasedVoiceNumber {
    pub destination_number: Option<String>,
    pub origination_number: Option<String>,
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub outbound_call_prices: Vec<TwilioOriginBasedOutboundCallPrice>,
    pub inbound_call_price: Option<TwilioInboundCallPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingOriginBasedVoiceNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingOriginBasedVoiceNumber")
            .field(
                "destination_number",
                &redacted_option(&self.destination_number),
            )
            .field(
                "origination_number",
                &redacted_option(&self.origination_number),
            )
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("outbound_call_prices", &self.outbound_call_prices)
            .field("inbound_call_price", &self.inbound_call_price)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioPricingTrunkingNumber {
    pub destination_number: Option<String>,
    pub origination_number: Option<String>,
    pub country: Option<String>,
    pub iso_country: Option<String>,
    pub terminating_prefix_prices: Vec<TwilioOriginBasedPrefixPrice>,
    pub originating_call_price: Option<TwilioInboundCallPrice>,
    pub price_unit: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioPricingTrunkingNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPricingTrunkingNumber")
            .field(
                "destination_number",
                &redacted_option(&self.destination_number),
            )
            .field(
                "origination_number",
                &redacted_option(&self.origination_number),
            )
            .field("country", &self.country)
            .field("iso_country", &self.iso_country)
            .field("terminating_prefix_prices", &self.terminating_prefix_prices)
            .field("originating_call_price", &self.originating_call_price)
            .field("price_unit", &self.price_unit)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

fn deserialize_optional_decimal<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(value) if value.trim().is_empty() => Ok(None),
        serde_json::Value::String(value) => value
            .parse::<Decimal>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        serde_json::Value::Number(value) => value
            .to_string()
            .parse::<Decimal>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!(
            "expected decimal string or number, got {other}"
        ))),
    }
}

#[derive(Deserialize)]
struct WirePricingCountrySummary {
    country: Option<String>,
    iso_country: Option<String>,
    url: Option<String>,
}

impl WirePricingCountrySummary {
    fn into_summary(self) -> TwilioPricingCountrySummary {
        TwilioPricingCountrySummary {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingCountryPage {
    #[serde(default)]
    countries: Vec<WirePricingCountrySummary>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WirePricingCountryPage {
    fn into_page(self) -> TwilioPricingCountryPage {
        TwilioPricingCountryPage {
            countries: self
                .countries
                .into_iter()
                .map(WirePricingCountrySummary::into_summary)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WirePhoneNumberPrice {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    number_type: Option<String>,
}

impl WirePhoneNumberPrice {
    fn into_price(self) -> TwilioPhoneNumberPrice {
        TwilioPhoneNumberPrice {
            base_price: self.base_price,
            current_price: self.current_price,
            number_type: non_empty(self.number_type),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingPhoneNumberCountry {
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    phone_number_prices: Vec<WirePhoneNumberPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingPhoneNumberCountry {
    fn into_country(self) -> TwilioPricingPhoneNumberCountry {
        TwilioPricingPhoneNumberCountry {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            phone_number_prices: self
                .phone_number_prices
                .into_iter()
                .map(WirePhoneNumberPrice::into_price)
                .collect(),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WireVoicePrefixPrice {
    #[serde(default)]
    prefixes: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    friendly_name: Option<String>,
}

impl WireVoicePrefixPrice {
    fn into_price(self) -> TwilioVoicePrefixPrice {
        TwilioVoicePrefixPrice {
            prefixes: self
                .prefixes
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            base_price: self.base_price,
            current_price: self.current_price,
            friendly_name: non_empty(self.friendly_name),
        }
    }
}

#[derive(Deserialize)]
struct WireInboundCallPrice {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    number_type: Option<String>,
}

impl WireInboundCallPrice {
    fn into_price(self) -> TwilioInboundCallPrice {
        TwilioInboundCallPrice {
            base_price: self.base_price,
            current_price: self.current_price,
            number_type: non_empty(self.number_type),
        }
    }
}

#[derive(Deserialize)]
struct WireOriginBasedPrefixPrice {
    #[serde(default)]
    origination_prefixes: Vec<String>,
    #[serde(default)]
    destination_prefixes: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
    friendly_name: Option<String>,
}

impl WireOriginBasedPrefixPrice {
    fn into_price(self) -> TwilioOriginBasedPrefixPrice {
        TwilioOriginBasedPrefixPrice {
            origination_prefixes: self
                .origination_prefixes
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            destination_prefixes: self
                .destination_prefixes
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            base_price: self.base_price,
            current_price: self.current_price,
            friendly_name: non_empty(self.friendly_name),
        }
    }
}

#[derive(Deserialize)]
struct WireOriginBasedOutboundCallPrice {
    #[serde(default)]
    origination_prefixes: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    base_price: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    current_price: Option<Decimal>,
}

impl WireOriginBasedOutboundCallPrice {
    fn into_price(self) -> TwilioOriginBasedOutboundCallPrice {
        TwilioOriginBasedOutboundCallPrice {
            origination_prefixes: self
                .origination_prefixes
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            base_price: self.base_price,
            current_price: self.current_price,
        }
    }
}

#[derive(Deserialize)]
struct WirePricingVoiceCountry {
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    outbound_prefix_prices: Vec<WireVoicePrefixPrice>,
    #[serde(default)]
    inbound_call_prices: Vec<WireInboundCallPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingVoiceCountry {
    fn into_country(self) -> TwilioPricingVoiceCountry {
        TwilioPricingVoiceCountry {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            outbound_prefix_prices: self
                .outbound_prefix_prices
                .into_iter()
                .map(WireVoicePrefixPrice::into_price)
                .collect(),
            inbound_call_prices: self
                .inbound_call_prices
                .into_iter()
                .map(WireInboundCallPrice::into_price)
                .collect(),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingOriginBasedVoiceCountry {
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    outbound_prefix_prices: Vec<WireOriginBasedPrefixPrice>,
    #[serde(default)]
    inbound_call_prices: Vec<WireInboundCallPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingOriginBasedVoiceCountry {
    fn into_country(self) -> TwilioPricingOriginBasedVoiceCountry {
        TwilioPricingOriginBasedVoiceCountry {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            outbound_prefix_prices: self
                .outbound_prefix_prices
                .into_iter()
                .map(WireOriginBasedPrefixPrice::into_price)
                .collect(),
            inbound_call_prices: self
                .inbound_call_prices
                .into_iter()
                .map(WireInboundCallPrice::into_price)
                .collect(),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingTrunkingCountry {
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    terminating_prefix_prices: Vec<WireOriginBasedPrefixPrice>,
    #[serde(default)]
    originating_call_prices: Vec<WireInboundCallPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingTrunkingCountry {
    fn into_country(self) -> TwilioPricingTrunkingCountry {
        TwilioPricingTrunkingCountry {
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            terminating_prefix_prices: self
                .terminating_prefix_prices
                .into_iter()
                .map(WireOriginBasedPrefixPrice::into_price)
                .collect(),
            originating_call_prices: self
                .originating_call_prices
                .into_iter()
                .map(WireInboundCallPrice::into_price)
                .collect(),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingOriginBasedVoiceNumber {
    destination_number: Option<String>,
    origination_number: Option<String>,
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    outbound_call_prices: Vec<WireOriginBasedOutboundCallPrice>,
    inbound_call_price: Option<WireInboundCallPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingOriginBasedVoiceNumber {
    fn into_number(self) -> TwilioPricingOriginBasedVoiceNumber {
        TwilioPricingOriginBasedVoiceNumber {
            destination_number: non_empty(self.destination_number),
            origination_number: non_empty(self.origination_number),
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            outbound_call_prices: self
                .outbound_call_prices
                .into_iter()
                .map(WireOriginBasedOutboundCallPrice::into_price)
                .collect(),
            inbound_call_price: self
                .inbound_call_price
                .map(WireInboundCallPrice::into_price),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

#[derive(Deserialize)]
struct WirePricingTrunkingNumber {
    destination_number: Option<String>,
    origination_number: Option<String>,
    country: Option<String>,
    iso_country: Option<String>,
    #[serde(default)]
    terminating_prefix_prices: Vec<WireOriginBasedPrefixPrice>,
    originating_call_price: Option<WireInboundCallPrice>,
    price_unit: Option<String>,
    url: Option<String>,
}

impl WirePricingTrunkingNumber {
    fn into_number(self) -> TwilioPricingTrunkingNumber {
        TwilioPricingTrunkingNumber {
            destination_number: non_empty(self.destination_number),
            origination_number: non_empty(self.origination_number),
            country: non_empty(self.country),
            iso_country: non_empty(self.iso_country),
            terminating_prefix_prices: self
                .terminating_prefix_prices
                .into_iter()
                .map(WireOriginBasedPrefixPrice::into_price)
                .collect(),
            originating_call_price: self
                .originating_call_price
                .map(WireInboundCallPrice::into_price),
            price_unit: non_empty(self.price_unit),
            url: non_empty(self.url),
        }
    }
}

/// Pricing product resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// Pricing v1 resources.
    #[must_use]
    pub fn v1(self) -> PricingV1Resource<'a> {
        PricingV1Resource {
            account: self.account,
        }
    }

    /// Pricing v2 resources.
    #[must_use]
    pub fn v2(self) -> PricingV2Resource<'a> {
        PricingV2Resource {
            account: self.account,
        }
    }
}

/// Pricing v1 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV1Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV1Resource<'a> {
    /// Pricing v1 Messaging resource.
    #[must_use]
    pub fn messaging(self) -> PricingMessagingResource<'a> {
        PricingMessagingResource::new(self.account)
    }

    /// Pricing v1 `PhoneNumbers` resource.
    #[must_use]
    pub fn phone_numbers(self) -> PricingV1PhoneNumbersResource<'a> {
        PricingV1PhoneNumbersResource::new(self.account)
    }

    /// Pricing v1 Voice resource.
    #[must_use]
    pub fn voice(self) -> PricingV1VoiceResource<'a> {
        PricingV1VoiceResource::new(self.account)
    }
}

/// Pricing v2 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV2Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV2Resource<'a> {
    /// Pricing v2 Voice resource.
    #[must_use]
    pub fn voice(self) -> PricingV2VoiceResource<'a> {
        PricingV2VoiceResource::new(self.account)
    }

    /// Pricing v2 Trunking resource.
    #[must_use]
    pub fn trunking(self) -> PricingV2TrunkingResource<'a> {
        PricingV2TrunkingResource::new(self.account)
    }
}

/// Pricing v1 Messaging resource.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingMessagingResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingMessagingResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Messaging`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioPricingMessaging, TwilioError> {
        async move {
            let spec = RequestSpec::new(ApiFamily::PricingV1, Method::GET, ["Messaging"])
                .operation("pricing.messaging.fetch");
            let parsed: WirePricingMessaging = self
                .account
                .send_spec_json(spec, &sensitive_values(self.account))
                .await?;
            Ok(parsed.into_pricing_messaging())
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.fetch",
            "GET",
        ))
        .await
    }

    /// Pricing Messaging Countries collection.
    #[must_use]
    pub fn countries(self) -> PricingMessagingCountriesResource<'a> {
        PricingMessagingCountriesResource::new(self.account)
    }
}

/// Pricing Messaging Countries collection.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingMessagingCountriesResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingMessagingCountriesResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Messaging/Countries`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListPricingMessagingCountriesRequest<'a>,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        async move {
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self
                .account
                .client
                .pricing_endpoint(&["Messaging", "Countries"])?;
            let spec = RequestSpec::new(
                ApiFamily::PricingV1,
                Method::GET,
                ["Messaging", "Countries"],
            )
            .operation("pricing.messaging.countries.list")
            .query_pairs(request.query_pairs());
            append_query_pairs(&mut url, &spec);
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if the page URL leaves the configured Pricing API
    /// base, changes stable filters, or the HTTP request/response fails.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        async move {
            let mut sensitive_values = sensitive_values(self.account);
            sensitive_values.push(next_page_url);
            let resource = PricingPageResource::V1Messaging;
            let url = self
                .account
                .client
                .pricing_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::PricingV1,
                Method::GET,
                url.clone(),
                "pricing.messaging.countries.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.list_page_url",
            "GET",
        ))
        .await
    }

    /// Lazily list all Pricing Messaging countries using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, TwilioPricingMessagingCountryPage, TwilioPricingMessagingCountrySummary>
    {
        self.list_all_with(ListPricingMessagingCountriesRequest::new())
    }

    /// Lazily list all Pricing Messaging countries using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListPricingMessagingCountriesRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioPricingMessagingCountryPage, TwilioPricingMessagingCountrySummary>
    {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        let resource = self;
        TwilioPaginator::new(
            move |cursor| {
                Box::pin(async move {
                    match cursor {
                        Some(cursor) => resource.list_page_url(&cursor).await,
                        None => resource.list(request).await,
                    }
                }) as PageFuture<'a, TwilioPricingMessagingCountryPage>
            },
            split_country_page,
        )
    }

    /// `GET /Messaging/Countries/{IsoCountry}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid ISO country codes, transport
    /// failures, non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(
        self,
        iso_country: &str,
    ) -> Result<TwilioPricingMessagingCountry, TwilioError> {
        async move {
            let iso_country = normalize_iso_country(iso_country)?;
            let spec = RequestSpec::new(
                ApiFamily::PricingV1,
                Method::GET,
                vec!["Messaging".to_owned(), "Countries".to_owned(), iso_country],
            )
            .operation("pricing.messaging.countries.fetch");
            let parsed: WirePricingMessagingCountry = self
                .account
                .send_spec_json(spec, &sensitive_values(self.account))
                .await?;
            Ok(parsed.into_country())
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.fetch",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::common::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        let parsed: WirePricingMessagingCountryPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = PricingPageResource::V1Messaging;
        validate_pricing_meta_key(&page.meta, resource)?;
        validate_next_page(
            &self.account.client.config.pricing,
            current_url,
            page.meta.next_page_url.as_deref(),
            resource,
        )?;
        Ok(page)
    }
}

fn split_country_page(
    page: TwilioPricingMessagingCountryPage,
) -> (Vec<TwilioPricingMessagingCountrySummary>, Option<String>) {
    (page.countries, page.meta.next_page_url)
}

fn split_pricing_country_page(
    page: TwilioPricingCountryPage,
) -> (Vec<TwilioPricingCountrySummary>, Option<String>) {
    (page.countries, page.meta.next_page_url)
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV1PhoneNumbersResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV1PhoneNumbersResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> PricingV1PhoneNumberCountriesResource<'a> {
        PricingV1PhoneNumberCountriesResource {
            account: self.account,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV1VoiceResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV1VoiceResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> PricingV1VoiceCountriesResource<'a> {
        PricingV1VoiceCountriesResource {
            account: self.account,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV2VoiceResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV2VoiceResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> PricingV2VoiceCountriesResource<'a> {
        PricingV2VoiceCountriesResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn number(self, destination_number: &'a str) -> PricingV2VoiceNumberResource<'a> {
        PricingV2VoiceNumberResource {
            account: self.account,
            destination_number,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV2TrunkingResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> PricingV2TrunkingResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> PricingV2TrunkingCountriesResource<'a> {
        PricingV2TrunkingCountriesResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn number(self, destination_number: &'a str) -> PricingV2TrunkingNumberResource<'a> {
        PricingV2TrunkingNumberResource {
            account: self.account,
            destination_number,
        }
    }
}

macro_rules! impl_async_pricing_countries_resource {
    (
        $name:ident,
        $output:ty,
        $wire:ty,
        $family:expr,
        $endpoint:ident,
        $page_resource:expr,
        [$product:literal, $countries:literal],
        $operation_prefix:literal
    ) => {
        #[derive(Clone, Copy)]
        #[cfg(feature = "async")]
        pub struct $name<'a> {
            account: TwilioAccount<'a>,
        }

        #[cfg(feature = "async")]
        impl<'a> $name<'a> {
            /// List Pricing countries.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, malformed JSON responses, or invalid
            /// pagination metadata.
            pub async fn list(
                self,
                request: ListPricingCountriesRequest<'a>,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                async move {
                    request.validate()?;
                    let mut sensitive_values = sensitive_values(self.account);
                    sensitive_values.extend(request.sensitive_values());
                    let mut url = self.account.client.$endpoint(&[$product, $countries])?;
                    let spec = RequestSpec::new($family, Method::GET, [$product, $countries])
                        .operation(concat!($operation_prefix, ".list"))
                        .query_pairs(request.query_pairs());
                    append_query_pairs(&mut url, &spec);
                    let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                }
                .instrument(request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".list"),
                    "GET",
                ))
                .await
            }

            /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] if the page URL leaves the configured
            /// Pricing API base, changes stable filters, or the HTTP
            /// request/response fails.
            pub async fn list_page_url(
                self,
                next_page_url: &str,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                async move {
                    let mut sensitive_values = sensitive_values(self.account);
                    sensitive_values.push(next_page_url);
                    let resource = $page_resource;
                    let url = self
                        .account
                        .client
                        .pricing_page_url(next_page_url, resource)?;
                    let spec = RequestSpec::from_url(
                        $family,
                        Method::GET,
                        url.clone(),
                        concat!($operation_prefix, ".list_page_url"),
                    );
                    let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                }
                .instrument(request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".list_page_url"),
                    "GET",
                ))
                .await
            }

            /// Lazily list all Pricing countries using a default page size of 50.
            #[must_use]
            pub fn list_all(
                self,
            ) -> TwilioPaginator<'a, TwilioPricingCountryPage, TwilioPricingCountrySummary> {
                self.list_all_with(ListPricingCountriesRequest::new())
            }

            /// Lazily list all Pricing countries using supplied first-page filters.
            #[must_use]
            pub fn list_all_with(
                self,
                mut request: ListPricingCountriesRequest<'a>,
            ) -> TwilioPaginator<'a, TwilioPricingCountryPage, TwilioPricingCountrySummary> {
                if request.page_size.is_none() {
                    request.page_size = Some(DEFAULT_PAGE_SIZE);
                }
                let resource = self;
                TwilioPaginator::new(
                    move |cursor| {
                        Box::pin(async move {
                            match cursor {
                                Some(cursor) => resource.list_page_url(&cursor).await,
                                None => resource.list(request).await,
                            }
                        }) as PageFuture<'a, TwilioPricingCountryPage>
                    },
                    split_pricing_country_page,
                )
            }

            /// Fetch Pricing for one country.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid ISO country codes, transport
            /// failures, non-2xx API responses, or malformed JSON responses.
            pub async fn fetch(self, iso_country: &str) -> Result<$output, TwilioError> {
                async move {
                    let iso_country = normalize_iso_country(iso_country)?;
                    let spec = RequestSpec::new(
                        $family,
                        Method::GET,
                        vec![$product.to_owned(), $countries.to_owned(), iso_country],
                    )
                    .operation(concat!($operation_prefix, ".fetch"));
                    let parsed: $wire = self
                        .account
                        .send_spec_json(spec, &sensitive_values(self.account))
                        .await?;
                    Ok(parsed.into_country())
                }
                .instrument(request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".fetch"),
                    "GET",
                ))
                .await
            }

            fn read_page(
                self,
                raw: &crate::common::RawResponse,
                sensitive_values: &[&str],
                current_url: Option<&Url>,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                let parsed: WirePricingCountryPage = decode_json_response(raw, sensitive_values)?;
                let page = parsed.into_page();
                let resource = $page_resource;
                validate_pricing_meta_key(&page.meta, resource)?;
                validate_next_page(
                    &self.account.client.config.pricing,
                    current_url,
                    page.meta.next_page_url.as_deref(),
                    resource,
                )?;
                Ok(page)
            }
        }
    };
}

impl_async_pricing_countries_resource!(
    PricingV1PhoneNumberCountriesResource,
    TwilioPricingPhoneNumberCountry,
    WirePricingPhoneNumberCountry,
    ApiFamily::PricingV1,
    pricing_endpoint,
    PricingPageResource::V1PhoneNumbers,
    ["PhoneNumbers", "Countries"],
    "pricing.v1.phone_numbers.countries"
);

impl_async_pricing_countries_resource!(
    PricingV1VoiceCountriesResource,
    TwilioPricingVoiceCountry,
    WirePricingVoiceCountry,
    ApiFamily::PricingV1,
    pricing_endpoint,
    PricingPageResource::V1Voice,
    ["Voice", "Countries"],
    "pricing.v1.voice.countries"
);

impl_async_pricing_countries_resource!(
    PricingV2VoiceCountriesResource,
    TwilioPricingOriginBasedVoiceCountry,
    WirePricingOriginBasedVoiceCountry,
    ApiFamily::PricingV2,
    pricing_v2_endpoint,
    PricingPageResource::V2Voice,
    ["Voice", "Countries"],
    "pricing.v2.voice.countries"
);

impl_async_pricing_countries_resource!(
    PricingV2TrunkingCountriesResource,
    TwilioPricingTrunkingCountry,
    WirePricingTrunkingCountry,
    ApiFamily::PricingV2,
    pricing_v2_endpoint,
    PricingPageResource::V2Trunking,
    ["Trunking", "Countries"],
    "pricing.v2.trunking.countries"
);

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV2VoiceNumberResource<'a> {
    account: TwilioAccount<'a>,
    destination_number: &'a str,
}

#[cfg(feature = "async")]
impl<'a> PricingV2VoiceNumberResource<'a> {
    /// `GET /Voice/Numbers/{DestinationNumber}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(
        self,
        request: FetchPricingOriginBasedNumberRequest<'a>,
    ) -> Result<TwilioPricingOriginBasedVoiceNumber, TwilioError> {
        async move {
            validate_required("DestinationNumber", self.destination_number)?;
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account);
            sensitive_values.push(self.destination_number);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self.account.client.pricing_v2_endpoint(&[
                "Voice",
                "Numbers",
                self.destination_number,
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::new(
                ApiFamily::PricingV2,
                Method::GET,
                ["Voice", "Numbers", self.destination_number],
            )
            .operation("pricing.v2.voice.numbers.fetch")
            .query_pairs(request.query_pairs());
            let parsed: WirePricingOriginBasedVoiceNumber =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_number())
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.v2.voice.numbers.fetch",
            "GET",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct PricingV2TrunkingNumberResource<'a> {
    account: TwilioAccount<'a>,
    destination_number: &'a str,
}

#[cfg(feature = "async")]
impl<'a> PricingV2TrunkingNumberResource<'a> {
    /// `GET /Trunking/Numbers/{DestinationNumber}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(
        self,
        request: FetchPricingOriginBasedNumberRequest<'a>,
    ) -> Result<TwilioPricingTrunkingNumber, TwilioError> {
        async move {
            validate_required("DestinationNumber", self.destination_number)?;
            request.validate()?;
            let mut sensitive_values = sensitive_values(self.account);
            sensitive_values.push(self.destination_number);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self.account.client.pricing_v2_endpoint(&[
                "Trunking",
                "Numbers",
                self.destination_number,
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::new(
                ApiFamily::PricingV2,
                Method::GET,
                ["Trunking", "Numbers", self.destination_number],
            )
            .operation("pricing.v2.trunking.numbers.fetch")
            .query_pairs(request.query_pairs());
            let parsed: WirePricingTrunkingNumber =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_number())
        }
        .instrument(request_span(
            &self.account.client.config.pricing,
            "pricing.v2.trunking.numbers.fetch",
            "GET",
        ))
        .await
    }
}

/// Blocking Pricing product resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// Pricing v1 resources.
    #[must_use]
    pub fn v1(self) -> BlockingPricingV1Resource<'a> {
        BlockingPricingV1Resource {
            account: self.account,
        }
    }

    /// Pricing v2 resources.
    #[must_use]
    pub fn v2(self) -> BlockingPricingV2Resource<'a> {
        BlockingPricingV2Resource {
            account: self.account,
        }
    }
}

/// Blocking Pricing v1 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV1Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV1Resource<'a> {
    /// Pricing v1 Messaging resource.
    #[must_use]
    pub fn messaging(self) -> BlockingPricingMessagingResource<'a> {
        BlockingPricingMessagingResource::new(self.account)
    }

    /// Pricing v1 `PhoneNumbers` resource.
    #[must_use]
    pub fn phone_numbers(self) -> BlockingPricingV1PhoneNumbersResource<'a> {
        BlockingPricingV1PhoneNumbersResource::new(self.account)
    }

    /// Pricing v1 Voice resource.
    #[must_use]
    pub fn voice(self) -> BlockingPricingV1VoiceResource<'a> {
        BlockingPricingV1VoiceResource::new(self.account)
    }
}

/// Blocking Pricing v2 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV2Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV2Resource<'a> {
    /// Pricing v2 Voice resource.
    #[must_use]
    pub fn voice(self) -> BlockingPricingV2VoiceResource<'a> {
        BlockingPricingV2VoiceResource::new(self.account)
    }

    /// Pricing v2 Trunking resource.
    #[must_use]
    pub fn trunking(self) -> BlockingPricingV2TrunkingResource<'a> {
        BlockingPricingV2TrunkingResource::new(self.account)
    }
}

/// Blocking Pricing v1 Messaging resource.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingMessagingResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingMessagingResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Messaging`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioPricingMessaging, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.fetch",
            "GET",
        )
        .in_scope(|| {
            let spec = RequestSpec::new(ApiFamily::PricingV1, Method::GET, ["Messaging"])
                .operation("pricing.messaging.fetch");
            let parsed: WirePricingMessaging = self
                .account
                .send_spec_json(spec, &sensitive_values_blocking(self.account))?;
            Ok(parsed.into_pricing_messaging())
        })
    }

    /// Pricing Messaging Countries collection.
    #[must_use]
    pub fn countries(self) -> BlockingPricingMessagingCountriesResource<'a> {
        BlockingPricingMessagingCountriesResource::new(self.account)
    }
}

/// Blocking Pricing Messaging Countries collection.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingMessagingCountriesResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingMessagingCountriesResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Messaging/Countries`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(
        self,
        request: ListPricingMessagingCountriesRequest<'a>,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let mut sensitive_values = sensitive_values_blocking(self.account);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self
                .account
                .client
                .pricing_endpoint(&["Messaging", "Countries"])?;
            let spec = RequestSpec::new(
                ApiFamily::PricingV1,
                Method::GET,
                ["Messaging", "Countries"],
            )
            .operation("pricing.messaging.countries.list")
            .query_pairs(request.query_pairs());
            append_query_pairs(&mut url, &spec);
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if the page URL leaves the configured Pricing API
    /// base, changes stable filters, or the HTTP request/response fails.
    pub fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let mut sensitive_values = sensitive_values_blocking(self.account);
            sensitive_values.push(next_page_url);
            let resource = PricingPageResource::V1Messaging;
            let url = self
                .account
                .client
                .pricing_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::PricingV1,
                Method::GET,
                url.clone(),
                "pricing.messaging.countries.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Lazily list all Pricing Messaging countries using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<
        'a,
        TwilioPricingMessagingCountryPage,
        TwilioPricingMessagingCountrySummary,
    > {
        self.list_all_with(ListPricingMessagingCountriesRequest::new())
    }

    /// Lazily list all Pricing Messaging countries using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListPricingMessagingCountriesRequest<'a>,
    ) -> BlockingTwilioPaginator<
        'a,
        TwilioPricingMessagingCountryPage,
        TwilioPricingMessagingCountrySummary,
    > {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        let resource = self;
        BlockingTwilioPaginator::new(
            move |cursor| match cursor {
                Some(cursor) => resource.list_page_url(&cursor),
                None => resource.list(request),
            },
            split_country_page,
        )
    }

    /// `GET /Messaging/Countries/{IsoCountry}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid ISO country codes, transport
    /// failures, non-2xx API responses, or malformed JSON responses.
    pub fn fetch(self, iso_country: &str) -> Result<TwilioPricingMessagingCountry, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.messaging.countries.fetch",
            "GET",
        )
        .in_scope(|| {
            let iso_country = normalize_iso_country(iso_country)?;
            let spec = RequestSpec::new(
                ApiFamily::PricingV1,
                Method::GET,
                vec!["Messaging".to_owned(), "Countries".to_owned(), iso_country],
            )
            .operation("pricing.messaging.countries.fetch");
            let parsed: WirePricingMessagingCountry = self
                .account
                .send_spec_json(spec, &sensitive_values_blocking(self.account))?;
            Ok(parsed.into_country())
        })
    }

    fn read_page(
        self,
        raw: &crate::common::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioPricingMessagingCountryPage, TwilioError> {
        let parsed: WirePricingMessagingCountryPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = PricingPageResource::V1Messaging;
        validate_pricing_meta_key(&page.meta, resource)?;
        validate_next_page(
            &self.account.client.config.pricing,
            current_url,
            page.meta.next_page_url.as_deref(),
            resource,
        )?;
        Ok(page)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV1PhoneNumbersResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV1PhoneNumbersResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> BlockingPricingV1PhoneNumberCountriesResource<'a> {
        BlockingPricingV1PhoneNumberCountriesResource {
            account: self.account,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV1VoiceResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV1VoiceResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> BlockingPricingV1VoiceCountriesResource<'a> {
        BlockingPricingV1VoiceCountriesResource {
            account: self.account,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV2VoiceResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV2VoiceResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> BlockingPricingV2VoiceCountriesResource<'a> {
        BlockingPricingV2VoiceCountriesResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn number(self, destination_number: &'a str) -> BlockingPricingV2VoiceNumberResource<'a> {
        BlockingPricingV2VoiceNumberResource {
            account: self.account,
            destination_number,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV2TrunkingResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV2TrunkingResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn countries(self) -> BlockingPricingV2TrunkingCountriesResource<'a> {
        BlockingPricingV2TrunkingCountriesResource {
            account: self.account,
        }
    }

    #[must_use]
    pub fn number(
        self,
        destination_number: &'a str,
    ) -> BlockingPricingV2TrunkingNumberResource<'a> {
        BlockingPricingV2TrunkingNumberResource {
            account: self.account,
            destination_number,
        }
    }
}

macro_rules! impl_blocking_pricing_countries_resource {
    (
        $name:ident,
        $output:ty,
        $wire:ty,
        $family:expr,
        $endpoint:ident,
        $page_resource:expr,
        [$product:literal, $countries:literal],
        $operation_prefix:literal
    ) => {
        #[derive(Clone, Copy)]
        #[cfg(feature = "sync")]
        pub struct $name<'a> {
            account: BlockingTwilioAccount<'a>,
        }

        #[cfg(feature = "sync")]
        impl<'a> $name<'a> {
            /// List Pricing countries.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, malformed JSON responses, or invalid
            /// pagination metadata.
            pub fn list(
                self,
                request: ListPricingCountriesRequest<'a>,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".list"),
                    "GET",
                )
                .in_scope(|| {
                    request.validate()?;
                    let mut sensitive_values = sensitive_values_blocking(self.account);
                    sensitive_values.extend(request.sensitive_values());
                    let mut url = self.account.client.$endpoint(&[$product, $countries])?;
                    let spec = RequestSpec::new($family, Method::GET, [$product, $countries])
                        .operation(concat!($operation_prefix, ".list"))
                        .query_pairs(request.query_pairs());
                    append_query_pairs(&mut url, &spec);
                    let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                })
            }

            /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] if the page URL leaves the configured
            /// Pricing API base, changes stable filters, or the HTTP
            /// request/response fails.
            pub fn list_page_url(
                self,
                next_page_url: &str,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".list_page_url"),
                    "GET",
                )
                .in_scope(|| {
                    let mut sensitive_values = sensitive_values_blocking(self.account);
                    sensitive_values.push(next_page_url);
                    let resource = $page_resource;
                    let url = self
                        .account
                        .client
                        .pricing_page_url(next_page_url, resource)?;
                    let spec = RequestSpec::from_url(
                        $family,
                        Method::GET,
                        url.clone(),
                        concat!($operation_prefix, ".list_page_url"),
                    );
                    let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                })
            }

            /// Lazily list all Pricing countries using a default page size of 50.
            #[must_use]
            pub fn list_all(
                self,
            ) -> BlockingTwilioPaginator<'a, TwilioPricingCountryPage, TwilioPricingCountrySummary>
            {
                self.list_all_with(ListPricingCountriesRequest::new())
            }

            /// Lazily list all Pricing countries using supplied first-page filters.
            #[must_use]
            pub fn list_all_with(
                self,
                mut request: ListPricingCountriesRequest<'a>,
            ) -> BlockingTwilioPaginator<'a, TwilioPricingCountryPage, TwilioPricingCountrySummary>
            {
                if request.page_size.is_none() {
                    request.page_size = Some(DEFAULT_PAGE_SIZE);
                }
                let resource = self;
                BlockingTwilioPaginator::new(
                    move |cursor| match cursor {
                        Some(cursor) => resource.list_page_url(&cursor),
                        None => resource.list(request),
                    },
                    split_pricing_country_page,
                )
            }

            /// Fetch Pricing for one country.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid ISO country codes, transport
            /// failures, non-2xx API responses, or malformed JSON responses.
            pub fn fetch(self, iso_country: &str) -> Result<$output, TwilioError> {
                request_span(
                    &self.account.client.config.pricing,
                    concat!($operation_prefix, ".fetch"),
                    "GET",
                )
                .in_scope(|| {
                    let iso_country = normalize_iso_country(iso_country)?;
                    let spec = RequestSpec::new(
                        $family,
                        Method::GET,
                        vec![$product.to_owned(), $countries.to_owned(), iso_country],
                    )
                    .operation(concat!($operation_prefix, ".fetch"));
                    let parsed: $wire = self
                        .account
                        .send_spec_json(spec, &sensitive_values_blocking(self.account))?;
                    Ok(parsed.into_country())
                })
            }

            fn read_page(
                self,
                raw: &crate::common::RawResponse,
                sensitive_values: &[&str],
                current_url: Option<&Url>,
            ) -> Result<TwilioPricingCountryPage, TwilioError> {
                let parsed: WirePricingCountryPage = decode_json_response(raw, sensitive_values)?;
                let page = parsed.into_page();
                let resource = $page_resource;
                validate_pricing_meta_key(&page.meta, resource)?;
                validate_next_page(
                    &self.account.client.config.pricing,
                    current_url,
                    page.meta.next_page_url.as_deref(),
                    resource,
                )?;
                Ok(page)
            }
        }
    };
}

impl_blocking_pricing_countries_resource!(
    BlockingPricingV1PhoneNumberCountriesResource,
    TwilioPricingPhoneNumberCountry,
    WirePricingPhoneNumberCountry,
    ApiFamily::PricingV1,
    pricing_endpoint,
    PricingPageResource::V1PhoneNumbers,
    ["PhoneNumbers", "Countries"],
    "pricing.v1.phone_numbers.countries"
);

impl_blocking_pricing_countries_resource!(
    BlockingPricingV1VoiceCountriesResource,
    TwilioPricingVoiceCountry,
    WirePricingVoiceCountry,
    ApiFamily::PricingV1,
    pricing_endpoint,
    PricingPageResource::V1Voice,
    ["Voice", "Countries"],
    "pricing.v1.voice.countries"
);

impl_blocking_pricing_countries_resource!(
    BlockingPricingV2VoiceCountriesResource,
    TwilioPricingOriginBasedVoiceCountry,
    WirePricingOriginBasedVoiceCountry,
    ApiFamily::PricingV2,
    pricing_v2_endpoint,
    PricingPageResource::V2Voice,
    ["Voice", "Countries"],
    "pricing.v2.voice.countries"
);

impl_blocking_pricing_countries_resource!(
    BlockingPricingV2TrunkingCountriesResource,
    TwilioPricingTrunkingCountry,
    WirePricingTrunkingCountry,
    ApiFamily::PricingV2,
    pricing_v2_endpoint,
    PricingPageResource::V2Trunking,
    ["Trunking", "Countries"],
    "pricing.v2.trunking.countries"
);

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV2VoiceNumberResource<'a> {
    account: BlockingTwilioAccount<'a>,
    destination_number: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV2VoiceNumberResource<'a> {
    /// `GET /Voice/Numbers/{DestinationNumber}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn fetch(
        self,
        request: FetchPricingOriginBasedNumberRequest<'a>,
    ) -> Result<TwilioPricingOriginBasedVoiceNumber, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.v2.voice.numbers.fetch",
            "GET",
        )
        .in_scope(|| {
            validate_required("DestinationNumber", self.destination_number)?;
            request.validate()?;
            let mut sensitive_values = sensitive_values_blocking(self.account);
            sensitive_values.push(self.destination_number);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self.account.client.pricing_v2_endpoint(&[
                "Voice",
                "Numbers",
                self.destination_number,
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::new(
                ApiFamily::PricingV2,
                Method::GET,
                ["Voice", "Numbers", self.destination_number],
            )
            .operation("pricing.v2.voice.numbers.fetch")
            .query_pairs(request.query_pairs());
            let parsed: WirePricingOriginBasedVoiceNumber =
                self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_number())
        })
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingPricingV2TrunkingNumberResource<'a> {
    account: BlockingTwilioAccount<'a>,
    destination_number: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingPricingV2TrunkingNumberResource<'a> {
    /// `GET /Trunking/Numbers/{DestinationNumber}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn fetch(
        self,
        request: FetchPricingOriginBasedNumberRequest<'a>,
    ) -> Result<TwilioPricingTrunkingNumber, TwilioError> {
        request_span(
            &self.account.client.config.pricing,
            "pricing.v2.trunking.numbers.fetch",
            "GET",
        )
        .in_scope(|| {
            validate_required("DestinationNumber", self.destination_number)?;
            request.validate()?;
            let mut sensitive_values = sensitive_values_blocking(self.account);
            sensitive_values.push(self.destination_number);
            sensitive_values.extend(request.sensitive_values());
            let mut url = self.account.client.pricing_v2_endpoint(&[
                "Trunking",
                "Numbers",
                self.destination_number,
            ])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::new(
                ApiFamily::PricingV2,
                Method::GET,
                ["Trunking", "Numbers", self.destination_number],
            )
            .operation("pricing.v2.trunking.numbers.fetch")
            .query_pairs(request.query_pairs());
            let parsed: WirePricingTrunkingNumber =
                self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_number())
        })
    }
}

#[cfg(feature = "async")]
fn sensitive_values(account: TwilioAccount<'_>) -> Vec<&str> {
    vec![account.creds.account_sid(), account.creds.auth_secret()]
}

#[cfg(feature = "sync")]
fn sensitive_values_blocking(account: BlockingTwilioAccount<'_>) -> Vec<&str> {
    vec![account.creds.account_sid(), account.creds.auth_secret()]
}

fn append_query_pairs(url: &mut Url, spec: &RequestSpec) {
    if spec.query.is_empty() {
        return;
    }
    let mut query = url.query_pairs_mut();
    for (key, value) in &spec.query {
        query.append_pair(key, value);
    }
}

fn validate_next_page(
    base_url: &Url,
    current_url: Option<&Url>,
    next_page_url: Option<&str>,
    resource: PricingPageResource,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = pricing_page_url_from_base(base_url, next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_pricing_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}

fn normalize_iso_country(value: &str) -> Result<String, TwilioError> {
    let trimmed = value.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(TwilioError::InvalidRequest(
            "IsoCountry must be a two-letter ASCII country code".to_owned(),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

fn validate_required(name: &str, value: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}
