use std::fmt;

use http::Method;
use serde::{Deserialize, Serialize};
#[cfg(feature = "async")]
use tracing::Instrument as _;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
use crate::common::{
    ApiFamily, FormParam, RequestSpec, TwilioAuth, TwilioError, push_sensitive, push_str,
    redacted_option, redacted_optional, request_span,
};

const BULK_MAX_ITEMS: usize = 25;

#[derive(Clone, Copy, Serialize)]
pub struct ContactItem<'a> {
    contact_id: &'a str,
    correlation_id: &'a str,
    country_iso_code: &'a str,
    zip_code: &'a str,
}

impl<'a> ContactItem<'a> {
    #[must_use]
    pub fn new(
        contact_id: &'a str,
        correlation_id: &'a str,
        country_iso_code: &'a str,
        zip_code: &'a str,
    ) -> Self {
        Self {
            contact_id,
            correlation_id,
            country_iso_code,
            zip_code,
        }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("contact_id", self.contact_id)?;
        validate_required("correlation_id", self.correlation_id)?;
        validate_required("country_iso_code", self.country_iso_code)?;
        validate_required("zip_code", self.zip_code)
    }

    fn push_sensitive_values(self, values: &mut Vec<&'a str>) {
        values.extend([
            self.contact_id,
            self.correlation_id,
            self.country_iso_code,
            self.zip_code,
        ]);
    }
}

impl fmt::Debug for ContactItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContactItem")
            .field("contact_id", &crate::common::REDACTED)
            .field("correlation_id", &crate::common::REDACTED)
            .field("country_iso_code", &crate::common::REDACTED)
            .field("zip_code", &crate::common::REDACTED)
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct BulkContactsRequest<'a> {
    items: Vec<ContactItem<'a>>,
}

impl<'a> BulkContactsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn item(mut self, item: ContactItem<'a>) -> Self {
        self.items.push(item);
        self
    }

    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = ContactItem<'a>>) -> Self {
        self.items.extend(items);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        validate_bulk_items("Items", self.items.len())?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }

    fn form_params(&self) -> Result<Vec<FormParam>, TwilioError> {
        json_items_form(self.items.iter())
    }

    fn sensitive_values(&self, auth: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        for item in &self.items {
            item.push_sensitive_values(&mut values);
        }
        values
    }
}

impl fmt::Debug for BulkContactsRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkContactsRequest")
            .field("items", &format_args!("[{} redacted]", self.items.len()))
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentStatus<'a> {
    OptIn,
    OptOut,
    Custom(&'a str),
}

impl<'a> ConsentStatus<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::OptIn => "opt-in",
            Self::OptOut => "opt-out",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentSource<'a> {
    Website,
    Offline,
    OptInMessage,
    OptOutMessage,
    Others,
    Custom(&'a str),
}

impl<'a> ConsentSource<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::Website => "website",
            Self::Offline => "offline",
            Self::OptInMessage => "opt-in-message",
            Self::OptOutMessage => "opt-out-message",
            Self::Others => "others",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ConsentItem<'a> {
    contact_id: &'a str,
    correlation_id: &'a str,
    sender_id: &'a str,
    status: ConsentStatus<'a>,
    source: ConsentSource<'a>,
    date_of_consent: Option<&'a str>,
}

impl<'a> ConsentItem<'a> {
    #[must_use]
    pub fn new(
        contact_id: &'a str,
        correlation_id: &'a str,
        sender_id: &'a str,
        status: ConsentStatus<'a>,
        source: ConsentSource<'a>,
    ) -> Self {
        Self {
            contact_id,
            correlation_id,
            sender_id,
            status,
            source,
            date_of_consent: None,
        }
    }

    #[must_use]
    pub fn date_of_consent(mut self, value: &'a str) -> Self {
        self.date_of_consent = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("contact_id", self.contact_id)?;
        validate_required("correlation_id", self.correlation_id)?;
        validate_required("sender_id", self.sender_id)?;
        validate_required("status", self.status.form_value())?;
        validate_required("source", self.source.form_value())?;
        if let Some(date_of_consent) = self.date_of_consent {
            validate_required("date_of_consent", date_of_consent)?;
        }
        Ok(())
    }

    fn push_sensitive_values(self, values: &mut Vec<&'a str>) {
        values.extend([self.contact_id, self.correlation_id, self.sender_id]);
        if let ConsentStatus::Custom(value) = self.status {
            values.push(value);
        }
        if let ConsentSource::Custom(value) = self.source {
            values.push(value);
        }
        push_sensitive(values, self.date_of_consent);
    }
}

impl Serialize for ConsentItem<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireConsentItem<'a> {
            contact_id: &'a str,
            correlation_id: &'a str,
            sender_id: &'a str,
            status: &'a str,
            source: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            date_of_consent: Option<&'a str>,
        }

        WireConsentItem {
            contact_id: self.contact_id,
            correlation_id: self.correlation_id,
            sender_id: self.sender_id,
            status: self.status.form_value(),
            source: self.source.form_value(),
            date_of_consent: self.date_of_consent,
        }
        .serialize(serializer)
    }
}

impl fmt::Debug for ConsentItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsentItem")
            .field("contact_id", &crate::common::REDACTED)
            .field("correlation_id", &crate::common::REDACTED)
            .field("sender_id", &crate::common::REDACTED)
            .field("status", &crate::common::REDACTED)
            .field("source", &crate::common::REDACTED)
            .field(
                "date_of_consent",
                &redacted_optional(self.date_of_consent.is_some()),
            )
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct BulkConsentsRequest<'a> {
    items: Vec<ConsentItem<'a>>,
}

impl<'a> BulkConsentsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn item(mut self, item: ConsentItem<'a>) -> Self {
        self.items.push(item);
        self
    }

    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = ConsentItem<'a>>) -> Self {
        self.items.extend(items);
        self
    }

    fn validate(&self) -> Result<(), TwilioError> {
        validate_bulk_items("Items", self.items.len())?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }

    fn form_params(&self) -> Result<Vec<FormParam>, TwilioError> {
        json_items_form(self.items.iter())
    }

    fn sensitive_values(&self, auth: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        for item in &self.items {
            item.push_sensitive_values(&mut values);
        }
        values
    }
}

impl fmt::Debug for BulkConsentsRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BulkConsentsRequest")
            .field("items", &format_args!("[{} redacted]", self.items.len()))
            .finish()
    }
}

#[derive(Clone, Copy)]
pub struct SafeListNumberRequest<'a> {
    phone_number: &'a str,
}

impl<'a> SafeListNumberRequest<'a> {
    #[must_use]
    pub fn new(phone_number: &'a str) -> Self {
        Self { phone_number }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("PhoneNumber", self.phone_number)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "PhoneNumber", Some(self.phone_number));
        params
    }

    fn apply_query(self, url: &mut url::Url) {
        url.query_pairs_mut()
            .append_pair("PhoneNumber", self.phone_number);
    }

    fn sensitive_values(self, auth: &'a TwilioAuth) -> Vec<&'a str> {
        let mut values = auth.sensitive_values();
        values.push(self.phone_number);
        values
    }
}

impl fmt::Debug for SafeListNumberRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SafeListNumberRequest")
            .field("phone_number", &crate::common::REDACTED)
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioBulkContactResult {
    pub contact_id: Option<String>,
    pub correlation_id: Option<String>,
    pub country_iso_code: Option<String>,
    pub zip_code: Option<String>,
    pub error_code: Option<i64>,
    #[serde(default)]
    pub error_messages: Vec<String>,
}

impl fmt::Debug for TwilioBulkContactResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioBulkContactResult")
            .field("contact_id", &redacted_option(&self.contact_id))
            .field("correlation_id", &redacted_option(&self.correlation_id))
            .field("country_iso_code", &redacted_option(&self.country_iso_code))
            .field("zip_code", &redacted_option(&self.zip_code))
            .field("error_code", &self.error_code)
            .field(
                "error_messages",
                &format_args!("[{} redacted]", self.error_messages.len()),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioBulkContactsResponse {
    #[serde(default)]
    pub items: Vec<TwilioBulkContactResult>,
}

impl fmt::Debug for TwilioBulkContactsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioBulkContactsResponse")
            .field(
                "items",
                &format_args!("[{}; {}]", crate::common::REDACTED, self.items.len()),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioBulkConsentResult {
    pub contact_id: Option<String>,
    pub correlation_id: Option<String>,
    pub sender_id: Option<String>,
    pub status: Option<String>,
    pub source: Option<String>,
    pub date_of_consent: Option<String>,
    pub error_code: Option<i64>,
    #[serde(default)]
    pub error_messages: Vec<String>,
}

impl fmt::Debug for TwilioBulkConsentResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioBulkConsentResult")
            .field("contact_id", &redacted_option(&self.contact_id))
            .field("correlation_id", &redacted_option(&self.correlation_id))
            .field("sender_id", &redacted_option(&self.sender_id))
            .field("status", &redacted_option(&self.status))
            .field("source", &redacted_option(&self.source))
            .field("date_of_consent", &redacted_option(&self.date_of_consent))
            .field("error_code", &self.error_code)
            .field(
                "error_messages",
                &format_args!("[{} redacted]", self.error_messages.len()),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioBulkConsentsResponse {
    #[serde(default)]
    pub items: Vec<TwilioBulkConsentResult>,
}

impl fmt::Debug for TwilioBulkConsentsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioBulkConsentsResponse")
            .field(
                "items",
                &format_args!("[{}; {}]", crate::common::REDACTED, self.items.len()),
            )
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwilioSafeListNumber {
    pub sid: Option<String>,
    pub phone_number: Option<String>,
}

impl fmt::Debug for TwilioSafeListNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioSafeListNumber")
            .field("sid", &redacted_option(&self.sid))
            .field("phone_number", &redacted_option(&self.phone_number))
            .finish()
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ContactsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> ContactsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Contacts/Bulk`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, JSON serialization failures, or malformed JSON responses.
    pub async fn bulk_upsert(
        self,
        request: BulkContactsRequest<'a>,
    ) -> Result<TwilioBulkContactsResponse, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["Contacts", "Bulk"])
                .operation("contacts.bulk_upsert")
                .form_params(request.form_params()?);
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.accounts,
            "contacts.bulk_upsert",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct ConsentsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> ConsentsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Consents/Bulk`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, JSON serialization failures, or malformed JSON responses.
    pub async fn bulk_upsert(
        self,
        request: BulkConsentsRequest<'a>,
    ) -> Result<TwilioBulkConsentsResponse, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["Consents", "Bulk"])
                .operation("consents.bulk_upsert")
                .form_params(request.form_params()?);
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.accounts,
            "consents.bulk_upsert",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct GlobalSafeListResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> GlobalSafeListResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /SafeList/Numbers`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, or malformed JSON responses.
    pub async fn add(
        self,
        request: SafeListNumberRequest<'a>,
    ) -> Result<TwilioSafeListNumber, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["SafeList", "Numbers"])
                .operation("global_safe_list.add")
                .form_params(request.form_params());
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.accounts,
            "global_safe_list.add",
            "POST",
        ))
        .await
    }

    /// `GET /SafeList/Numbers?PhoneNumber=...`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, or malformed JSON responses.
    pub async fn check(
        self,
        request: SafeListNumberRequest<'a>,
    ) -> Result<TwilioSafeListNumber, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self
                .account
                .client
                .accounts_endpoint(&["SafeList", "Numbers"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Accounts,
                Method::GET,
                url,
                "global_safe_list.check",
            );
            self.account.send_spec_json(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.accounts,
            "global_safe_list.check",
            "GET",
        ))
        .await
    }

    /// `DELETE /SafeList/Numbers?PhoneNumber=...`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or API
    /// errors.
    pub async fn remove(self, request: SafeListNumberRequest<'a>) -> Result<(), TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self
                .account
                .client
                .accounts_endpoint(&["SafeList", "Numbers"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Accounts,
                Method::DELETE,
                url,
                "global_safe_list.remove",
            );
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.accounts,
            "global_safe_list.remove",
            "DELETE",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingContactsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingContactsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Contacts/Bulk`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, JSON serialization failures, or malformed JSON responses.
    #[allow(clippy::needless_pass_by_value)]
    pub fn bulk_upsert(
        self,
        request: BulkContactsRequest<'a>,
    ) -> Result<TwilioBulkContactsResponse, TwilioError> {
        request_span(
            &self.account.client.config.accounts,
            "contacts.bulk_upsert",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["Contacts", "Bulk"])
                .operation("contacts.bulk_upsert")
                .form_params(request.form_params()?);
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingConsentsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingConsentsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Consents/Bulk`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, JSON serialization failures, or malformed JSON responses.
    #[allow(clippy::needless_pass_by_value)]
    pub fn bulk_upsert(
        self,
        request: BulkConsentsRequest<'a>,
    ) -> Result<TwilioBulkConsentsResponse, TwilioError> {
        request_span(
            &self.account.client.config.accounts,
            "consents.bulk_upsert",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["Consents", "Bulk"])
                .operation("consents.bulk_upsert")
                .form_params(request.form_params()?);
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingGlobalSafeListResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingGlobalSafeListResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /SafeList/Numbers`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, or malformed JSON responses.
    #[allow(clippy::should_implement_trait)]
    pub fn add(
        self,
        request: SafeListNumberRequest<'a>,
    ) -> Result<TwilioSafeListNumber, TwilioError> {
        request_span(
            &self.account.client.config.accounts,
            "global_safe_list.add",
            "POST",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Accounts, Method::POST, ["SafeList", "Numbers"])
                .operation("global_safe_list.add")
                .form_params(request.form_params());
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// `GET /SafeList/Numbers?PhoneNumber=...`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, API
    /// errors, or malformed JSON responses.
    pub fn check(
        self,
        request: SafeListNumberRequest<'a>,
    ) -> Result<TwilioSafeListNumber, TwilioError> {
        request_span(
            &self.account.client.config.accounts,
            "global_safe_list.check",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self
                .account
                .client
                .accounts_endpoint(&["SafeList", "Numbers"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Accounts,
                Method::GET,
                url,
                "global_safe_list.check",
            );
            self.account.send_spec_json(spec, &sensitive_values)
        })
    }

    /// `DELETE /SafeList/Numbers?PhoneNumber=...`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or API
    /// errors.
    pub fn remove(self, request: SafeListNumberRequest<'a>) -> Result<(), TwilioError> {
        request_span(
            &self.account.client.config.accounts,
            "global_safe_list.remove",
            "DELETE",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self
                .account
                .client
                .accounts_endpoint(&["SafeList", "Numbers"])?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Accounts,
                Method::DELETE,
                url,
                "global_safe_list.remove",
            );
            self.account.send_spec_empty(spec, &sensitive_values)
        })
    }
}

fn json_items_form<'a, T: Serialize + 'a>(
    items: impl Iterator<Item = &'a T>,
) -> Result<Vec<FormParam>, TwilioError> {
    let mut params = Vec::new();
    for item in items {
        let encoded = serde_json::to_string(item).map_err(|error| {
            TwilioError::InvalidRequest(format!("Items could not be serialized: {error}"))
        })?;
        push_str(&mut params, "Items", Some(&encoded));
    }
    Ok(params)
}

fn validate_bulk_items(name: &str, len: usize) -> Result<(), TwilioError> {
    if len == 0 {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must contain at least one item"
        )));
    }
    if len > BULK_MAX_ITEMS {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must contain at most {BULK_MAX_ITEMS} items"
        )));
    }
    Ok(())
}

fn validate_required(name: &str, value: &str) -> Result<(), TwilioError> {
    if value.trim().is_empty() {
        Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )))
    } else {
        Ok(())
    }
}
