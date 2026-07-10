#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::collections::BTreeMap;
use std::fmt;

use http::Method;
use serde::Deserialize;
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
    ApiFamily, DEFAULT_PAGE_SIZE, FormParam, RawResponse, RequestSpec, TwilioAuth, TwilioError,
    V1PageMeta, V1PageResource, WireV1PageMeta, decode_json_response, non_empty, push_bool,
    push_sensitive, push_str, redacted_option, redacted_optional, request_span, validate_page_size,
    validate_v1_meta_key, validate_v1_next_page_continuation,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};
#[cfg(feature = "sync")]
use crate::services::BlockingServiceResource;
#[cfg(feature = "async")]
use crate::services::ServiceResource;

macro_rules! setter {
    ($field:ident) => {
        #[must_use]
        pub fn $field(mut self, value: &'a str) -> Self {
            self.$field = Some(value);
            self
        }
    };
}

macro_rules! slice_setter {
    ($field:ident) => {
        #[must_use]
        pub fn $field(mut self, value: &'a [&'a str]) -> Self {
            self.$field = value;
            self
        }
    };
}

macro_rules! bool_setter {
    ($field:ident) => {
        #[must_use]
        pub fn $field(mut self, value: bool) -> Self {
            self.$field = Some(value);
            self
        }
    };
}

macro_rules! enum_setter {
    ($field:ident, $ty:ty) => {
        #[must_use]
        pub fn $field(mut self, value: $ty) -> Self {
            self.$field = Some(value);
            self
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A2PBrandType<'a> {
    Standard,
    SoleProprietor,
    LowVolumeStandard,
    Custom(&'a str),
}

impl<'a> A2PBrandType<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::Standard => "STANDARD",
            Self::SoleProprietor => "SOLE_PROPRIETOR",
            Self::LowVolumeStandard => "LOW_VOLUME_STANDARD",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A2PVettingProvider<'a> {
    CampaignVerify,
    Aegis,
    Custom(&'a str),
}

impl<'a> A2PVettingProvider<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::CampaignVerify => "campaign-verify",
            Self::Aegis => "aegis",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A2PUsecase<'a> {
    TwoFactorAuth,
    AccountNotification,
    Marketing,
    SoleProprietor,
    CustomerCare,
    DeliveryNotification,
    FraudAlert,
    SecurityAlert,
    HigherEducation,
    LowVolume,
    Mixed,
    PollingVoting,
    PublicServiceAnnouncement,
    Custom(&'a str),
}

impl<'a> A2PUsecase<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::TwoFactorAuth => "2FA",
            Self::AccountNotification => "ACCOUNT_NOTIFICATION",
            Self::Marketing => "MARKETING",
            Self::SoleProprietor => "SOLE_PROPRIETOR",
            Self::CustomerCare => "CUSTOMER_CARE",
            Self::DeliveryNotification => "DELIVERY_NOTIFICATION",
            Self::FraudAlert => "FRAUD_ALERT",
            Self::SecurityAlert => "SECURITY_ALERT",
            Self::HigherEducation => "HIGHER_EDUCATION",
            Self::LowVolume => "LOW_VOLUME",
            Self::Mixed => "MIXED",
            Self::PollingVoting => "POLLING_VOTING",
            Self::PublicServiceAnnouncement => "PUBLIC_SERVICE_ANNOUNCEMENT",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct CreateA2PBrandRegistrationRequest<'a> {
    customer_profile_bundle_sid: Option<&'a str>,
    a2p_profile_bundle_sid: Option<&'a str>,
    brand_type: Option<A2PBrandType<'a>>,
    skip_automatic_sec_vet: Option<bool>,
    mock: Option<bool>,
}

impl<'a> CreateA2PBrandRegistrationRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn customer_profile_bundle_sid(mut self, value: &'a str) -> Self {
        self.customer_profile_bundle_sid = Some(value);
        self
    }

    #[must_use]
    pub fn a2p_profile_bundle_sid(mut self, value: &'a str) -> Self {
        self.a2p_profile_bundle_sid = Some(value);
        self
    }

    #[must_use]
    pub fn brand_type(mut self, value: A2PBrandType<'a>) -> Self {
        self.brand_type = Some(value);
        self
    }

    #[must_use]
    pub fn skip_automatic_sec_vet(mut self, value: bool) -> Self {
        self.skip_automatic_sec_vet = Some(value);
        self
    }

    #[must_use]
    pub fn mock(mut self, value: bool) -> Self {
        self.mock = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("CustomerProfileBundleSid", self.customer_profile_bundle_sid)?;
        validate_required("A2PProfileBundleSid", self.a2p_profile_bundle_sid)?;
        if let Some(A2PBrandType::Custom(value)) = self.brand_type {
            validate_required("BrandType", Some(value))?;
        }
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(
            &mut params,
            "CustomerProfileBundleSid",
            self.customer_profile_bundle_sid,
        );
        push_str(
            &mut params,
            "A2PProfileBundleSid",
            self.a2p_profile_bundle_sid,
        );
        push_value(
            &mut params,
            "BrandType",
            self.brand_type.map(A2PBrandType::form_value),
        );
        push_bool(
            &mut params,
            "SkipAutomaticSecVet",
            self.skip_automatic_sec_vet,
        );
        push_bool(&mut params, "Mock", self.mock);
        params
    }

    fn sensitive_values(self, auth: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        push_sensitive(&mut values, self.customer_profile_bundle_sid);
        push_sensitive(&mut values, self.a2p_profile_bundle_sid);
        if let Some(A2PBrandType::Custom(value)) = self.brand_type {
            values.push(value);
        }
        values
    }
}

#[derive(Clone, Copy, Default)]
pub struct ListA2PBrandRegistrationsRequest<'a> {
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListA2PBrandRegistrationsRequest<'a> {
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
        validate_page_size(self.page_size)
    }

    fn apply_query(self, url: &mut Url) {
        apply_page_query(url, self.page_size, self.page, self.page_token);
    }

    fn sensitive_values(self, auth: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        push_sensitive(&mut values, self.page_token);
        values
    }
}

#[derive(Clone, Copy)]
pub struct CreateA2PBrandVettingRequest<'a> {
    vetting_provider: A2PVettingProvider<'a>,
    vetting_id: Option<&'a str>,
}

impl<'a> CreateA2PBrandVettingRequest<'a> {
    #[must_use]
    pub fn new(vetting_provider: A2PVettingProvider<'a>) -> Self {
        Self {
            vetting_provider,
            vetting_id: None,
        }
    }

    #[must_use]
    pub fn vetting_id(mut self, value: &'a str) -> Self {
        self.vetting_id = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        if self.vetting_provider.form_value().trim().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "VettingProvider must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_value(
            &mut params,
            "VettingProvider",
            Some(self.vetting_provider.form_value()),
        );
        push_str(&mut params, "VettingId", self.vetting_id);
        params
    }

    fn sensitive_values(self, auth: &'a TwilioAuth, brand_sid: &'a str) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(brand_sid);
        if let A2PVettingProvider::Custom(value) = self.vetting_provider {
            values.push(value);
        }
        push_sensitive(&mut values, self.vetting_id);
        values
    }
}

#[derive(Clone, Copy, Default)]
pub struct ListA2PBrandVettingsRequest<'a> {
    vetting_provider: Option<A2PVettingProvider<'a>>,
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListA2PBrandVettingsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn vetting_provider(mut self, value: A2PVettingProvider<'a>) -> Self {
        self.vetting_provider = Some(value);
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

    fn validate(self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)?;
        if let Some(value) = self.vetting_provider {
            validate_required("VettingProvider", Some(value.form_value()))?;
        }
        Ok(())
    }

    fn apply_query(self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        if let Some(value) = self.vetting_provider {
            query.append_pair("VettingProvider", value.form_value());
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

    fn sensitive_values(self, auth: &'a TwilioAuth, brand_sid: &'a str) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(brand_sid);
        if let Some(A2PVettingProvider::Custom(value)) = self.vetting_provider {
            values.push(value);
        }
        push_sensitive(&mut values, self.page_token);
        values
    }
}

#[derive(Clone, Copy, Default)]
pub struct CreateUsa2pRequest<'a> {
    brand_registration_sid: Option<&'a str>,
    description: Option<&'a str>,
    message_flow: Option<&'a str>,
    message_samples: &'a [&'a str],
    us_app_to_person_usecase: Option<A2PUsecase<'a>>,
    has_embedded_links: Option<bool>,
    has_embedded_phone: Option<bool>,
    opt_in_message: Option<&'a str>,
    opt_out_message: Option<&'a str>,
    help_message: Option<&'a str>,
    opt_in_keywords: &'a [&'a str],
    opt_out_keywords: &'a [&'a str],
    help_keywords: &'a [&'a str],
    subscriber_opt_in: Option<bool>,
    age_gated: Option<bool>,
    direct_lending: Option<bool>,
    is_externally_registered: Option<bool>,
    campaign_id: Option<&'a str>,
    mock: Option<bool>,
    api_version: Option<&'a str>,
}

impl<'a> CreateUsa2pRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    setter!(brand_registration_sid);
    setter!(description);
    setter!(message_flow);
    slice_setter!(message_samples);
    enum_setter!(us_app_to_person_usecase, A2PUsecase<'a>);
    bool_setter!(has_embedded_links);
    bool_setter!(has_embedded_phone);
    setter!(opt_in_message);
    setter!(opt_out_message);
    setter!(help_message);
    slice_setter!(opt_in_keywords);
    slice_setter!(opt_out_keywords);
    slice_setter!(help_keywords);
    bool_setter!(subscriber_opt_in);
    bool_setter!(age_gated);
    bool_setter!(direct_lending);
    bool_setter!(is_externally_registered);
    setter!(campaign_id);
    bool_setter!(mock);

    #[must_use]
    pub fn api_version(mut self, value: &'a str) -> Self {
        self.api_version = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("BrandRegistrationSid", self.brand_registration_sid)?;
        validate_required("Description", self.description)?;
        validate_required("MessageFlow", self.message_flow)?;
        validate_required(
            "UsAppToPersonUsecase",
            self.us_app_to_person_usecase.map(A2PUsecase::form_value),
        )?;
        validate_required_bool("HasEmbeddedLinks", self.has_embedded_links)?;
        validate_required_bool("HasEmbeddedPhone", self.has_embedded_phone)?;
        if !(2..=5).contains(&self.message_samples.len()) {
            return Err(TwilioError::InvalidRequest(
                "MessageSamples must contain 2..=5 values".to_owned(),
            ));
        }
        if self
            .message_samples
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err(TwilioError::InvalidRequest(
                "MessageSamples values must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(
            &mut params,
            "BrandRegistrationSid",
            self.brand_registration_sid,
        );
        push_str(&mut params, "Description", self.description);
        push_str(&mut params, "MessageFlow", self.message_flow);
        for value in self.message_samples {
            push_str(&mut params, "MessageSamples", Some(value));
        }
        push_value(
            &mut params,
            "UsAppToPersonUsecase",
            self.us_app_to_person_usecase.map(A2PUsecase::form_value),
        );
        push_bool(&mut params, "HasEmbeddedLinks", self.has_embedded_links);
        push_bool(&mut params, "HasEmbeddedPhone", self.has_embedded_phone);
        push_str(&mut params, "OptInMessage", self.opt_in_message);
        push_str(&mut params, "OptOutMessage", self.opt_out_message);
        push_str(&mut params, "HelpMessage", self.help_message);
        for value in self.opt_in_keywords {
            push_str(&mut params, "OptInKeywords", Some(value));
        }
        for value in self.opt_out_keywords {
            push_str(&mut params, "OptOutKeywords", Some(value));
        }
        for value in self.help_keywords {
            push_str(&mut params, "HelpKeywords", Some(value));
        }
        push_bool(&mut params, "SubscriberOptIn", self.subscriber_opt_in);
        push_bool(&mut params, "AgeGated", self.age_gated);
        push_bool(&mut params, "DirectLending", self.direct_lending);
        push_bool(
            &mut params,
            "IsExternallyRegistered",
            self.is_externally_registered,
        );
        push_str(&mut params, "CampaignId", self.campaign_id);
        push_bool(&mut params, "Mock", self.mock);
        params
    }

    fn apply_headers(self, spec: RequestSpec) -> RequestSpec {
        if let Some(api_version) = self.api_version {
            spec.header("x-Twilio-Api-Version", api_version)
        } else {
            spec
        }
    }

    fn sensitive_values(self, auth: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(service_sid);
        push_sensitive(&mut values, self.brand_registration_sid);
        push_sensitive(&mut values, self.description);
        push_sensitive(&mut values, self.message_flow);
        values.extend(self.message_samples.iter().copied());
        if let Some(A2PUsecase::Custom(value)) = self.us_app_to_person_usecase {
            values.push(value);
        }
        push_sensitive(&mut values, self.opt_in_message);
        push_sensitive(&mut values, self.opt_out_message);
        push_sensitive(&mut values, self.help_message);
        values.extend(self.opt_in_keywords.iter().copied());
        values.extend(self.opt_out_keywords.iter().copied());
        values.extend(self.help_keywords.iter().copied());
        push_sensitive(&mut values, self.campaign_id);
        push_sensitive(&mut values, self.api_version);
        values
    }
}

#[derive(Clone, Copy, Default)]
pub struct ListUsa2pRequest<'a> {
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListUsa2pRequest<'a> {
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
        validate_page_size(self.page_size)
    }

    fn apply_query(self, url: &mut Url) {
        apply_page_query(url, self.page_size, self.page, self.page_token);
    }

    fn sensitive_values(self, auth: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(service_sid);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

#[derive(Clone, Copy, Default)]
pub struct FetchUsa2pUsecasesRequest<'a> {
    brand_registration_sid: Option<&'a str>,
}

impl<'a> FetchUsa2pUsecasesRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn brand_registration_sid(mut self, value: &'a str) -> Self {
        self.brand_registration_sid = Some(value);
        self
    }

    fn apply_query(self, url: &mut Url) {
        if let Some(value) = self.brand_registration_sid {
            url.query_pairs_mut()
                .append_pair("BrandRegistrationSid", value);
        }
    }

    fn sensitive_values(self, auth: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(service_sid);
        push_sensitive(&mut values, self.brand_registration_sid);
        values
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioA2PBrandRegistration {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub customer_profile_bundle_sid: Option<String>,
    pub a2p_profile_bundle_sid: Option<String>,
    pub date_created: Option<String>,
    pub date_updated: Option<String>,
    pub brand_type: Option<String>,
    pub status: Option<String>,
    pub tcr_id: Option<String>,
    pub failure_reason: Option<String>,
    pub url: Option<String>,
    pub brand_score: Option<i64>,
    #[serde(default)]
    pub brand_feedback: Vec<String>,
    pub identity_status: Option<String>,
    pub russell_3000: Option<bool>,
    pub government_entity: Option<bool>,
    pub tax_exempt_status: Option<String>,
    pub skip_automatic_sec_vet: Option<bool>,
    pub mock: Option<bool>,
    #[serde(default)]
    pub errors: Vec<String>,
    pub links: Option<BTreeMap<String, String>>,
}

impl fmt::Debug for TwilioA2PBrandRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioA2PBrandRegistration")
            .field("sid", &redacted_option(&self.sid))
            .field("account_sid", &redacted_option(&self.account_sid))
            .field(
                "customer_profile_bundle_sid",
                &redacted_option(&self.customer_profile_bundle_sid),
            )
            .field(
                "a2p_profile_bundle_sid",
                &redacted_option(&self.a2p_profile_bundle_sid),
            )
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("brand_type", &redacted_option(&self.brand_type))
            .field("status", &self.status)
            .field("tcr_id", &redacted_option(&self.tcr_id))
            .field("failure_reason", &redacted_option(&self.failure_reason))
            .field("url", &redacted_option(&self.url))
            .field("brand_score", &self.brand_score)
            .field(
                "brand_feedback",
                &format_args!("[{} redacted]", self.brand_feedback.len()),
            )
            .field("identity_status", &self.identity_status)
            .field("russell_3000", &self.russell_3000)
            .field("government_entity", &self.government_entity)
            .field(
                "tax_exempt_status",
                &redacted_option(&self.tax_exempt_status),
            )
            .field("skip_automatic_sec_vet", &self.skip_automatic_sec_vet)
            .field("mock", &self.mock)
            .field("errors", &format_args!("[{} redacted]", self.errors.len()))
            .field("links", &redacted_optional_map(self.links.as_ref()))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioA2PBrandRegistrationPage {
    pub brand_registrations: Vec<TwilioA2PBrandRegistration>,
    pub meta: V1PageMeta,
}

#[derive(Clone, Deserialize)]
pub struct TwilioA2PBrandVetting {
    pub account_sid: Option<String>,
    pub brand_sid: Option<String>,
    pub brand_vetting_sid: Option<String>,
    pub vetting_provider: Option<String>,
    pub vetting_id: Option<String>,
    pub vetting_class: Option<String>,
    pub vetting_status: Option<String>,
    pub date_created: Option<String>,
    pub date_updated: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioA2PBrandVetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioA2PBrandVetting")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("brand_sid", &redacted_option(&self.brand_sid))
            .field(
                "brand_vetting_sid",
                &redacted_option(&self.brand_vetting_sid),
            )
            .field("vetting_provider", &redacted_option(&self.vetting_provider))
            .field("vetting_id", &redacted_option(&self.vetting_id))
            .field("vetting_class", &self.vetting_class)
            .field("vetting_status", &self.vetting_status)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioA2PBrandVettingPage {
    pub vettings: Vec<TwilioA2PBrandVetting>,
    pub meta: V1PageMeta,
}

#[derive(Clone, Deserialize)]
pub struct TwilioUsa2p {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub brand_registration_sid: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub message_samples: Vec<String>,
    pub us_app_to_person_usecase: Option<String>,
    pub has_embedded_links: Option<bool>,
    pub has_embedded_phone: Option<bool>,
    pub subscriber_opt_in: Option<bool>,
    pub age_gated: Option<bool>,
    pub direct_lending: Option<bool>,
    pub campaign_status: Option<String>,
    pub campaign_id: Option<String>,
    pub is_externally_registered: Option<bool>,
    pub message_flow: Option<String>,
    pub opt_in_message: Option<String>,
    pub opt_out_message: Option<String>,
    pub help_message: Option<String>,
    #[serde(default)]
    pub opt_in_keywords: Vec<String>,
    #[serde(default)]
    pub opt_out_keywords: Vec<String>,
    #[serde(default)]
    pub help_keywords: Vec<String>,
    pub date_created: Option<String>,
    pub date_updated: Option<String>,
    pub url: Option<String>,
    pub mock: Option<bool>,
    #[serde(default)]
    pub errors: Vec<String>,
    pub rate_limits: Option<serde_json::Value>,
}

impl fmt::Debug for TwilioUsa2p {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioUsa2p")
            .field("sid", &redacted_option(&self.sid))
            .field("account_sid", &redacted_option(&self.account_sid))
            .field(
                "brand_registration_sid",
                &redacted_option(&self.brand_registration_sid),
            )
            .field(
                "messaging_service_sid",
                &redacted_option(&self.messaging_service_sid),
            )
            .field("description", &redacted_option(&self.description))
            .field(
                "message_samples",
                &format_args!("[{} redacted]", self.message_samples.len()),
            )
            .field(
                "us_app_to_person_usecase",
                &redacted_option(&self.us_app_to_person_usecase),
            )
            .field("has_embedded_links", &self.has_embedded_links)
            .field("has_embedded_phone", &self.has_embedded_phone)
            .field("subscriber_opt_in", &self.subscriber_opt_in)
            .field("age_gated", &self.age_gated)
            .field("direct_lending", &self.direct_lending)
            .field("campaign_status", &self.campaign_status)
            .field("campaign_id", &redacted_option(&self.campaign_id))
            .field("is_externally_registered", &self.is_externally_registered)
            .field("message_flow", &redacted_option(&self.message_flow))
            .field("opt_in_message", &redacted_option(&self.opt_in_message))
            .field("opt_out_message", &redacted_option(&self.opt_out_message))
            .field("help_message", &redacted_option(&self.help_message))
            .field(
                "opt_in_keywords",
                &format_args!("[{} redacted]", self.opt_in_keywords.len()),
            )
            .field(
                "opt_out_keywords",
                &format_args!("[{} redacted]", self.opt_out_keywords.len()),
            )
            .field(
                "help_keywords",
                &format_args!("[{} redacted]", self.help_keywords.len()),
            )
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("url", &redacted_option(&self.url))
            .field("mock", &self.mock)
            .field("errors", &format_args!("[{} redacted]", self.errors.len()))
            .field(
                "rate_limits",
                &redacted_optional(self.rate_limits.is_some()),
            )
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioUsa2pPage {
    pub compliance: Vec<TwilioUsa2p>,
    pub meta: V1PageMeta,
}

#[derive(Clone, Deserialize)]
pub struct TwilioUsa2pUsecase {
    pub code: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub post_approval_required: Option<bool>,
}

impl fmt::Debug for TwilioUsa2pUsecase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioUsa2pUsecase")
            .field("code", &self.code)
            .field("name", &self.name)
            .field("description", &self.description)
            .field("post_approval_required", &self.post_approval_required)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TwilioUsa2pUsecases {
    #[serde(default)]
    pub us_app_to_person_usecases: Vec<TwilioUsa2pUsecase>,
}

#[derive(Clone)]
pub struct TwilioA2PBrandRegistrationOtp {
    pub account_sid: Option<String>,
    pub brand_registration_sid: Option<String>,
}

impl fmt::Debug for TwilioA2PBrandRegistrationOtp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioA2PBrandRegistrationOtp")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field(
                "brand_registration_sid",
                &redacted_option(&self.brand_registration_sid),
            )
            .finish()
    }
}

#[derive(Deserialize)]
struct WireA2PBrandRegistrationOtp {
    account_sid: Option<String>,
    brand_registration_sid: Option<String>,
}

impl WireA2PBrandRegistrationOtp {
    fn into_otp(self) -> TwilioA2PBrandRegistrationOtp {
        TwilioA2PBrandRegistrationOtp {
            account_sid: non_empty(self.account_sid),
            brand_registration_sid: non_empty(self.brand_registration_sid),
        }
    }
}

#[derive(Deserialize)]
struct WireA2PBrandRegistrationPage {
    #[serde(default)]
    data: Vec<TwilioA2PBrandRegistration>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireA2PBrandRegistrationPage {
    fn into_page(self) -> TwilioA2PBrandRegistrationPage {
        TwilioA2PBrandRegistrationPage {
            brand_registrations: self.data,
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireA2PBrandVettingPage {
    #[serde(default)]
    data: Vec<TwilioA2PBrandVetting>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireA2PBrandVettingPage {
    fn into_page(self) -> TwilioA2PBrandVettingPage {
        TwilioA2PBrandVettingPage {
            vettings: self.data,
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireUsa2pPage {
    #[serde(default)]
    compliance: Vec<TwilioUsa2p>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireUsa2pPage {
    fn into_page(self) -> TwilioUsa2pPage {
        TwilioUsa2pPage {
            compliance: self.compliance,
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct A2PBrandRegistrationsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> A2PBrandRegistrationsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn create(
        self,
        request: CreateA2PBrandRegistrationRequest<'a>,
    ) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["a2p", "BrandRegistrations"],
            )
            .operation("a2p.brand_registrations.create")
            .form_params(request.form_params());
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.create",
            "POST",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn list(
        self,
        request: ListA2PBrandRegistrationsRequest<'a>,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        async move {
            request.validate()?;
            let mut url = self
                .account
                .client
                .messaging_endpoint(&["a2p", "BrandRegistrations"])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_registrations.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.list",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        async move {
            let resource = V1PageResource::A2PBrandRegistrations;
            let url = self.account.client.v1_page_url(next_page_url, resource)?;
            let sensitive_values = self.account.creds.sensitive_values();
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_registrations.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.list_page_url",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        let parsed: WireA2PBrandRegistrationPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::A2PBrandRegistrations;
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url(
            self.account,
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, TwilioA2PBrandRegistrationPage, TwilioA2PBrandRegistration> {
        self.list_all_with(ListA2PBrandRegistrationsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListA2PBrandRegistrationsRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioA2PBrandRegistrationPage, TwilioA2PBrandRegistration> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_url(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioA2PBrandRegistrationPage>
            },
            split_brand_registration_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct A2PBrandRegistrationResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> A2PBrandRegistrationResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    #[must_use]
    pub fn vettings(self) -> A2PBrandVettingsResource<'a> {
        A2PBrandVettingsResource::new(self)
    }

    #[must_use]
    pub fn sms_otp(self) -> A2PBrandRegistrationSmsOtpResource<'a> {
        A2PBrandRegistrationSmsOtpResource::new(self)
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn fetch(self) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.brand_spec(Method::GET, "a2p.brand_registration.fetch");
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registration.fetch",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn update(self) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.brand_spec(Method::POST, "a2p.brand_registration.update");
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registration.update",
            "POST",
        ))
        .await
    }

    fn brand_spec(self, method: Method, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.sid],
        )
        .operation(operation)
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = self.account.creds.sensitive_values();
        values.push(self.sid);
        values
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct A2PBrandRegistrationSmsOtpResource<'a> {
    brand: A2PBrandRegistrationResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> A2PBrandRegistrationSmsOtpResource<'a> {
    fn new(brand: A2PBrandRegistrationResource<'a>) -> Self {
        Self { brand }
    }

    /// Retry SMS OTP verification for a Sole Proprietor Brand Registration.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn create(self) -> Result<TwilioA2PBrandRegistrationOtp, TwilioError> {
        async move {
            let sensitive_values = self.brand.sensitive_values();
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["a2p", "BrandRegistrations", self.brand.sid, "SmsOtp"],
            )
            .operation("a2p.brand_registration.sms_otp.create");
            let parsed: WireA2PBrandRegistrationOtp = self
                .brand
                .account
                .send_spec_json(spec, &sensitive_values)
                .await?;
            Ok(parsed.into_otp())
        }
        .instrument(request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_registration.sms_otp.create",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct A2PBrandVettingsResource<'a> {
    brand: A2PBrandRegistrationResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> A2PBrandVettingsResource<'a> {
    fn new(brand: A2PBrandRegistrationResource<'a>) -> Self {
        Self { brand }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn create(
        self,
        request: CreateA2PBrandVettingRequest<'a>,
    ) -> Result<TwilioA2PBrandVetting, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.brand.account.creds, self.brand.sid);
            let spec = self
                .collection_spec(Method::POST, "a2p.brand_vettings.create")
                .form_params(request.form_params());
            self.brand
                .account
                .send_spec_json(spec, &sensitive_values)
                .await
        }
        .instrument(request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.create",
            "POST",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn list(
        self,
        request: ListA2PBrandVettingsRequest<'a>,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        async move {
            request.validate()?;
            let mut url = self.brand.account.client.messaging_endpoint(&[
                "a2p",
                "BrandRegistrations",
                self.brand.sid,
                "Vettings",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values =
                request.sensitive_values(self.brand.account.creds, self.brand.sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_vettings.list",
            );
            let raw = self
                .brand
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.list",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        async move {
            let resource = V1PageResource::A2PBrandVettings {
                brand_sid: self.brand.sid,
            };
            let url = self
                .brand
                .account
                .client
                .v1_page_url(next_page_url, resource)?;
            let mut sensitive_values = self.brand.account.creds.sensitive_values();
            sensitive_values.push(self.brand.sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_vettings.list_page_url",
            );
            let raw = self
                .brand
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.list_page_url",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn fetch(self, sid: &'a str) -> Result<TwilioA2PBrandVetting, TwilioError> {
        async move {
            let mut sensitive_values = self.brand.sensitive_values();
            sensitive_values.push(sid);
            let spec = self.vetting_spec(Method::GET, sid, "a2p.brand_vetting.fetch");
            self.brand
                .account
                .send_spec_json(spec, &sensitive_values)
                .await
        }
        .instrument(request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vetting.fetch",
            "GET",
        ))
        .await
    }

    fn collection_spec(self, method: Method, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.brand.sid, "Vettings"],
        )
        .operation(operation)
    }

    fn vetting_spec(self, method: Method, sid: &'a str, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.brand.sid, "Vettings", sid],
        )
        .operation(operation)
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        let parsed: WireA2PBrandVettingPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::A2PBrandVettings {
            brand_sid: self.brand.sid,
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url(
            self.brand.account,
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioA2PBrandVettingPage, TwilioA2PBrandVetting> {
        self.list_all_with(ListA2PBrandVettingsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListA2PBrandVettingsRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioA2PBrandVettingPage, TwilioA2PBrandVetting> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_url(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioA2PBrandVettingPage>
            },
            split_vetting_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ServiceUsa2pResource<'a> {
    service: ServiceResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> ServiceUsa2pResource<'a> {
    pub(crate) fn new(service: ServiceResource<'a>) -> Self {
        Self { service }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn create(self, request: CreateUsa2pRequest<'a>) -> Result<TwilioUsa2p, TwilioError> {
        async move {
            request.validate()?;
            let account = self.service.account();
            let service_sid = self.service.sid();
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = request.apply_headers(
                RequestSpec::new(
                    ApiFamily::MessagingV1,
                    Method::POST,
                    ["Services", service_sid, "Compliance", "Usa2p"],
                )
                .operation("service.usa2p.create")
                .form_params(request.form_params()),
            );
            account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.create",
            "POST",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub async fn list(self, request: ListUsa2pRequest<'a>) -> Result<TwilioUsa2pPage, TwilioError> {
        async move {
            request.validate()?;
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut url = account.client.messaging_endpoint(&[
                "Services",
                service_sid,
                "Compliance",
                "Usa2p",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "service.usa2p.list",
            );
            let raw = account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.list",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub async fn list_page_url(self, next_page_url: &str) -> Result<TwilioUsa2pPage, TwilioError> {
        async move {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let resource = V1PageResource::Usa2p { service_sid };
            let url = account.client.v1_page_url(next_page_url, resource)?;
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.push(service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "service.usa2p.list_page_url",
            );
            let raw = account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.list_page_url",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn fetch(self, sid: &'a str) -> Result<TwilioUsa2p, TwilioError> {
        async move {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.extend([service_sid, sid]);
            let spec = self.instance_spec(Method::GET, sid, "service.usa2p.fetch");
            account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.fetch",
            "GET",
        ))
        .await
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures or API errors.
    pub async fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
        async move {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.extend([service_sid, sid]);
            let spec = self.instance_spec(Method::DELETE, sid, "service.usa2p.delete");
            account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.delete",
            "DELETE",
        ))
        .await
    }

    fn instance_spec(self, method: Method, sid: &'a str, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["Services", self.service.sid(), "Compliance", "Usa2p", sid],
        )
        .operation(operation)
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioUsa2pPage, TwilioError> {
        let parsed: WireUsa2pPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::Usa2p {
            service_sid: self.service.sid(),
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url(
            self.service.account(),
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioUsa2pPage, TwilioUsa2p> {
        self.list_all_with(ListUsa2pRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListUsa2pRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioUsa2pPage, TwilioUsa2p> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_url(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioUsa2pPage>
            },
            split_usa2p_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ServiceUsa2pUsecasesResource<'a> {
    service: ServiceResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> ServiceUsa2pUsecasesResource<'a> {
    pub(crate) fn new(service: ServiceResource<'a>) -> Self {
        Self { service }
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub async fn fetch(
        self,
        request: FetchUsa2pUsecasesRequest<'a>,
    ) -> Result<TwilioUsa2pUsecases, TwilioError> {
        async move {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut url = account.client.messaging_endpoint(&[
                "Services",
                service_sid,
                "Compliance",
                "Usa2p",
                "Usecases",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url,
                "service.usa2p_usecases.fetch",
            );
            account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p_usecases.fetch",
            "GET",
        ))
        .await
    }
}

// Blocking mirrors are intentionally thin wrappers over the same request helpers.

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingA2PBrandRegistrationsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingA2PBrandRegistrationsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn create(
        self,
        request: CreateA2PBrandRegistrationRequest<'a>,
    ) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["a2p", "BrandRegistrations"],
            )
            .operation("a2p.brand_registrations.create")
            .form_params(request.form_params());
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn list(
        self,
        request: ListA2PBrandRegistrationsRequest<'a>,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let mut url = self
                .account
                .client
                .messaging_endpoint(&["a2p", "BrandRegistrations"])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_registrations.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registrations.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let resource = V1PageResource::A2PBrandRegistrations;
            let url = self.account.client.v1_page_url(next_page_url, resource)?;
            let sensitive_values = self.account.creds.sensitive_values();
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_registrations.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioA2PBrandRegistrationPage, TwilioError> {
        let parsed: WireA2PBrandRegistrationPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::A2PBrandRegistrations;
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url_blocking(
            self.account,
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, TwilioA2PBrandRegistrationPage, TwilioA2PBrandRegistration>
    {
        self.list_all_with(ListA2PBrandRegistrationsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListA2PBrandRegistrationsRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioA2PBrandRegistrationPage, TwilioA2PBrandRegistration>
    {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_url(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_brand_registration_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingA2PBrandRegistrationResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingA2PBrandRegistrationResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    #[must_use]
    pub fn vettings(self) -> BlockingA2PBrandVettingsResource<'a> {
        BlockingA2PBrandVettingsResource::new(self)
    }

    #[must_use]
    pub fn sms_otp(self) -> BlockingA2PBrandRegistrationSmsOtpResource<'a> {
        BlockingA2PBrandRegistrationSmsOtpResource::new(self)
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn fetch(self) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registration.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values();
            let spec = self.brand_spec(Method::GET, "a2p.brand_registration.fetch");
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn update(self) -> Result<TwilioA2PBrandRegistration, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "a2p.brand_registration.update",
            "POST",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values();
            let spec = self.brand_spec(Method::POST, "a2p.brand_registration.update");
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }

    fn brand_spec(self, method: Method, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.sid],
        )
        .operation(operation)
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = self.account.creds.sensitive_values();
        values.push(self.sid);
        values
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingA2PBrandRegistrationSmsOtpResource<'a> {
    brand: BlockingA2PBrandRegistrationResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingA2PBrandRegistrationSmsOtpResource<'a> {
    fn new(brand: BlockingA2PBrandRegistrationResource<'a>) -> Self {
        Self { brand }
    }

    /// Retry SMS OTP verification for a Sole Proprietor Brand Registration.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn create(self) -> Result<TwilioA2PBrandRegistrationOtp, TwilioError> {
        request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_registration.sms_otp.create",
            "POST",
        )
        .in_scope(|| {
            let sensitive_values = self.brand.sensitive_values();
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["a2p", "BrandRegistrations", self.brand.sid, "SmsOtp"],
            )
            .operation("a2p.brand_registration.sms_otp.create");
            let parsed: WireA2PBrandRegistrationOtp =
                self.brand.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_otp())
        })
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingA2PBrandVettingsResource<'a> {
    brand: BlockingA2PBrandRegistrationResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingA2PBrandVettingsResource<'a> {
    fn new(brand: BlockingA2PBrandRegistrationResource<'a>) -> Self {
        Self { brand }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn create(
        self,
        request: CreateA2PBrandVettingRequest<'a>,
    ) -> Result<TwilioA2PBrandVetting, TwilioError> {
        request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.brand.account.creds, self.brand.sid);
            let spec = self
                .collection_spec(Method::POST, "a2p.brand_vettings.create")
                .form_params(request.form_params());
            self.brand.account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn list(
        self,
        request: ListA2PBrandVettingsRequest<'a>,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let mut url = self.brand.account.client.messaging_endpoint(&[
                "a2p",
                "BrandRegistrations",
                self.brand.sid,
                "Vettings",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values =
                request.sensitive_values(self.brand.account.creds, self.brand.sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_vettings.list",
            );
            let raw = self.brand.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vettings.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let resource = V1PageResource::A2PBrandVettings {
                brand_sid: self.brand.sid,
            };
            let url = self
                .brand
                .account
                .client
                .v1_page_url(next_page_url, resource)?;
            let mut sensitive_values = self.brand.account.creds.sensitive_values();
            sensitive_values.push(self.brand.sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "a2p.brand_vettings.list_page_url",
            );
            let raw = self.brand.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn fetch(self, sid: &'a str) -> Result<TwilioA2PBrandVetting, TwilioError> {
        request_span(
            &self.brand.account.client.config.messaging,
            "a2p.brand_vetting.fetch",
            "GET",
        )
        .in_scope(|| {
            let mut sensitive_values = self.brand.sensitive_values();
            sensitive_values.push(sid);
            let spec = self.vetting_spec(Method::GET, sid, "a2p.brand_vetting.fetch");
            self.brand.account.send_spec_json(spec, &sensitive_values)
        })
    }

    fn collection_spec(self, method: Method, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.brand.sid, "Vettings"],
        )
        .operation(operation)
    }

    fn vetting_spec(self, method: Method, sid: &'a str, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["a2p", "BrandRegistrations", self.brand.sid, "Vettings", sid],
        )
        .operation(operation)
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioA2PBrandVettingPage, TwilioError> {
        let parsed: WireA2PBrandVettingPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::A2PBrandVettings {
            brand_sid: self.brand.sid,
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url_blocking(
            self.brand.account,
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, TwilioA2PBrandVettingPage, TwilioA2PBrandVetting> {
        self.list_all_with(ListA2PBrandVettingsRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListA2PBrandVettingsRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioA2PBrandVettingPage, TwilioA2PBrandVetting> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_url(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_vetting_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingServiceUsa2pResource<'a> {
    service: BlockingServiceResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingServiceUsa2pResource<'a> {
    pub(crate) fn new(service: BlockingServiceResource<'a>) -> Self {
        Self { service }
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn create(self, request: CreateUsa2pRequest<'a>) -> Result<TwilioUsa2p, TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let account = self.service.account();
            let service_sid = self.service.sid();
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = request.apply_headers(
                RequestSpec::new(
                    ApiFamily::MessagingV1,
                    Method::POST,
                    ["Services", service_sid, "Compliance", "Usa2p"],
                )
                .operation("service.usa2p.create")
                .form_params(request.form_params()),
            );
            account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for invalid requests, transport failures, API errors, or malformed JSON.
    pub fn list(self, request: ListUsa2pRequest<'a>) -> Result<TwilioUsa2pPage, TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut url = account.client.messaging_endpoint(&[
                "Services",
                service_sid,
                "Compliance",
                "Usa2p",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "service.usa2p.list",
            );
            let raw = account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] when the URL is invalid, transport fails, or JSON is malformed.
    pub fn list_page_url(self, next_page_url: &str) -> Result<TwilioUsa2pPage, TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let resource = V1PageResource::Usa2p { service_sid };
            let url = account.client.v1_page_url(next_page_url, resource)?;
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.push(service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url.clone(),
                "service.usa2p.list_page_url",
            );
            let raw = account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn fetch(self, sid: &'a str) -> Result<TwilioUsa2p, TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.fetch",
            "GET",
        )
        .in_scope(|| {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.extend([service_sid, sid]);
            let spec = self.instance_spec(Method::GET, sid, "service.usa2p.fetch");
            account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures or API errors.
    pub fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p.delete",
            "DELETE",
        )
        .in_scope(|| {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut sensitive_values = account.creds.sensitive_values();
            sensitive_values.extend([service_sid, sid]);
            let spec = self.instance_spec(Method::DELETE, sid, "service.usa2p.delete");
            account.send_spec_empty(spec, &sensitive_values)
        })
    }

    fn instance_spec(self, method: Method, sid: &'a str, operation: &'static str) -> RequestSpec {
        RequestSpec::new(
            ApiFamily::MessagingV1,
            method,
            ["Services", self.service.sid(), "Compliance", "Usa2p", sid],
        )
        .operation(operation)
    }

    fn read_page(
        self,
        raw: &RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioUsa2pPage, TwilioError> {
        let parsed: WireUsa2pPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::Usa2p {
            service_sid: self.service.sid(),
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url_blocking(
            self.service.account(),
            page.meta.next_page_url.as_deref(),
            current_url,
            resource,
        )?;
        Ok(page)
    }

    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, TwilioUsa2pPage, TwilioUsa2p> {
        self.list_all_with(ListUsa2pRequest::new())
    }

    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListUsa2pRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioUsa2pPage, TwilioUsa2p> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_url(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_usa2p_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingServiceUsa2pUsecasesResource<'a> {
    service: BlockingServiceResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingServiceUsa2pUsecasesResource<'a> {
    pub(crate) fn new(service: BlockingServiceResource<'a>) -> Self {
        Self { service }
    }

    /// # Errors
    /// Returns [`TwilioError`] for transport failures, API errors, or malformed JSON.
    pub fn fetch(
        self,
        request: FetchUsa2pUsecasesRequest<'a>,
    ) -> Result<TwilioUsa2pUsecases, TwilioError> {
        request_span(
            &self.service.account().client.config.messaging,
            "service.usa2p_usecases.fetch",
            "GET",
        )
        .in_scope(|| {
            let account = self.service.account();
            let service_sid = self.service.sid();
            let mut url = account.client.messaging_endpoint(&[
                "Services",
                service_sid,
                "Compliance",
                "Usa2p",
                "Usecases",
            ])?;
            request.apply_query(&mut url);
            let sensitive_values = request.sensitive_values(account.creds, service_sid);
            let spec = RequestSpec::from_url(
                ApiFamily::MessagingV1,
                Method::GET,
                url,
                "service.usa2p_usecases.fetch",
            );
            account.send_spec_json(spec, &sensitive_values)
        })
    }
}

fn push_value(params: &mut Vec<FormParam>, key: &'static str, value: Option<&str>) {
    push_str(params, key, value);
}

fn validate_required(name: &str, value: Option<&str>) -> Result<(), TwilioError> {
    if value.is_some_and(|value| !value.trim().is_empty()) {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )))
    }
}

fn validate_required_bool(name: &str, value: Option<bool>) -> Result<(), TwilioError> {
    if value.is_some() {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(format!("{name} is required")))
    }
}

fn apply_page_query(
    url: &mut Url,
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&str>,
) {
    let mut query = url.query_pairs_mut();
    if let Some(value) = page_size {
        query.append_pair("PageSize", &value.to_string());
    }
    if let Some(value) = page {
        query.append_pair("Page", &value.to_string());
    }
    if let Some(value) = page_token {
        query.append_pair("PageToken", value);
    }
}

#[cfg(feature = "async")]
fn validate_next_v1_url(
    account: TwilioAccount<'_>,
    next_page_url: Option<&str>,
    current_url: Option<&Url>,
    resource: V1PageResource<'_>,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = account.client.v1_page_url(next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_v1_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}

#[cfg(feature = "sync")]
fn validate_next_v1_url_blocking(
    account: BlockingTwilioAccount<'_>,
    next_page_url: Option<&str>,
    current_url: Option<&Url>,
    resource: V1PageResource<'_>,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = account.client.v1_page_url(next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_v1_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}

fn split_brand_registration_page(
    page: TwilioA2PBrandRegistrationPage,
) -> (Vec<TwilioA2PBrandRegistration>, Option<String>) {
    (page.brand_registrations, page.meta.next_page_url)
}

fn split_vetting_page(
    page: TwilioA2PBrandVettingPage,
) -> (Vec<TwilioA2PBrandVetting>, Option<String>) {
    (page.vettings, page.meta.next_page_url)
}

fn split_usa2p_page(page: TwilioUsa2pPage) -> (Vec<TwilioUsa2p>, Option<String>) {
    (page.compliance, page.meta.next_page_url)
}

fn redacted_optional_map(value: Option<&BTreeMap<String, String>>) -> Option<&'static str> {
    value.map(|_| crate::common::REDACTED)
}
