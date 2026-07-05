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

/// Pricing v1 resources.
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

    /// Pricing v1 Messaging resource.
    #[must_use]
    pub fn messaging(self) -> PricingMessagingResource<'a> {
        PricingMessagingResource::new(self.account)
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
            let spec = RequestSpec::new(ApiFamily::Pricing, Method::GET, ["Messaging"])
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
            let spec =
                RequestSpec::new(ApiFamily::Pricing, Method::GET, ["Messaging", "Countries"])
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
            let resource = PricingPageResource::MessagingCountries;
            let url = self
                .account
                .client
                .pricing_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Pricing,
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
                ApiFamily::Pricing,
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
        let resource = PricingPageResource::MessagingCountries;
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

/// Pricing v1 resources.
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

    /// Pricing v1 Messaging resource.
    #[must_use]
    pub fn messaging(self) -> BlockingPricingMessagingResource<'a> {
        BlockingPricingMessagingResource::new(self.account)
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
            let spec = RequestSpec::new(ApiFamily::Pricing, Method::GET, ["Messaging"])
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
            let spec =
                RequestSpec::new(ApiFamily::Pricing, Method::GET, ["Messaging", "Countries"])
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
            let resource = PricingPageResource::MessagingCountries;
            let url = self
                .account
                .client
                .pricing_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Pricing,
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
                ApiFamily::Pricing,
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
        let resource = PricingPageResource::MessagingCountries;
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

#[cfg(feature = "async")]
fn sensitive_values(account: TwilioAccount<'_>) -> Vec<&str> {
    vec![account.creds.account_sid(), account.creds.auth_token()]
}

#[cfg(feature = "sync")]
fn sensitive_values_blocking(account: BlockingTwilioAccount<'_>) -> Vec<&str> {
    vec![account.creds.account_sid(), account.creds.auth_token()]
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
