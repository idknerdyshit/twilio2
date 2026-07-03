use std::panic::{AssertUnwindSafe, catch_unwind};

use reqwest::Url;
use tracing::Instrument as _;

use crate::common::{
    ApiResponse, Operation, ParsedConfig, RawResponse, RequestBody, RequestOptions, RequestSpec,
    RequestTarget, ResponseMeta, TwilioClientConfig, TwilioConfig, TwilioCreds, TwilioError,
    api_error_from_response, decode_json_response, endpoint_url_from_base,
    legacy_page_uri_url_from_base, request_span, transport_error, v1_page_url_from_base,
};
use crate::messages::{MessageResource, MessagesResource};
use crate::services::{ServiceResource, ServicesResource};

/// A thin Twilio API client over an injected [`reqwest::Client`].
///
/// The client stores HTTP/base URL configuration only. Account credentials are
/// supplied to [`Self::account`] and borrowed by the account-scoped API handle.
#[derive(Clone)]
pub struct TwilioClient {
    pub(crate) http: reqwest::Client,
    pub(crate) config: ParsedConfig,
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
        Self::try_with_config(http, config.base_urls)
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
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        options.validate()?;
        if spec.is_continuation()
            && (options.rest_base_url.is_some()
                || options.messaging_base_url.is_some()
                || !options.query.is_empty())
        {
            return Err(TwilioError::InvalidRequest(
                "pagination continuation requests do not accept base URL or extra query overrides"
                    .to_owned(),
            ));
        }
        let retry = options.retry_or_default();
        if retry.max_retries > 0 && !spec.is_safe_method() {
            return Err(TwilioError::InvalidRequest(
                "automatic retries are only supported for safe HTTP methods".to_owned(),
            ));
        }

        let mut retries_done = 0;
        loop {
            let result = self
                .send_raw_once(creds, &spec, &options, sensitive_values)
                .await;
            match result {
                Ok(response) => return Ok(response),
                Err(err) if retry.should_retry(retries_done, &err) => {
                    let delay = retry.delay_for(retries_done, &err);
                    tokio::time::sleep(delay).await;
                    retries_done += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn send_raw_once(
        &self,
        creds: TwilioCreds<'_>,
        spec: &RequestSpec,
        options: &RequestOptions,
        sensitive_values: &[&str],
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        let url = self.url_for_spec(spec, options)?;
        let mut request = self
            .http
            .request(spec.method.clone(), url)
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

        let response = request
            .send()
            .await
            .map_err(|e| transport_error(&e, sensitive_values))?;
        let status = response.status();
        if !status.is_success() {
            let status = status.as_u16();
            return Err(api_error_from_response(response, status, sensitive_values).await);
        }
        let status = status.as_u16();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e, sensitive_values))?
            .to_vec();
        let raw = RawResponse::new(status, headers, body);
        let meta = ResponseMeta::from_headers(raw.status, &raw.headers);
        Ok(ApiResponse {
            output: raw.clone(),
            meta,
            raw,
        })
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
        let spec = operation.request(self.creds.account_sid)?;
        let mut sensitive_owned = vec![
            self.creds.account_sid.to_owned(),
            self.creds.auth_token.to_owned(),
        ];
        sensitive_owned.extend(operation.sensitive_values());
        let sensitive_refs: Vec<&str> = sensitive_owned.iter().map(String::as_str).collect();
        let response = async {
            let raw = self
                .client
                .send_raw_spec(self.creds, spec.clone(), options, &sensitive_refs)
                .await?;
            let output = operation.decode(raw.output, &sensitive_refs)?;
            Ok(ApiResponse {
                output,
                meta: raw.meta,
                raw: raw.raw,
            })
        }
        .instrument(request_span(
            match spec.family {
                crate::common::ApiFamily::Rest => &self.client.config.rest_base_url,
                crate::common::ApiFamily::Messaging => &self.client.config.messaging_base_url,
            },
            spec.operation,
            spec.method.as_str(),
        ))
        .await?;
        Ok(response)
    }

    pub(crate) async fn send_spec_json<T: serde::de::DeserializeOwned>(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<T, TwilioError> {
        let raw = self
            .client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await?;
        decode_json_response(&raw.output, sensitive_values)
    }

    pub(crate) async fn send_spec_empty(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<(), TwilioError> {
        self.client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await
            .map(|_| ())
    }

    pub(crate) async fn send_spec_raw(
        self,
        spec: RequestSpec,
        sensitive_values: &[&str],
    ) -> Result<ApiResponse<RawResponse>, TwilioError> {
        self.client
            .send_raw_spec(self.creds, spec, RequestOptions::new(), sensitive_values)
            .await
    }
}
