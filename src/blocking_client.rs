#![allow(clippy::needless_pass_by_value)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use http::HeaderMap;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use url::Url;

use crate::common::{
    ApiFamily, ApiResponse, AttemptTrace, Operation, OperationTrace, ParsedConfig, RawResponse,
    RequestBody, RequestOptions, RequestSpec, RequestTarget, ResponseMeta, RetryPolicy,
    TwilioClientConfig, TwilioConfig, TwilioCreds, TwilioError, api_error_from_body,
    api_error_from_read_error_message, attempt_error, attempt_response, attempt_span,
    decode_json_response, endpoint_url_from_base, legacy_page_uri_url_from_base,
    owned_sensitive_values, pricing_page_url_from_base, read_limited_reader_body,
    transport_error_from_message, v1_page_url_from_base,
};
use crate::deactivations::BlockingDeactivationsResource;
#[cfg(feature = "sensitive-diagnostics")]
use crate::diagnostics::{
    SensitiveDiagnosticEvent, SensitiveDiagnostics, SensitiveRequestSnapshot,
    SensitiveRequestSnapshotParts, SensitiveResponseSnapshot, SensitiveTransportErrorSnapshot,
    SensitiveTransportErrorStage,
};
use crate::messages::{BlockingMessageResource, BlockingMessagesResource};
use crate::pricing::BlockingPricingResource;
use crate::services::{BlockingServiceResource, BlockingServicesResource};
use crate::short_codes::{BlockingAccountShortCodeResource, BlockingAccountShortCodesResource};
use crate::tollfree_verifications::{
    BlockingTollfreeVerificationResource, BlockingTollfreeVerificationsResource,
};

/// A thin blocking Twilio API client over an injected [`ureq::Agent`].
///
/// The client stores HTTP/base URL configuration only. Account credentials are
/// supplied to [`Self::account`] and borrowed by the account-scoped API handle.
#[derive(Clone)]
pub struct BlockingTwilioClient {
    pub(crate) agent: ureq::Agent,
    pub(crate) config: ParsedConfig,
    #[cfg(feature = "sensitive-diagnostics")]
    pub(crate) sensitive_diagnostics: Option<SensitiveDiagnostics>,
}

pub(crate) struct BlockingRawAttemptResponse {
    response: ApiResponse<RawResponse>,
    attempts: u32,
}

pub(crate) struct BlockingRawAttemptError {
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

enum BuiltRequest {
    Empty(http::Request<()>),
    Body(http::Request<Vec<u8>>),
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
    client: &'a BlockingTwilioClient,
    options: &'a RequestOptions,
    operation: &'static str,
    method: http::Method,
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
        client: &'a BlockingTwilioClient,
        options: &'a RequestOptions,
    ) -> Option<&'a SensitiveDiagnostics> {
        options
            .sensitive_diagnostics
            .as_ref()
            .or(client.sensitive_diagnostics.as_ref())
    }

    fn new(
        client: &'a BlockingTwilioClient,
        options: &'a RequestOptions,
        request: &BuiltRequest,
        operation: &'static str,
        attempt: u32,
        max_retries: u32,
    ) -> Self {
        let diagnostics = Self::diagnostics_for(client, options);
        let snapshot = diagnostics.map(|_| match request {
            BuiltRequest::Empty(request) => {
                SensitiveRequestSnapshot::from_parts(SensitiveRequestSnapshotParts {
                    method: request.method().clone(),
                    url: request.uri().to_string(),
                    operation,
                    attempt,
                    max_retries,
                    trace_label: options.trace_label.clone(),
                    headers: request.headers().clone(),
                    body: None,
                })
            }
            BuiltRequest::Body(request) => {
                SensitiveRequestSnapshot::from_parts(SensitiveRequestSnapshotParts {
                    method: request.method().clone(),
                    url: request.uri().to_string(),
                    operation,
                    attempt,
                    max_retries,
                    trace_label: options.trace_label.clone(),
                    headers: request.headers().clone(),
                    body: Some(bytes::Bytes::copy_from_slice(request.body())),
                })
            }
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
            headers: HeaderMap::default(),
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

impl BlockingTwilioClient {
    /// Build a blocking client from construction-time config.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either configured base URL
    /// is invalid, or [`TwilioError::Transport`] when `ureq::Agent`
    /// construction panics.
    pub fn from_config(config: TwilioClientConfig) -> Result<Self, TwilioError> {
        let agent = catch_unwind(AssertUnwindSafe(|| default_agent(&config))).map_err(|_| {
            TwilioError::Transport(
                "ureq agent construction panicked; check TLS provider configuration".to_owned(),
            )
        })?;
        Self::from_config_and_agent(config, agent)
    }

    /// Build a blocking client from construction-time config and a caller-provided agent.
    ///
    /// The supplied agent is used as-is. Timeout and user-agent values in
    /// `config` cannot be applied to an already-built `ureq::Agent`; only the
    /// base URL configuration is retained.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either base URL is invalid.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_config_and_agent(
        config: TwilioClientConfig,
        agent: ureq::Agent,
    ) -> Result<Self, TwilioError> {
        #[cfg(feature = "sensitive-diagnostics")]
        let sensitive_diagnostics = config.sensitive_diagnostics.clone();
        let client = Self::try_with_config(agent, config.base_urls)?;
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

    /// Build with default Twilio base URLs from a caller-provided agent.
    #[must_use]
    pub fn from_agent(agent: ureq::Agent) -> Self {
        Self::new(agent)
    }

    /// Build a blocking client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when environment values or agent construction
    /// fail.
    pub fn from_env() -> Result<Self, TwilioError> {
        Self::from_config(TwilioClientConfig::from_env()?)
    }

    /// Build with the default Twilio REST and Messaging API base URLs.
    ///
    /// # Panics
    ///
    /// Panics only if the library's built-in default base URLs are invalid.
    #[must_use]
    pub fn new(agent: ureq::Agent) -> Self {
        Self::try_with_config(agent, TwilioConfig::default())
            .expect("invalid default Twilio config")
    }

    /// Build with explicit base URL configuration.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when either base URL is empty,
    /// not HTTPS, lacks a host, includes embedded credentials, or includes a
    /// query string or fragment.
    #[allow(clippy::needless_pass_by_value)]
    pub fn try_with_config(agent: ureq::Agent, config: TwilioConfig) -> Result<Self, TwilioError> {
        Ok(Self {
            agent,
            config: ParsedConfig::parse(&config)?,
            #[cfg(feature = "sensitive-diagnostics")]
            sensitive_diagnostics: None,
        })
    }

    /// Create an account-scoped handle. Credentials are borrowed and are not
    /// retained by [`BlockingTwilioClient`].
    #[must_use]
    pub fn account<'a>(&'a self, creds: &'a TwilioCreds) -> BlockingTwilioAccount<'a> {
        BlockingTwilioAccount {
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
        endpoint_url_from_base(&self.config.rest, segments)
    }

    pub(crate) fn messaging_endpoint(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.config.messaging, segments)
    }

    pub(crate) fn pricing_endpoint(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.config.pricing, segments)
    }

    pub(crate) fn legacy_page_url(
        &self,
        page_uri: &str,
        account_sid: &str,
        resource: crate::common::LegacyPageResource<'_>,
    ) -> Result<Url, TwilioError> {
        legacy_page_uri_url_from_base(&self.config.rest, page_uri, account_sid, resource)
    }

    pub(crate) fn v1_page_url(
        &self,
        page_url: &str,
        resource: crate::common::V1PageResource<'_>,
    ) -> Result<Url, TwilioError> {
        v1_page_url_from_base(&self.config.messaging, page_url, resource)
    }

    pub(crate) fn pricing_page_url(
        &self,
        page_url: &str,
        resource: crate::common::PricingPageResource,
    ) -> Result<Url, TwilioError> {
        pricing_page_url_from_base(&self.config.pricing, page_url, resource)
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
                            ApiFamily::Rest => self.config.rest.clone(),
                            ApiFamily::Messaging => self.config.messaging.clone(),
                            ApiFamily::Pricing => self.config.pricing.clone(),
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

    pub(crate) fn send_raw_spec(
        &self,
        creds: &TwilioCreds,
        spec: RequestSpec,
        options: RequestOptions,
        sensitive_values: &[&str],
    ) -> Result<BlockingRawAttemptResponse, BlockingRawAttemptError> {
        let retry = options.retry_or_default();
        options
            .validate()
            .map_err(|error| BlockingRawAttemptError { error, attempts: 0 })?;
        if spec.is_continuation()
            && (options.rest_base_url.is_some()
                || options.messaging_base_url.is_some()
                || options.pricing_base_url.is_some()
                || !options.query.is_empty())
        {
            return Err(BlockingRawAttemptError {
                error: TwilioError::InvalidRequest(
                    "pagination continuation requests do not accept base URL or extra query overrides"
                        .to_owned(),
                ),
                attempts: 0,
            });
        }
        if retry.max_retries > 0 && !spec.is_safe_method() {
            return Err(BlockingRawAttemptError {
                error: TwilioError::InvalidRequest(
                    "automatic retries are only supported for safe HTTP methods".to_owned(),
                ),
                attempts: 0,
            });
        }
        let url = self
            .url_for_spec(&spec, &options)
            .map_err(|error| BlockingRawAttemptError { error, attempts: 0 })?;

        let mut retries_done = 0;
        loop {
            let attempt = retries_done + 1;
            let result = self.send_raw_once(
                creds,
                RawAttemptContext {
                    spec: &spec,
                    url: &url,
                    options: &options,
                    sensitive_values,
                    attempt,
                    retry,
                },
            );
            match result {
                Ok(response) => {
                    return Ok(BlockingRawAttemptResponse {
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
                    std::thread::sleep(delay);
                    retries_done += 1;
                }
                Err(error) => {
                    return Err(BlockingRawAttemptError {
                        error,
                        attempts: attempt,
                    });
                }
            }
        }
    }

    fn build_raw_request(
        creds: &TwilioCreds,
        spec: &RequestSpec,
        url: &Url,
        options: &RequestOptions,
    ) -> Result<BuiltRequest, http::Error> {
        let auth_header = creds.basic_auth_header();
        let mut builder = http::Request::builder()
            .method(spec.method.clone())
            .uri(url.as_str())
            .header(AUTHORIZATION, auth_header.expose().as_str());
        for (key, value) in &options.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        match &spec.body {
            RequestBody::Empty => builder.body(()).map(BuiltRequest::Empty),
            RequestBody::Form(params) => {
                builder = builder.header(CONTENT_TYPE, "application/x-www-form-urlencoded");
                let mut encoded = url::form_urlencoded::Serializer::new(String::new());
                for (key, value) in params {
                    encoded.append_pair(key, value);
                }
                let body = encoded.finish();
                builder.body(body.into_bytes()).map(BuiltRequest::Body)
            }
        }
    }

    fn run_built_request(
        &self,
        request: BuiltRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<http::Response<ureq::Body>, ureq::Error> {
        match request {
            BuiltRequest::Empty(request) => {
                let request = self.configure_built_request(request, timeout);
                self.agent.run(request)
            }
            BuiltRequest::Body(request) => {
                let request = self.configure_built_request(request, timeout);
                self.agent.run(request)
            }
        }
    }

    fn configure_built_request<S: ureq::AsSendBody>(
        &self,
        request: http::Request<S>,
        timeout: Option<std::time::Duration>,
    ) -> http::Request<S> {
        let builder = self
            .agent
            .configure_request(request)
            .http_status_as_error(false)
            .max_redirects(0);
        if let Some(timeout) = timeout {
            builder.timeout_global(Some(timeout)).build()
        } else {
            builder.build()
        }
    }

    fn read_api_error_response(
        mut response: http::Response<ureq::Body>,
        status: u16,
        headers: HeaderMap,
        sensitive_values: &[&str],
        capture_sensitive_response: bool,
    ) -> ApiErrorRead {
        if capture_sensitive_response {
            return match response.body_mut().read_to_vec() {
                Ok(body) => ApiErrorRead {
                    error: api_error_from_body(status, &body, sensitive_values),
                    raw_response: Some(RawResponse::new(status, headers, body)),
                    transport_error: None,
                },
                Err(error) => ApiErrorRead {
                    error: api_error_from_read_error_message(
                        status,
                        error_message(&error),
                        sensitive_values,
                    ),
                    raw_response: None,
                    transport_error: Some(error_message(&error)),
                },
            };
        }

        let mut reader = response.body_mut().as_reader();
        let limited = match read_limited_reader_body(&mut reader) {
            Ok(limited) => limited,
            Err(error) => {
                return ApiErrorRead {
                    error: api_error_from_read_error_message(
                        status,
                        error_message(&error),
                        sensitive_values,
                    ),
                    raw_response: None,
                    transport_error: Some(error_message(&error)),
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

    fn read_response_body(
        mut response: http::Response<ureq::Body>,
        sensitive_values: &[&str],
    ) -> Result<Vec<u8>, BodyReadError> {
        response
            .body_mut()
            .read_to_vec()
            .map_err(|e| BodyReadError {
                raw_error: error_message(&e),
                error: transport_error_from_message(error_message(&e), sensitive_values),
            })
    }

    fn raw_request_build_error(
        &self,
        error: &http::Error,
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
            error: error_message(error),
        });
        let error = transport_error_from_message(error_message(error), context.sensitive_values);
        attempt_error(span, attempt_trace, start.elapsed(), &error);
        error
    }

    fn send_raw_once(
        &self,
        creds: &TwilioCreds,
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
        let _entered = span.enter();
        let start = Instant::now();
        let request =
            match Self::build_raw_request(creds, context.spec, context.url, context.options) {
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

        let response = match self.run_built_request(request, context.options.timeout) {
            Ok(response) => response,
            Err(e) => {
                #[cfg(feature = "sensitive-diagnostics")]
                let raw_error = error_message(&e);
                #[cfg(feature = "sensitive-diagnostics")]
                sensitive.transport_error(SensitiveTransportErrorStage::Send, raw_error);
                let error =
                    transport_error_from_message(error_message(&e), context.sensitive_values);
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
            );
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
        let body = match Self::read_response_body(response, context.sensitive_values) {
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

/// Account-scoped blocking Twilio API handle.
#[derive(Clone, Copy)]
pub struct BlockingTwilioAccount<'a> {
    pub(crate) client: &'a BlockingTwilioClient,
    pub(crate) creds: &'a TwilioCreds,
}

impl<'a> BlockingTwilioAccount<'a> {
    /// Account-level Messages collection.
    #[must_use]
    pub fn messages(self) -> BlockingMessagesResource<'a> {
        BlockingMessagesResource::new(self)
    }

    /// One Message resource and its subresources.
    #[must_use]
    pub fn message(self, sid: &'a str) -> BlockingMessageResource<'a> {
        BlockingMessageResource::new(self, sid)
    }

    /// Messaging v1 Deactivations collection.
    #[must_use]
    pub fn deactivations(self) -> BlockingDeactivationsResource<'a> {
        BlockingDeactivationsResource::new(self)
    }

    /// Legacy account-level `ShortCodes` collection.
    #[must_use]
    pub fn short_codes(self) -> BlockingAccountShortCodesResource<'a> {
        BlockingAccountShortCodesResource::new(self)
    }

    /// One legacy account-level `ShortCode` resource.
    #[must_use]
    pub fn short_code(self, sid: &'a str) -> BlockingAccountShortCodeResource<'a> {
        BlockingAccountShortCodeResource::new(self, sid)
    }

    /// Messaging Services collection.
    #[must_use]
    pub fn services(self) -> BlockingServicesResource<'a> {
        BlockingServicesResource::new(self)
    }

    /// One Messaging Service resource and its subresources.
    #[must_use]
    pub fn service(self, sid: &'a str) -> BlockingServiceResource<'a> {
        BlockingServiceResource::new(self, sid)
    }

    /// Messaging v1 Toll-free Verifications collection.
    #[must_use]
    pub fn tollfree_verifications(self) -> BlockingTollfreeVerificationsResource<'a> {
        BlockingTollfreeVerificationsResource::new(self)
    }

    /// One Messaging v1 Toll-free Verification resource.
    #[must_use]
    pub fn tollfree_verification(self, sid: &'a str) -> BlockingTollfreeVerificationResource<'a> {
        BlockingTollfreeVerificationResource::new(self, sid)
    }

    /// Pricing v1 resources.
    #[must_use]
    pub fn pricing(self) -> BlockingPricingResource<'a> {
        BlockingPricingResource::new(self)
    }

    /// Execute a custom Twilio operation and decode its response.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub fn send<O: Operation>(self, operation: O) -> Result<O::Output, TwilioError> {
        self.send_with_meta(operation).map(|(output, _meta)| output)
    }

    /// Execute a custom operation with request-scoped transport options.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub fn send_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<O::Output, TwilioError> {
        self.send_with_meta_with_options(operation, options)
            .map(|(output, _meta)| output)
    }

    /// Execute a custom operation and return decoded output plus response metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub fn send_with_meta<O: Operation>(
        self,
        operation: O,
    ) -> Result<(O::Output, ResponseMeta), TwilioError> {
        self.send_with_meta_with_options(operation, RequestOptions::new())
    }

    /// Execute a custom operation with request options and return decoded output
    /// plus response metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub fn send_with_meta_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<(O::Output, ResponseMeta), TwilioError> {
        let response = self.send_with_response_with_options(operation, options)?;
        Ok((response.output, response.meta))
    }

    /// Execute a custom operation and return decoded output plus raw HTTP data.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations, transport failures,
    /// non-2xx API responses, or operation decode failures.
    pub fn send_with_response<O: Operation>(
        self,
        operation: O,
    ) -> Result<ApiResponse<O::Output>, TwilioError> {
        self.send_with_response_with_options(operation, RequestOptions::new())
    }

    /// Execute a custom operation with request options and return decoded output
    /// plus raw HTTP data.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid operations/options, transport
    /// failures, non-2xx API responses, or operation decode failures.
    pub fn send_with_response_with_options<O: Operation>(
        self,
        operation: O,
        options: RequestOptions,
    ) -> Result<ApiResponse<O::Output>, TwilioError> {
        let retry = options.retry_or_default();
        let sensitive_owned = owned_sensitive_values(operation.sensitive_values());
        let mut sensitive_refs: Vec<&str> = Vec::with_capacity(sensitive_owned.len() + 2);
        sensitive_refs.push(self.creds.account_sid());
        sensitive_refs.push(self.creds.auth_token());
        sensitive_refs.extend(sensitive_owned.iter().map(String::as_str));
        let fallback_operation = std::any::type_name::<O>();
        let pre_trace = OperationTrace::new(
            fallback_operation,
            retry.max_retries,
            options.trace_label.as_deref(),
            &sensitive_refs,
        );
        let spec = match operation.request(self.creds.account_sid()) {
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

    pub(crate) fn send_spec_json<T: serde::de::DeserializeOwned>(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<T, TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        let raw = match self.client.send_raw_spec(
            self.creds,
            spec,
            RequestOptions::new(),
            sensitive_values,
        ) {
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

    pub(crate) fn send_spec_empty(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<(), TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        match self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
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

    pub(crate) fn send_spec_raw(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        let method = spec.method.as_str().to_owned();
        let trace = OperationTrace::new(spec.operation, 0, None, sensitive_values);
        match self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
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

fn default_agent(config: &TwilioClientConfig) -> ureq::Agent {
    let builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(config.timeout))
        .user_agent(config.user_agent.clone());
    #[cfg(all(
        feature = "native-tls",
        not(feature = "rustls"),
        not(feature = "rustls-no-provider")
    ))]
    let builder = builder.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    ureq::Agent::new_with_config(builder.build())
}

fn error_message(e: &impl std::error::Error) -> String {
    let mut msg = e.to_string();
    if let Some(source) = e.source() {
        let source = source.to_string();
        if !source.is_empty() && !msg.contains(&source) {
            msg = format!("{msg}: {source}");
        }
    }
    msg
}
