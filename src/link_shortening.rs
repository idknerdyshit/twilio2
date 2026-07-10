#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use std::fmt;

use http::Method;
use serde::Deserialize;
use time::OffsetDateTime;
#[cfg(feature = "async")]
use tracing::Instrument as _;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "async")]
use crate::common::request_span;
use crate::common::{
    ApiFamily, FormParam, RequestSpec, TwilioAuth, TwilioError, has_non_empty, non_empty,
    parse_iso8601, push_bool, push_sensitive, push_str, redacted_option,
};

#[derive(Clone, Copy)]
pub struct UpdateLinkShorteningDomainCertificateRequest<'a> {
    tls_cert: &'a str,
}

impl<'a> UpdateLinkShorteningDomainCertificateRequest<'a> {
    #[must_use]
    pub fn new(tls_cert: &'a str) -> Self {
        Self { tls_cert }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("TlsCert", self.tls_cert)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "TlsCert", Some(self.tls_cert));
        params
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![self.tls_cert]
    }
}

impl fmt::Debug for UpdateLinkShorteningDomainCertificateRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateLinkShorteningDomainCertificateRequest")
            .field("tls_cert", &crate::common::REDACTED)
            .finish()
    }
}

#[derive(Clone, Copy, Default)]
pub struct UpdateLinkShorteningDomainConfigRequest<'a> {
    callback_url: Option<&'a str>,
    fallback_url: Option<&'a str>,
    continue_on_failure: Option<bool>,
    disable_https: Option<bool>,
}

impl<'a> UpdateLinkShorteningDomainConfigRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn callback_url(mut self, value: &'a str) -> Self {
        self.callback_url = Some(value);
        self
    }

    #[must_use]
    pub fn fallback_url(mut self, value: &'a str) -> Self {
        self.fallback_url = Some(value);
        self
    }

    #[must_use]
    pub fn continue_on_failure(mut self, value: bool) -> Self {
        self.continue_on_failure = Some(value);
        self
    }

    #[must_use]
    pub fn disable_https(mut self, value: bool) -> Self {
        self.disable_https = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        if !has_non_empty(self.callback_url)
            && !has_non_empty(self.fallback_url)
            && self.continue_on_failure.is_none()
            && self.disable_https.is_none()
        {
            return Err(TwilioError::InvalidRequest(
                "at least one Link Shortening domain config field must be set".to_owned(),
            ));
        }
        validate_optional_non_empty("CallbackUrl", self.callback_url)?;
        validate_optional_non_empty("FallbackUrl", self.fallback_url)
    }

    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "CallbackUrl", self.callback_url);
        push_str(&mut params, "FallbackUrl", self.fallback_url);
        push_bool(&mut params, "ContinueOnFailure", self.continue_on_failure);
        push_bool(&mut params, "DisableHttps", self.disable_https);
        params
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        let mut values = Vec::new();
        push_sensitive(&mut values, self.callback_url);
        push_sensitive(&mut values, self.fallback_url);
        values
    }
}

impl fmt::Debug for UpdateLinkShorteningDomainConfigRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateLinkShorteningDomainConfigRequest")
            .field(
                "callback_url",
                &self.callback_url.map(|_| crate::common::REDACTED),
            )
            .field(
                "fallback_url",
                &self.fallback_url.map(|_| crate::common::REDACTED),
            )
            .field("continue_on_failure", &self.continue_on_failure)
            .field("disable_https", &self.disable_https)
            .finish()
    }
}

#[derive(Clone)]
pub struct TwilioLinkShorteningDomainCertificate {
    pub domain_sid: Option<String>,
    pub certificate_sid: Option<String>,
    pub domain_name: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub date_expires: Option<OffsetDateTime>,
    pub managed: Option<bool>,
    pub requesting: Option<bool>,
    pub cert_in_validation: Option<TwilioCertificateValidationStatus>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioLinkShorteningDomainCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLinkShorteningDomainCertificate")
            .field("domain_sid", &redacted_option(&self.domain_sid))
            .field("certificate_sid", &redacted_option(&self.certificate_sid))
            .field("domain_name", &redacted_option(&self.domain_name))
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("date_expires", &self.date_expires)
            .field("managed", &self.managed)
            .field("requesting", &self.requesting)
            .field("cert_in_validation", &self.cert_in_validation)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct TwilioCertificateValidationStatus {
    pub date_expires: Option<OffsetDateTime>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
struct WireCertificateValidationStatus {
    date_expires: Option<String>,
    status: Option<String>,
}

impl WireCertificateValidationStatus {
    fn into_status(self) -> TwilioCertificateValidationStatus {
        TwilioCertificateValidationStatus {
            date_expires: parse_iso8601(self.date_expires),
            status: non_empty(self.status),
        }
    }
}

#[derive(Deserialize)]
struct WireDomainCertificate {
    domain_sid: Option<String>,
    certificate_sid: Option<String>,
    domain_name: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    date_expires: Option<String>,
    managed: Option<bool>,
    requesting: Option<bool>,
    cert_in_validation: Option<WireCertificateValidationStatus>,
    url: Option<String>,
}

impl WireDomainCertificate {
    fn into_certificate(self) -> TwilioLinkShorteningDomainCertificate {
        TwilioLinkShorteningDomainCertificate {
            domain_sid: non_empty(self.domain_sid),
            certificate_sid: non_empty(self.certificate_sid),
            domain_name: non_empty(self.domain_name),
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            date_expires: parse_iso8601(self.date_expires),
            managed: self.managed,
            requesting: self.requesting,
            cert_in_validation: self
                .cert_in_validation
                .map(WireCertificateValidationStatus::into_status),
            url: non_empty(self.url),
        }
    }
}

#[derive(Clone)]
pub struct TwilioLinkShorteningDomainConfig {
    pub domain_sid: Option<String>,
    pub config_sid: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub callback_url: Option<String>,
    pub fallback_url: Option<String>,
    pub continue_on_failure: Option<bool>,
    pub disable_https: Option<bool>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioLinkShorteningDomainConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLinkShorteningDomainConfig")
            .field("domain_sid", &redacted_option(&self.domain_sid))
            .field("config_sid", &redacted_option(&self.config_sid))
            .field(
                "messaging_service_sid",
                &redacted_option(&self.messaging_service_sid),
            )
            .field("callback_url", &redacted_option(&self.callback_url))
            .field("fallback_url", &redacted_option(&self.fallback_url))
            .field("continue_on_failure", &self.continue_on_failure)
            .field("disable_https", &self.disable_https)
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Deserialize)]
struct WireDomainConfig {
    domain_sid: Option<String>,
    config_sid: Option<String>,
    messaging_service_sid: Option<String>,
    callback_url: Option<String>,
    fallback_url: Option<String>,
    continue_on_failure: Option<bool>,
    disable_https: Option<bool>,
    date_created: Option<String>,
    date_updated: Option<String>,
    url: Option<String>,
}

impl WireDomainConfig {
    fn into_config(self) -> TwilioLinkShorteningDomainConfig {
        TwilioLinkShorteningDomainConfig {
            domain_sid: non_empty(self.domain_sid),
            config_sid: non_empty(self.config_sid),
            messaging_service_sid: non_empty(self.messaging_service_sid),
            callback_url: non_empty(self.callback_url),
            fallback_url: non_empty(self.fallback_url),
            continue_on_failure: self.continue_on_failure,
            disable_https: self.disable_https,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            url: non_empty(self.url),
        }
    }
}

#[derive(Clone)]
pub struct TwilioLinkShorteningDnsValidation {
    pub domain_sid: Option<String>,
    pub is_valid: Option<bool>,
    pub reason: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioLinkShorteningDnsValidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLinkShorteningDnsValidation")
            .field("domain_sid", &redacted_option(&self.domain_sid))
            .field("is_valid", &self.is_valid)
            .field("reason", &redacted_option(&self.reason))
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Deserialize)]
struct WireDnsValidation {
    domain_sid: Option<String>,
    is_valid: Option<bool>,
    reason: Option<String>,
    url: Option<String>,
}

impl WireDnsValidation {
    fn into_validation(self) -> TwilioLinkShorteningDnsValidation {
        TwilioLinkShorteningDnsValidation {
            domain_sid: non_empty(self.domain_sid),
            is_valid: self.is_valid,
            reason: non_empty(self.reason),
            url: non_empty(self.url),
        }
    }
}

#[derive(Clone)]
pub struct TwilioLinkShorteningMessagingService {
    pub domain_sid: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub url: Option<String>,
}

impl fmt::Debug for TwilioLinkShorteningMessagingService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioLinkShorteningMessagingService")
            .field("domain_sid", &redacted_option(&self.domain_sid))
            .field(
                "messaging_service_sid",
                &redacted_option(&self.messaging_service_sid),
            )
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Deserialize)]
struct WireMessagingServiceAssociation {
    domain_sid: Option<String>,
    messaging_service_sid: Option<String>,
    url: Option<String>,
}

impl WireMessagingServiceAssociation {
    fn into_association(self) -> TwilioLinkShorteningMessagingService {
        TwilioLinkShorteningMessagingService {
            domain_sid: non_empty(self.domain_sid),
            messaging_service_sid: non_empty(self.messaging_service_sid),
            url: non_empty(self.url),
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

fn validate_optional_non_empty(name: &str, value: Option<&str>) -> Result<(), TwilioError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}

fn sensitive_values<'a>(
    creds: &'a TwilioAuth,
    domain_sid: Option<&'a str>,
    messaging_service_sid: Option<&'a str>,
) -> Vec<&'a str> {
    let mut values = vec![creds.account_sid(), creds.auth_secret()];
    push_sensitive(&mut values, domain_sid);
    push_sensitive(&mut values, messaging_service_sid);
    values
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV1LinkShorteningResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn domain(self, domain_sid: &'a str) -> MessagingV1LinkShorteningDomainResource<'a> {
        MessagingV1LinkShorteningDomainResource {
            account: self.account,
            domain_sid,
        }
    }

    #[must_use]
    pub fn messaging_service(
        self,
        messaging_service_sid: &'a str,
    ) -> MessagingV1LinkShorteningMessagingServiceResource<'a> {
        MessagingV1LinkShorteningMessagingServiceResource {
            account: self.account,
            messaging_service_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningDomainResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> MessagingV1LinkShorteningDomainResource<'a> {
    #[must_use]
    pub fn certificate(self) -> MessagingV1LinkShorteningDomainCertificateResource<'a> {
        MessagingV1LinkShorteningDomainCertificateResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }

    #[must_use]
    pub fn config(self) -> MessagingV1LinkShorteningDomainConfigResource<'a> {
        MessagingV1LinkShorteningDomainConfigResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }

    #[must_use]
    pub fn messaging_service(
        self,
        messaging_service_sid: &'a str,
    ) -> MessagingV1LinkShorteningDomainMessagingServiceResource<'a> {
        MessagingV1LinkShorteningDomainMessagingServiceResource {
            account: self.account,
            domain_sid: self.domain_sid,
            messaging_service_sid,
        }
    }

    /// `GET /LinkShortening/Domains/{DomainSid}/ValidateDns`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn validate_dns(self) -> Result<TwilioLinkShorteningDnsValidation, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::GET,
                ["LinkShortening", "Domains", self.domain_sid, "ValidateDns"],
            )
            .operation("messaging.v1.link_shortening.domain.validate_dns");
            let parsed: WireDnsValidation =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_validation())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.validate_dns",
            "GET",
        ))
        .await
    }

    /// `POST /LinkShortening/Domains/{DomainSid}/RequestManagedCert`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn request_managed_certificate(
        self,
    ) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                [
                    "LinkShortening",
                    "Domains",
                    self.domain_sid,
                    "RequestManagedCert",
                ],
            )
            .operation("messaging.v1.link_shortening.domain.request_managed_certificate");
            let parsed: WireDomainCertificate =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_certificate())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.request_managed_certificate",
            "POST",
        ))
        .await
    }
}

macro_rules! impl_async_certificate_resource {
    ($resource:ident, $family:expr, $operation_prefix:literal) => {
        #[cfg(feature = "async")]
        impl<'a> $resource<'a> {
            /// Fetch the Link Shortening domain certificate.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, or malformed JSON responses.
            pub async fn fetch(self) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
                async move {
                    validate_required("DomainSid", self.domain_sid)?;
                    let sensitive_values =
                        sensitive_values(self.account.creds, Some(self.domain_sid), None);
                    let spec = RequestSpec::new(
                        $family,
                        Method::GET,
                        ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
                    )
                    .operation(concat!($operation_prefix, ".fetch"));
                    let parsed: WireDomainCertificate =
                        self.account.send_spec_json(spec, &sensitive_values).await?;
                    Ok(parsed.into_certificate())
                }
                .instrument(request_span(
                    &self.account.client.config.messaging,
                    concat!($operation_prefix, ".fetch"),
                    "GET",
                ))
                .await
            }
        }
    };
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningDomainCertificateResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
}

impl_async_certificate_resource!(
    MessagingV1LinkShorteningDomainCertificateResource,
    ApiFamily::MessagingV1,
    "messaging.v1.link_shortening.domain.certificate"
);

#[cfg(feature = "async")]
impl<'a> MessagingV1LinkShorteningDomainCertificateResource<'a> {
    /// `POST /LinkShortening/Domains/{DomainSid}/Certificate`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update(
        self,
        request: UpdateLinkShorteningDomainCertificateRequest<'a>,
    ) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            request.validate()?;
            let mut sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
            )
            .operation("messaging.v1.link_shortening.domain.certificate.update")
            .form_params(request.form_params());
            let parsed: WireDomainCertificate =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_certificate())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.certificate.update",
            "POST",
        ))
        .await
    }

    /// `DELETE /LinkShortening/Domains/{DomainSid}/Certificate`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::DELETE,
                ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
            )
            .operation("messaging.v1.link_shortening.domain.certificate.delete");
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.certificate.delete",
            "DELETE",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningDomainConfigResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> MessagingV1LinkShorteningDomainConfigResource<'a> {
    /// `GET /LinkShortening/Domains/{DomainSid}/Config`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::GET,
                ["LinkShortening", "Domains", self.domain_sid, "Config"],
            )
            .operation("messaging.v1.link_shortening.domain.config.fetch");
            let parsed: WireDomainConfig =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_config())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.config.fetch",
            "GET",
        ))
        .await
    }

    /// `POST /LinkShortening/Domains/{DomainSid}/Config`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update(
        self,
        request: UpdateLinkShorteningDomainConfigRequest<'a>,
    ) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            request.validate()?;
            let mut sensitive_values =
                sensitive_values(self.account.creds, Some(self.domain_sid), None);
            sensitive_values.extend(request.sensitive_values());
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                ["LinkShortening", "Domains", self.domain_sid, "Config"],
            )
            .operation("messaging.v1.link_shortening.domain.config.update")
            .form_params(request.form_params());
            let parsed: WireDomainConfig =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_config())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.config.update",
            "POST",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningDomainMessagingServiceResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
    messaging_service_sid: &'a str,
}

#[cfg(feature = "async")]
impl MessagingV1LinkShorteningDomainMessagingServiceResource<'_> {
    /// `POST /LinkShortening/Domains/{DomainSid}/MessagingServices/{MessagingServiceSid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(self) -> Result<TwilioLinkShorteningMessagingService, TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            validate_required("MessagingServiceSid", self.messaging_service_sid)?;
            let sensitive_values = sensitive_values(
                self.account.creds,
                Some(self.domain_sid),
                Some(self.messaging_service_sid),
            );
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::POST,
                [
                    "LinkShortening",
                    "Domains",
                    self.domain_sid,
                    "MessagingServices",
                    self.messaging_service_sid,
                ],
            )
            .operation("messaging.v1.link_shortening.domain.messaging_service.create");
            let parsed: WireMessagingServiceAssociation =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_association())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.messaging_service.create",
            "POST",
        ))
        .await
    }

    /// `DELETE /LinkShortening/Domains/{DomainSid}/MessagingServices/{MessagingServiceSid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            validate_required("DomainSid", self.domain_sid)?;
            validate_required("MessagingServiceSid", self.messaging_service_sid)?;
            let sensitive_values = sensitive_values(
                self.account.creds,
                Some(self.domain_sid),
                Some(self.messaging_service_sid),
            );
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::DELETE,
                [
                    "LinkShortening",
                    "Domains",
                    self.domain_sid,
                    "MessagingServices",
                    self.messaging_service_sid,
                ],
            )
            .operation("messaging.v1.link_shortening.domain.messaging_service.delete");
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.domain.messaging_service.delete",
            "DELETE",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1LinkShorteningMessagingServiceResource<'a> {
    account: TwilioAccount<'a>,
    messaging_service_sid: &'a str,
}

#[cfg(feature = "async")]
impl MessagingV1LinkShorteningMessagingServiceResource<'_> {
    /// `GET /LinkShortening/MessagingService/{MessagingServiceSid}/DomainConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn domain_config(self) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        async move {
            validate_required("MessagingServiceSid", self.messaging_service_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, None, Some(self.messaging_service_sid));
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::GET,
                [
                    "LinkShortening",
                    "MessagingService",
                    self.messaging_service_sid,
                    "DomainConfig",
                ],
            )
            .operation("messaging.v1.link_shortening.messaging_service.domain_config.fetch");
            let parsed: WireDomainConfig =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_config())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.messaging_service.domain_config.fetch",
            "GET",
        ))
        .await
    }

    /// `GET /LinkShortening/MessagingServices/{MessagingServiceSid}/Domain`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn domain(self) -> Result<TwilioLinkShorteningMessagingService, TwilioError> {
        async move {
            validate_required("MessagingServiceSid", self.messaging_service_sid)?;
            let sensitive_values =
                sensitive_values(self.account.creds, None, Some(self.messaging_service_sid));
            let spec = RequestSpec::new(
                ApiFamily::MessagingV1,
                Method::GET,
                [
                    "LinkShortening",
                    "MessagingServices",
                    self.messaging_service_sid,
                    "Domain",
                ],
            )
            .operation("messaging.v1.link_shortening.messaging_service.domain.fetch");
            let parsed: WireMessagingServiceAssociation =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_association())
        }
        .instrument(request_span(
            &self.account.client.config.messaging,
            "messaging.v1.link_shortening.messaging_service.domain.fetch",
            "GET",
        ))
        .await
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2LinkShorteningResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2LinkShorteningResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn domain(self, domain_sid: &'a str) -> MessagingV2LinkShorteningDomainResource<'a> {
        MessagingV2LinkShorteningDomainResource {
            account: self.account,
            domain_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2LinkShorteningDomainResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2LinkShorteningDomainResource<'a> {
    #[must_use]
    pub fn certificate(self) -> MessagingV2LinkShorteningDomainCertificateResource<'a> {
        MessagingV2LinkShorteningDomainCertificateResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2LinkShorteningDomainCertificateResource<'a> {
    account: TwilioAccount<'a>,
    domain_sid: &'a str,
}

impl_async_certificate_resource!(
    MessagingV2LinkShorteningDomainCertificateResource,
    ApiFamily::MessagingV2,
    "messaging.v2.link_shortening.domain.certificate"
);

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV1LinkShorteningResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn domain(
        self,
        domain_sid: &'a str,
    ) -> BlockingMessagingV1LinkShorteningDomainResource<'a> {
        BlockingMessagingV1LinkShorteningDomainResource {
            account: self.account,
            domain_sid,
        }
    }

    #[must_use]
    pub fn messaging_service(
        self,
        messaging_service_sid: &'a str,
    ) -> BlockingMessagingV1LinkShorteningMessagingServiceResource<'a> {
        BlockingMessagingV1LinkShorteningMessagingServiceResource {
            account: self.account,
            messaging_service_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningDomainResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV1LinkShorteningDomainResource<'a> {
    #[must_use]
    pub fn certificate(self) -> BlockingMessagingV1LinkShorteningDomainCertificateResource<'a> {
        BlockingMessagingV1LinkShorteningDomainCertificateResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }

    #[must_use]
    pub fn config(self) -> BlockingMessagingV1LinkShorteningDomainConfigResource<'a> {
        BlockingMessagingV1LinkShorteningDomainConfigResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }

    #[must_use]
    pub fn messaging_service(
        self,
        messaging_service_sid: &'a str,
    ) -> BlockingMessagingV1LinkShorteningDomainMessagingServiceResource<'a> {
        BlockingMessagingV1LinkShorteningDomainMessagingServiceResource {
            account: self.account,
            domain_sid: self.domain_sid,
            messaging_service_sid,
        }
    }

    /// `GET /LinkShortening/Domains/{DomainSid}/ValidateDns`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn validate_dns(self) -> Result<TwilioLinkShorteningDnsValidation, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.domain_sid), None);
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::GET,
            ["LinkShortening", "Domains", self.domain_sid, "ValidateDns"],
        )
        .operation("messaging.v1.link_shortening.domain.validate_dns");
        let parsed: WireDnsValidation = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_validation())
    }

    /// `POST /LinkShortening/Domains/{DomainSid}/RequestManagedCert`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn request_managed_certificate(
        self,
    ) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.domain_sid), None);
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::POST,
            [
                "LinkShortening",
                "Domains",
                self.domain_sid,
                "RequestManagedCert",
            ],
        )
        .operation("messaging.v1.link_shortening.domain.request_managed_certificate");
        let parsed: WireDomainCertificate = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_certificate())
    }
}

macro_rules! impl_blocking_certificate_resource {
    ($resource:ident, $family:expr, $operation_prefix:literal) => {
        #[cfg(feature = "sync")]
        impl<'a> $resource<'a> {
            /// Fetch the Link Shortening domain certificate.
            ///
            /// # Errors
            ///
            /// Returns [`TwilioError`] for invalid requests, transport failures,
            /// non-2xx API responses, or malformed JSON responses.
            pub fn fetch(self) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
                validate_required("DomainSid", self.domain_sid)?;
                let sensitive_values =
                    sensitive_values(self.account.creds, Some(self.domain_sid), None);
                let spec = RequestSpec::new(
                    $family,
                    Method::GET,
                    ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
                )
                .operation(concat!($operation_prefix, ".fetch"));
                let parsed: WireDomainCertificate =
                    self.account.send_spec_json(spec, &sensitive_values)?;
                Ok(parsed.into_certificate())
            }
        }
    };
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningDomainCertificateResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
}

impl_blocking_certificate_resource!(
    BlockingMessagingV1LinkShorteningDomainCertificateResource,
    ApiFamily::MessagingV1,
    "messaging.v1.link_shortening.domain.certificate"
);

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV1LinkShorteningDomainCertificateResource<'a> {
    /// `POST /LinkShortening/Domains/{DomainSid}/Certificate`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn update(
        self,
        request: UpdateLinkShorteningDomainCertificateRequest<'a>,
    ) -> Result<TwilioLinkShorteningDomainCertificate, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        request.validate()?;
        let mut sensitive_values =
            sensitive_values(self.account.creds, Some(self.domain_sid), None);
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::POST,
            ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
        )
        .operation("messaging.v1.link_shortening.domain.certificate.update")
        .form_params(request.form_params());
        let parsed: WireDomainCertificate = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_certificate())
    }

    /// `DELETE /LinkShortening/Domains/{DomainSid}/Certificate`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub fn delete(self) -> Result<(), TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.domain_sid), None);
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::DELETE,
            ["LinkShortening", "Domains", self.domain_sid, "Certificate"],
        )
        .operation("messaging.v1.link_shortening.domain.certificate.delete");
        self.account.send_spec_empty(spec, &sensitive_values)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningDomainConfigResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV1LinkShorteningDomainConfigResource<'a> {
    /// `GET /LinkShortening/Domains/{DomainSid}/Config`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn fetch(self) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        let sensitive_values = sensitive_values(self.account.creds, Some(self.domain_sid), None);
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::GET,
            ["LinkShortening", "Domains", self.domain_sid, "Config"],
        )
        .operation("messaging.v1.link_shortening.domain.config.fetch");
        let parsed: WireDomainConfig = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_config())
    }

    /// `POST /LinkShortening/Domains/{DomainSid}/Config`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn update(
        self,
        request: UpdateLinkShorteningDomainConfigRequest<'a>,
    ) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        request.validate()?;
        let mut sensitive_values =
            sensitive_values(self.account.creds, Some(self.domain_sid), None);
        sensitive_values.extend(request.sensitive_values());
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::POST,
            ["LinkShortening", "Domains", self.domain_sid, "Config"],
        )
        .operation("messaging.v1.link_shortening.domain.config.update")
        .form_params(request.form_params());
        let parsed: WireDomainConfig = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_config())
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningDomainMessagingServiceResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
    messaging_service_sid: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingMessagingV1LinkShorteningDomainMessagingServiceResource<'_> {
    /// `POST /LinkShortening/Domains/{DomainSid}/MessagingServices/{MessagingServiceSid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn create(self) -> Result<TwilioLinkShorteningMessagingService, TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        validate_required("MessagingServiceSid", self.messaging_service_sid)?;
        let sensitive_values = sensitive_values(
            self.account.creds,
            Some(self.domain_sid),
            Some(self.messaging_service_sid),
        );
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::POST,
            [
                "LinkShortening",
                "Domains",
                self.domain_sid,
                "MessagingServices",
                self.messaging_service_sid,
            ],
        )
        .operation("messaging.v1.link_shortening.domain.messaging_service.create");
        let parsed: WireMessagingServiceAssociation =
            self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_association())
    }

    /// `DELETE /LinkShortening/Domains/{DomainSid}/MessagingServices/{MessagingServiceSid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures, or
    /// non-2xx API responses.
    pub fn delete(self) -> Result<(), TwilioError> {
        validate_required("DomainSid", self.domain_sid)?;
        validate_required("MessagingServiceSid", self.messaging_service_sid)?;
        let sensitive_values = sensitive_values(
            self.account.creds,
            Some(self.domain_sid),
            Some(self.messaging_service_sid),
        );
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::DELETE,
            [
                "LinkShortening",
                "Domains",
                self.domain_sid,
                "MessagingServices",
                self.messaging_service_sid,
            ],
        )
        .operation("messaging.v1.link_shortening.domain.messaging_service.delete");
        self.account.send_spec_empty(spec, &sensitive_values)
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1LinkShorteningMessagingServiceResource<'a> {
    account: BlockingTwilioAccount<'a>,
    messaging_service_sid: &'a str,
}

#[cfg(feature = "sync")]
impl BlockingMessagingV1LinkShorteningMessagingServiceResource<'_> {
    /// `GET /LinkShortening/MessagingService/{MessagingServiceSid}/DomainConfig`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn domain_config(self) -> Result<TwilioLinkShorteningDomainConfig, TwilioError> {
        validate_required("MessagingServiceSid", self.messaging_service_sid)?;
        let sensitive_values =
            sensitive_values(self.account.creds, None, Some(self.messaging_service_sid));
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::GET,
            [
                "LinkShortening",
                "MessagingService",
                self.messaging_service_sid,
                "DomainConfig",
            ],
        )
        .operation("messaging.v1.link_shortening.messaging_service.domain_config.fetch");
        let parsed: WireDomainConfig = self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_config())
    }

    /// `GET /LinkShortening/MessagingServices/{MessagingServiceSid}/Domain`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn domain(self) -> Result<TwilioLinkShorteningMessagingService, TwilioError> {
        validate_required("MessagingServiceSid", self.messaging_service_sid)?;
        let sensitive_values =
            sensitive_values(self.account.creds, None, Some(self.messaging_service_sid));
        let spec = RequestSpec::new(
            ApiFamily::MessagingV1,
            Method::GET,
            [
                "LinkShortening",
                "MessagingServices",
                self.messaging_service_sid,
                "Domain",
            ],
        )
        .operation("messaging.v1.link_shortening.messaging_service.domain.fetch");
        let parsed: WireMessagingServiceAssociation =
            self.account.send_spec_json(spec, &sensitive_values)?;
        Ok(parsed.into_association())
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2LinkShorteningResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2LinkShorteningResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    #[must_use]
    pub fn domain(
        self,
        domain_sid: &'a str,
    ) -> BlockingMessagingV2LinkShorteningDomainResource<'a> {
        BlockingMessagingV2LinkShorteningDomainResource {
            account: self.account,
            domain_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2LinkShorteningDomainResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2LinkShorteningDomainResource<'a> {
    #[must_use]
    pub fn certificate(self) -> BlockingMessagingV2LinkShorteningDomainCertificateResource<'a> {
        BlockingMessagingV2LinkShorteningDomainCertificateResource {
            account: self.account,
            domain_sid: self.domain_sid,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2LinkShorteningDomainCertificateResource<'a> {
    account: BlockingTwilioAccount<'a>,
    domain_sid: &'a str,
}

impl_blocking_certificate_resource!(
    BlockingMessagingV2LinkShorteningDomainCertificateResource,
    ApiFamily::MessagingV2,
    "messaging.v2.link_shortening.domain.certificate"
);
