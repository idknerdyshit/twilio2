#[cfg(feature = "async")]
use std::error::Error as _;
use std::fmt;
#[cfg(feature = "async")]
use std::future::Future;
#[cfg(feature = "async")]
use std::pin::Pin;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(any(feature = "async", feature = "sync"))]
use base64::Engine as _;
use http::Method;
use http::header::{
    AUTHORIZATION, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, HeaderMap, HeaderName,
    HeaderValue,
};
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::{Iso8601, Rfc2822};
use url::Url;
use zeroize::Zeroizing;

#[cfg(feature = "sensitive-diagnostics")]
use crate::diagnostics::SensitiveDiagnostics;
use crate::secret::Secret;

pub(crate) const REDACTED: &str = "<redacted>";
const MAX_DIAGNOSTIC_BODY_BYTES: usize = 2048;
pub(crate) const TRACE_TARGET: &str = "twilio2::trace";

/// Default Twilio 2010 REST API root, with no trailing slash.
pub const DEFAULT_REST_BASE_URL: &str = "https://api.twilio.com";

/// Default Twilio Messaging v1 API root, with no trailing slash.
pub const DEFAULT_MESSAGING_BASE_URL: &str = "https://messaging.twilio.com/v1";

/// Default page size used by `*_all` paginator helpers.
pub const DEFAULT_PAGE_SIZE: u32 = 50;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_USER_AGENT: &str = concat!("twilio2-rs/", env!("CARGO_PKG_VERSION"));

/// Error type for request validation, transport, API, decode, and response
/// metadata failures.
///
/// Diagnostic strings stored in these variants are sanitized first because these
/// errors commonly flow into application logs.
#[derive(Debug, thiserror::Error)]
pub enum TwilioError {
    #[error("invalid twilio base url: {0}")]
    InvalidBaseUrl(String),
    #[error("invalid twilio request: {0}")]
    InvalidRequest(String),
    #[error("invalid twilio response metadata: {0}")]
    InvalidResponseMetadata(String),
    #[error("http transport error: {0}")]
    Transport(String),
    /// Non-2xx response. `status` is the HTTP code; `body` is truncated
    /// diagnostic text and sanitized before being returned.
    #[error("twilio api error: status {status}")]
    Api { status: u16, body: String },
    #[error("malformed twilio response: {0}")]
    Decode(String),
}

impl TwilioError {
    pub(crate) fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api { status, .. } => matches!(*status, 408 | 425 | 429 | 500..=599),
            Self::InvalidBaseUrl(_)
            | Self::InvalidRequest(_)
            | Self::InvalidResponseMetadata(_)
            | Self::Decode(_) => false,
        }
    }

    pub(crate) fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            Self::InvalidBaseUrl(_)
            | Self::InvalidRequest(_)
            | Self::InvalidResponseMetadata(_)
            | Self::Transport(_)
            | Self::Decode(_) => None,
        }
    }

    pub(crate) fn error_kind(&self) -> &'static str {
        match self {
            Self::InvalidBaseUrl(_) => "invalid_base_url",
            Self::InvalidRequest(_) => "invalid_request",
            Self::InvalidResponseMetadata(_) => "invalid_response_metadata",
            Self::Transport(_) => "transport",
            Self::Api { .. } => "api",
            Self::Decode(_) => "decode",
        }
    }
}

/// Credentials for one account-scoped API handle. Owned by the caller and never
/// stored on [`crate::TwilioClient`].
///
/// Pass `&TwilioCreds` to [`crate::TwilioClient::account`] or, with the `sync`
/// feature, `BlockingTwilioClient::account`. The credential buffers are
/// redacted in [`Debug`](std::fmt::Debug) and the buffers owned by this value
/// are zeroized when dropped. Caller-owned source strings and transport-created
/// header copies are outside that guarantee.
#[derive(Clone)]
pub struct TwilioCreds {
    account_sid: Secret<String>,
    auth_token: Secret<String>,
}

impl TwilioCreds {
    /// Create credentials from an account SID and auth token.
    pub fn new(
        account_sid: impl Into<Secret<String>>,
        auth_token: impl Into<Secret<String>>,
    ) -> Self {
        Self {
            account_sid: account_sid.into(),
            auth_token: auth_token.into(),
        }
    }

    pub(crate) fn account_sid(&self) -> &str {
        self.account_sid.expose().as_str()
    }

    pub(crate) fn auth_token(&self) -> &str {
        self.auth_token.expose().as_str()
    }

    #[cfg(any(feature = "async", feature = "sync"))]
    pub(crate) fn basic_auth_header(&self) -> Secret<String> {
        let token = Zeroizing::new(format!("{}:{}", self.account_sid(), self.auth_token()));
        let mut header = String::from("Basic ");
        base64::engine::general_purpose::STANDARD.encode_string(token.as_bytes(), &mut header);
        Secret::new(header)
    }
}

impl std::fmt::Debug for TwilioCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioCreds")
            .field("account_sid", &self.account_sid)
            .field("auth_token", &self.auth_token)
            .finish()
    }
}

/// Base URL configuration for Twilio API families used by this crate.
#[derive(Clone, PartialEq, Eq)]
pub struct TwilioConfig {
    pub rest_base_url: String,
    pub messaging_base_url: String,
}

impl fmt::Debug for TwilioConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioConfig")
            .field("rest_base_url", &redacted_str(&self.rest_base_url))
            .field(
                "messaging_base_url",
                &redacted_str(&self.messaging_base_url),
            )
            .finish()
    }
}

impl Default for TwilioConfig {
    fn default() -> Self {
        Self {
            rest_base_url: DEFAULT_REST_BASE_URL.to_owned(),
            messaging_base_url: DEFAULT_MESSAGING_BASE_URL.to_owned(),
        }
    }
}

impl TwilioConfig {
    /// Construct default Twilio API base URLs.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct base URL configuration from environment variables.
    ///
    /// Reads `TWILIO_REST_BASE_URL` and `TWILIO_MESSAGING_BASE_URL` when they
    /// are present and non-empty. Account credentials are intentionally not read
    /// here; pass a [`TwilioCreds`] reference to [`crate::TwilioClient::account`].
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when an environment-provided base
    /// URL fails validation, or [`TwilioError::InvalidRequest`] when an
    /// environment variable is not valid Unicode.
    pub fn from_env() -> Result<Self, TwilioError> {
        Self::from_env_values(
            env_var("TWILIO_REST_BASE_URL")?,
            env_var("TWILIO_MESSAGING_BASE_URL")?,
        )
    }

    pub(crate) fn from_env_values(
        rest_base_url: Option<String>,
        messaging_base_url: Option<String>,
    ) -> Result<Self, TwilioError> {
        let mut config = Self::new();
        if let Some(rest_base_url) = non_empty_env(rest_base_url) {
            normalize_base_url(&rest_base_url).map_err(TwilioError::InvalidBaseUrl)?;
            config = config.rest_base_url(rest_base_url);
        }
        if let Some(messaging_base_url) = non_empty_env(messaging_base_url) {
            normalize_base_url(&messaging_base_url).map_err(TwilioError::InvalidBaseUrl)?;
            config = config.messaging_base_url(messaging_base_url);
        }
        Ok(config)
    }

    /// Set the Twilio 2010 REST API base URL.
    #[must_use]
    pub fn rest_base_url(mut self, value: impl Into<String>) -> Self {
        self.rest_base_url = value.into();
        self
    }

    /// Set the Twilio Messaging v1 API base URL.
    #[must_use]
    pub fn messaging_base_url(mut self, value: impl Into<String>) -> Self {
        self.messaging_base_url = value.into();
        self
    }
}

/// Full construction-time configuration for `twilio2` clients.
///
/// Clients consume this value when they build their underlying transport. If a
/// caller provides an already-built transport, only the base URLs are used;
/// timeout and user-agent settings cannot be applied to an existing transport.
#[derive(Clone)]
pub struct TwilioClientConfig {
    pub base_urls: TwilioConfig,
    pub timeout: Duration,
    pub user_agent: String,
    /// Explicitly sensitive request/response diagnostics sink.
    ///
    /// This field exists only with the `sensitive-diagnostics` feature. When
    /// set, it receives raw request/response material for every request made by
    /// clients built from this config, unless a request-level diagnostics
    /// override is supplied. The sink itself is redacted from [`Debug`].
    #[cfg(feature = "sensitive-diagnostics")]
    pub sensitive_diagnostics: Option<SensitiveDiagnostics>,
}

impl fmt::Debug for TwilioClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("TwilioClientConfig");
        debug
            .field("base_urls", &self.base_urls)
            .field("timeout", &self.timeout)
            .field("user_agent", &self.user_agent);
        #[cfg(feature = "sensitive-diagnostics")]
        debug.field(
            "sensitive_diagnostics",
            &redacted_optional(self.sensitive_diagnostics.is_some()),
        );
        debug.finish()
    }
}

impl Default for TwilioClientConfig {
    fn default() -> Self {
        Self {
            base_urls: TwilioConfig::default(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            #[cfg(feature = "sensitive-diagnostics")]
            sensitive_diagnostics: None,
        }
    }
}

impl TwilioClientConfig {
    /// Construct default client configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct client configuration from environment variables.
    ///
    /// Reads `TWILIO_REST_BASE_URL`, `TWILIO_MESSAGING_BASE_URL`,
    /// `TWILIO_TIMEOUT_SECS`, and `TWILIO_USER_AGENT`. Account credentials are
    /// intentionally not read into client configuration.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when environment values are invalid.
    pub fn from_env() -> Result<Self, TwilioError> {
        Self::from_env_values(
            env_var("TWILIO_REST_BASE_URL")?,
            env_var("TWILIO_MESSAGING_BASE_URL")?,
            env_var("TWILIO_TIMEOUT_SECS")?,
            env_var("TWILIO_USER_AGENT")?,
        )
    }

    pub(crate) fn from_env_values(
        rest_base_url: Option<String>,
        messaging_base_url: Option<String>,
        timeout_secs: Option<String>,
        user_agent: Option<String>,
    ) -> Result<Self, TwilioError> {
        let base_urls = TwilioConfig::from_env_values(rest_base_url, messaging_base_url)?;
        let mut config = Self::new().base_urls(base_urls);
        if let Some(timeout_secs) = non_empty_env(timeout_secs) {
            let timeout_secs = timeout_secs.parse::<u64>().map_err(|_| {
                TwilioError::InvalidRequest(
                    "TWILIO_TIMEOUT_SECS must be a positive integer".to_owned(),
                )
            })?;
            if timeout_secs == 0 {
                return Err(TwilioError::InvalidRequest(
                    "TWILIO_TIMEOUT_SECS must be a positive integer".to_owned(),
                ));
            }
            config = config.timeout(Duration::from_secs(timeout_secs));
        }
        if let Some(user_agent) = non_empty_env(user_agent) {
            config = config.user_agent(user_agent);
        }
        Ok(config)
    }

    /// Replace both Twilio base URLs.
    #[must_use]
    pub fn base_urls(mut self, base_urls: TwilioConfig) -> Self {
        self.base_urls = base_urls;
        self
    }

    /// Set the Twilio 2010 REST API base URL.
    #[must_use]
    pub fn rest_base_url(mut self, value: impl Into<String>) -> Self {
        self.base_urls = self.base_urls.rest_base_url(value);
        self
    }

    /// Set the Twilio Messaging v1 API base URL.
    #[must_use]
    pub fn messaging_base_url(mut self, value: impl Into<String>) -> Self {
        self.base_urls = self.base_urls.messaging_base_url(value);
        self
    }

    /// Set the timeout used when this config builds a new transport.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the user-agent used when this config builds a new transport.
    #[must_use]
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Attach a client-wide sensitive diagnostics sink.
    ///
    /// This is available only with the `sensitive-diagnostics` feature and can
    /// expose auth tokens, URLs, headers, request bodies, response bodies, and
    /// raw transport errors to the supplied sink. It is intended for local
    /// protocol debugging, not production logging.
    #[cfg(feature = "sensitive-diagnostics")]
    #[must_use]
    pub fn with_sensitive_diagnostics(mut self, diagnostics: SensitiveDiagnostics) -> Self {
        self.sensitive_diagnostics = Some(diagnostics);
        self
    }

    /// Remove any client-wide sensitive diagnostics sink.
    #[cfg(feature = "sensitive-diagnostics")]
    #[must_use]
    pub fn without_sensitive_diagnostics(mut self) -> Self {
        self.sensitive_diagnostics = None;
        self
    }
}

fn env_var(name: &'static str) -> Result<Option<String>, TwilioError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(TwilioError::InvalidRequest(format!(
            "{name} is not valid Unicode"
        ))),
    }
}

fn non_empty_env(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[derive(Clone)]
pub(crate) struct ParsedConfig {
    pub(crate) rest_base_url: Url,
    pub(crate) messaging_base_url: Url,
}

impl ParsedConfig {
    pub(crate) fn parse(config: &TwilioConfig) -> Result<Self, TwilioError> {
        Ok(Self {
            rest_base_url: normalize_base_url(&config.rest_base_url)
                .map_err(TwilioError::InvalidBaseUrl)?,
            messaging_base_url: normalize_base_url(&config.messaging_base_url)
                .map_err(TwilioError::InvalidBaseUrl)?,
        })
    }

    pub(crate) fn as_public_config(&self) -> TwilioConfig {
        TwilioConfig {
            rest_base_url: public_base_url(&self.rest_base_url),
            messaging_base_url: public_base_url(&self.messaging_base_url),
        }
    }
}

fn public_base_url(url: &Url) -> String {
    let mut out = url.as_str().to_owned();
    if out.ends_with('/') {
        out.pop();
    }
    out
}

#[derive(Clone)]
pub(crate) struct FormParam {
    key: &'static str,
    value: String,
}

pub(crate) fn push_str(params: &mut Vec<FormParam>, key: &'static str, value: Option<&str>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_owned(),
        });
    }
}

pub(crate) fn push_bool(params: &mut Vec<FormParam>, key: &'static str, value: Option<bool>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_string(),
        });
    }
}

pub(crate) fn push_u32(params: &mut Vec<FormParam>, key: &'static str, value: Option<u32>) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.to_string(),
        });
    }
}

pub(crate) trait FormEnum {
    fn form_value(self) -> &'static str;
}

pub(crate) fn push_enum<T: FormEnum + Copy>(
    params: &mut Vec<FormParam>,
    key: &'static str,
    value: Option<T>,
) {
    if let Some(value) = value {
        params.push(FormParam {
            key,
            value: value.form_value().to_owned(),
        });
    }
}

fn owned_form_pairs(params: Vec<FormParam>) -> Vec<(String, String)> {
    params
        .into_iter()
        .map(|param| (param.key.to_owned(), param.value))
        .collect()
}

/// Twilio API family used to resolve an operation's base URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFamily {
    /// Twilio 2010 REST API rooted at [`DEFAULT_REST_BASE_URL`].
    Rest,
    /// Twilio Messaging v1 API rooted at [`DEFAULT_MESSAGING_BASE_URL`].
    Messaging,
}

/// Retry policy for request-scoped safe-method retries.
///
/// Automatic retries are disabled by default. This crate only retries safe HTTP
/// methods (`GET`, `HEAD`, `OPTIONS`) in this pass; mutating requests with a
/// non-zero retry budget return [`TwilioError::InvalidRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::none()
    }
}

impl RetryPolicy {
    /// A policy that performs no retries.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(20),
            jitter: true,
        }
    }

    /// Set the maximum number of retries after the initial attempt.
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the base exponential-backoff delay.
    #[must_use]
    pub fn with_base_delay(mut self, base_delay: Duration) -> Self {
        self.base_delay = base_delay;
        self
    }

    /// Set the maximum delay between attempts.
    #[must_use]
    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Enable or disable simple jitter for exponential-backoff delays.
    #[must_use]
    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    pub(crate) fn should_retry(self, retries_done: u32, err: &TwilioError) -> bool {
        retries_done < self.max_retries && err.is_retryable()
    }

    pub(crate) fn delay_for(self, retries_done: u32, _err: &TwilioError) -> Duration {
        let factor = 2u32.saturating_pow(retries_done);
        let raw = self.base_delay.saturating_mul(factor).min(self.max_delay);
        if self.jitter {
            raw.mul_f64(jitter_fraction())
        } else {
            raw
        }
    }
}

fn jitter_fraction() -> f64 {
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos());
    let mut x = (nanos ^ COUNTER.fetch_add(1, Ordering::Relaxed)) | 1;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    f64::from(x % 1_000_000) / 1_000_000.0
}

/// Request-scoped transport options for [`crate::TwilioAccount::send_with_options`].
#[derive(Clone, Default)]
pub struct RequestOptions {
    pub(crate) rest_base_url: Option<String>,
    pub(crate) messaging_base_url: Option<String>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) retry: Option<RetryPolicy>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) query: Vec<(String, String)>,
    pub(crate) trace_label: Option<String>,
    #[cfg(feature = "sensitive-diagnostics")]
    pub(crate) sensitive_diagnostics: Option<SensitiveDiagnostics>,
    validation_error: Option<String>,
}

impl RequestOptions {
    /// Create empty request options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the REST API base URL for this request.
    #[must_use]
    pub fn rest_base_url(mut self, value: impl Into<String>) -> Self {
        self.rest_base_url = Some(value.into());
        self
    }

    /// Override the REST API base URL for this request, validating it now.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when the URL is not a valid
    /// HTTPS base URL accepted by this crate.
    pub fn try_rest_base_url(mut self, value: impl AsRef<str>) -> Result<Self, TwilioError> {
        let value = value.as_ref();
        normalize_base_url(value).map_err(TwilioError::InvalidBaseUrl)?;
        self.rest_base_url = Some(value.to_owned());
        Ok(self)
    }

    /// Override the Messaging API base URL for this request.
    #[must_use]
    pub fn messaging_base_url(mut self, value: impl Into<String>) -> Self {
        self.messaging_base_url = Some(value.into());
        self
    }

    /// Override the Messaging API base URL for this request, validating it now.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when the URL is not a valid
    /// HTTPS base URL accepted by this crate.
    pub fn try_messaging_base_url(mut self, value: impl AsRef<str>) -> Result<Self, TwilioError> {
        let value = value.as_ref();
        normalize_base_url(value).map_err(TwilioError::InvalidBaseUrl)?;
        self.messaging_base_url = Some(value.to_owned());
        Ok(self)
    }

    /// Set a per-request timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set a request-scoped retry policy.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Disable retries for this request.
    #[must_use]
    pub fn no_retry(self) -> Self {
        self.retry(RetryPolicy::none())
    }

    /// Add an extra request header.
    ///
    /// Invalid or blocked headers are recorded and rejected before the request
    /// is sent. Use [`try_header`](Self::try_header) when the caller needs the
    /// validation error immediately.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        let value = value.into();
        match validate_extra_header(&name, &value) {
            Ok(()) => self.headers.push((name, value)),
            Err(TwilioError::InvalidRequest(message)) => {
                self.validation_error = Some(message);
            }
            Err(err) => {
                self.validation_error = Some(err.to_string());
            }
        }
        self
    }

    /// Add an extra request header after validating its name/value.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidRequest`] for invalid or blocked headers.
    pub fn try_header(
        mut self,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) -> Result<Self, TwilioError> {
        let name = name.as_ref();
        let value = value.as_ref();
        validate_extra_header(name, value)?;
        self.headers.push((name.to_owned(), value.to_owned()));
        Ok(self)
    }

    /// Append an extra query parameter.
    #[must_use]
    pub fn query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.push((key.into(), value.into()));
        self
    }

    /// Attach a caller-provided safe correlation label to twilio2 tracing
    /// events.
    ///
    /// The label is emitted only by this crate's structured tracing
    /// instrumentation after it is checked against known sensitive request
    /// values. It is not sent over the wire and is redacted from
    /// [`Debug`](fmt::Debug). Pass an empty string to clear a previously set
    /// label. Only put values here that are already safe for your logs.
    #[must_use]
    pub fn trace_label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.trace_label = if label.is_empty() { None } else { Some(label) };
        self
    }

    /// Override the client-wide sensitive diagnostics sink for this request.
    ///
    /// This is available only with the `sensitive-diagnostics` feature and can
    /// expose auth tokens, URLs, headers, request bodies, response bodies, and
    /// raw transport errors to the supplied sink. Use
    /// [`SensitiveDiagnostics::noop`] to disable a client-wide diagnostics sink
    /// for one request.
    #[cfg(feature = "sensitive-diagnostics")]
    #[must_use]
    pub fn sensitive_diagnostics(mut self, diagnostics: SensitiveDiagnostics) -> Self {
        self.sensitive_diagnostics = Some(diagnostics);
        self
    }

    pub(crate) fn retry_or_default(&self) -> RetryPolicy {
        self.retry.unwrap_or_default()
    }

    pub(crate) fn validate(&self) -> Result<(), TwilioError> {
        if let Some(message) = &self.validation_error {
            return Err(TwilioError::InvalidRequest(message.clone()));
        }
        Ok(())
    }

    pub(crate) fn base_url_for(&self, family: ApiFamily) -> Result<Option<Url>, TwilioError> {
        match family {
            ApiFamily::Rest => self
                .rest_base_url
                .as_deref()
                .map(normalize_base_url)
                .transpose()
                .map_err(TwilioError::InvalidBaseUrl),
            ApiFamily::Messaging => self
                .messaging_base_url
                .as_deref()
                .map(normalize_base_url)
                .transpose()
                .map_err(TwilioError::InvalidBaseUrl),
        }
    }
}

impl fmt::Debug for RequestOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("RequestOptions");
        debug
            .field(
                "rest_base_url",
                &if self.rest_base_url.is_some() {
                    Some(REDACTED)
                } else {
                    None
                },
            )
            .field(
                "messaging_base_url",
                &if self.messaging_base_url.is_some() {
                    Some(REDACTED)
                } else {
                    None
                },
            )
            .field("timeout", &self.timeout)
            .field("retry", &self.retry)
            .field(
                "headers",
                &format_args!("[{REDACTED}; {}]", self.headers.len()),
            )
            .field("query", &format_args!("[{REDACTED}; {}]", self.query.len()))
            .field(
                "trace_label",
                &redacted_optional(self.trace_label.is_some()),
            )
            .field(
                "validation_error",
                &self.validation_error.as_ref().map(|_| REDACTED),
            );
        #[cfg(feature = "sensitive-diagnostics")]
        debug.field(
            "sensitive_diagnostics",
            &redacted_optional(self.sensitive_diagnostics.is_some()),
        );
        debug.finish()
    }
}

fn validate_extra_header(name: &str, value: &str) -> Result<(), TwilioError> {
    let parsed_name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| TwilioError::InvalidRequest(e.to_string()))?;
    HeaderValue::from_str(value).map_err(|e| TwilioError::InvalidRequest(e.to_string()))?;
    if blocked_header_name(&parsed_name) {
        return Err(TwilioError::InvalidRequest(format!(
            "header {name:?} cannot be overridden"
        )));
    }
    Ok(())
}

fn blocked_header_name(name: &HeaderName) -> bool {
    *name == AUTHORIZATION
        || name == CONTENT_TYPE
        || name == CONTENT_LENGTH
        || name == HOST
        || name == CONNECTION
        || matches!(
            name.as_str(),
            "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        )
}

#[derive(Clone)]
pub(crate) enum RequestTarget {
    Segments(Vec<String>),
    Url(Url),
}

#[derive(Clone)]
pub(crate) enum RequestBody {
    Empty,
    Form(Vec<(String, String)>),
}

/// A fully resolved Twilio request description consumed by the executor.
#[derive(Clone)]
pub struct RequestSpec {
    pub(crate) family: ApiFamily,
    pub(crate) method: Method,
    pub(crate) target: RequestTarget,
    pub(crate) query: Vec<(String, String)>,
    pub(crate) body: RequestBody,
    pub(crate) operation: &'static str,
    pub(crate) continuation: bool,
    pub(crate) accepted_statuses: Vec<u16>,
}

impl RequestSpec {
    /// Create a request from path segments relative to the configured API base.
    #[must_use]
    pub fn new(
        family: ApiFamily,
        method: Method,
        segments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            family,
            method,
            target: RequestTarget::Segments(segments.into_iter().map(Into::into).collect()),
            query: Vec::new(),
            body: RequestBody::Empty,
            operation: "custom",
            continuation: false,
            accepted_statuses: Vec::new(),
        }
    }

    pub(crate) fn from_url(
        family: ApiFamily,
        method: Method,
        url: Url,
        operation: &'static str,
    ) -> Self {
        Self {
            family,
            method,
            target: RequestTarget::Url(url),
            query: Vec::new(),
            body: RequestBody::Empty,
            operation,
            continuation: true,
            accepted_statuses: Vec::new(),
        }
    }

    /// Set the operation label used in tracing.
    #[must_use]
    pub fn operation(mut self, operation: &'static str) -> Self {
        self.operation = operation;
        self
    }

    /// Append a query parameter.
    #[must_use]
    pub fn query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.push((key.into(), value.into()));
        self
    }

    /// Append many query parameters.
    #[must_use]
    pub fn query_pairs(
        mut self,
        pairs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.query.extend(
            pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    /// Add one form parameter and make this request a form request.
    #[must_use]
    pub fn form_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self.body {
            RequestBody::Empty => {
                self.body = RequestBody::Form(vec![(key.into(), value.into())]);
            }
            RequestBody::Form(params) => params.push((key.into(), value.into())),
        }
        self
    }

    pub(crate) fn form_params(mut self, params: Vec<FormParam>) -> Self {
        self.body = RequestBody::Form(owned_form_pairs(params));
        self
    }

    pub(crate) fn is_safe_method(&self) -> bool {
        matches!(self.method, Method::GET | Method::HEAD | Method::OPTIONS)
    }

    pub(crate) fn is_continuation(&self) -> bool {
        self.continuation
    }

    pub(crate) fn accept_status(mut self, status: u16) -> Self {
        self.accepted_statuses.push(status);
        self
    }

    pub(crate) fn accepts_status(&self, status: u16) -> bool {
        self.accepted_statuses.contains(&status)
    }
}

impl fmt::Debug for RequestSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match &self.body {
            RequestBody::Empty => "empty".to_owned(),
            RequestBody::Form(params) => format!("form[{REDACTED}; {}]", params.len()),
        };
        f.debug_struct("RequestSpec")
            .field("family", &self.family)
            .field("method", &self.method)
            .field("target", &REDACTED)
            .field("query", &format_args!("[{REDACTED}; {}]", self.query.len()))
            .field("body", &body)
            .field("operation", &self.operation)
            .field("continuation", &self.continuation)
            .field("accepted_statuses", &self.accepted_statuses)
            .finish()
    }
}

/// Parsed metadata about a single HTTP response.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ResponseMeta {
    pub status: u16,
    pub retry_after: Option<Duration>,
}

impl ResponseMeta {
    pub(crate) fn from_headers(status: u16, headers: &HeaderMap) -> Self {
        Self {
            status,
            retry_after: retry_after(headers),
        }
    }
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Raw HTTP response data from a successful request.
#[derive(Clone)]
pub struct RawResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub(crate) fn new(status: u16, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }
}

impl fmt::Debug for RawResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawResponse")
            .field("status", &self.status)
            .field(
                "headers",
                &format_args!("[{REDACTED}; {}]", self.headers.len()),
            )
            .field(
                "body",
                &format_args!("[{REDACTED}; {} bytes]", self.body.len()),
            )
            .finish()
    }
}

/// Decoded output plus HTTP metadata and raw response bytes.
#[derive(Clone)]
pub struct ApiResponse<T> {
    pub output: T,
    pub meta: ResponseMeta,
    pub raw: RawResponse,
}

impl<T: fmt::Debug> fmt::Debug for ApiResponse<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiResponse")
            .field("output", &self.output)
            .field("meta", &self.meta)
            .field("raw", &self.raw)
            .finish()
    }
}

/// Decode a successful raw response body as JSON.
///
/// # Errors
///
/// Returns [`TwilioError::Decode`] when the body does not match `T`.
pub fn decode_json_response<T: serde::de::DeserializeOwned>(
    raw: &RawResponse,
    sensitive_values: &[&str],
) -> Result<T, TwilioError> {
    serde_json::from_slice(&raw.body).map_err(|e| {
        let message = sanitize_diagnostic(e.to_string(), sensitive_values);
        tracing::warn!(error = %message, "failed to decode twilio response");
        TwilioError::Decode(message)
    })
}

/// Public escape-hatch operation interface.
///
/// Resource methods use the same executor internally, but callers can implement
/// this trait for uncovered Twilio endpoints.
pub trait Operation {
    type Output;

    /// Resolve the request for this account SID. Do not include credentials in
    /// the returned request.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operation state.
    fn request(&self, account_sid: &str) -> Result<RequestSpec, TwilioError>;

    /// Sensitive non-credential values that should be redacted from diagnostics.
    fn sensitive_values(&self) -> Vec<String> {
        Vec::new()
    }

    /// Decode a successful raw response.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::Decode`] or [`TwilioError::InvalidResponseMetadata`]
    /// for invalid successful responses.
    fn decode(
        &self,
        raw: RawResponse,
        sensitive_values: &[&str],
    ) -> Result<Self::Output, TwilioError>;
}

pub(crate) fn owned_sensitive_values(operation_values: Vec<String>) -> Zeroizing<Vec<String>> {
    Zeroizing::new(operation_values)
}

#[cfg(feature = "async")]
pub(crate) type PageFuture<'a, P> = Pin<Box<dyn Future<Output = Result<P, TwilioError>> + 'a>>;
#[cfg(feature = "async")]
type FetchPage<'a, P> = Box<dyn FnMut(Option<String>) -> PageFuture<'a, P> + 'a>;

/// Lazy async paginator returned by `*_all` helpers.
#[cfg(feature = "async")]
pub struct TwilioPaginator<'a, P, T> {
    fetch: FetchPage<'a, P>,
    split: fn(P) -> (Vec<T>, Option<String>),
    next_cursor: Option<String>,
    first: bool,
    done: bool,
}

#[cfg(feature = "async")]
impl<'a, P, T> TwilioPaginator<'a, P, T> {
    pub(crate) fn new(
        fetch: impl FnMut(Option<String>) -> PageFuture<'a, P> + 'a,
        split: fn(P) -> (Vec<T>, Option<String>),
    ) -> Self {
        Self {
            fetch: Box::new(fetch),
            split,
            next_cursor: None,
            first: true,
            done: false,
        }
    }

    /// Fetch the next page of items, or `None` after exhaustion.
    pub async fn next_page(&mut self) -> Option<Result<Vec<T>, TwilioError>> {
        if self.done {
            return None;
        }
        let was_first = self.first;
        let cursor = if was_first {
            self.first = false;
            None
        } else {
            self.next_cursor.take()
        };
        if !was_first && cursor.is_none() {
            self.done = true;
            return None;
        }

        let page = (self.fetch)(cursor).await;
        match page {
            Ok(page) => {
                let (items, next_cursor) = (self.split)(page);
                self.next_cursor = next_cursor;
                if self.next_cursor.is_none() {
                    self.done = true;
                }
                Some(Ok(items))
            }
            Err(err) => {
                self.done = true;
                Some(Err(err))
            }
        }
    }

    /// Drain all remaining pages into one vector.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when fetching any page fails.
    pub async fn collect_all(mut self) -> Result<Vec<T>, TwilioError> {
        let mut out = Vec::new();
        while let Some(page) = self.next_page().await {
            out.extend(page?);
        }
        Ok(out)
    }

    /// Consume the paginator as an item stream.
    pub fn stream(self) -> impl futures_core::Stream<Item = Result<T, TwilioError>> + 'a
    where
        P: 'a,
        T: 'a,
    {
        async_stream::stream! {
            let mut paginator = self;
            while let Some(page) = paginator.next_page().await {
                match page {
                    Ok(items) => {
                        for item in items {
                            yield Ok(item);
                        }
                    }
                    Err(err) => {
                        yield Err(err);
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(feature = "async")]
impl<P, T> fmt::Debug for TwilioPaginator<'_, P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioPaginator")
            .field("first", &self.first)
            .field("done", &self.done)
            .field(
                "next_cursor",
                &if self.next_cursor.is_some() {
                    Some(REDACTED)
                } else {
                    None
                },
            )
            .finish()
    }
}

#[cfg(feature = "sync")]
type BlockingFetchPage<'a, P> = Box<dyn FnMut(Option<String>) -> Result<P, TwilioError> + 'a>;

/// Lazy blocking paginator returned by blocking `*_all` helpers.
#[cfg(feature = "sync")]
pub struct BlockingTwilioPaginator<'a, P, T> {
    fetch: BlockingFetchPage<'a, P>,
    split: fn(P) -> (Vec<T>, Option<String>),
    next_cursor: Option<String>,
    first: bool,
    done: bool,
}

#[cfg(feature = "sync")]
impl<'a, P, T> BlockingTwilioPaginator<'a, P, T> {
    pub(crate) fn new(
        fetch: impl FnMut(Option<String>) -> Result<P, TwilioError> + 'a,
        split: fn(P) -> (Vec<T>, Option<String>),
    ) -> Self {
        Self {
            fetch: Box::new(fetch),
            split,
            next_cursor: None,
            first: true,
            done: false,
        }
    }

    /// Fetch the next page of items, or `None` after exhaustion.
    pub fn next_page(&mut self) -> Option<Result<Vec<T>, TwilioError>> {
        if self.done {
            return None;
        }
        let was_first = self.first;
        let cursor = if was_first {
            self.first = false;
            None
        } else {
            self.next_cursor.take()
        };
        if !was_first && cursor.is_none() {
            self.done = true;
            return None;
        }

        let page = (self.fetch)(cursor);
        match page {
            Ok(page) => {
                let (items, next_cursor) = (self.split)(page);
                self.next_cursor = next_cursor;
                if self.next_cursor.is_none() {
                    self.done = true;
                }
                Some(Ok(items))
            }
            Err(err) => {
                self.done = true;
                Some(Err(err))
            }
        }
    }

    /// Drain all remaining pages into one vector.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when fetching any page fails.
    pub fn collect_all(mut self) -> Result<Vec<T>, TwilioError> {
        let mut out = Vec::new();
        while let Some(page) = self.next_page() {
            out.extend(page?);
        }
        Ok(out)
    }
}

#[cfg(feature = "sync")]
impl<P, T> Iterator for BlockingTwilioPaginator<'_, P, T> {
    type Item = Result<Vec<T>, TwilioError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_page()
    }
}

#[cfg(feature = "sync")]
impl<P, T> fmt::Debug for BlockingTwilioPaginator<'_, P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockingTwilioPaginator")
            .field("first", &self.first)
            .field("done", &self.done)
            .field(
                "next_cursor",
                &if self.next_cursor.is_some() {
                    Some(REDACTED)
                } else {
                    None
                },
            )
            .finish()
    }
}

pub(crate) fn push_sensitive<'a>(values: &mut Vec<&'a str>, value: Option<&'a str>) {
    if let Some(value) = value {
        values.push(value);
    }
}

pub(crate) fn has_non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

pub(crate) fn validate_page_size(page_size: Option<u32>) -> Result<(), TwilioError> {
    if let Some(page_size) = page_size {
        if !(1..=1000).contains(&page_size) {
            return Err(TwilioError::InvalidRequest(
                "PageSize must be in 1..=1000".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn parse_rfc2822(value: Option<String>) -> Option<OffsetDateTime> {
    value.and_then(|value| OffsetDateTime::parse(&value, &Rfc2822).ok())
}

pub(crate) fn parse_iso8601(value: Option<String>) -> Option<OffsetDateTime> {
    value.and_then(|value| OffsetDateTime::parse(&value, &Iso8601::DEFAULT).ok())
}

pub(crate) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

/// Raw bytes returned by the extensionless Media endpoint.
#[derive(Clone)]
pub struct TwilioMediaContent {
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for TwilioMediaContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMediaContent")
            .field("content_type", &self.content_type)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

pub(crate) fn api_error_from_text(
    status: u16,
    body: String,
    sensitive_values: &[&str],
) -> TwilioError {
    let body = sanitize_diagnostic(body, sensitive_values);
    tracing::warn!(
        http.status_code = status,
        response.body_len = body.len(),
        "twilio api error"
    );
    TwilioError::Api {
        status,
        body: truncate(body),
    }
}

pub(crate) struct LimitedResponseBody {
    pub(crate) body: Vec<u8>,
    pub(crate) complete: bool,
}

#[cfg(feature = "async")]
pub(crate) async fn read_limited_response_body(
    mut response: reqwest::Response,
) -> Result<LimitedResponseBody, reqwest::Error> {
    let mut body = Vec::new();
    while body.len() <= MAX_DIAGNOSTIC_BODY_BYTES {
        let Some(chunk) = response.chunk().await? else {
            return Ok(LimitedResponseBody {
                body,
                complete: true,
            });
        };
        let remaining = MAX_DIAGNOSTIC_BODY_BYTES + 1 - body.len();
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok(LimitedResponseBody {
                body,
                complete: false,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(LimitedResponseBody {
        body,
        complete: false,
    })
}

#[cfg(feature = "sync")]
pub(crate) fn read_limited_reader_body<R: std::io::Read>(
    reader: &mut R,
) -> Result<LimitedResponseBody, std::io::Error> {
    let mut body = Vec::new();
    while body.len() <= MAX_DIAGNOSTIC_BODY_BYTES {
        let mut chunk = [0_u8; 8192];
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(LimitedResponseBody {
                body,
                complete: true,
            });
        }
        let remaining = MAX_DIAGNOSTIC_BODY_BYTES + 1 - body.len();
        if read > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok(LimitedResponseBody {
                body,
                complete: false,
            });
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(LimitedResponseBody {
        body,
        complete: false,
    })
}

pub(crate) fn api_error_from_body(
    status: u16,
    body: &[u8],
    sensitive_values: &[&str],
) -> TwilioError {
    api_error_from_text(
        status,
        String::from_utf8_lossy(body).into_owned(),
        sensitive_values,
    )
}

#[cfg(feature = "async")]
pub(crate) fn api_error_from_body_read_error(
    status: u16,
    error: &reqwest::Error,
    sensitive_values: &[&str],
) -> TwilioError {
    api_error_from_text(status, reqwest_error_message(error), sensitive_values)
}

#[cfg(feature = "sync")]
pub(crate) fn api_error_from_read_error_message(
    status: u16,
    message: impl Into<String>,
    sensitive_values: &[&str],
) -> TwilioError {
    api_error_from_text(status, message.into(), sensitive_values)
}

#[cfg(feature = "async")]
pub(crate) fn transport_error(e: &reqwest::Error, sensitive_values: &[&str]) -> TwilioError {
    let message = sanitize_diagnostic(reqwest_error_message(e), sensitive_values);
    tracing::warn!(error = %message, "twilio transport error");
    TwilioError::Transport(message)
}

#[cfg(feature = "sync")]
pub(crate) fn transport_error_from_message(
    message: impl Into<String>,
    sensitive_values: &[&str],
) -> TwilioError {
    let message = sanitize_diagnostic(message.into(), sensitive_values);
    tracing::warn!(error = %message, "twilio transport error");
    TwilioError::Transport(message)
}

#[cfg(feature = "async")]
fn reqwest_error_message(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if let Some(status) = e.status() {
        if !msg.contains(status.as_str()) {
            msg = format!("status {status}: {msg}");
        }
    }
    if let Some(source) = e.source() {
        let source = source.to_string();
        if !source.is_empty() && !msg.contains(&source) {
            msg = format!("{msg}: {source}");
        }
    }
    msg
}

fn truncate(s: String) -> String {
    if s.len() > MAX_DIAGNOSTIC_BODY_BYTES {
        let mut end = MAX_DIAGNOSTIC_BODY_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut t = s;
        t.truncate(end);
        t.push('…');
        t
    } else {
        s
    }
}

macro_rules! trace_debug {
    ($trace_label:expr, $($field:tt)*) => {
        if let Some(trace_label) = $trace_label {
            tracing::debug!(target: TRACE_TARGET, trace_label, $($field)*);
        } else {
            tracing::debug!(target: TRACE_TARGET, $($field)*);
        }
    };
}

macro_rules! trace_warn {
    ($trace_label:expr, $($field:tt)*) => {
        if let Some(trace_label) = $trace_label {
            tracing::warn!(target: TRACE_TARGET, trace_label, $($field)*);
        } else {
            tracing::warn!(target: TRACE_TARGET, $($field)*);
        }
    };
}

pub(crate) struct OperationTrace {
    operation: &'static str,
    max_retries: u32,
    trace_label: Option<String>,
    start: Instant,
}

impl OperationTrace {
    pub(crate) fn new(
        operation: &'static str,
        max_retries: u32,
        trace_label: Option<&str>,
        sensitive_values: &[&str],
    ) -> Self {
        Self {
            operation,
            max_retries,
            trace_label: safe_trace_label(trace_label, sensitive_values).map(ToOwned::to_owned),
            start: Instant::now(),
        }
    }

    pub(crate) fn success(&self, method: &str, attempts: u32, status: u16) {
        trace_debug!(
            self.trace_label.as_deref(),
            event = "twilio2.operation.success",
            method,
            operation = self.operation,
            attempts,
            max_retries = self.max_retries,
            status,
            elapsed_ms = duration_ms(self.start.elapsed()),
        );
    }

    pub(crate) fn failure(
        &self,
        method: &str,
        attempts: u32,
        status: Option<u16>,
        error: &TwilioError,
    ) {
        let elapsed_ms = duration_ms(self.start.elapsed());
        let error_kind = error.error_kind();
        let status = status.or_else(|| error.status());
        match status {
            Some(status) => trace_warn!(
                self.trace_label.as_deref(),
                event = "twilio2.operation.failure",
                method,
                operation = self.operation,
                attempts,
                max_retries = self.max_retries,
                elapsed_ms,
                error_kind,
                status,
            ),
            None => trace_warn!(
                self.trace_label.as_deref(),
                event = "twilio2.operation.failure",
                method,
                operation = self.operation,
                attempts,
                max_retries = self.max_retries,
                elapsed_ms,
                error_kind,
            ),
        }
    }

    pub(crate) fn retry(
        &self,
        method: &str,
        attempt: u32,
        next_attempt: u32,
        delay: Duration,
        error: &TwilioError,
    ) {
        let delay_ms = duration_ms(delay);
        let delay_source = "backoff";
        let error_kind = error.error_kind();
        match error.status() {
            Some(status) => trace_warn!(
                self.trace_label.as_deref(),
                event = "twilio2.request.retry",
                method,
                operation = self.operation,
                attempt,
                next_attempt,
                max_retries = self.max_retries,
                delay_ms,
                delay_source,
                error_kind,
                status,
            ),
            None => trace_warn!(
                self.trace_label.as_deref(),
                event = "twilio2.request.retry",
                method,
                operation = self.operation,
                attempt,
                next_attempt,
                max_retries = self.max_retries,
                delay_ms,
                delay_source,
                error_kind,
            ),
        }
    }
}

pub(crate) struct AttemptTrace<'a> {
    method: &'a str,
    operation: &'static str,
    attempt: u32,
    max_retries: u32,
    trace_label: Option<&'a str>,
}

impl<'a> AttemptTrace<'a> {
    pub(crate) fn new(
        method: &'a str,
        operation: &'static str,
        attempt: u32,
        max_retries: u32,
        trace_label: Option<&'a str>,
        sensitive_values: &[&str],
    ) -> Self {
        Self {
            method,
            operation,
            attempt,
            max_retries,
            trace_label: safe_trace_label(trace_label, sensitive_values),
        }
    }
}

pub(crate) fn attempt_span(trace: &AttemptTrace<'_>) -> tracing::Span {
    if let Some(trace_label) = trace.trace_label {
        tracing::info_span!(
            target: TRACE_TARGET,
            "twilio2.request",
            method = trace.method,
            operation = trace.operation,
            attempt = trace.attempt,
            max_retries = trace.max_retries,
            trace_label,
            status = tracing::field::Empty,
            elapsed_ms = tracing::field::Empty,
        )
    } else {
        tracing::info_span!(
            target: TRACE_TARGET,
            "twilio2.request",
            method = trace.method,
            operation = trace.operation,
            attempt = trace.attempt,
            max_retries = trace.max_retries,
            status = tracing::field::Empty,
            elapsed_ms = tracing::field::Empty,
        )
    }
}

pub(crate) fn attempt_response(
    span: &tracing::Span,
    trace: &AttemptTrace<'_>,
    status: u16,
    elapsed: Duration,
) {
    let elapsed_ms = duration_ms(elapsed);
    span.record("status", status);
    span.record("elapsed_ms", elapsed_ms);
    trace_debug!(
        trace.trace_label,
        event = "twilio2.request.attempt.response",
        method = trace.method,
        operation = trace.operation,
        attempt = trace.attempt,
        max_retries = trace.max_retries,
        status,
        elapsed_ms,
    );
}

pub(crate) fn attempt_error(
    span: &tracing::Span,
    trace: &AttemptTrace<'_>,
    elapsed: Duration,
    error: &TwilioError,
) {
    let elapsed_ms = duration_ms(elapsed);
    span.record("elapsed_ms", elapsed_ms);
    trace_warn!(
        trace.trace_label,
        event = "twilio2.request.attempt.error",
        method = trace.method,
        operation = trace.operation,
        attempt = trace.attempt,
        max_retries = trace.max_retries,
        elapsed_ms,
        error_kind = error.error_kind(),
    );
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn safe_trace_label<'a>(label: Option<&'a str>, sensitive_values: &[&str]) -> Option<&'a str> {
    let label = label?;
    if sensitive_values
        .iter()
        .any(|value| !value.is_empty() && label.contains(value))
    {
        None
    } else {
        Some(label)
    }
}

pub(crate) fn request_span(
    _base_url: &Url,
    _operation: &'static str,
    _method: &str,
) -> tracing::Span {
    tracing::Span::none()
}

pub(crate) fn normalize_base_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("base URL is empty".to_owned());
    }

    let mut url = Url::parse(trimmed).map_err(|e| e.to_string())?;
    match url.scheme() {
        "https" => {}
        scheme => {
            return Err(format!("unsupported scheme {scheme:?}; expected https"));
        }
    }
    if url.host_str().is_none() {
        return Err("base URL must include a host".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("base URL must not include embedded credentials".to_owned());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("base URL must not include a query string or fragment".to_owned());
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

pub(crate) fn endpoint_url_from_base(
    base_url: &Url,
    segments: &[&str],
) -> Result<Url, TwilioError> {
    let mut url = base_url.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|()| TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned()))?;
        path.pop_if_empty();
        path.extend(segments);
    }
    Ok(url)
}

#[derive(Clone, Copy)]
pub(crate) enum LegacyPageResource<'a> {
    Messages,
    Media { message_sid: &'a str },
    ShortCodes,
}

#[derive(Clone, Copy)]
pub(crate) enum V1PageResource<'a> {
    Services,
    TollfreeVerifications,
    PhoneNumbers { service_sid: &'a str },
    ShortCodes { service_sid: &'a str },
    AlphaSenders { service_sid: &'a str },
    ChannelSenders { service_sid: &'a str },
    DestinationAlphaSenders { service_sid: &'a str },
}

impl V1PageResource<'_> {
    pub(crate) fn response_key(self) -> &'static str {
        match self {
            Self::Services => "services",
            Self::TollfreeVerifications => "verifications",
            Self::PhoneNumbers { .. } => "phone_numbers",
            Self::ShortCodes { .. } => "short_codes",
            Self::AlphaSenders { .. } | Self::DestinationAlphaSenders { .. } => "alpha_senders",
            Self::ChannelSenders { .. } => "senders",
        }
    }
}

pub(crate) fn legacy_page_uri_url_from_base(
    base_url: &Url,
    next_page_uri: &str,
    account_sid: &str,
    resource: LegacyPageResource<'_>,
) -> Result<Url, TwilioError> {
    let url = page_url_from_base(base_url, next_page_uri)?;
    validate_page_origin(base_url, &url, "next_page_uri")?;
    validate_legacy_page_uri(base_url, &url, account_sid, resource)?;
    Ok(url)
}

pub(crate) fn v1_page_url_from_base(
    base_url: &Url,
    next_page_url: &str,
    resource: V1PageResource<'_>,
) -> Result<Url, TwilioError> {
    let url = page_url_from_base(base_url, next_page_url)?;
    validate_page_origin(base_url, &url, "next_page_url")?;
    validate_v1_page_url(base_url, &url, resource)?;
    Ok(url)
}

fn page_url_from_base(base_url: &Url, page_url: &str) -> Result<Url, TwilioError> {
    if page_url.starts_with('/') && !page_url.starts_with("//") {
        let mut url = base_url.clone();
        let base_prefix: Vec<String> = base_url
            .path_segments()
            .map(|segments| {
                segments
                    .filter(|segment| !segment.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let path_and_query = page_url.strip_prefix('/').unwrap_or(page_url);
        if path_and_query.contains('#') {
            return Err(TwilioError::InvalidResponseMetadata(
                "page URL included a fragment".to_owned(),
            ));
        }
        let (path_part, query) = path_and_query
            .split_once('?')
            .map_or((path_and_query, None), |(path, query)| (path, Some(query)));
        {
            let mut path = url.path_segments_mut().map_err(|()| {
                TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned())
            })?;
            path.clear();
            path.extend(base_prefix.iter().map(String::as_str));
            path.extend(path_part.split('/').filter(|segment| !segment.is_empty()));
        }
        url.set_query(query);
        url.set_fragment(None);
        Ok(url)
    } else {
        base_url
            .join(page_url)
            .map_err(|e| TwilioError::InvalidResponseMetadata(e.to_string()))
    }
}

fn validate_page_origin(
    base_url: &Url,
    page_url: &Url,
    label: &'static str,
) -> Result<(), TwilioError> {
    if page_url.scheme() != base_url.scheme()
        || page_url.host_str() != base_url.host_str()
        || page_url.port_or_known_default() != base_url.port_or_known_default()
    {
        return Err(TwilioError::InvalidResponseMetadata(format!(
            "{label} changed API origin"
        )));
    }
    if !page_url.username().is_empty() || page_url.password().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(format!(
            "{label} included embedded credentials"
        )));
    }
    if page_url.fragment().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(format!(
            "{label} included a fragment"
        )));
    }
    Ok(())
}

fn api_segments(base_url: &Url, page_url: &Url) -> Result<Vec<String>, TwilioError> {
    let base_prefix: Vec<String> = base_url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let page_segments: Vec<String> = page_url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let Some(api_segments) = page_segments.strip_prefix(base_prefix.as_slice()) else {
        return Err(TwilioError::InvalidResponseMetadata(
            "page URL left the configured API base path".to_owned(),
        ));
    };
    Ok(api_segments.to_vec())
}

fn validate_legacy_page_uri(
    base_url: &Url,
    page_url: &Url,
    account_sid: &str,
    resource: LegacyPageResource<'_>,
) -> Result<(), TwilioError> {
    let segments = api_segments(base_url, page_url)?;
    match resource {
        LegacyPageResource::Messages => {
            if segments.as_slice() != ["2010-04-01", "Accounts", account_sid, "Messages.json"] {
                return Err(TwilioError::InvalidResponseMetadata(
                    "next_page_uri is not a Messages page for this account".to_owned(),
                ));
            }
        }
        LegacyPageResource::Media { message_sid } => {
            if segments.len() != 6
                || segments[0] != "2010-04-01"
                || segments[1] != "Accounts"
                || segments[2] != account_sid
                || segments[3] != "Messages"
                || segments[4] != message_sid
                || segments[5] != "Media.json"
            {
                return Err(TwilioError::InvalidResponseMetadata(
                    "next_page_uri is not a Media page for this message".to_owned(),
                ));
            }
        }
        LegacyPageResource::ShortCodes => {
            if segments.as_slice()
                != [
                    "2010-04-01",
                    "Accounts",
                    account_sid,
                    "SMS",
                    "ShortCodes.json",
                ]
            {
                return Err(TwilioError::InvalidResponseMetadata(
                    "next_page_uri is not a ShortCodes page for this account".to_owned(),
                ));
            }
        }
    }
    validate_page_query_keys(
        page_url,
        |key| allowed_legacy_page_query_key(key, resource),
        |_key| false,
    )?;
    Ok(())
}

fn validate_v1_page_url(
    base_url: &Url,
    page_url: &Url,
    resource: V1PageResource<'_>,
) -> Result<(), TwilioError> {
    let segments = api_segments(base_url, page_url)?;
    let expected: Vec<&str> = match resource {
        V1PageResource::Services => vec!["Services"],
        V1PageResource::TollfreeVerifications => vec!["Tollfree", "Verifications"],
        V1PageResource::PhoneNumbers { service_sid } => {
            vec!["Services", service_sid, "PhoneNumbers"]
        }
        V1PageResource::ShortCodes { service_sid } => vec!["Services", service_sid, "ShortCodes"],
        V1PageResource::AlphaSenders { service_sid } => {
            vec!["Services", service_sid, "AlphaSenders"]
        }
        V1PageResource::ChannelSenders { service_sid } => {
            vec!["Services", service_sid, "ChannelSenders"]
        }
        V1PageResource::DestinationAlphaSenders { service_sid } => {
            vec!["Services", service_sid, "DestinationAlphaSenders"]
        }
    };
    if segments.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_url is not a page for this resource".to_owned(),
        ));
    }
    validate_page_query_keys(
        page_url,
        |key| allowed_v1_page_query_key(key, resource),
        |key| duplicate_v1_page_query_key_allowed(key, resource),
    )?;
    Ok(())
}

fn validate_page_query_keys<F, G>(
    page_url: &Url,
    mut allowed: F,
    mut duplicate_allowed: G,
) -> Result<(), TwilioError>
where
    F: FnMut(&str) -> bool,
    G: FnMut(&str) -> bool,
{
    let mut seen = Vec::new();
    for (key, _) in page_url.query_pairs() {
        if !allowed(key.as_ref()) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "page URL has unsupported query parameter {key:?}"
            )));
        }
        if !duplicate_allowed(key.as_ref())
            && seen.iter().any(|candidate| candidate == key.as_ref())
        {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "page URL repeated query parameter {key:?}"
            )));
        }
        seen.push(key.into_owned());
    }
    Ok(())
}

fn allowed_legacy_page_query_key(key: &str, resource: LegacyPageResource<'_>) -> bool {
    match resource {
        LegacyPageResource::Messages => matches!(
            key,
            "To" | "From"
                | "DateSent"
                | "DateSent<"
                | "DateSent>"
                | "PageSize"
                | "Page"
                | "PageToken"
        ),
        LegacyPageResource::Media { .. } => matches!(
            key,
            "DateCreated" | "DateCreated<" | "DateCreated>" | "PageSize" | "Page" | "PageToken"
        ),
        LegacyPageResource::ShortCodes => {
            matches!(
                key,
                "FriendlyName" | "ShortCode" | "PageSize" | "Page" | "PageToken"
            )
        }
    }
}

fn allowed_v1_page_query_key(key: &str, resource: V1PageResource<'_>) -> bool {
    matches!(key, "PageSize" | "Page" | "PageToken")
        || matches!(resource, V1PageResource::DestinationAlphaSenders { .. })
            && key == "IsoCountryCode"
        || matches!(resource, V1PageResource::TollfreeVerifications)
            && matches!(
                key,
                "TollfreePhoneNumberSid"
                    | "Status"
                    | "ExternalReferenceId"
                    | "IncludeSubAccounts"
                    | "TrustProductSid"
            )
}

fn duplicate_v1_page_query_key_allowed(key: &str, resource: V1PageResource<'_>) -> bool {
    matches!(resource, V1PageResource::TollfreeVerifications) && key == "TrustProductSid"
}

pub(crate) fn validate_legacy_next_page_continuation(
    current_url: &Url,
    next_url: &Url,
    resource: LegacyPageResource<'_>,
) -> Result<(), TwilioError> {
    if current_url.path() != next_url.path() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri changed resource path".to_owned(),
        ));
    }
    for key in legacy_stable_page_query_keys(resource) {
        if query_values(current_url, key) != query_values(next_url, key) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri changed {key} query parameter"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_v1_next_page_continuation(
    current_url: &Url,
    next_url: &Url,
    resource: V1PageResource<'_>,
) -> Result<(), TwilioError> {
    if current_url.path() != next_url.path() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_url changed resource path".to_owned(),
        ));
    }
    for key in v1_stable_page_query_keys(resource) {
        if query_values(current_url, key) != query_values(next_url, key) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_url changed {key} query parameter"
            )));
        }
    }
    Ok(())
}

fn legacy_stable_page_query_keys(resource: LegacyPageResource<'_>) -> &'static [&'static str] {
    match resource {
        LegacyPageResource::Messages => &[
            "To",
            "From",
            "DateSent",
            "DateSent<",
            "DateSent>",
            "PageSize",
        ],
        LegacyPageResource::Media { .. } => {
            &["DateCreated", "DateCreated<", "DateCreated>", "PageSize"]
        }
        LegacyPageResource::ShortCodes => &["FriendlyName", "ShortCode", "PageSize"],
    }
}

fn v1_stable_page_query_keys(resource: V1PageResource<'_>) -> &'static [&'static str] {
    match resource {
        V1PageResource::DestinationAlphaSenders { .. } => &["IsoCountryCode", "PageSize"],
        V1PageResource::TollfreeVerifications => &[
            "TollfreePhoneNumberSid",
            "Status",
            "ExternalReferenceId",
            "IncludeSubAccounts",
            "TrustProductSid",
            "PageSize",
        ],
        _ => &["PageSize"],
    }
}

fn query_values(url: &Url, key: &str) -> Vec<String> {
    url.query_pairs()
        .filter_map(|(candidate, value)| {
            if candidate == key {
                Some(value.into_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Messaging v1 pagination metadata.
#[derive(Clone)]
pub struct V1PageMeta {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub first_page_url: Option<String>,
    pub previous_page_url: Option<String>,
    pub next_page_url: Option<String>,
    pub key: Option<String>,
    pub url: Option<String>,
}

impl std::fmt::Debug for V1PageMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V1PageMeta")
            .field("page", &self.page)
            .field("page_size", &self.page_size)
            .field("first_page_url", &redacted_option(&self.first_page_url))
            .field(
                "previous_page_url",
                &redacted_option(&self.previous_page_url),
            )
            .field("next_page_url", &redacted_option(&self.next_page_url))
            .field("key", &self.key)
            .field("url", &redacted_option(&self.url))
            .finish()
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct WireV1PageMeta {
    #[serde(default)]
    pub(crate) page: Option<i64>,
    #[serde(default)]
    pub(crate) page_size: Option<i64>,
    #[serde(default)]
    pub(crate) first_page_url: Option<String>,
    #[serde(default)]
    pub(crate) previous_page_url: Option<String>,
    #[serde(default)]
    pub(crate) next_page_url: Option<String>,
    #[serde(default)]
    pub(crate) key: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
}

impl WireV1PageMeta {
    pub(crate) fn into_meta(self) -> V1PageMeta {
        V1PageMeta {
            page: self.page,
            page_size: self.page_size,
            first_page_url: non_empty(self.first_page_url),
            previous_page_url: non_empty(self.previous_page_url),
            next_page_url: non_empty(self.next_page_url),
            key: non_empty(self.key),
            url: non_empty(self.url),
        }
    }
}

pub(crate) fn validate_v1_meta_key(
    meta: &V1PageMeta,
    resource: V1PageResource<'_>,
) -> Result<(), TwilioError> {
    if let Some(key) = meta.key.as_deref() {
        if key != resource.response_key() {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "page meta key {key:?} does not match expected key {:?}",
                resource.response_key()
            )));
        }
    }
    Ok(())
}

pub(crate) fn redacted_str(value: &str) -> &str {
    if value.is_empty() { "" } else { REDACTED }
}

#[allow(
    clippy::ref_option,
    reason = "Debug impls pass struct fields directly; Option<&str> would move the noise to every call site."
)]
pub(crate) fn redacted_option(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(redacted_str)
}

pub(crate) fn redacted_optional(is_some: bool) -> impl fmt::Debug {
    struct RedactedOptional(bool);
    impl fmt::Debug for RedactedOptional {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if self.0 {
                f.write_str("Some(<redacted>)")
            } else {
                f.write_str("None")
            }
        }
    }
    RedactedOptional(is_some)
}

pub(crate) fn sanitize_diagnostic(s: String, sensitive_values: &[&str]) -> String {
    let s = redact_known_values(s, sensitive_values);
    let s = redact_sensitive_key_values(&s);
    let s = redact_authorization_schemes(&s);
    redact_urls(&s)
}

fn redact_known_values(mut s: String, sensitive_values: &[&str]) -> String {
    for value in sensitive_values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        s = s.replace(value, REDACTED);
    }
    s
}

fn redact_sensitive_key_values(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if is_key_start(ch) || ch == '"' {
            let quoted_key = ch == '"';
            let key_start = if quoted_key { i + ch.len_utf8() } else { i };
            let Some(first_key_ch) = s[key_start..].chars().next() else {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            };
            if !is_key_start(first_key_ch) {
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }

            let mut key_end = key_start + first_key_ch.len_utf8();
            while key_end < s.len() {
                let Some(ch) = s[key_end..].chars().next() else {
                    break;
                };
                if !is_key_continue(ch) {
                    break;
                }
                key_end += ch.len_utf8();
            }

            let mut cursor = if quoted_key {
                if !s[key_end..].starts_with('"') {
                    out.push(ch);
                    i += ch.len_utf8();
                    continue;
                }
                key_end + '"'.len_utf8()
            } else {
                key_end
            };
            while cursor < s.len() {
                let Some(ch) = s[cursor..].chars().next() else {
                    break;
                };
                if !ch.is_ascii_whitespace() {
                    break;
                }
                cursor += ch.len_utf8();
            }

            let sep = s[cursor..].chars().next();
            if matches!(sep, Some('=' | ':')) && is_sensitive_key(&s[key_start..key_end]) {
                let sensitive_key = normalized_key(&s[key_start..key_end]);
                cursor += sep.map_or(0, char::len_utf8);
                while cursor < s.len() {
                    let Some(ch) = s[cursor..].chars().next() else {
                        break;
                    };
                    if !ch.is_ascii_whitespace() {
                        break;
                    }
                    cursor += ch.len_utf8();
                }

                out.push_str(&s[i..cursor]);
                let value_end = if sensitive_key == "authorization" {
                    if s[cursor..].starts_with('"') {
                        out.push('"');
                        let (end, had_quote) = quoted_value_end(s, cursor + 1);
                        out.push_str(REDACTED);
                        if had_quote {
                            out.push('"');
                            end + 1
                        } else {
                            end
                        }
                    } else {
                        out.push_str(REDACTED);
                        authorization_value_end(s, cursor)
                    }
                } else if s[cursor..].starts_with('"') {
                    out.push('"');
                    let (end, had_quote) = quoted_value_end(s, cursor + 1);
                    out.push_str(REDACTED);
                    if had_quote {
                        out.push('"');
                        end + 1
                    } else {
                        end
                    }
                } else {
                    out.push_str(REDACTED);
                    delimited_value_end(s, cursor)
                };
                i = value_end;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_authorization_schemes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some(scheme_len) = auth_scheme_at(s, i) {
            out.push_str(&s[i..i + scheme_len]);
            let value_start = i + scheme_len;
            let value_end = delimited_value_end(s, value_start);
            if value_end > value_start {
                out.push_str(REDACTED);
            }
            i = value_end;
            continue;
        }
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if url_scheme_at(s, i).is_some() {
            out.push_str(REDACTED);
            i = url_value_end(s, i);
            continue;
        }
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn url_scheme_at(s: &str, i: usize) -> Option<usize> {
    const HTTP: &str = "http://";
    const HTTPS: &str = "https://";
    if !is_url_scheme_boundary(s, i) {
        return None;
    }
    let rest = &s[i..];
    if rest.len() >= HTTPS.len() && rest[..HTTPS.len()].eq_ignore_ascii_case(HTTPS) {
        Some(HTTPS.len())
    } else if rest.len() >= HTTP.len() && rest[..HTTP.len()].eq_ignore_ascii_case(HTTP) {
        Some(HTTP.len())
    } else {
        None
    }
}

fn is_url_scheme_boundary(s: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    s[..i]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

fn url_value_end(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if ch.is_ascii_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']' | '}') {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        normalized_key(key).as_str(),
        "authorization"
            | "authtoken"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "token"
            | "password"
            | "passwd"
            | "secret"
            | "apikey"
            | "key"
            | "sid"
            | "accountsid"
            | "messagesid"
            | "message_sid"
            | "mediasid"
            | "parentsid"
            | "servicesid"
            | "messagingservicesid"
            | "phonenumbersid"
            | "shortcodesid"
            | "body"
            | "to"
            | "from"
            | "fallbackfrom"
            | "url"
            | "uri"
            | "mediaurl"
            | "mediaurls"
            | "persistentaction"
            | "persistentactions"
            | "contentvariables"
            | "contentvariablesjson"
            | "tags"
            | "tagsjson"
            | "statuscallback"
            | "callbackurl"
            | "inboundrequesturl"
            | "fallbackurl"
            | "sender"
            | "alphasender"
            | "friendlyname"
            | "phone_number"
            | "phonenumber"
            | "shortcode"
            | "subresourceuris"
            | "links"
            | "nextpageuri"
            | "firstpageuri"
            | "previouspageuri"
            | "nextpageurl"
            | "firstpageurl"
            | "previouspageurl"
    )
}

fn normalized_key(key: &str) -> String {
    key.to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn is_key_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_key_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn quoted_value_end(s: &str, mut i: usize) -> (usize, bool) {
    let mut escaped = false;
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
            i += ch.len_utf8();
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                i += ch.len_utf8();
            }
            '"' => return (i, true),
            _ => i += ch.len_utf8(),
        }
    }
    (i, false)
}

fn delimited_value_end(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let Some(ch) = s[i..].chars().next() else {
            break;
        };
        if ch.is_ascii_whitespace() || ch == '&' {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn authorization_value_end(s: &str, i: usize) -> usize {
    if let Some(scheme_len) = auth_scheme_at(s, i) {
        return delimited_value_end(s, i + scheme_len);
    }
    delimited_value_end(s, i)
}

fn auth_scheme_at(s: &str, i: usize) -> Option<usize> {
    const BASIC: &str = "basic ";
    const BEARER: &str = "bearer ";
    if !is_auth_scheme_boundary(s, i) {
        return None;
    }
    let rest = &s[i..];
    if rest.len() >= BASIC.len() && rest[..BASIC.len()].eq_ignore_ascii_case(BASIC) {
        Some(BASIC.len())
    } else if rest.len() >= BEARER.len() && rest[..BEARER.len()].eq_ignore_ascii_case(BEARER) {
        Some(BEARER.len())
    } else {
        None
    }
}

fn is_auth_scheme_boundary(s: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    s[..i]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "sensitive-diagnostics")]
    use crate::diagnostics::SensitiveDiagnostics;

    use super::{
        LegacyPageResource, RequestOptions, RetryPolicy, TwilioClientConfig, TwilioConfig,
        TwilioCreds, TwilioError, V1PageResource, endpoint_url_from_base,
        legacy_page_uri_url_from_base, normalize_base_url, sanitize_diagnostic,
        v1_page_url_from_base, validate_legacy_next_page_continuation,
        validate_v1_next_page_continuation,
    };
    use std::time::Duration;

    #[test]
    fn normalizes_and_rejects_base_urls() {
        let url = normalize_base_url(" https://api.twilio.com/ ")
            .expect("default REST base URL should normalize");
        assert_eq!(url.as_str(), "https://api.twilio.com/");

        let url = normalize_base_url("https://api.twilio.com/proxy")
            .expect("proxy REST base URL should normalize");
        assert_eq!(url.as_str(), "https://api.twilio.com/proxy/");

        for bad in [
            "",
            "http://api.twilio.com",
            "file:///tmp/twilio",
            "https://sid:token@api.twilio.com",
            "https://api.twilio.com?x=1",
            "https://api.twilio.com#frag",
        ] {
            assert!(
                normalize_base_url(bad).is_err(),
                "accepted bad base URL {bad:?}"
            );
        }
    }

    #[test]
    fn env_value_helpers_build_base_and_client_config() {
        let base = TwilioConfig::from_env_values(
            Some("https://proxy.example.test/rest".to_owned()),
            Some("https://proxy.example.test/messaging/v1".to_owned()),
        )
        .expect("valid env base URLs should parse");
        assert_eq!(base.rest_base_url, "https://proxy.example.test/rest");
        assert_eq!(
            base.messaging_base_url,
            "https://proxy.example.test/messaging/v1"
        );

        let config = TwilioClientConfig::from_env_values(
            Some("https://proxy.example.test/rest".to_owned()),
            Some("https://proxy.example.test/messaging/v1".to_owned()),
            Some("9".to_owned()),
            Some("test-agent/2.0".to_owned()),
        )
        .expect("valid client env config should parse");
        assert_eq!(config.timeout, Duration::from_secs(9));
        assert_eq!(config.user_agent, "test-agent/2.0");
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("proxy.example.test"));
        assert!(rendered.contains("<redacted>"));

        let err = TwilioClientConfig::from_env_values(None, None, Some("0".to_owned()), None)
            .expect_err("zero timeout should fail");
        assert!(matches!(err, TwilioError::InvalidRequest(_)));
    }

    #[test]
    fn twilio_creds_debug_redacts_owned_secrets() {
        let creds = TwilioCreds::new("ACdebug-secret", "auth-token-secret");
        let rendered = format!("{creds:?}");

        assert!(!rendered.contains("ACdebug-secret"));
        assert!(!rendered.contains("auth-token-secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn twilio_creds_accepts_secret_values() {
        let creds = TwilioCreds::new(
            crate::Secret::from("ACdirect-secret"),
            crate::Secret::new("direct-auth-token-secret".to_owned()),
        );
        let rendered = format!("{creds:?}");

        assert!(!rendered.contains("ACdirect-secret"));
        assert!(!rendered.contains("direct-auth-token-secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn request_options_validate_and_redact_sensitive_transport_overrides() {
        let options = RequestOptions::new()
            .try_rest_base_url("https://proxy.example.test/rest")
            .expect("valid override")
            .query("token", "secret")
            .header("x-token", "secret")
            .retry(RetryPolicy::none().with_max_retries(1));
        let rendered = format!("{options:?}");
        assert!(!rendered.contains("proxy.example.test"));
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("<redacted>"));

        let err = RequestOptions::new()
            .try_header("content-type", "application/json")
            .expect_err("content-type override should be rejected");
        assert!(matches!(err, TwilioError::InvalidRequest(_)));
    }

    #[test]
    fn request_options_trace_label_empty_clears_and_debug_redacts() {
        let options = RequestOptions::new().trace_label("trace-secret");
        assert_eq!(options.trace_label.as_deref(), Some("trace-secret"));
        let rendered = format!("{options:?}");
        assert!(!rendered.contains("trace-secret"));
        assert!(rendered.contains("<redacted>"));

        let cleared = options.trace_label("");
        assert_eq!(cleared.trace_label, None);
        assert!(!format!("{cleared:?}").contains("trace-secret"));
    }

    #[cfg(feature = "sensitive-diagnostics")]
    #[test]
    fn sensitive_diagnostics_config_and_options_debug_are_redacted() {
        let config =
            TwilioClientConfig::new().with_sensitive_diagnostics(SensitiveDiagnostics::noop());
        let rendered = format!("{config:?}");
        assert!(rendered.contains("sensitive_diagnostics"));
        assert!(rendered.contains("<redacted>"));

        let options = RequestOptions::new().sensitive_diagnostics(SensitiveDiagnostics::noop());
        let rendered = format!("{options:?}");
        assert!(rendered.contains("sensitive_diagnostics"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn url_join_and_legacy_page_uri_validation_cover_messages_and_media() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy")
            .expect("proxy REST base URL should normalize");
        assert_eq!(
            endpoint_url_from_base(
                &base_url,
                &[
                    "2010-04-01",
                    "Accounts",
                    "AC/123",
                    "Messages",
                    "SM 123.json"
                ],
            )
            .expect("endpoint URL should join")
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC%2F123/Messages/SM%20123.json"
        );
        assert_eq!(
            legacy_page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?DateSent%3E=2026-07-01&Page=1",
                "AC123",
                LegacyPageResource::Messages,
            )
            .expect("valid messages page URI should pass")
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages.json?DateSent%3E=2026-07-01&Page=1"
        );
        assert_eq!(
            legacy_page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated%3C=2026-07-01&Page=1",
                "AC123",
                LegacyPageResource::Media {
                    message_sid: "SM123"
                },
            )
            .expect("valid media page URI should pass")
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages/SM123/Media.json?DateCreated%3C=2026-07-01&Page=1"
        );
        assert_eq!(
            legacy_page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&ShortCode=12345&PageSize=50&Page=1",
                "AC123",
                LegacyPageResource::ShortCodes,
            )
            .expect("valid account ShortCodes page URI should pass")
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&ShortCode=12345&PageSize=50&Page=1"
        );

        for bad in [
            "https://example.test/2010-04-01/Accounts/AC123/Messages.json",
            "/2010-04-01/Accounts/AC123/Calls.json?Page=1",
            "/2010-04-01/Accounts/AC999/Messages.json?Page=1",
            "/2010-04-01/Accounts/AC123/Messages.json?Unexpected=1",
            "https://user:pass@api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1",
            "https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1#frag",
            "/2010-04-01/Accounts/AC123/Messages.json?Page=1&Page=2",
        ] {
            assert!(
                legacy_page_uri_url_from_base(
                    &base_url,
                    bad,
                    "AC123",
                    LegacyPageResource::Messages
                )
                .is_err(),
                "accepted bad uri {bad}"
            );
        }
        assert!(
            legacy_page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/SMS/ShortCodes.json?FriendlyName=Alerts&FriendlyName=Other&Page=1",
                "AC123",
                LegacyPageResource::ShortCodes,
            )
            .is_err()
        );
    }

    #[test]
    fn v1_page_url_validation_covers_services_and_subresources() {
        let base_url = normalize_base_url("https://messaging.twilio.com/v1/proxy")
            .expect("proxy Messaging base URL should normalize");
        assert_eq!(
            v1_page_url_from_base(
                &base_url,
                "https://messaging.twilio.com/v1/proxy/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&Page=1",
                V1PageResource::DestinationAlphaSenders {
                    service_sid: "MG123"
                },
            )
            .expect("valid destination alpha sender page URL should pass")
            .as_str(),
            "https://messaging.twilio.com/v1/proxy/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&Page=1"
        );
        assert_eq!(
            v1_page_url_from_base(
                &base_url,
                "https://messaging.twilio.com/v1/proxy/Tollfree/Verifications?TrustProductSid=BU1&TrustProductSid=BU2&Page=1",
                V1PageResource::TollfreeVerifications,
            )
            .expect("valid Tollfree Verifications page URL should pass")
            .as_str(),
            "https://messaging.twilio.com/v1/proxy/Tollfree/Verifications?TrustProductSid=BU1&TrustProductSid=BU2&Page=1"
        );

        for bad in [
            "https://example.test/v1/proxy/Services?Page=1",
            "https://user:pass@messaging.twilio.com/v1/proxy/Services?Page=1",
            "https://messaging.twilio.com/v1/proxy/Services?Unexpected=1",
            "https://messaging.twilio.com/v1/proxy/Services?Page=1#frag",
            "https://messaging.twilio.com/v1/proxy/Services?Page=1&Page=2",
            "https://messaging.twilio.com/v1/proxy/Services/MG999/PhoneNumbers?Page=1",
            "https://messaging.twilio.com/v1/proxy/Tollfree/Verifications?Page=1&Page=2",
        ] {
            let resource = if bad.contains("PhoneNumbers") {
                V1PageResource::PhoneNumbers {
                    service_sid: "MG123",
                }
            } else if bad.contains("Tollfree") {
                V1PageResource::TollfreeVerifications
            } else {
                V1PageResource::Services
            };
            assert!(
                v1_page_url_from_base(&base_url, bad, resource).is_err(),
                "accepted bad v1 page URL {bad}"
            );
        }
    }

    #[test]
    fn next_page_continuation_preserves_stable_filters_and_resource_path() {
        let rest_base = normalize_base_url("https://api.twilio.com/proxy")
            .expect("proxy REST base URL should normalize");
        let current = legacy_page_uri_url_from_base(
            &rest_base,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&DateSent%3E=2026-07-01&PageSize=50&Page=0",
            "AC123",
            LegacyPageResource::Messages,
        )
        .expect("valid current legacy page URI should pass");
        let next = legacy_page_uri_url_from_base(
            &rest_base,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&DateSent%3E=2026-07-01&PageSize=50&Page=1&PageToken=abc",
            "AC123",
            LegacyPageResource::Messages,
        )
        .expect("valid next legacy page URI should pass");
        assert!(
            validate_legacy_next_page_continuation(&current, &next, LegacyPageResource::Messages)
                .is_ok()
        );

        let messaging_base = normalize_base_url("https://messaging.twilio.com/v1")
            .expect("Messaging base URL should normalize");
        let current = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=50&Page=0",
            V1PageResource::DestinationAlphaSenders {
                service_sid: "MG123",
            },
        )
        .expect("valid current v1 page URL should pass");
        let next = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=FR&PageSize=50&Page=1&PageToken=abc",
            V1PageResource::DestinationAlphaSenders {
                service_sid: "MG123",
            },
        )
        .expect("valid next v1 page URL should pass");
        assert!(
            validate_v1_next_page_continuation(
                &current,
                &next,
                V1PageResource::DestinationAlphaSenders {
                    service_sid: "MG123"
                }
            )
            .is_ok()
        );

        let changed = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Services/MG123/DestinationAlphaSenders?IsoCountryCode=GB&PageSize=50&Page=1&PageToken=abc",
            V1PageResource::DestinationAlphaSenders {
                service_sid: "MG123",
            },
        )
        .expect("valid changed-filter v1 page URL should parse before continuation check");
        assert!(
            validate_v1_next_page_continuation(
                &current,
                &changed,
                V1PageResource::DestinationAlphaSenders {
                    service_sid: "MG123"
                }
            )
            .is_err()
        );

        let current = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PN123&Status=TWILIO_APPROVED&ExternalReferenceId=external&IncludeSubAccounts=true&TrustProductSid=BU1&TrustProductSid=BU2&PageSize=50&Page=0",
            V1PageResource::TollfreeVerifications,
        )
        .expect("valid current TFV page URL should pass");
        let next = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PN123&Status=TWILIO_APPROVED&ExternalReferenceId=external&IncludeSubAccounts=true&TrustProductSid=BU1&TrustProductSid=BU2&PageSize=50&Page=1&PageToken=abc",
            V1PageResource::TollfreeVerifications,
        )
        .expect("valid next TFV page URL should pass");
        assert!(
            validate_v1_next_page_continuation(
                &current,
                &next,
                V1PageResource::TollfreeVerifications
            )
            .is_ok()
        );
        let changed = v1_page_url_from_base(
            &messaging_base,
            "https://messaging.twilio.com/v1/Tollfree/Verifications?TollfreePhoneNumberSid=PN123&Status=TWILIO_APPROVED&ExternalReferenceId=external&IncludeSubAccounts=true&TrustProductSid=BU2&TrustProductSid=BU1&PageSize=50&Page=1&PageToken=abc",
            V1PageResource::TollfreeVerifications,
        )
        .expect("valid changed TFV page URL should parse before continuation check");
        assert!(
            validate_v1_next_page_continuation(
                &current,
                &changed,
                V1PageResource::TollfreeVerifications
            )
            .is_err()
        );
    }

    #[test]
    fn diagnostics_redact_sensitive_values() {
        let diagnostic = concat!(
            "url=https://messaging.twilio.com/v1/Services/MG123?",
            "FriendlyName=Secret&AlphaSender=MyCo&PhoneNumber=%2B15551234567&AuthToken=abc123 ",
            "Authorization: Basic dXNlcjpwYXNz ",
            r#"json={"password":"pw","api_key":"key123","body":"secret text"} "#,
            "raw=super-secret-token"
        );

        let redacted = sanitize_diagnostic(diagnostic.to_owned(), &["super-secret-token"]);

        for leaked in [
            "messaging.twilio.com",
            "MG123",
            "Secret",
            "MyCo",
            "%2B15551234567",
            "abc123",
            "dXNlcjpwYXNz",
            "pw",
            "key123",
            "secret text",
            "super-secret-token",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }
}
