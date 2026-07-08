#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::collections::BTreeMap;

use http::Method;
use serde::Deserialize;
use time::OffsetDateTime;
#[cfg(feature = "async")]
use tracing::Instrument as _;
use url::Url;

#[cfg(feature = "sync")]
use crate::a2p::{BlockingServiceUsa2pResource, BlockingServiceUsa2pUsecasesResource};
#[cfg(feature = "async")]
use crate::a2p::{ServiceUsa2pResource, ServiceUsa2pUsecasesResource};
#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "sync")]
use crate::common::BlockingTwilioPaginator;
use crate::common::{
    ApiFamily, DEFAULT_PAGE_SIZE, FormEnum, FormParam, RequestSpec, TwilioAuth, TwilioError,
    V1PageMeta, V1PageResource, WireV1PageMeta, decode_json_response, has_non_empty, parse_iso8601,
    push_bool, push_enum, push_sensitive, push_str, push_u32, redacted_option, request_span,
    validate_page_size, validate_v1_meta_key, validate_v1_next_page_continuation,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};

const SERVICE_FRIENDLY_NAME_MAX_CHARS: usize = 64;
const SERVICE_VALIDITY_PERIOD_MIN_SECONDS: u32 = 1;
const SERVICE_VALIDITY_PERIOD_MAX_SECONDS: u32 = 36_000;

/// HTTP method values accepted by Messaging Service webhook fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl FormEnum for HttpMethod {
    fn form_value(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// Values accepted by the `ScanMessageContent` Service setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanMessageContent {
    Inherit,
    Enable,
    Disable,
}

impl FormEnum for ScanMessageContent {
    fn form_value(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }
}

/// Documented Messaging Service use cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceUsecase {
    Notifications,
    Marketing,
    Verification,
    Discussion,
    Poll,
    Undeclared,
}

impl FormEnum for ServiceUsecase {
    fn form_value(self) -> &'static str {
        match self {
            Self::Notifications => "notifications",
            Self::Marketing => "marketing",
            Self::Verification => "verification",
            Self::Discussion => "discussion",
            Self::Poll => "poll",
            Self::Undeclared => "undeclared",
        }
    }
}

#[derive(Clone, Copy)]
enum StringSetting<'a> {
    Set(&'a str),
    Clear,
}

#[derive(Clone, Copy, Default)]
struct ServiceFields<'a> {
    friendly_name: Option<&'a str>,
    inbound_request_url: Option<StringSetting<'a>>,
    inbound_method: Option<HttpMethod>,
    fallback_url: Option<StringSetting<'a>>,
    fallback_method: Option<HttpMethod>,
    status_callback: Option<StringSetting<'a>>,
    sticky_sender: Option<bool>,
    mms_converter: Option<bool>,
    smart_encoding: Option<bool>,
    scan_message_content: Option<ScanMessageContent>,
    fallback_to_long_code: Option<bool>,
    area_code_geomatch: Option<bool>,
    synchronous_validation: Option<bool>,
    validity_period: Option<u32>,
    usecase: Option<ServiceUsecase>,
    use_inbound_webhook_on_number: Option<bool>,
}

impl<'a> ServiceFields<'a> {
    fn is_empty(self) -> bool {
        self.form_params().is_empty()
    }

    fn validate_documented_limits(self) -> Result<(), TwilioError> {
        // These are Twilio API contract checks, not Rust/framework safeguards.
        // Keeping them local gives callers deterministic InvalidRequest errors
        // for simple documented hard limits before any network request is made.
        if let Some(friendly_name) = self.friendly_name {
            if friendly_name.trim().is_empty() {
                return Err(TwilioError::InvalidRequest(
                    "FriendlyName must not be empty".to_owned(),
                ));
            }
            if friendly_name.chars().count() > SERVICE_FRIENDLY_NAME_MAX_CHARS {
                return Err(TwilioError::InvalidRequest(format!(
                    "FriendlyName must be at most {SERVICE_FRIENDLY_NAME_MAX_CHARS} characters"
                )));
            }
        }
        if let Some(validity_period) = self.validity_period {
            if !(SERVICE_VALIDITY_PERIOD_MIN_SECONDS..=SERVICE_VALIDITY_PERIOD_MAX_SECONDS)
                .contains(&validity_period)
            {
                return Err(TwilioError::InvalidRequest(format!(
                    "ValidityPeriod must be in {SERVICE_VALIDITY_PERIOD_MIN_SECONDS}..={SERVICE_VALIDITY_PERIOD_MAX_SECONDS}"
                )));
            }
        }
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "FriendlyName", self.friendly_name);
        push_string_setting(&mut params, "InboundRequestUrl", self.inbound_request_url);
        push_enum(&mut params, "InboundMethod", self.inbound_method);
        push_string_setting(&mut params, "FallbackUrl", self.fallback_url);
        push_enum(&mut params, "FallbackMethod", self.fallback_method);
        push_string_setting(&mut params, "StatusCallback", self.status_callback);
        push_bool(&mut params, "StickySender", self.sticky_sender);
        push_bool(&mut params, "MmsConverter", self.mms_converter);
        push_bool(&mut params, "SmartEncoding", self.smart_encoding);
        push_enum(&mut params, "ScanMessageContent", self.scan_message_content);
        push_bool(
            &mut params,
            "FallbackToLongCode",
            self.fallback_to_long_code,
        );
        push_bool(&mut params, "AreaCodeGeomatch", self.area_code_geomatch);
        push_bool(
            &mut params,
            "SynchronousValidation",
            self.synchronous_validation,
        );
        push_u32(&mut params, "ValidityPeriod", self.validity_period);
        push_enum(&mut params, "Usecase", self.usecase);
        push_bool(
            &mut params,
            "UseInboundWebhookOnNumber",
            self.use_inbound_webhook_on_number,
        );
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: Option<&'a str>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_secret()];
        push_sensitive(&mut values, service_sid);
        push_sensitive(&mut values, self.friendly_name);
        push_string_setting_sensitive(&mut values, self.inbound_request_url);
        push_string_setting_sensitive(&mut values, self.fallback_url);
        push_string_setting_sensitive(&mut values, self.status_callback);
        values
    }
}

fn push_string_setting(
    params: &mut Vec<FormParam>,
    key: &'static str,
    value: Option<StringSetting<'_>>,
) {
    match value {
        Some(StringSetting::Set(value)) => push_str(params, key, Some(value)),
        Some(StringSetting::Clear) => push_str(params, key, Some("")),
        None => {}
    }
}

fn push_string_setting_sensitive<'a>(values: &mut Vec<&'a str>, value: Option<StringSetting<'a>>) {
    if let Some(StringSetting::Set(value)) = value {
        values.push(value);
    }
}

macro_rules! service_field_setters {
    () => {
        #[must_use]
        pub fn friendly_name(mut self, value: &'a str) -> Self {
            self.fields.friendly_name = Some(value);
            self
        }

        #[must_use]
        pub fn inbound_request_url(mut self, value: &'a str) -> Self {
            self.fields.inbound_request_url = Some(StringSetting::Set(value));
            self
        }

        #[must_use]
        pub fn clear_inbound_request_url(mut self) -> Self {
            self.fields.inbound_request_url = Some(StringSetting::Clear);
            self
        }

        #[must_use]
        pub fn inbound_method(mut self, value: HttpMethod) -> Self {
            self.fields.inbound_method = Some(value);
            self
        }

        #[must_use]
        pub fn fallback_url(mut self, value: &'a str) -> Self {
            self.fields.fallback_url = Some(StringSetting::Set(value));
            self
        }

        #[must_use]
        pub fn clear_fallback_url(mut self) -> Self {
            self.fields.fallback_url = Some(StringSetting::Clear);
            self
        }

        #[must_use]
        pub fn fallback_method(mut self, value: HttpMethod) -> Self {
            self.fields.fallback_method = Some(value);
            self
        }

        #[must_use]
        pub fn status_callback(mut self, value: &'a str) -> Self {
            self.fields.status_callback = Some(StringSetting::Set(value));
            self
        }

        #[must_use]
        pub fn clear_status_callback(mut self) -> Self {
            self.fields.status_callback = Some(StringSetting::Clear);
            self
        }

        #[must_use]
        pub fn sticky_sender(mut self, value: bool) -> Self {
            self.fields.sticky_sender = Some(value);
            self
        }

        #[must_use]
        pub fn mms_converter(mut self, value: bool) -> Self {
            self.fields.mms_converter = Some(value);
            self
        }

        #[must_use]
        pub fn smart_encoding(mut self, value: bool) -> Self {
            self.fields.smart_encoding = Some(value);
            self
        }

        #[must_use]
        pub fn scan_message_content(mut self, value: ScanMessageContent) -> Self {
            self.fields.scan_message_content = Some(value);
            self
        }

        #[must_use]
        pub fn fallback_to_long_code(mut self, value: bool) -> Self {
            self.fields.fallback_to_long_code = Some(value);
            self
        }

        #[must_use]
        pub fn area_code_geomatch(mut self, value: bool) -> Self {
            self.fields.area_code_geomatch = Some(value);
            self
        }

        #[must_use]
        pub fn synchronous_validation(mut self, value: bool) -> Self {
            self.fields.synchronous_validation = Some(value);
            self
        }

        #[must_use]
        pub fn validity_period(mut self, value: u32) -> Self {
            self.fields.validity_period = Some(value);
            self
        }

        #[must_use]
        pub fn usecase(mut self, value: ServiceUsecase) -> Self {
            self.fields.usecase = Some(value);
            self
        }

        #[must_use]
        pub fn use_inbound_webhook_on_number(mut self, value: bool) -> Self {
            self.fields.use_inbound_webhook_on_number = Some(value);
            self
        }
    };
}

/// Request body for `POST /Services`.
#[derive(Clone, Copy)]
pub struct CreateServiceRequest<'a> {
    fields: ServiceFields<'a>,
}

impl<'a> CreateServiceRequest<'a> {
    #[must_use]
    pub fn new(friendly_name: &'a str) -> Self {
        Self {
            fields: ServiceFields {
                friendly_name: Some(friendly_name),
                ..ServiceFields::default()
            },
        }
    }

    service_field_setters!();

    fn validate(self) -> Result<(), TwilioError> {
        if !has_non_empty(self.fields.friendly_name) {
            return Err(TwilioError::InvalidRequest(
                "FriendlyName must not be empty".to_owned(),
            ));
        }
        self.fields.validate_documented_limits()?;
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        self.fields.form_params()
    }

    fn sensitive_values(self, creds: &'a TwilioAuth) -> Vec<&'a str> {
        self.fields.sensitive_values(creds, None)
    }
}

/// Request body for `POST /Services/{Sid}`.
#[derive(Clone, Copy, Default)]
pub struct UpdateServiceRequest<'a> {
    fields: ServiceFields<'a>,
}

impl<'a> UpdateServiceRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    service_field_setters!();

    fn validate(self) -> Result<(), TwilioError> {
        if self.fields.is_empty() {
            return Err(TwilioError::InvalidRequest(
                "service update requires at least one field".to_owned(),
            ));
        }
        self.fields.validate_documented_limits()?;
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        self.fields.form_params()
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        self.fields.sensitive_values(creds, Some(service_sid))
    }
}

/// Query parameters for `GET /Services`.
#[derive(Clone, Copy, Default)]
pub struct ListServicesRequest<'a> {
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListServicesRequest<'a> {
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
        apply_v1_page_query(url, self.page_size, self.page, self.page_token);
    }

    fn sensitive_values(self, creds: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_secret()];
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Query parameters for common Service subresource list endpoints.
#[derive(Clone, Copy, Default)]
pub struct ListServiceSubresourcesRequest<'a> {
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListServiceSubresourcesRequest<'a> {
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
        apply_v1_page_query(url, self.page_size, self.page, self.page_token);
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_secret(), service_sid];
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Query parameters for `GET /Services/{ServiceSid}/DestinationAlphaSenders`.
#[derive(Clone, Copy, Default)]
pub struct ListDestinationAlphaSendersRequest<'a> {
    iso_country_code: Option<&'a str>,
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListDestinationAlphaSendersRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn iso_country_code(mut self, value: &'a str) -> Self {
        self.iso_country_code = Some(value);
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
        validate_page_size(self.page_size)
    }

    fn apply_query(self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        if let Some(value) = self.iso_country_code {
            query.append_pair("IsoCountryCode", value);
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

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid(), creds.auth_secret(), service_sid];
        push_sensitive(&mut values, self.iso_country_code);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

fn apply_v1_page_query(
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

#[derive(Clone, Copy)]
pub struct CreateServicePhoneNumberRequest<'a> {
    phone_number_sid: &'a str,
}

impl<'a> CreateServicePhoneNumberRequest<'a> {
    #[must_use]
    pub fn new(phone_number_sid: &'a str) -> Self {
        Self { phone_number_sid }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("PhoneNumberSid", self.phone_number_sid)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "PhoneNumberSid", Some(self.phone_number_sid));
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        vec![
            creds.account_sid(),
            creds.auth_secret(),
            service_sid,
            self.phone_number_sid,
        ]
    }
}

#[derive(Clone, Copy)]
pub struct CreateServiceShortCodeRequest<'a> {
    short_code_sid: &'a str,
}

impl<'a> CreateServiceShortCodeRequest<'a> {
    #[must_use]
    pub fn new(short_code_sid: &'a str) -> Self {
        Self { short_code_sid }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("ShortCodeSid", self.short_code_sid)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "ShortCodeSid", Some(self.short_code_sid));
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        vec![
            creds.account_sid(),
            creds.auth_secret(),
            service_sid,
            self.short_code_sid,
        ]
    }
}

#[derive(Clone, Copy)]
pub struct CreateAlphaSenderRequest<'a> {
    alpha_sender: &'a str,
}

impl<'a> CreateAlphaSenderRequest<'a> {
    #[must_use]
    pub fn new(alpha_sender: &'a str) -> Self {
        Self { alpha_sender }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("AlphaSender", self.alpha_sender)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "AlphaSender", Some(self.alpha_sender));
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        vec![
            creds.account_sid(),
            creds.auth_secret(),
            service_sid,
            self.alpha_sender,
        ]
    }
}

#[derive(Clone, Copy)]
pub struct CreateChannelSenderRequest<'a> {
    sid: &'a str,
}

impl<'a> CreateChannelSenderRequest<'a> {
    #[must_use]
    pub fn new(sid: &'a str) -> Self {
        Self { sid }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("Sid", self.sid)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "Sid", Some(self.sid));
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        vec![
            creds.account_sid(),
            creds.auth_secret(),
            service_sid,
            self.sid,
        ]
    }
}

#[derive(Clone, Copy)]
pub struct CreateDestinationAlphaSenderRequest<'a> {
    alpha_sender: &'a str,
    iso_country_code: Option<&'a str>,
}

impl<'a> CreateDestinationAlphaSenderRequest<'a> {
    #[must_use]
    pub fn new(alpha_sender: &'a str) -> Self {
        Self {
            alpha_sender,
            iso_country_code: None,
        }
    }

    #[must_use]
    pub fn iso_country_code(mut self, value: &'a str) -> Self {
        self.iso_country_code = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("AlphaSender", self.alpha_sender)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "AlphaSender", Some(self.alpha_sender));
        push_str(&mut params, "IsoCountryCode", self.iso_country_code);
        params
    }

    fn sensitive_values(self, creds: &'a TwilioAuth, service_sid: &'a str) -> Vec<&'a str> {
        let mut values = vec![
            creds.account_sid(),
            creds.auth_secret(),
            service_sid,
            self.alpha_sender,
        ];
        push_sensitive(&mut values, self.iso_country_code);
        values
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

/// A Twilio Messaging Service.
#[derive(Clone)]
pub struct TwilioService {
    pub account_sid: Option<String>,
    pub friendly_name: Option<String>,
    pub sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub sticky_sender: Option<bool>,
    pub mms_converter: Option<bool>,
    pub smart_encoding: Option<bool>,
    pub fallback_to_long_code: Option<bool>,
    pub scan_message_content: Option<String>,
    pub synchronous_validation: Option<bool>,
    pub area_code_geomatch: Option<bool>,
    pub validity_period: Option<i64>,
    pub inbound_request_url: Option<String>,
    pub inbound_method: Option<String>,
    pub fallback_url: Option<String>,
    pub fallback_method: Option<String>,
    pub status_callback: Option<String>,
    pub usecase: Option<String>,
    pub us_app_to_person_registered: Option<bool>,
    pub use_inbound_webhook_on_number: Option<bool>,
    pub links: Option<BTreeMap<String, String>>,
    pub url: Option<String>,
}

impl std::fmt::Debug for TwilioService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioService")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("friendly_name", &redacted_option(&self.friendly_name))
            .field("sid", &redacted_option(&self.sid))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("sticky_sender", &self.sticky_sender)
            .field("mms_converter", &self.mms_converter)
            .field("smart_encoding", &self.smart_encoding)
            .field("fallback_to_long_code", &self.fallback_to_long_code)
            .field("scan_message_content", &self.scan_message_content)
            .field("synchronous_validation", &self.synchronous_validation)
            .field("area_code_geomatch", &self.area_code_geomatch)
            .field("validity_period", &self.validity_period)
            .field(
                "inbound_request_url",
                &redacted_option(&self.inbound_request_url),
            )
            .field("inbound_method", &self.inbound_method)
            .field("fallback_url", &redacted_option(&self.fallback_url))
            .field("fallback_method", &self.fallback_method)
            .field("status_callback", &redacted_option(&self.status_callback))
            .field("usecase", &self.usecase)
            .field(
                "us_app_to_person_registered",
                &self.us_app_to_person_registered,
            )
            .field(
                "use_inbound_webhook_on_number",
                &self.use_inbound_webhook_on_number,
            )
            .field(
                "links",
                &self.links.as_ref().map(|_| crate::common::REDACTED),
            )
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

/// One page of Messaging Services.
#[derive(Clone, Debug)]
pub struct TwilioServicePage {
    pub services: Vec<TwilioService>,
    pub meta: V1PageMeta,
}

#[derive(Clone)]
pub struct TwilioServicePhoneNumber {
    pub account_sid: Option<String>,
    pub service_sid: Option<String>,
    pub sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub phone_number: Option<String>,
    pub country_code: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub url: Option<String>,
}

impl std::fmt::Debug for TwilioServicePhoneNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioServicePhoneNumber")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("service_sid", &redacted_option(&self.service_sid))
            .field("sid", &redacted_option(&self.sid))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("phone_number", &redacted_option(&self.phone_number))
            .field("country_code", &self.country_code)
            .field("capabilities", &self.capabilities)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioServicePhoneNumberPage {
    pub phone_numbers: Vec<TwilioServicePhoneNumber>,
    pub meta: V1PageMeta,
}

#[derive(Clone)]
pub struct TwilioServiceShortCode {
    pub account_sid: Option<String>,
    pub service_sid: Option<String>,
    pub sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub short_code: Option<String>,
    pub country_code: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub url: Option<String>,
}

impl std::fmt::Debug for TwilioServiceShortCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioServiceShortCode")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("service_sid", &redacted_option(&self.service_sid))
            .field("sid", &redacted_option(&self.sid))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("short_code", &redacted_option(&self.short_code))
            .field("country_code", &self.country_code)
            .field("capabilities", &self.capabilities)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioServiceShortCodePage {
    pub short_codes: Vec<TwilioServiceShortCode>,
    pub meta: V1PageMeta,
}

#[derive(Clone)]
pub struct TwilioAlphaSender {
    pub account_sid: Option<String>,
    pub service_sid: Option<String>,
    pub sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub alpha_sender: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub url: Option<String>,
}

impl std::fmt::Debug for TwilioAlphaSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioAlphaSender")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("service_sid", &redacted_option(&self.service_sid))
            .field("sid", &redacted_option(&self.sid))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("alpha_sender", &redacted_option(&self.alpha_sender))
            .field("capabilities", &self.capabilities)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioAlphaSenderPage {
    pub alpha_senders: Vec<TwilioAlphaSender>,
    pub meta: V1PageMeta,
}

#[derive(Clone)]
pub struct TwilioChannelSender {
    pub account_sid: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub sid: Option<String>,
    pub sender: Option<String>,
    pub sender_type: Option<String>,
    pub country_code: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub url: Option<String>,
}

impl std::fmt::Debug for TwilioChannelSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioChannelSender")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field(
                "messaging_service_sid",
                &redacted_option(&self.messaging_service_sid),
            )
            .field("sid", &redacted_option(&self.sid))
            .field("sender", &redacted_option(&self.sender))
            .field("sender_type", &self.sender_type)
            .field("country_code", &self.country_code)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioChannelSenderPage {
    pub senders: Vec<TwilioChannelSender>,
    pub meta: V1PageMeta,
}

#[derive(Clone)]
pub struct TwilioDestinationAlphaSender {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub service_sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub alpha_sender: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub url: Option<String>,
    pub iso_country_code: Option<String>,
}

impl std::fmt::Debug for TwilioDestinationAlphaSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioDestinationAlphaSender")
            .field("sid", &redacted_option(&self.sid))
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("service_sid", &redacted_option(&self.service_sid))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("alpha_sender", &redacted_option(&self.alpha_sender))
            .field("capabilities", &self.capabilities)
            .field("url", &redacted_option(&self.url))
            .field("iso_country_code", &self.iso_country_code)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioDestinationAlphaSenderPage {
    pub alpha_senders: Vec<TwilioDestinationAlphaSender>,
    pub meta: V1PageMeta,
}

#[derive(Deserialize)]
struct WireService {
    account_sid: Option<String>,
    friendly_name: Option<String>,
    sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    sticky_sender: Option<bool>,
    mms_converter: Option<bool>,
    smart_encoding: Option<bool>,
    fallback_to_long_code: Option<bool>,
    scan_message_content: Option<String>,
    synchronous_validation: Option<bool>,
    area_code_geomatch: Option<bool>,
    validity_period: Option<i64>,
    inbound_request_url: Option<String>,
    inbound_method: Option<String>,
    fallback_url: Option<String>,
    fallback_method: Option<String>,
    status_callback: Option<String>,
    usecase: Option<String>,
    us_app_to_person_registered: Option<bool>,
    use_inbound_webhook_on_number: Option<bool>,
    links: Option<BTreeMap<String, String>>,
    url: Option<String>,
}

impl WireService {
    fn into_service(self) -> TwilioService {
        TwilioService {
            account_sid: self.account_sid,
            friendly_name: self.friendly_name,
            sid: self.sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            sticky_sender: self.sticky_sender,
            mms_converter: self.mms_converter,
            smart_encoding: self.smart_encoding,
            fallback_to_long_code: self.fallback_to_long_code,
            scan_message_content: self.scan_message_content,
            synchronous_validation: self.synchronous_validation,
            area_code_geomatch: self.area_code_geomatch,
            validity_period: self.validity_period,
            inbound_request_url: self.inbound_request_url,
            inbound_method: self.inbound_method,
            fallback_url: self.fallback_url,
            fallback_method: self.fallback_method,
            status_callback: self.status_callback,
            usecase: self.usecase,
            us_app_to_person_registered: self.us_app_to_person_registered,
            use_inbound_webhook_on_number: self.use_inbound_webhook_on_number,
            links: self.links,
            url: self.url,
        }
    }
}

#[derive(Deserialize)]
struct WireServicePage {
    #[serde(default)]
    services: Vec<WireService>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireServicePage {
    fn into_page(self) -> TwilioServicePage {
        TwilioServicePage {
            services: self
                .services
                .into_iter()
                .map(WireService::into_service)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireServicePhoneNumber {
    account_sid: Option<String>,
    service_sid: Option<String>,
    sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    phone_number: Option<String>,
    country_code: Option<String>,
    capabilities: Option<Vec<String>>,
    url: Option<String>,
}

impl WireServicePhoneNumber {
    fn into_phone_number(self) -> TwilioServicePhoneNumber {
        TwilioServicePhoneNumber {
            account_sid: self.account_sid,
            service_sid: self.service_sid,
            sid: self.sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            phone_number: self.phone_number,
            country_code: self.country_code,
            capabilities: self.capabilities,
            url: self.url,
        }
    }
}

#[derive(Deserialize)]
struct WirePhoneNumberPage {
    #[serde(default)]
    phone_numbers: Vec<WireServicePhoneNumber>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WirePhoneNumberPage {
    fn into_page(self) -> TwilioServicePhoneNumberPage {
        TwilioServicePhoneNumberPage {
            phone_numbers: self
                .phone_numbers
                .into_iter()
                .map(WireServicePhoneNumber::into_phone_number)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireServiceShortCode {
    account_sid: Option<String>,
    service_sid: Option<String>,
    sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    short_code: Option<String>,
    country_code: Option<String>,
    capabilities: Option<Vec<String>>,
    url: Option<String>,
}

impl WireServiceShortCode {
    fn into_short_code(self) -> TwilioServiceShortCode {
        TwilioServiceShortCode {
            account_sid: self.account_sid,
            service_sid: self.service_sid,
            sid: self.sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            short_code: self.short_code,
            country_code: self.country_code,
            capabilities: self.capabilities,
            url: self.url,
        }
    }
}

#[derive(Deserialize)]
struct WireShortCodePage {
    #[serde(default)]
    short_codes: Vec<WireServiceShortCode>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireShortCodePage {
    fn into_page(self) -> TwilioServiceShortCodePage {
        TwilioServiceShortCodePage {
            short_codes: self
                .short_codes
                .into_iter()
                .map(WireServiceShortCode::into_short_code)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireAlphaSender {
    account_sid: Option<String>,
    service_sid: Option<String>,
    sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    alpha_sender: Option<String>,
    capabilities: Option<Vec<String>>,
    url: Option<String>,
}

impl WireAlphaSender {
    fn into_alpha_sender(self) -> TwilioAlphaSender {
        TwilioAlphaSender {
            account_sid: self.account_sid,
            service_sid: self.service_sid,
            sid: self.sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            alpha_sender: self.alpha_sender,
            capabilities: self.capabilities,
            url: self.url,
        }
    }
}

#[derive(Deserialize)]
struct WireAlphaSenderPage {
    #[serde(default)]
    alpha_senders: Vec<WireAlphaSender>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireAlphaSenderPage {
    fn into_page(self) -> TwilioAlphaSenderPage {
        TwilioAlphaSenderPage {
            alpha_senders: self
                .alpha_senders
                .into_iter()
                .map(WireAlphaSender::into_alpha_sender)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireChannelSender {
    account_sid: Option<String>,
    messaging_service_sid: Option<String>,
    sid: Option<String>,
    sender: Option<String>,
    sender_type: Option<String>,
    country_code: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    url: Option<String>,
}

impl WireChannelSender {
    fn into_channel_sender(self) -> TwilioChannelSender {
        TwilioChannelSender {
            account_sid: self.account_sid,
            messaging_service_sid: self.messaging_service_sid,
            sid: self.sid,
            sender: self.sender,
            sender_type: self.sender_type,
            country_code: self.country_code,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            url: self.url,
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
    fn into_page(self) -> TwilioChannelSenderPage {
        TwilioChannelSenderPage {
            senders: self
                .senders
                .into_iter()
                .map(WireChannelSender::into_channel_sender)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Deserialize)]
struct WireDestinationAlphaSender {
    sid: Option<String>,
    account_sid: Option<String>,
    service_sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    alpha_sender: Option<String>,
    capabilities: Option<Vec<String>>,
    url: Option<String>,
    iso_country_code: Option<String>,
}

impl WireDestinationAlphaSender {
    fn into_destination_alpha_sender(self) -> TwilioDestinationAlphaSender {
        TwilioDestinationAlphaSender {
            sid: self.sid,
            account_sid: self.account_sid,
            service_sid: self.service_sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            alpha_sender: self.alpha_sender,
            capabilities: self.capabilities,
            url: self.url,
            iso_country_code: self.iso_country_code,
        }
    }
}

#[derive(Deserialize)]
struct WireDestinationAlphaSenderPage {
    #[serde(default)]
    alpha_senders: Vec<WireDestinationAlphaSender>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireDestinationAlphaSenderPage {
    fn into_page(self) -> TwilioDestinationAlphaSenderPage {
        TwilioDestinationAlphaSenderPage {
            alpha_senders: self
                .alpha_senders
                .into_iter()
                .map(WireDestinationAlphaSender::into_destination_alpha_sender)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ServicesResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> ServicesResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Services`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateServiceRequest<'a>,
    ) -> Result<TwilioService, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let form_params = request.form_params();
            let spec = RequestSpec::new(ApiFamily::Messaging, Method::POST, ["Services"])
                .operation("services.create")
                .form_params(form_params);
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(service.into_service())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "services.create",
            "POST",
        ))
        .await
    }

    /// `GET /Services`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListServicesRequest<'a>,
    ) -> Result<TwilioServicePage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.account.client.messaging_endpoint(&["Services"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "services.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "services.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent Services page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URL is invalid, leaves the configured
    /// Messaging API base, changes stable filters, or the HTTP request/response
    /// fails.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioServicePage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_secret(),
                next_page_url,
            ];
            let resource = V1PageResource::Services;
            let url = self.account.client.v1_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "services.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "services.list_page_url",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioServicePage, TwilioError> {
        let parsed: WireServicePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        validate_v1_meta_key(&page.meta, V1PageResource::Services)?;
        validate_next_v1_url(
            self.account,
            page.meta.next_page_url.as_deref(),
            V1PageResource::Services,
            current_url,
        )?;
        Ok(page)
    }

    /// Lazily list all Messaging Services using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> TwilioPaginator<'a, TwilioServicePage, TwilioService> {
        self.list_all_with(ListServicesRequest::new())
    }

    /// Lazily list all Messaging Services using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListServicesRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioServicePage, TwilioService> {
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
                }) as PageFuture<'a, TwilioServicePage>
            },
            split_service_page,
        )
    }
}

fn split_service_page(page: TwilioServicePage) -> (Vec<TwilioService>, Option<String>) {
    (page.services, page.meta.next_page_url)
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ServiceResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> ServiceResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    pub(crate) fn account(self) -> TwilioAccount<'a> {
        self.account
    }

    pub(crate) fn sid(self) -> &'a str {
        self.sid
    }

    /// `GET /Services/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioService, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.service_spec(Method::GET, "service.fetch")?;
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(service.into_service())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "service.fetch",
            "GET",
        ))
        .await
    }

    /// `POST /Services/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update(
        self,
        request: UpdateServiceRequest<'a>,
    ) -> Result<TwilioService, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let form_params = request.form_params();
            let spec = self
                .service_spec(Method::POST, "service.update")?
                .form_params(form_params);
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(service.into_service())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "service.update",
            "POST",
        ))
        .await
    }

    /// `DELETE /Services/{Sid}`.
    ///
    /// Deleting a Messaging Service can disrupt A2P 10DLC messaging for that
    /// service. Twilio returns phone numbers and short codes to the account,
    /// but A2P campaigns can be deleted and require re-registration.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.service_spec(Method::DELETE, "service.delete")?;
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "service.delete",
            "DELETE",
        ))
        .await
    }

    /// `PhoneNumbers` subresource.
    #[must_use]
    pub fn phone_numbers(self) -> ServicePhoneNumbersResource<'a> {
        ServicePhoneNumbersResource { service: self }
    }

    /// `ShortCodes` subresource.
    #[must_use]
    pub fn short_codes(self) -> ServiceShortCodesResource<'a> {
        ServiceShortCodesResource { service: self }
    }

    /// `AlphaSenders` subresource.
    #[must_use]
    pub fn alpha_senders(self) -> ServiceAlphaSendersResource<'a> {
        ServiceAlphaSendersResource { service: self }
    }

    /// `ChannelSenders` subresource.
    #[must_use]
    pub fn channel_senders(self) -> ServiceChannelSendersResource<'a> {
        ServiceChannelSendersResource { service: self }
    }

    /// `DestinationAlphaSenders` subresource.
    #[must_use]
    pub fn destination_alpha_senders(self) -> ServiceDestinationAlphaSendersResource<'a> {
        ServiceDestinationAlphaSendersResource { service: self }
    }

    /// A2P 10DLC `Usa2p` Compliance resources for this Messaging Service.
    #[must_use]
    pub fn usa2p(self) -> ServiceUsa2pResource<'a> {
        ServiceUsa2pResource::new(self)
    }

    /// A2P 10DLC use case lookup for this Messaging Service.
    #[must_use]
    pub fn usa2p_usecases(self) -> ServiceUsa2pUsecasesResource<'a> {
        ServiceUsa2pUsecasesResource::new(self)
    }

    fn service_url(self) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid])
    }

    fn service_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service_url()?,
            operation,
        ))
    }

    fn subresource_collection_url(self, resource: &'static str) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid, resource])
    }

    fn subresource_instance_url(
        self,
        resource: &'static str,
        sid: &'a str,
    ) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid, resource, sid])
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![
            self.account.creds.account_sid(),
            self.account.creds.auth_secret(),
            self.sid,
        ]
    }
}

#[cfg(feature = "async")]
fn validate_next_v1_url(
    account: TwilioAccount<'_>,
    next_page_url: Option<&str>,
    resource: V1PageResource<'_>,
    current_url: Option<&Url>,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = account.client.v1_page_url(next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_v1_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}

macro_rules! impl_sender_resource {
    (
        $resource:ident,
        $create_req:ty,
        $item:ty,
        $page:ty,
        $wire_item:ty,
        $wire_page:ty,
        $segment:literal,
        $kind:ident,
        $create_map:ident,
        $page_map:ident,
        $items_field:ident
    ) => {
        #[derive(Clone, Copy)]
        #[cfg(feature = "async")]
        pub struct $resource<'a> {
            service: ServiceResource<'a>,
        }

        #[cfg(feature = "async")]
        impl<'a> $resource<'a> {
            /// Create a sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, or malformed JSON responses.
            pub async fn create(self, request: $create_req) -> Result<$item, TwilioError> {
                async move {
                    request.validate()?;
                    let sensitive_values =
                        request.sensitive_values(self.service.account.creds, self.service.sid);
                    let form_params = request.form_params();
                    let spec = self
                        .subresource_spec(
                            Method::POST,
                            concat!("service.", stringify!($items_field), ".create"),
                        )?
                        .form_params(form_params);
                    let parsed: $wire_item = self
                        .service
                        .account
                        .send_spec_json(spec, &sensitive_values)
                        .await?;
                    Ok(parsed.$create_map())
                }
                .instrument(request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".create"),
                    "POST",
                ))
                .await
            }

            /// Fetch one sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for transport failures, non-2xx API
            /// responses, or malformed JSON responses.
            pub async fn fetch(self, sid: &'a str) -> Result<$item, TwilioError> {
                async move {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        sid,
                    ];
                    let spec = self.subresource_instance_spec(
                        sid,
                        Method::GET,
                        concat!("service.", stringify!($items_field), ".fetch"),
                    )?;
                    let parsed: $wire_item = self
                        .service
                        .account
                        .send_spec_json(spec, &sensitive_values)
                        .await?;
                    Ok(parsed.$create_map())
                }
                .instrument(request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".fetch"),
                    "GET",
                ))
                .await
            }

            /// List sender associations under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, malformed JSON responses, or invalid
            /// pagination metadata.
            pub async fn list(
                self,
                request: ListServiceSubresourcesRequest<'a>,
            ) -> Result<$page, TwilioError> {
                async move {
                    request.validate()?;
                    let sensitive_values =
                        request.sensitive_values(self.service.account.creds, self.service.sid);
                    let mut url = self.service.subresource_collection_url($segment)?;
                    request.apply_query(&mut url);
                    let spec = RequestSpec::from_url(
                        ApiFamily::Messaging,
                        Method::GET,
                        url.clone(),
                        concat!("service.", stringify!($items_field), ".list"),
                    );
                    let raw = self
                        .service
                        .account
                        .send_spec_raw(spec, &sensitive_values)
                        .await?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                }
                .instrument(request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".list"),
                    "GET",
                ))
                .await
            }

            /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] when the URL is invalid, leaves the
            /// configured Messaging API base, changes stable filters, or the
            /// HTTP request/response fails.
            pub async fn list_page_url(self, next_page_url: &str) -> Result<$page, TwilioError> {
                async move {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        next_page_url,
                    ];
                    let resource = V1PageResource::$kind {
                        service_sid: self.service.sid,
                    };
                    let url = self
                        .service
                        .account
                        .client
                        .v1_page_url(next_page_url, resource)?;
                    let spec = RequestSpec::from_url(
                        ApiFamily::Messaging,
                        Method::GET,
                        url.clone(),
                        concat!("service.", stringify!($items_field), ".list_page_url"),
                    );
                    let raw = self
                        .service
                        .account
                        .send_spec_raw(spec, &sensitive_values)
                        .await?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                }
                .instrument(request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".list_page_url"),
                    "GET",
                ))
                .await
            }

            /// Delete one sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for transport failures or non-2xx API
            /// responses.
            pub async fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
                async move {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        sid,
                    ];
                    let spec = self.subresource_instance_spec(
                        sid,
                        Method::DELETE,
                        concat!("service.", stringify!($items_field), ".delete"),
                    )?;
                    self.service
                        .account
                        .send_spec_empty(spec, &sensitive_values)
                        .await
                }
                .instrument(request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".delete"),
                    "DELETE",
                ))
                .await
            }

            fn read_page(
                self,
                raw: &crate::RawResponse,
                sensitive_values: &[&str],
                current_url: Option<&Url>,
            ) -> Result<$page, TwilioError> {
                let parsed: $wire_page = decode_json_response(raw, sensitive_values)?;
                let page = parsed.$page_map();
                let resource = V1PageResource::$kind {
                    service_sid: self.service.sid,
                };
                validate_v1_meta_key(&page.meta, resource)?;
                validate_next_v1_url(
                    self.service.account,
                    page.meta.next_page_url.as_deref(),
                    resource,
                    current_url,
                )?;
                Ok(page)
            }

            fn subresource_spec(
                self,
                method: Method,
                operation: &'static str,
            ) -> Result<RequestSpec, TwilioError> {
                Ok(RequestSpec::from_url(
                    ApiFamily::Messaging,
                    method,
                    self.service.subresource_collection_url($segment)?,
                    operation,
                ))
            }

            fn subresource_instance_spec(
                self,
                sid: &'a str,
                method: Method,
                operation: &'static str,
            ) -> Result<RequestSpec, TwilioError> {
                Ok(RequestSpec::from_url(
                    ApiFamily::Messaging,
                    method,
                    self.service.subresource_instance_url($segment, sid)?,
                    operation,
                ))
            }

            /// Lazily list all sender associations using a default page size of 50.
            #[must_use]
            pub fn list_all(self) -> TwilioPaginator<'a, $page, $item> {
                self.list_all_with(ListServiceSubresourcesRequest::new())
            }

            /// Lazily list all sender associations using supplied first-page filters.
            #[must_use]
            pub fn list_all_with(
                self,
                mut request: ListServiceSubresourcesRequest<'a>,
            ) -> TwilioPaginator<'a, $page, $item> {
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
                        }) as PageFuture<'a, $page>
                    },
                    |page| (page.$items_field, page.meta.next_page_url),
                )
            }
        }
    };
}

impl_sender_resource!(
    ServicePhoneNumbersResource,
    CreateServicePhoneNumberRequest<'a>,
    TwilioServicePhoneNumber,
    TwilioServicePhoneNumberPage,
    WireServicePhoneNumber,
    WirePhoneNumberPage,
    "PhoneNumbers",
    PhoneNumbers,
    into_phone_number,
    into_page,
    phone_numbers
);

impl_sender_resource!(
    ServiceShortCodesResource,
    CreateServiceShortCodeRequest<'a>,
    TwilioServiceShortCode,
    TwilioServiceShortCodePage,
    WireServiceShortCode,
    WireShortCodePage,
    "ShortCodes",
    ShortCodes,
    into_short_code,
    into_page,
    short_codes
);

impl_sender_resource!(
    ServiceAlphaSendersResource,
    CreateAlphaSenderRequest<'a>,
    TwilioAlphaSender,
    TwilioAlphaSenderPage,
    WireAlphaSender,
    WireAlphaSenderPage,
    "AlphaSenders",
    AlphaSenders,
    into_alpha_sender,
    into_page,
    alpha_senders
);

impl_sender_resource!(
    ServiceChannelSendersResource,
    CreateChannelSenderRequest<'a>,
    TwilioChannelSender,
    TwilioChannelSenderPage,
    WireChannelSender,
    WireChannelSenderPage,
    "ChannelSenders",
    ChannelSenders,
    into_channel_sender,
    into_page,
    senders
);

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ServiceDestinationAlphaSendersResource<'a> {
    service: ServiceResource<'a>,
}

#[cfg(feature = "async")]
impl<'a> ServiceDestinationAlphaSendersResource<'a> {
    /// Create a `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateDestinationAlphaSenderRequest<'a>,
    ) -> Result<TwilioDestinationAlphaSender, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.service.account.creds, self.service.sid);
            let form_params = request.form_params();
            let spec = self
                .destination_spec(Method::POST, "service.destination_alpha_senders.create")?
                .form_params(form_params);
            let parsed: WireDestinationAlphaSender = self
                .service
                .account
                .send_spec_json(spec, &sensitive_values)
                .await?;
            Ok(parsed.into_destination_alpha_sender())
        }
        .instrument(request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.create",
            "POST",
        ))
        .await
    }

    /// Fetch one `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self, sid: &'a str) -> Result<TwilioDestinationAlphaSender, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                sid,
            ];
            let spec = self.destination_instance_spec(
                sid,
                Method::GET,
                "service.destination_alpha_senders.fetch",
            )?;
            let parsed: WireDestinationAlphaSender = self
                .service
                .account
                .send_spec_json(spec, &sensitive_values)
                .await?;
            Ok(parsed.into_destination_alpha_sender())
        }
        .instrument(request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.fetch",
            "GET",
        ))
        .await
    }

    /// List `DestinationAlphaSenders` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListDestinationAlphaSendersRequest<'a>,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.service.account.creds, self.service.sid);
            let mut url = self
                .service
                .subresource_collection_url("DestinationAlphaSenders")?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "service.destination_alpha_senders.list",
            );
            let raw = self
                .service
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URL is invalid, leaves the configured
    /// Messaging API base, changes stable filters, or the HTTP request/response
    /// fails.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                next_page_url,
            ];
            let resource = V1PageResource::DestinationAlphaSenders {
                service_sid: self.service.sid,
            };
            let url = self
                .service
                .account
                .client
                .v1_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "service.destination_alpha_senders.list_page_url",
            );
            let raw = self
                .service
                .account
                .send_spec_raw(spec, &sensitive_values)
                .await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.list_page_url",
            "GET",
        ))
        .await
    }

    /// Delete one `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                sid,
            ];
            let spec = self.destination_instance_spec(
                sid,
                Method::DELETE,
                "service.destination_alpha_senders.delete",
            )?;
            self.service
                .account
                .send_spec_empty(spec, &sensitive_values)
                .await
        }
        .instrument(request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.delete",
            "DELETE",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        let parsed: WireDestinationAlphaSenderPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::DestinationAlphaSenders {
            service_sid: self.service.sid,
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url(
            self.service.account,
            page.meta.next_page_url.as_deref(),
            resource,
            current_url,
        )?;
        Ok(page)
    }

    fn destination_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service
                .subresource_collection_url("DestinationAlphaSenders")?,
            operation,
        ))
    }

    fn destination_instance_spec(
        self,
        sid: &'a str,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service
                .subresource_instance_url("DestinationAlphaSenders", sid)?,
            operation,
        ))
    }

    /// Lazily list all destination alpha senders using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, TwilioDestinationAlphaSenderPage, TwilioDestinationAlphaSender> {
        self.list_all_with(ListDestinationAlphaSendersRequest::new())
    }

    /// Lazily list all destination alpha senders using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListDestinationAlphaSendersRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioDestinationAlphaSenderPage, TwilioDestinationAlphaSender> {
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
                }) as PageFuture<'a, TwilioDestinationAlphaSenderPage>
            },
            |page| (page.alpha_senders, page.meta.next_page_url),
        )
    }
}

#[cfg(feature = "sync")]
fn validate_next_v1_url_blocking(
    account: BlockingTwilioAccount<'_>,
    next_page_url: Option<&str>,
    resource: V1PageResource<'_>,
    current_url: Option<&Url>,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = account.client.v1_page_url(next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_v1_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingServicesResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingServicesResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Services`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn create(self, request: CreateServiceRequest<'a>) -> Result<TwilioService, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "services.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Messaging, Method::POST, ["Services"])
                .operation("services.create")
                .form_params(request.form_params());
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(service.into_service())
        })
    }

    /// `GET /Services`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(self, request: ListServicesRequest<'a>) -> Result<TwilioServicePage, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "services.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.account.client.messaging_endpoint(&["Services"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "services.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent Services page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URL is invalid, leaves the configured
    /// Messaging API base, changes stable filters, or the HTTP request/response
    /// fails.
    pub fn list_page_url(self, next_page_url: &str) -> Result<TwilioServicePage, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "services.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.account.creds.account_sid(),
                self.account.creds.auth_secret(),
                next_page_url,
            ];
            let resource = V1PageResource::Services;
            let url = self.account.client.v1_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "services.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioServicePage, TwilioError> {
        let parsed: WireServicePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        validate_v1_meta_key(&page.meta, V1PageResource::Services)?;
        validate_next_v1_url_blocking(
            self.account,
            page.meta.next_page_url.as_deref(),
            V1PageResource::Services,
            current_url,
        )?;
        Ok(page)
    }

    /// Lazily list all Messaging Services using a default page size of 50.
    #[must_use]
    pub fn list_all(self) -> BlockingTwilioPaginator<'a, TwilioServicePage, TwilioService> {
        self.list_all_with(ListServicesRequest::new())
    }

    /// Lazily list all Messaging Services using the supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListServicesRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioServicePage, TwilioService> {
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
            split_service_page,
        )
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingServiceResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingServiceResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    pub(crate) fn account(self) -> BlockingTwilioAccount<'a> {
        self.account
    }

    pub(crate) fn sid(self) -> &'a str {
        self.sid
    }

    /// `GET /Services/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioService, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "service.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values();
            let spec = self.service_spec(Method::GET, "service.fetch")?;
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(service.into_service())
        })
    }

    /// `POST /Services/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn update(self, request: UpdateServiceRequest<'a>) -> Result<TwilioService, TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "service.update",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let spec = self
                .service_spec(Method::POST, "service.update")?
                .form_params(request.form_params());
            let service: WireService = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(service.into_service())
        })
    }

    /// `DELETE /Services/{Sid}`.
    ///
    /// Deleting a Messaging Service can disrupt A2P 10DLC messaging for that
    /// service. Twilio returns phone numbers and short codes to the account,
    /// but A2P campaigns can be deleted and require re-registration.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub fn delete(self) -> Result<(), TwilioError> {
        request_span(
            &self.account.client.config.messaging,
            "service.delete",
            "DELETE",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values();
            let spec = self.service_spec(Method::DELETE, "service.delete")?;
            self.account.send_spec_empty(spec, &sensitive_values)
        })
    }

    /// `PhoneNumbers` subresource.
    #[must_use]
    pub fn phone_numbers(self) -> BlockingServicePhoneNumbersResource<'a> {
        BlockingServicePhoneNumbersResource { service: self }
    }

    /// `ShortCodes` subresource.
    #[must_use]
    pub fn short_codes(self) -> BlockingServiceShortCodesResource<'a> {
        BlockingServiceShortCodesResource { service: self }
    }

    /// `AlphaSenders` subresource.
    #[must_use]
    pub fn alpha_senders(self) -> BlockingServiceAlphaSendersResource<'a> {
        BlockingServiceAlphaSendersResource { service: self }
    }

    /// `ChannelSenders` subresource.
    #[must_use]
    pub fn channel_senders(self) -> BlockingServiceChannelSendersResource<'a> {
        BlockingServiceChannelSendersResource { service: self }
    }

    /// `DestinationAlphaSenders` subresource.
    #[must_use]
    pub fn destination_alpha_senders(self) -> BlockingServiceDestinationAlphaSendersResource<'a> {
        BlockingServiceDestinationAlphaSendersResource { service: self }
    }

    /// A2P 10DLC `Usa2p` Compliance resources for this Messaging Service.
    #[must_use]
    pub fn usa2p(self) -> BlockingServiceUsa2pResource<'a> {
        BlockingServiceUsa2pResource::new(self)
    }

    /// A2P 10DLC use case lookup for this Messaging Service.
    #[must_use]
    pub fn usa2p_usecases(self) -> BlockingServiceUsa2pUsecasesResource<'a> {
        BlockingServiceUsa2pUsecasesResource::new(self)
    }

    fn service_url(self) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid])
    }

    fn service_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service_url()?,
            operation,
        ))
    }

    fn subresource_collection_url(self, resource: &'static str) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid, resource])
    }

    fn subresource_instance_url(
        self,
        resource: &'static str,
        sid: &'a str,
    ) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Services", self.sid, resource, sid])
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![
            self.account.creds.account_sid(),
            self.account.creds.auth_secret(),
            self.sid,
        ]
    }
}

macro_rules! impl_blocking_sender_resource {
    (
        $resource:ident,
        $create_req:ty,
        $item:ty,
        $page:ty,
        $wire_item:ty,
        $wire_page:ty,
        $segment:literal,
        $kind:ident,
        $create_map:ident,
        $page_map:ident,
        $items_field:ident
    ) => {
        #[derive(Clone, Copy)]
        #[cfg(feature = "sync")]
        pub struct $resource<'a> {
            service: BlockingServiceResource<'a>,
        }

        #[cfg(feature = "sync")]
        impl<'a> $resource<'a> {
            /// Create a sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, or malformed JSON responses.
            pub fn create(self, request: $create_req) -> Result<$item, TwilioError> {
                request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".create"),
                    "POST",
                )
                .in_scope(|| {
                    request.validate()?;
                    let sensitive_values =
                        request.sensitive_values(self.service.account.creds, self.service.sid);
                    let spec = self
                        .subresource_spec(
                            Method::POST,
                            concat!("service.", stringify!($items_field), ".create"),
                        )?
                        .form_params(request.form_params());
                    let parsed: $wire_item = self
                        .service
                        .account
                        .send_spec_json(spec, &sensitive_values)?;
                    Ok(parsed.$create_map())
                })
            }

            /// Fetch one sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for transport failures, non-2xx API
            /// responses, or malformed JSON responses.
            pub fn fetch(self, sid: &'a str) -> Result<$item, TwilioError> {
                request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".fetch"),
                    "GET",
                )
                .in_scope(|| {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        sid,
                    ];
                    let spec = self.subresource_instance_spec(
                        sid,
                        Method::GET,
                        concat!("service.", stringify!($items_field), ".fetch"),
                    )?;
                    let parsed: $wire_item = self
                        .service
                        .account
                        .send_spec_json(spec, &sensitive_values)?;
                    Ok(parsed.$create_map())
                })
            }

            /// List sender associations under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, malformed JSON responses, or invalid
            /// pagination metadata.
            pub fn list(
                self,
                request: ListServiceSubresourcesRequest<'a>,
            ) -> Result<$page, TwilioError> {
                request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".list"),
                    "GET",
                )
                .in_scope(|| {
                    request.validate()?;
                    let sensitive_values =
                        request.sensitive_values(self.service.account.creds, self.service.sid);
                    let mut url = self.service.subresource_collection_url($segment)?;
                    request.apply_query(&mut url);
                    let spec = RequestSpec::from_url(
                        ApiFamily::Messaging,
                        Method::GET,
                        url.clone(),
                        concat!("service.", stringify!($items_field), ".list"),
                    );
                    let raw = self
                        .service
                        .account
                        .send_spec_raw(spec, &sensitive_values)?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                })
            }

            /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] when the URL is invalid, leaves the
            /// configured Messaging API base, changes stable filters, or the
            /// HTTP request/response fails.
            pub fn list_page_url(self, next_page_url: &str) -> Result<$page, TwilioError> {
                request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".list_page_url"),
                    "GET",
                )
                .in_scope(|| {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        next_page_url,
                    ];
                    let resource = V1PageResource::$kind {
                        service_sid: self.service.sid,
                    };
                    let url = self
                        .service
                        .account
                        .client
                        .v1_page_url(next_page_url, resource)?;
                    let spec = RequestSpec::from_url(
                        ApiFamily::Messaging,
                        Method::GET,
                        url.clone(),
                        concat!("service.", stringify!($items_field), ".list_page_url"),
                    );
                    let raw = self
                        .service
                        .account
                        .send_spec_raw(spec, &sensitive_values)?;
                    self.read_page(&raw.output, &sensitive_values, Some(&url))
                })
            }

            /// Delete one sender association under this Messaging Service.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for transport failures or non-2xx API
            /// responses.
            pub fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
                request_span(
                    &self.service.account.client.config.messaging,
                    concat!("service.", stringify!($items_field), ".delete"),
                    "DELETE",
                )
                .in_scope(|| {
                    let sensitive_values = vec![
                        self.service.account.creds.account_sid(),
                        self.service.account.creds.auth_secret(),
                        self.service.sid,
                        sid,
                    ];
                    let spec = self.subresource_instance_spec(
                        sid,
                        Method::DELETE,
                        concat!("service.", stringify!($items_field), ".delete"),
                    )?;
                    self.service
                        .account
                        .send_spec_empty(spec, &sensitive_values)
                })
            }

            fn read_page(
                self,
                raw: &crate::RawResponse,
                sensitive_values: &[&str],
                current_url: Option<&Url>,
            ) -> Result<$page, TwilioError> {
                let parsed: $wire_page = decode_json_response(raw, sensitive_values)?;
                let page = parsed.$page_map();
                let resource = V1PageResource::$kind {
                    service_sid: self.service.sid,
                };
                validate_v1_meta_key(&page.meta, resource)?;
                validate_next_v1_url_blocking(
                    self.service.account,
                    page.meta.next_page_url.as_deref(),
                    resource,
                    current_url,
                )?;
                Ok(page)
            }

            fn subresource_spec(
                self,
                method: Method,
                operation: &'static str,
            ) -> Result<RequestSpec, TwilioError> {
                Ok(RequestSpec::from_url(
                    ApiFamily::Messaging,
                    method,
                    self.service.subresource_collection_url($segment)?,
                    operation,
                ))
            }

            fn subresource_instance_spec(
                self,
                sid: &'a str,
                method: Method,
                operation: &'static str,
            ) -> Result<RequestSpec, TwilioError> {
                Ok(RequestSpec::from_url(
                    ApiFamily::Messaging,
                    method,
                    self.service.subresource_instance_url($segment, sid)?,
                    operation,
                ))
            }

            /// Lazily list all sender associations using a default page size of 50.
            #[must_use]
            pub fn list_all(self) -> BlockingTwilioPaginator<'a, $page, $item> {
                self.list_all_with(ListServiceSubresourcesRequest::new())
            }

            /// Lazily list all sender associations using supplied first-page filters.
            #[must_use]
            pub fn list_all_with(
                self,
                mut request: ListServiceSubresourcesRequest<'a>,
            ) -> BlockingTwilioPaginator<'a, $page, $item> {
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
                    |page| (page.$items_field, page.meta.next_page_url),
                )
            }
        }
    };
}

impl_blocking_sender_resource!(
    BlockingServicePhoneNumbersResource,
    CreateServicePhoneNumberRequest<'a>,
    TwilioServicePhoneNumber,
    TwilioServicePhoneNumberPage,
    WireServicePhoneNumber,
    WirePhoneNumberPage,
    "PhoneNumbers",
    PhoneNumbers,
    into_phone_number,
    into_page,
    phone_numbers
);

impl_blocking_sender_resource!(
    BlockingServiceShortCodesResource,
    CreateServiceShortCodeRequest<'a>,
    TwilioServiceShortCode,
    TwilioServiceShortCodePage,
    WireServiceShortCode,
    WireShortCodePage,
    "ShortCodes",
    ShortCodes,
    into_short_code,
    into_page,
    short_codes
);

impl_blocking_sender_resource!(
    BlockingServiceAlphaSendersResource,
    CreateAlphaSenderRequest<'a>,
    TwilioAlphaSender,
    TwilioAlphaSenderPage,
    WireAlphaSender,
    WireAlphaSenderPage,
    "AlphaSenders",
    AlphaSenders,
    into_alpha_sender,
    into_page,
    alpha_senders
);

impl_blocking_sender_resource!(
    BlockingServiceChannelSendersResource,
    CreateChannelSenderRequest<'a>,
    TwilioChannelSender,
    TwilioChannelSenderPage,
    WireChannelSender,
    WireChannelSenderPage,
    "ChannelSenders",
    ChannelSenders,
    into_channel_sender,
    into_page,
    senders
);

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingServiceDestinationAlphaSendersResource<'a> {
    service: BlockingServiceResource<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingServiceDestinationAlphaSendersResource<'a> {
    /// Create a `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn create(
        self,
        request: CreateDestinationAlphaSenderRequest<'a>,
    ) -> Result<TwilioDestinationAlphaSender, TwilioError> {
        request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.create",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.service.account.creds, self.service.sid);
            let spec = self
                .destination_spec(Method::POST, "service.destination_alpha_senders.create")?
                .form_params(request.form_params());
            let parsed: WireDestinationAlphaSender = self
                .service
                .account
                .send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_destination_alpha_sender())
        })
    }

    /// Fetch one `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self, sid: &'a str) -> Result<TwilioDestinationAlphaSender, TwilioError> {
        request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                sid,
            ];
            let spec = self.destination_instance_spec(
                sid,
                Method::GET,
                "service.destination_alpha_senders.fetch",
            )?;
            let parsed: WireDestinationAlphaSender = self
                .service
                .account
                .send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_destination_alpha_sender())
        })
    }

    /// List `DestinationAlphaSenders` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(
        self,
        request: ListDestinationAlphaSendersRequest<'a>,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values =
                request.sensitive_values(self.service.account.creds, self.service.sid);
            let mut url = self
                .service
                .subresource_collection_url("DestinationAlphaSenders")?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "service.destination_alpha_senders.list",
            );
            let raw = self
                .service
                .account
                .send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URL is invalid, leaves the configured
    /// Messaging API base, changes stable filters, or the HTTP request/response
    /// fails.
    pub fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.list_page_url",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                next_page_url,
            ];
            let resource = V1PageResource::DestinationAlphaSenders {
                service_sid: self.service.sid,
            };
            let url = self
                .service
                .account
                .client
                .v1_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "service.destination_alpha_senders.list_page_url",
            );
            let raw = self
                .service
                .account
                .send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Delete one `DestinationAlphaSender` under this Messaging Service.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub fn delete(self, sid: &'a str) -> Result<(), TwilioError> {
        request_span(
            &self.service.account.client.config.messaging,
            "service.destination_alpha_senders.delete",
            "DELETE",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.service.account.creds.account_sid(),
                self.service.account.creds.auth_secret(),
                self.service.sid,
                sid,
            ];
            let spec = self.destination_instance_spec(
                sid,
                Method::DELETE,
                "service.destination_alpha_senders.delete",
            )?;
            self.service
                .account
                .send_spec_empty(spec, &sensitive_values)
        })
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioDestinationAlphaSenderPage, TwilioError> {
        let parsed: WireDestinationAlphaSenderPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::DestinationAlphaSenders {
            service_sid: self.service.sid,
        };
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url_blocking(
            self.service.account,
            page.meta.next_page_url.as_deref(),
            resource,
            current_url,
        )?;
        Ok(page)
    }

    fn destination_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service
                .subresource_collection_url("DestinationAlphaSenders")?,
            operation,
        ))
    }

    fn destination_instance_spec(
        self,
        sid: &'a str,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.service
                .subresource_instance_url("DestinationAlphaSenders", sid)?,
            operation,
        ))
    }

    /// Lazily list all destination alpha senders using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, TwilioDestinationAlphaSenderPage, TwilioDestinationAlphaSender>
    {
        self.list_all_with(ListDestinationAlphaSendersRequest::new())
    }

    /// Lazily list all destination alpha senders using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListDestinationAlphaSendersRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioDestinationAlphaSenderPage, TwilioDestinationAlphaSender>
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
            |page| (page.alpha_senders, page.meta.next_page_url),
        )
    }
}
