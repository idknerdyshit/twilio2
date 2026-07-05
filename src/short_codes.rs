#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use http::Method;
use serde::Deserialize;
use time::OffsetDateTime;
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
    ApiFamily, DEFAULT_PAGE_SIZE, FormParam, LegacyPageResource, RequestSpec, TwilioCreds,
    TwilioError, decode_json_response, non_empty, parse_rfc2822, push_enum, push_sensitive,
    push_str, redacted_option, request_span, validate_legacy_next_page_continuation,
    validate_page_size,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};
use crate::services::HttpMethod;

#[derive(Clone, Copy)]
enum StringSetting<'a> {
    Set(&'a str),
    Clear,
}

#[derive(Clone, Copy, Default)]
struct AccountShortCodeFields<'a> {
    friendly_name: Option<&'a str>,
    api_version: Option<&'a str>,
    sms_url: Option<StringSetting<'a>>,
    sms_method: Option<HttpMethod>,
    sms_fallback_url: Option<StringSetting<'a>>,
    sms_fallback_method: Option<HttpMethod>,
}

impl<'a> AccountShortCodeFields<'a> {
    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "FriendlyName", self.friendly_name);
        push_str(&mut params, "ApiVersion", self.api_version);
        push_string_setting(&mut params, "SmsUrl", self.sms_url);
        push_enum(&mut params, "SmsMethod", self.sms_method);
        push_string_setting(&mut params, "SmsFallbackUrl", self.sms_fallback_url);
        push_enum(&mut params, "SmsFallbackMethod", self.sms_fallback_method);
        params
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>, sid: &'a str) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token, sid];
        push_sensitive(&mut values, self.friendly_name);
        push_sensitive(&mut values, self.api_version);
        push_string_setting_sensitive(&mut values, self.sms_url);
        push_string_setting_sensitive(&mut values, self.sms_fallback_url);
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

/// Query parameters for `GET /SMS/ShortCodes.json`.
#[derive(Clone, Copy, Default)]
pub struct ListAccountShortCodesRequest<'a> {
    friendly_name: Option<&'a str>,
    short_code: Option<&'a str>,
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListAccountShortCodesRequest<'a> {
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
    pub fn short_code(mut self, value: &'a str) -> Self {
        self.short_code = Some(value);
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
        if let Some(value) = self.friendly_name {
            query.append_pair("FriendlyName", value);
        }
        if let Some(value) = self.short_code {
            query.append_pair("ShortCode", value);
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

    fn sensitive_values(self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token];
        push_sensitive(&mut values, self.friendly_name);
        push_sensitive(&mut values, self.short_code);
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// Request body for `POST /SMS/ShortCodes/{Sid}.json`.
#[derive(Clone, Copy, Default)]
pub struct UpdateAccountShortCodeRequest<'a> {
    fields: AccountShortCodeFields<'a>,
}

impl<'a> UpdateAccountShortCodeRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn friendly_name(mut self, value: &'a str) -> Self {
        self.fields.friendly_name = Some(value);
        self
    }

    #[must_use]
    pub fn api_version(mut self, value: &'a str) -> Self {
        self.fields.api_version = Some(value);
        self
    }

    #[must_use]
    pub fn sms_url(mut self, value: &'a str) -> Self {
        self.fields.sms_url = Some(StringSetting::Set(value));
        self
    }

    #[must_use]
    pub fn clear_sms_url(mut self) -> Self {
        self.fields.sms_url = Some(StringSetting::Clear);
        self
    }

    #[must_use]
    pub fn sms_method(mut self, value: HttpMethod) -> Self {
        self.fields.sms_method = Some(value);
        self
    }

    #[must_use]
    pub fn sms_fallback_url(mut self, value: &'a str) -> Self {
        self.fields.sms_fallback_url = Some(StringSetting::Set(value));
        self
    }

    #[must_use]
    pub fn clear_sms_fallback_url(mut self) -> Self {
        self.fields.sms_fallback_url = Some(StringSetting::Clear);
        self
    }

    #[must_use]
    pub fn sms_fallback_method(mut self, value: HttpMethod) -> Self {
        self.fields.sms_fallback_method = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        if self.fields.form_params().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "short code update requires at least one field".to_owned(),
            ));
        }
        Ok(())
    }

    fn form_params(self) -> Vec<FormParam> {
        self.fields.form_params()
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>, sid: &'a str) -> Vec<&'a str> {
        self.fields.sensitive_values(creds, sid)
    }
}

/// A legacy account-level `ShortCode`.
#[derive(Clone)]
pub struct TwilioAccountShortCode {
    pub account_sid: Option<String>,
    pub api_version: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub friendly_name: Option<String>,
    pub short_code: Option<String>,
    pub sid: Option<String>,
    pub sms_fallback_method: Option<String>,
    pub sms_fallback_url: Option<String>,
    pub sms_method: Option<String>,
    pub sms_url: Option<String>,
    pub uri: Option<String>,
}

impl std::fmt::Debug for TwilioAccountShortCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioAccountShortCode")
            .field("account_sid", &redacted_option(&self.account_sid))
            .field("api_version", &self.api_version)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("friendly_name", &redacted_option(&self.friendly_name))
            .field("short_code", &redacted_option(&self.short_code))
            .field("sid", &redacted_option(&self.sid))
            .field("sms_fallback_method", &self.sms_fallback_method)
            .field("sms_fallback_url", &redacted_option(&self.sms_fallback_url))
            .field("sms_method", &self.sms_method)
            .field("sms_url", &redacted_option(&self.sms_url))
            .field("uri", &redacted_option(&self.uri))
            .finish()
    }
}

/// One page of legacy account-level `ShortCodes`.
#[derive(Clone)]
pub struct TwilioAccountShortCodePage {
    pub short_codes: Vec<TwilioAccountShortCode>,
    pub next_page_uri: Option<String>,
    pub first_page_uri: Option<String>,
    pub previous_page_uri: Option<String>,
    pub uri: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

impl std::fmt::Debug for TwilioAccountShortCodePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioAccountShortCodePage")
            .field("short_codes", &self.short_codes)
            .field("next_page_uri", &redacted_option(&self.next_page_uri))
            .field("first_page_uri", &redacted_option(&self.first_page_uri))
            .field(
                "previous_page_uri",
                &redacted_option(&self.previous_page_uri),
            )
            .field("uri", &redacted_option(&self.uri))
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

#[derive(Deserialize)]
struct WireAccountShortCode {
    account_sid: Option<String>,
    api_version: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    friendly_name: Option<String>,
    short_code: Option<String>,
    sid: Option<String>,
    sms_fallback_method: Option<String>,
    sms_fallback_url: Option<String>,
    sms_method: Option<String>,
    sms_url: Option<String>,
    uri: Option<String>,
}

impl WireAccountShortCode {
    fn into_short_code(self) -> TwilioAccountShortCode {
        TwilioAccountShortCode {
            account_sid: self.account_sid,
            api_version: self.api_version,
            date_created: parse_rfc2822(self.date_created),
            date_updated: parse_rfc2822(self.date_updated),
            friendly_name: self.friendly_name,
            short_code: self.short_code,
            sid: self.sid,
            sms_fallback_method: self.sms_fallback_method,
            sms_fallback_url: self.sms_fallback_url,
            sms_method: self.sms_method,
            sms_url: self.sms_url,
            uri: self.uri,
        }
    }
}

#[derive(Deserialize)]
struct WireAccountShortCodePage {
    #[serde(default)]
    short_codes: Vec<WireAccountShortCode>,
    #[serde(default)]
    next_page_uri: Option<String>,
    #[serde(default)]
    first_page_uri: Option<String>,
    #[serde(default)]
    previous_page_uri: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    start: Option<i64>,
    #[serde(default)]
    end: Option<i64>,
}

impl WireAccountShortCodePage {
    fn into_page(self) -> TwilioAccountShortCodePage {
        TwilioAccountShortCodePage {
            short_codes: self
                .short_codes
                .into_iter()
                .map(WireAccountShortCode::into_short_code)
                .collect(),
            next_page_uri: non_empty(self.next_page_uri),
            first_page_uri: non_empty(self.first_page_uri),
            previous_page_uri: non_empty(self.previous_page_uri),
            uri: non_empty(self.uri),
            page: self.page,
            page_size: self.page_size,
            start: self.start,
            end: self.end,
        }
    }
}

/// Legacy account-level `ShortCodes` collection.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct AccountShortCodesResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> AccountShortCodesResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListAccountShortCodesRequest<'a>,
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.collection_url()?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "account_short_codes.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "account_short_codes.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent `ShortCodes` page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, changes stable filters, or the HTTP request/response
    /// fails.
    pub async fn list_page_uri(
        self,
        next_page_uri: &str,
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid,
                self.account.creds.auth_token,
                next_page_uri,
            ];
            let url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid,
                LegacyPageResource::ShortCodes,
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "account_short_codes.list_page_uri",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "account_short_codes.list_page_uri",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        let parsed: WireAccountShortCodePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid,
                LegacyPageResource::ShortCodes,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(
                    current_url,
                    &next_url,
                    LegacyPageResource::ShortCodes,
                )?;
            }
        }
        Ok(page)
    }

    /// Lazily list all account-level `ShortCodes` using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, TwilioAccountShortCodePage, TwilioAccountShortCode> {
        self.list_all_with(ListAccountShortCodesRequest::new())
    }

    /// Lazily list all account-level `ShortCodes` using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListAccountShortCodesRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioAccountShortCodePage, TwilioAccountShortCode> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_uri(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioAccountShortCodePage>
            },
            split_account_short_code_page,
        )
    }

    fn collection_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid,
            "SMS",
            "ShortCodes.json",
        ])
    }
}

fn split_account_short_code_page(
    page: TwilioAccountShortCodePage,
) -> (Vec<TwilioAccountShortCode>, Option<String>) {
    (page.short_codes, page.next_page_uri)
}

/// One legacy account-level `ShortCode` resource.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct AccountShortCodeResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> AccountShortCodeResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioAccountShortCode, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.short_code_spec(Method::GET, "account_short_code.fetch")?;
            let parsed: WireAccountShortCode =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_short_code())
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "account_short_code.fetch",
            "GET",
        ))
        .await
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update(
        self,
        request: UpdateAccountShortCodeRequest<'a>,
    ) -> Result<TwilioAccountShortCode, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let spec = self
                .short_code_spec(Method::POST, "account_short_code.update")?
                .form_params(request.form_params());
            let parsed: WireAccountShortCode =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_short_code())
        }
        .instrument(request_span(
            &self.account.client.config.rest_base_url,
            "account_short_code.update",
            "POST",
        ))
        .await
    }

    fn short_code_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid,
            "SMS",
            "ShortCodes",
            &format!("{}.json", self.sid),
        ])
    }

    fn short_code_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.short_code_url()?,
            operation,
        ))
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![
            self.account.creds.account_sid,
            self.account.creds.auth_token,
            self.sid,
        ]
    }
}

/// Blocking legacy account-level `ShortCodes` collection.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingAccountShortCodesResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingAccountShortCodesResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub fn list(
        self,
        request: ListAccountShortCodesRequest<'a>,
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "account_short_codes.list",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.collection_url()?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "account_short_codes.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values)?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        })
    }

    /// Fetch a subsequent `ShortCodes` page by Twilio's `next_page_uri`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URI is invalid, leaves the configured
    /// origin/base path, changes stable filters, or the HTTP request/response
    /// fails.
    pub fn list_page_uri(
        self,
        next_page_uri: &str,
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "account_short_codes.list_page_uri",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = vec![
                self.account.creds.account_sid,
                self.account.creds.auth_token,
                next_page_uri,
            ];
            let url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid,
                LegacyPageResource::ShortCodes,
            )?;
            let spec = RequestSpec::from_url(
                ApiFamily::Rest,
                Method::GET,
                url.clone(),
                "account_short_codes.list_page_uri",
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
    ) -> Result<TwilioAccountShortCodePage, TwilioError> {
        let parsed: WireAccountShortCodePage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        if let Some(next_page_uri) = page.next_page_uri.as_ref() {
            let next_url = self.account.client.legacy_page_url(
                next_page_uri,
                self.account.creds.account_sid,
                LegacyPageResource::ShortCodes,
            )?;
            if let Some(current_url) = current_url {
                validate_legacy_next_page_continuation(
                    current_url,
                    &next_url,
                    LegacyPageResource::ShortCodes,
                )?;
            }
        }
        Ok(page)
    }

    /// Lazily list all account-level `ShortCodes` using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> BlockingTwilioPaginator<'a, TwilioAccountShortCodePage, TwilioAccountShortCode> {
        self.list_all_with(ListAccountShortCodesRequest::new())
    }

    /// Lazily list all account-level `ShortCodes` using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListAccountShortCodesRequest<'a>,
    ) -> BlockingTwilioPaginator<'a, TwilioAccountShortCodePage, TwilioAccountShortCode> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        BlockingTwilioPaginator::new(
            move |cursor| {
                let resource = self;
                if let Some(cursor) = cursor {
                    resource.list_page_uri(&cursor)
                } else {
                    resource.list(request)
                }
            },
            split_account_short_code_page,
        )
    }

    fn collection_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid,
            "SMS",
            "ShortCodes.json",
        ])
    }
}

/// One blocking legacy account-level `ShortCode` resource.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingAccountShortCodeResource<'a> {
    account: BlockingTwilioAccount<'a>,
    sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingAccountShortCodeResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    /// `GET /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioAccountShortCode, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "account_short_code.fetch",
            "GET",
        )
        .in_scope(|| {
            let sensitive_values = self.sensitive_values();
            let spec = self.short_code_spec(Method::GET, "account_short_code.fetch")?;
            let parsed: WireAccountShortCode =
                self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_short_code())
        })
    }

    /// `POST /2010-04-01/Accounts/{AccountSid}/SMS/ShortCodes/{Sid}.json`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn update(
        self,
        request: UpdateAccountShortCodeRequest<'a>,
    ) -> Result<TwilioAccountShortCode, TwilioError> {
        request_span(
            &self.account.client.config.rest_base_url,
            "account_short_code.update",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let spec = self
                .short_code_spec(Method::POST, "account_short_code.update")?
                .form_params(request.form_params());
            let parsed: WireAccountShortCode =
                self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_short_code())
        })
    }

    fn short_code_url(self) -> Result<Url, TwilioError> {
        self.account.client.rest_endpoint(&[
            "2010-04-01",
            "Accounts",
            self.account.creds.account_sid,
            "SMS",
            "ShortCodes",
            &format!("{}.json", self.sid),
        ])
    }

    fn short_code_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Rest,
            method,
            self.short_code_url()?,
            operation,
        ))
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![
            self.account.creds.account_sid,
            self.account.creds.auth_token,
            self.sid,
        ]
    }
}
