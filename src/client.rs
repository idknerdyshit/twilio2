use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use reqwest::Url;
use tracing::Instrument as _;

use crate::common::{
    ApiResponse, AttemptTrace, Operation, OperationTrace, ParsedConfig, RawResponse, RequestBody,
    RequestOptions, RequestSpec, RequestTarget, ResponseMeta, RetryPolicy, TwilioClientConfig,
    TwilioConfig, TwilioCreds, TwilioError, api_error_from_body, api_error_from_body_read_error,
    attempt_error, attempt_response, attempt_span, decode_json_response, endpoint_url_from_base,
    legacy_page_uri_url_from_base, read_limited_response_body, transport_error,
    v1_page_url_from_base,
};
use crate::deactivations::DeactivationsResource;
#[cfg(feature = "sensitive-diagnostics")]
use crate::diagnostics::{
    SensitiveDiagnosticEvent, SensitiveDiagnostics, SensitiveRequestSnapshot,
    SensitiveResponseSnapshot, SensitiveTransportErrorSnapshot, SensitiveTransportErrorStage,
};
use crate::messages::{MessageResource, MessagesResource};
use crate::services::{ServiceResource, ServicesResource};
use crate::short_codes::{AccountShortCodeResource, AccountShortCodesResource};
use crate::tollfree_verifications::{TollfreeVerificationResource, TollfreeVerificationsResource};

/// A thin Twilio API client over an injected [`reqwest::Client`].
///
/// The client stores HTTP/base URL configuration only. Account credentials are
/// supplied to [`Self::account`] and borrowed by the account-scoped API handle.
#[derive(Clone)]
pub struct TwilioClient {
    pub(crate) http: reqwest::Client,
    pub(crate) config: ParsedConfig,
    #[cfg(feature = "sensitive-diagnostics")]
    pub(crate) sensitive_diagnostics: Option<SensitiveDiagnostics>,
}

pub(crate) struct RawAttemptResponse {
    response: ApiResponse<RawResponse>,
    attempts: u32,
}

pub(crate) struct RawAttemptError {
    error: TwilioError,
    attempts: u32,
}

struct RawAttemptContext<'a, 's> {
    spec: &'a RequestSpec,
    url: &'a Url,
    options: &'a RequestOptions,
    sensitive_values: &'a [&'s str],
    attempt: u32,
    retry: RetryPolicy,
}

struct ApiErrorRead {
    error: TwilioError,
    raw_response: Option<RawResponse>,
    transport_error: Option<String>,
}

struct BodyReadError {
    error: TwilioError,
    raw_error: String,
}

fn raw_api_response(raw: RawResponse) -> ApiResponse<RawResponse> {
    let meta = ResponseMeta::from_headers(raw.status, &raw.headers);
    ApiResponse {
        output: raw.clone(),
        meta,
        raw,
    }
}

#[cfg(feature = "sensitive-diagnostics")]
struct SensitiveBuildError<'a> {
    client: &'a TwilioClient,
    options: &'a RequestOptions,
    operation: &'static str,
    method: reqwest::Method,
    url: &'a Url,
    attempt: u32,
    max_retries: u32,
    error: String,
}

#[cfg(feature = "sensitive-diagnostics")]
#[derive(Debug)]
struct SensitiveAttempt<'a> {
    diagnostics: Option<&'a SensitiveDiagnostics>,
    request: Option<SensitiveRequestSnapshot>,
}

#[cfg(feature = "sensitive-diagnostics")]
impl<'a> SensitiveAttempt<'a> {
    fn diagnostics_for(
        client: &'a TwilioClient,
        options: &'a RequestOptions,
    ) -> Option<&'a SensitiveDiagnostics> {
        options
            .sensitive_diagnostics
            .as_ref()
            .or(client.sensitive_diagnostics.as_ref())
    }

    fn new(
        client: &'a TwilioClient,
        options: &'a RequestOptions,
        request: &reqwest::Request,
        operation: &'static str,
        attempt: u32,
        max_retries: u32,
    ) -> Self {
        let diagnostics = Self::diagnostics_for(client, options);
        let snapshot = diagnostics.map(|_| {
            SensitiveRequestSnapshot::from_request(
                request,
                operation,
                attempt,
                max_retries,
                options.trace_label.as_deref(),
            )
        });
        Self {
            diagnostics,
            request: snapshot,
        }
    }

    fn build_error(error: SensitiveBuildError<'a>) {
        let Some(diagnostics) = Self::diagnostics_for(error.client, error.options) else {
            return;
        };
        let request = SensitiveRequestSnapshot {
            method: error.method,
            url: error.url.to_string(),
            operation: error.operation,
            attempt: error.attempt,
            max_retries: error.max_retries,
            trace_label: error.options.trace_label.clone(),
            headers: http::HeaderMap::default(),
            body: None,
        };
        diagnostics.record(SensitiveDiagnosticEvent::TransportError(
            SensitiveTransportErrorSnapshot::new(
                request,
                SensitiveTransportErrorStage::BuildRequest,
                error.error,
            ),
        ));
    }

    fn request(&self) {
        let (Some(diagnostics), Some(request)) = (self.diagnostics, &self.request) else {
            return;
        };
        diagnostics.record(SensitiveDiagnosticEvent::Request(request.clone()));
    }

    fn response(&self, raw: &RawResponse) {
        let (Some(diagnostics), Some(request)) = (self.diagnostics, &self.request) else {
            return;
        };
        diagnostics.record(SensitiveDiagnosticEvent::Response(
            SensitiveResponseSnapshot::from_raw(request, raw),
        ));
    }

    fn transport_error(&self, stage: SensitiveTransportErrorStage, error: String) {
        let (Some(diagnostics), Some(request)) = (self.diagnostics, &self.request) else {
            return;
        };
        diagnostics.record(SensitiveDiagnosticEvent::TransportError(
            SensitiveTransportErrorSnapshot::new(request.clone(), stage, error),
        ));
    }

    fn captures_events(&self) -> bool {
        self.diagnostics
            .is_some_and(SensitiveDiagnostics::captures_events)
    }
}

impl TwilioClient {
    /// Build a client from construction-time config.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either configured base URL
    /// is invalid, or [`TwilioError::Transport`] when `reqwest::Client`
    /// construction fails.
    pub fn from_config(config: TwilioClientConfig) -> Result<Self, TwilioError> {
        let http = catch_unwind(AssertUnwindSafe(|| {
            reqwest::Client::builder()
                .timeout(config.timeout)
                .user_agent(config.user_agent.clone())
                .build()
        }))
        .map_err(|_| {
            TwilioError::Transport(
                "reqwest client construction panicked; check TLS provider configuration".to_owned(),
            )
        })?
        .map_err(|e| transport_error(&e, &[]))?;
        Self::from_config_and_http_client(config, http)
    }

    /// Build a client from construction-time config and a caller-provided HTTP
    /// client.
    ///
    /// The supplied HTTP client is used as-is. Timeout and user-agent values in
    /// `config` cannot be applied to an already-built `reqwest::Client`; only
    /// the base URL configuration is retained.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either base URL is invalid.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_config_and_http_client(
        config: TwilioClientConfig,
        http: reqwest::Client,
    ) -> Result<Self, TwilioError> {
        #[cfg(feature = "sensitive-diagnostics")]
        let sensitive_diagnostics = config.sensitive_diagnostics.clone();
        let client = Self::try_with_config(http, config.base_urls)?;
        #[cfg(feature = "sensitive-diagnostics")]
        {
            let mut client = client;
            client.sensitive_diagnostics = sensitive_diagnostics;
            Ok(client)
        }
        #[cfg(not(feature = "sensitive-diagnostics"))]
        {
            Ok(client)
        }
    }

    /// Build with default Twilio base URLs from a caller-provided HTTP client.
    #[must_use]
    pub fn from_http_client(http: reqwest::Client) -> Self {
        Self::new(http)
    }

    /// Build a client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when environment values or HTTP client
    /// construction fail.
    pub fn from_env() -> Result<Self, TwilioError> {
        Self::from_config(TwilioClientConfig::from_env()?)
    }

    /// Build with the default Twilio REST and Messaging API base URLs.
    ///
    /// # Panics
    ///
    /// Panics only if the library's built-in default base URLs are invalid.
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self::try_with_config(http, TwilioConfig::default()).expect("invalid default Twilio config")
    }

    /// Build with explicit base URL configuration.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either base URL is empty,
    /// not HTTPS, lacks a host, includes embedded credentials, or includes a
    /// query string or fragment.
    #[allow(clippy::needless_pass_by_value)]
    pub fn try_with_config(
        http: reqwest::Client,
        config: TwilioConfig,
    ) -> Result<Self, TwilioError> {
        Ok(Self {
            http,
            config: ParsedConfig::parse(&config)?,
            #[cfg(feature = "sensitive-diagnostics")]
            sensitive_diagnostics: None,
        })
    }

    /// Create an account-scoped handle. Credentials are borrowed and are not
    /// retained by [`TwilioClient`].
    #[must_use]
    pub fn account<'a>(&'a self, creds: TwilioCreds<'a>) -> TwilioAccount<'a> {
        TwilioAccount {
            client: self,
            creds,
        }
    }

    /// The normalized base URL configuration retained by this client.
    #[must_use]
    pub fn config(&self) -> TwilioConfig {
        self.config.as_public_config()
    }

    pub(crate) fn rest_endpoint(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.config.rest_base_url, segments)
    }

    pub(crate) fn messaging_endpoint(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.config.messaging_base_url, segments)
    }

    pub(crate) fn legacy_page_url(
        &self,
        page_uri: &str,
        account_sid: &str,
        resource: crate::common::LegacyPageResource<'_>,
    ) -> Result<Url, TwilioError> {
        legacy_page_uri_url_from_base(&self.config.rest_base_url, page_uri, account_sid, resource)
    }

    pub(crate) fn v1_page_url(
        &self,
        page_url: &str,
        resource: crate::common::V1PageResource<'_>,
    ) -> Result<Url, TwilioError> {
        v1_page_url_from_base(&self.config.messaging_base_url, page_url, resource)
    }

    pub(crate) fn url_for_spec(
        &self,
        spec: &RequestSpec,
        options: &RequestOptions,
    ) -> Result<Url, TwilioError> {
        let mut url = match &spec.target {
            RequestTarget::Segments(segments) => {
                let base =
                    options
                        .base_url_for(spec.family)?
                        .unwrap_or_else(|| match spec.family {
                            crate::common::ApiFamily::Rest => self.config.rest_base_url.clone(),
                            crate::common::ApiFamily::Messaging => {
                                self.config.messaging_base_url.clone()
                            }
                        });
                let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
                endpoint_url_from_base(&base, &refs)?
            }
            RequestTarget::Url(url) => url.clone(),
        };

        if !spec.query.is_empty() || (!spec.is_continuation() && !options.query.is_empty()) {
            let mut query = url.query_pairs_mut();
            for (key, value) in &spec.query {
                query.append_pair(key, value);
            }
            if !spec.is_continuation() {
                for (key, value) in &options.query {
                    query.append_pair(key, value);
                }
            }
        }
        Ok(url)
    }

    pub(crate) async fn send_raw_spec(
        &self,
        creds: TwilioCreds<'_>,
        spec: RequestSpec,
        options: RequestOptions,
        sensitive_values: &[&str],
    ) -> Result<RawAttemptResponse, RawAttemptError> {
        let retry = options.retry_or_default();
        options
            .validate()
            .map_err(|error| RawAttemptError { error, attempts: 0 })?;
        if spec.is_continuation()
            && (options.rest_base_url.is_some()
                || options.messaging_base_url.is_some()
                || !options.query.is_empty())
        {
            return Err(RawAttemptError {
                error: TwilioError::InvalidRequest(
                    "pagination continuation requests do not accept base URL or extra query overrides"
                        .to_owned(),
                ),
                attempts: 0,
            });
        }
        if retry.max_retries > 0 && !spec.is_safe_method() {
            return Err(RawAttemptError {
                error: TwilioError::InvalidRequest(
                    "automatic retries are only supported for safe HTTP methods".to_owned(),
                ),
                attempts: 0,
            });
        }
        let url = self
            .url_for_spec(&spec, &options)
            .map_err(|error| RawAttemptError { error, attempts: 0 })?;

        let mut retries_done = 0;
        loop {
            let attempt = retries_done + 1;
            let result = self
                .send_raw_once(
                    creds,
                    RawAttemptContext {
                        spec: &spec,
                        url: &url,
                        options: &options,
                        sensitive_values,
                        attempt,
                        retry,
                    },
                )
                .await;
            match result {
                Ok(response) => {
                    return Ok(RawAttemptResponse {
                        response,
                        attempts: attempt,
                    });
                }
                Err(err) if retry.should_retry(retries_done, &err) => {
                    let delay = retry.delay_for(retries_done, &err);
                    let trace = OperationTrace::new(
                        spec.operation,
                        retry.max_retries,
                        options.trace_label.as_deref(),
                        sensitive_values,
                    );
                    trace.retry(spec.method.as_str(), attempt, attempt + 1, delay, &err);
                    tokio::time::sleep(delay).await;
                    retries_done += 1;
                }
                Err(error) => {
                    return Err(RawAttemptError {
                        error,
                        attempts: attempt,
                    });
                }
            }
        }
    }

    fn build_raw_request(
        &self,
        creds: TwilioCreds<'_>,
        spec: &RequestSpec,
        url: &Url,
        options: &RequestOptions,
    ) -> Result<reqwest::Request, reqwest::Error> {
        let mut request = self
            .http
            .request(spec.method.clone(), url.clone())
            .basic_auth(creds.account_sid, Some(creds.auth_token));
        if let Some(timeout) = options.timeout {
            request = request.timeout(timeout);
        }
        for (key, value) in &options.headers {
            request = request.header(key, value);
        }
        match &spec.body {
            RequestBody::Empty => {}
            RequestBody::Form(params) => {
                let form: Vec<(&str, &str)> = params
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect();
                request = request.form(&form);
            }
        }
        request.build()
    }

    async fn read_api_error_response(
        response: reqwest::Response,
        status: u16,
        headers: reqwest::header::HeaderMap,
        sensitive_values: &[&str],
        capture_sensitive_response: bool,
    ) -> ApiErrorRead {
        if capture_sensitive_response {
            return match response.bytes().await {
                Ok(body) => {
                    let body = body.to_vec();
                    ApiErrorRead {
                        error: api_error_from_body(status, &body, sensitive_values),
                        raw_response: Some(RawResponse::new(status, headers, body)),
                        transport_error: None,
                    }
                }
                Err(error) => ApiErrorRead {
                    error: api_error_from_body_read_error(status, &error, sensitive_values),
                    raw_response: None,
                    transport_error: Some(error.to_string()),
                },
            };
        }

        let limited = match read_limited_response_body(response).await {
            Ok(limited) => limited,
            Err(error) => {
                return ApiErrorRead {
                    error: api_error_from_body_read_error(status, &error, sensitive_values),
                    raw_response: None,
                    transport_error: Some(error.to_string()),
                };
            }
        };
        let raw_response = limited
            .complete
            .then(|| RawResponse::new(status, headers, limited.body.clone()));
        ApiErrorRead {
            error: api_error_from_body(status, &limited.body, sensitive_values),
            raw_response,
            transport_error: None,
        }
    }

    async fn read_response_body(
        response: reqwest::Response,
        sensitive_values: &[&str],
    ) -> Result<Vec<u8>, BodyReadError> {
        response
            .bytes()
            .await
            .map(|body| body.to_vec())
            .map_err(|e| BodyReadError {
                raw_error: e.to_string(),
                error: transport_error(&e, sensitive_values),
            })
    }

    fn raw_request_build_error(
        &self,
        error: &reqwest::Error,
        context: &RawAttemptContext<'_, '_>,
        attempt_trace: &AttemptTrace<'_>,
        span: &tracing::Span,
        start: Instant,
    ) -> TwilioError {
        #[cfg(not(feature = "sensitive-diagnostics"))]
        let _ = self;
        #[cfg(feature = "sensitive-diagnostics")]
        SensitiveAttempt::build_error(SensitiveBuildError {
            client: self,
            options: context.options,
            operation: context.spec.operation,
            method: context.spec.method.clone(),
            url: context.url,
            attempt: context.attempt,
            max_retries: context.retry.max_retries,
            error: error.to_string(),
        });
        let error = transport_error(error, context.sensitive_values);
        attempt_error(span, attempt_trace, start.elapsed(), &error);
        error
    }

    async fn send_raw_once(
        &self,
        creds: TwilioCreds<'_>,
        context: RawAttemptContext<'_, '_>,
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        let method = context.spec.method.as_str();
        let attempt_trace = AttemptTrace::new(
            method,
            context.spec.operation,
            context.attempt,
            context.retry.max_retries,
            context.options.trace_label.as_deref(),
            context.sensitive_values,
        );
        let span = attempt_span(&attempt_trace);
        let start = Instant::now();
        let request =
            match self.build_raw_request(creds, context.spec, context.url, context.options) {
                Ok(request) => request,
                Err(e) => {
                    return Err(self.raw_request_build_error(
                        &e,
                        &context,
                        &attempt_trace,
                        &span,
                        start,
                    ));
                }
            };
        #[cfg(feature = "sensitive-diagnostics")]
        let sensitive = SensitiveAttempt::new(
            self,
            context.options,
            &request,
            context.spec.operation,
            context.attempt,
            context.retry.max_retries,
        );
        #[cfg(feature = "sensitive-diagnostics")]
        sensitive.request();

        let response = match self.http.execute(request).instrument(span.clone()).await {
            Ok(response) => response,
            Err(e) => {
                #[cfg(feature = "sensitive-diagnostics")]
                let raw_error = e.to_string();
                #[cfg(feature = "sensitive-diagnostics")]
                sensitive.transport_error(SensitiveTransportErrorStage::Send, raw_error);
                let error = transport_error(&e, context.sensitive_values);
                attempt_error(&span, &attempt_trace, start.elapsed(), &error);
                return Err(error);
            }
        };
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        if !(200..=299).contains(&status) && !context.spec.accepts_status(status) {
            #[cfg(feature = "sensitive-diagnostics")]
            let capture_sensitive_response = sensitive.captures_events();
            #[cfg(not(feature = "sensitive-diagnostics"))]
            let capture_sensitive_response = false;
            let api_error = Self::read_api_error_response(
                response,
                status,
                headers,
                context.sensitive_values,
                capture_sensitive_response,
            )
            .await;
            #[cfg(not(feature = "sensitive-diagnostics"))]
            let _ = (&api_error.raw_response, &api_error.transport_error);
            #[cfg(feature = "sensitive-diagnostics")]
            if let Some(raw_response) = &api_error.raw_response {
                sensitive.response(raw_response);
            }
            #[cfg(feature = "sensitive-diagnostics")]
            if let Some(error) = api_error.transport_error {
                sensitive.transport_error(SensitiveTransportErrorStage::ReadBody, error);
            }
            attempt_response(&span, &attempt_trace, status, start.elapsed());
            return Err(api_error.error);
        }
        let body = match Self::read_response_body(response, context.sensitive_values).await {
            Ok(body) => body,
            Err(error) => {
                #[cfg(not(feature = "sensitive-diagnostics"))]
                let _ = &error.raw_error;
                #[cfg(feature = "sensitive-diagnostics")]
                sensitive.transport_error(SensitiveTransportErrorStage::ReadBody, error.raw_error);
                attempt_error(&span, &attempt_trace, start.elapsed(), &error.error);
                return Err(error.error);
            }
        };
        let raw = RawResponse::new(status, headers, body);
        #[cfg(feature = "sensitive-diagnostics")]
        sensitive.response(&raw);
        attempt_response(&span, &attempt_trace, raw.status, start.elapsed());
        Ok(raw_api_response(raw))
    }
}

/// Account-scoped Twilio API handle.
#[derive(Clone, Copy)]
pub struct TwilioAccount<'a> {
    pub(crate) client: &'a TwilioClient,
    pub(crate) creds: TwilioCreds<'a>,
}

impl<'a> TwilioAccount<'a> {
    /// Account-level Messages collection.
    #[must_use]
    pub fn messages(self) -> MessagesResource<'a> {
        MessagesResource::new(self)
    }

    /// One Message resource and its subresources.
    #[must_use]
    pub fn message(self, sid: &'a str) -> MessageResource<'a> {
        MessageResource::new(self, sid)
    }

    /// Messaging v1 Deactivations collection.
    #[must_use]
    pub fn deactivations(self) -> DeactivationsResource<'a> {
        DeactivationsResource::new(self)
    }

    /// Legacy account-level `ShortCodes` collection.
    #[must_use]
    pub fn short_codes(self) -> AccountShortCodesResource<'a> {
        AccountShortCodesResource::new(self)
    }

    /// One legacy account-level `ShortCode` resource.
    #[must_use]
    pub fn short_code(self, sid: &'a str) -> AccountShortCodeResource<'a> {
        AccountShortCodeResource::new(self, sid)
    }

    /// Messaging Services collection.
    #[must_use]
    pub fn services(self) -> ServicesResource<'a> {
        ServicesResource::new(self)
    }

    /// One Messaging Service resource and its subresources.
    #[must_use]
    pub fn service(self, sid: &'a str) -> ServiceResource<'a> {
        ServiceResource::new(self, sid)
    }

    /// Messaging v1 Toll-free Verifications collection.
    #[must_use]
    pub fn tollfree_verifications(self) -> TollfreeVerificationsResource<'a> {
        TollfreeVerificationsResource::new(self)
    }

    /// One Messaging v1 Toll-free Verification resource.
    #[must_use]
    pub fn tollfree_verification(self, sid: &'a str) -> TollfreeVerificationResource<'a> {
        TollfreeVerificationResource::new(self, sid)
    }

    /// Execute a custom Twilio operation and decode its response.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub async fn send<O: Operation>(self, operation: O) -> Result<O::Output, TwilioError> {
        self.send_with_meta(operation)
            .await
            .map(|(output, _meta)| output)
    }

    /// Execute a custom operation with request-scoped transport options.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub async fn send_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<O::Output, TwilioError> {
        self.send_with_meta_with_options(operation, options)
            .await
            .map(|(output, _meta)| output)
    }

    /// Execute a custom operation and return decoded output plus response
    /// metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub async fn send_with_meta<O: Operation>(
        self,
        operation: O,
    ) -> Result<(O::Output, ResponseMeta), TwilioError> {
        self.send_with_meta_with_options(operation, RequestOptions::new())
            .await
    }

    /// Execute a custom operation with request options and return decoded output
    /// plus response metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub async fn send_with_meta_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<(O::Output, ResponseMeta), TwilioError> {
        let response = self
            .send_with_response_with_options(operation, options)
            .await?;
        Ok((response.output, response.meta))
    }

    /// Execute a custom operation and return decoded output plus raw HTTP data.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub async fn send_with_response<O: Operation>(
        self,
        operation: O,
    ) -> Result<ApiResponse<O::Output>, TwilioError> {
        self.send_with_response_with_options(operation, RequestOptions::new())
            .await
    }

    /// Execute a custom operation with request options and return decoded output
    /// plus raw HTTP data.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub async fn send_with_response_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<ApiResponse<O::Output>, TwilioError> {
        let retry = options.retry_or_default();
        let mut sensitive_owned = vec![
            self.creds.account_sid.to_owned(),
            self.creds.auth_token.to_owned(),
        ];
        sensitive_owned.extend(operation.sensitive_values());
        let sensitive_refs: Vec<&str> = sensitive_owned.iter().map(String::as_str).collect();
        let fallback_operation = std::any::type_name::<O>();
        let pre_trace = OperationTrace::new(
            fallback_operation,
            retry.max_retries,
            options.trace_label.as_deref(),
            &sensitive_refs,
        );
        let spec = match operation.request(self.creds.account_sid) {
            Ok(spec) => spec,
            Err(error) => {
                pre_trace.failure("UNKNOWN", 0, None, &error);
                return Err(error);
            }
        };
        let trace = OperationTrace::new(
            spec.operation,
            retry.max_retries,
            options.trace_label.as_deref(),
            &sensitive_refs,
        );
        let method = spec.method.as_str().to_owned();
        let raw = match self
            .client
            .send_raw_spec(self.creds, spec, options, &sensitive_refs)
            .await
        {
            Ok(raw) => raw,
            Err(raw_error) => {
                trace.failure(
                    &method,
                    raw_error.attempts,
                    raw_error.error.status(),
                    &raw_error.error,
                );
                return Err(raw_error.error);
            }
        };
        let status = raw.response.raw.status;
        let output = match operation.decode(raw.response.output, &sensitive_refs) {
            Ok(output) => output,
            Err(error) => {
                trace.failure(&method, raw.attempts, Some(status), &error);
                return Err(error);
            }
        };
        trace.success(&method, raw.attempts, status);
        Ok(ApiResponse {
            output,
            meta: raw.response.meta,
            raw: raw.response.raw,
        })
    }

    pub(crate) async fn send_spec_json<T: serde::de::DeserializeOwned>(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<T, TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        let raw = match self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await
        {
            Ok(raw) => raw,
            Err(raw_error) => {
                trace.failure(
                    &method,
                    raw_error.attempts,
                    raw_error.error.status(),
                    &raw_error.error,
                );
                return Err(raw_error.error);
            }
        };
        let status = raw.response.raw.status;
        let decoded = match decode_json_response(&raw.response.output, sensitive_values) {
            Ok(decoded) => decoded,
            Err(error) => {
                trace.failure(&method, raw.attempts, Some(status), &error);
                return Err(error);
            }
        };
        trace.success(&method, raw.attempts, status);
        Ok(decoded)
    }

    pub(crate) async fn send_spec_empty(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<(), TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        match self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await
        {
            Ok(raw) => {
                trace.success(&method, raw.attempts, raw.response.raw.status);
                Ok(())
            }
            Err(raw_error) => {
                trace.failure(
                    &method,
                    raw_error.attempts,
                    raw_error.error.status(),
                    &raw_error.error,
                );
                Err(raw_error.error)
            }
        }
    }

    pub(crate) async fn send_spec_raw(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        match self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await
        {
            Ok(raw) => {
                trace.success(&method, raw.attempts, raw.response.raw.status);
                Ok(raw.response)
            }
            Err(raw_error) => {
                trace.failure(
                    &method,
                    raw_error.attempts,
                    raw_error.error.status(),
                    &raw_error.error,
                );
                Err(raw_error.error)
            }
        }
    }
}
