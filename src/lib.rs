//! `twilio2` — a thin `reqwest` client over the Twilio Programmable
//! Messaging REST API. It can create a Message (send), fetch a Message by SID
//! (status lookup), list Messages filtered by To/From, and continue pagination
//! from Twilio's `next_page_uri`. It has no Sovereign dependency; Sovereign's
//! `sv_sms::twilio` adapter can wrap it as an `SmsProvider`.
//!
//! The Account SID + Auth Token are passed per call (HTTP basic auth) and never
//! stored on the client. Bodies are returned verbatim — canonical hashing for
//! reconcile is the adapter's job (it shares `sv_sms::body_hash`).

#[cfg(not(any(
    feature = "rustls",
    feature = "native-tls",
    feature = "rustls-no-provider"
)))]
compile_error!(
    "twilio2 requires HTTPS support. Enable default features, or enable one of: rustls, native-tls, rustls-no-provider."
);

use std::error::Error as _;

use reqwest::Url;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use tracing::Instrument as _;

/// Default Twilio API root (no trailing slash).
pub const DEFAULT_BASE_URL: &str = "https://api.twilio.com";

#[derive(Debug, thiserror::Error)]
pub enum TwilioError {
    #[error("invalid twilio base url: {0}")]
    InvalidBaseUrl(String),
    #[error("invalid twilio response metadata: {0}")]
    InvalidResponseMetadata(String),
    #[error("http transport error: {0}")]
    Transport(String),
    /// Non-2xx response. `status` is the HTTP code; `body` is truncated diagnostic
    /// text — it must never carry the auth token.
    #[error("twilio api error: status {status}")]
    Api { status: u16, body: String },
    #[error("malformed twilio response: {0}")]
    Decode(String),
}

/// Credentials for one call. Borrowed (never stored on the client).
#[derive(Clone, Copy)]
pub struct TwilioCreds<'a> {
    pub account_sid: &'a str,
    pub auth_token: &'a str,
}

impl std::fmt::Debug for TwilioCreds<'_> {
    /// Redact `auth_token` so it never appears in logs or panic output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioCreds")
            .field("account_sid", &self.account_sid)
            .field("auth_token", &REDACTED)
            .finish()
    }
}

/// A Twilio Message resource (the fields the adapter needs). `date_created` is
/// parsed from Twilio's RFC-2822 string; a parse failure leaves it `None` (the
/// adapter treats a record with no parseable date as too old to match). Twilio
/// documents many Message fields as optional; absent string fields are normalized
/// to empty strings to preserve the existing public API.
#[derive(Clone)]
pub struct TwilioMessage {
    pub sid: String,
    pub status: String,
    pub body: String,
    pub direction: String,
    pub date_created: Option<OffsetDateTime>,
}

impl std::fmt::Debug for TwilioMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMessage")
            .field("sid", &redact_non_empty(&self.sid))
            .field("status", &self.status)
            .field("body", &redact_non_empty(&self.body))
            .field("direction", &self.direction)
            .field("date_created", &self.date_created)
            .finish()
    }
}

/// One page of a Messages list: the records plus the next page URI (absolute path
/// on the API host) when more pages exist.
#[derive(Clone)]
pub struct TwilioMessagePage {
    pub messages: Vec<TwilioMessage>,
    pub next_page_uri: Option<String>,
}

impl std::fmt::Debug for TwilioMessagePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let next_page_uri = self.next_page_uri.as_ref().map(|_| REDACTED);
        f.debug_struct("TwilioMessagePage")
            .field("messages", &self.messages)
            .field("next_page_uri", &next_page_uri)
            .finish()
    }
}

// --- wire types ------------------------------------------------------------

#[derive(Deserialize)]
struct WireMessage {
    sid: Option<String>,
    status: Option<String>,
    body: Option<String>,
    direction: Option<String>,
    date_created: Option<String>,
}

impl WireMessage {
    fn into_message(self) -> TwilioMessage {
        TwilioMessage {
            sid: self.sid.unwrap_or_default(),
            status: self.status.unwrap_or_default(),
            body: self.body.unwrap_or_default(),
            direction: self.direction.unwrap_or_default(),
            date_created: self
                .date_created
                .and_then(|s| OffsetDateTime::parse(&s, &Rfc2822).ok()),
        }
    }
}

#[derive(Deserialize)]
struct WireListResponse {
    messages: Vec<WireMessage>,
    #[serde(default)]
    next_page_uri: Option<String>,
}

const REDACTED: &str = "<redacted>";

fn redact_non_empty(value: &str) -> &str {
    if value.is_empty() { "" } else { REDACTED }
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

const MAX_DIAGNOSTIC_BODY_BYTES: usize = 2048;

fn request_span(base_url: &Url, operation: &'static str, method: &'static str) -> tracing::Span {
    let peer_name = base_url.host_str().unwrap_or("<unknown>");
    tracing::debug_span!(
        "twilio2.request",
        operation,
        http.method = method,
        net.peer.name = %peer_name
    )
}

fn transport_error(e: &reqwest::Error, sensitive_values: &[&str]) -> TwilioError {
    let message = sanitize_diagnostic(reqwest_error_message(e), sensitive_values);
    tracing::warn!(error = %message, "twilio transport error");
    TwilioError::Transport(message)
}

pub struct TwilioClient {
    http: reqwest::Client,
    base_url: Url,
}

impl TwilioClient {
    /// Build over an injected (shared) `reqwest::Client`. `base_url` may include
    /// a path prefix and is normalized with a trailing slash.
    ///
    /// # Panics
    ///
    /// Panics when `base_url` is not a valid HTTPS base URL. Use
    /// [`Self::try_new`] when invalid configuration should be reported as an
    /// error instead.
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self::try_new(http, base_url).expect("invalid Twilio base URL")
    }

    /// Fallible constructor for configuration paths that want to surface a typed
    /// startup error instead of panicking.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError::InvalidBaseUrl`] when `base_url` is empty, not
    /// HTTPS, lacks a host, includes embedded credentials, or includes a query
    /// string or fragment.
    pub fn try_new(
        http: reqwest::Client,
        base_url: impl Into<String>,
    ) -> Result<Self, TwilioError> {
        Ok(Self {
            http,
            base_url: normalize_base_url(base_url.into()).map_err(TwilioError::InvalidBaseUrl)?,
        })
    }

    /// `POST /2010-04-01/Accounts/{Sid}/Messages.json` — create (send) a message.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if URL construction fails, the HTTP request fails,
    /// Twilio returns a non-2xx status, or the JSON response cannot be decoded.
    pub async fn create_message(
        &self,
        creds: TwilioCreds<'_>,
        to: &str,
        from: &str,
        body: &str,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            tracing::debug!("sending twilio message create request");
            let sensitive_values = [creds.account_sid, creds.auth_token, to, from, body];
            let url =
                self.endpoint_url(&["2010-04-01", "Accounts", creds.account_sid, "Messages.json"])?;
            let form = [("To", to), ("From", from), ("Body", body)];
            let resp = self
                .http
                .post(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .form(&form)
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            let msg: WireMessage = decode_2xx(resp, &sensitive_values).await?;
            let msg = msg.into_message();
            tracing::debug!(
                twilio.message_status = %msg.status,
                twilio.message_direction = %msg.direction,
                twilio.message_sid_present = !msg.sid.is_empty(),
                "twilio message created"
            );
            Ok(msg)
        }
        .instrument(request_span(&self.base_url, "create_message", "POST"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{Sid}/Messages/{MessageSid}.json` — status lookup.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if URL construction fails, the HTTP request fails,
    /// Twilio returns a non-2xx status, or the JSON response cannot be decoded.
    pub async fn fetch_message(
        &self,
        creds: TwilioCreds<'_>,
        sid: &str,
    ) -> Result<TwilioMessage, TwilioError> {
        async move {
            tracing::debug!("sending twilio message fetch request");
            let sensitive_values = [creds.account_sid, creds.auth_token, sid];
            let url = self.endpoint_url(&[
                "2010-04-01",
                "Accounts",
                creds.account_sid,
                "Messages",
                &format!("{sid}.json"),
            ])?;
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            let msg: WireMessage = decode_2xx(resp, &sensitive_values).await?;
            let msg = msg.into_message();
            tracing::debug!(
                twilio.message_status = %msg.status,
                twilio.message_direction = %msg.direction,
                twilio.message_sid_present = !msg.sid.is_empty(),
                "twilio message fetched"
            );
            Ok(msg)
        }
        .instrument(request_span(&self.base_url, "fetch_message", "GET"))
        .await
    }

    /// `GET /2010-04-01/Accounts/{Sid}/Messages.json?To=&From=&PageSize=` — first
    /// page of a filtered list. The adapter drives paging via [`list_page_uri`].
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if URL construction fails, the HTTP request fails,
    /// Twilio returns a non-2xx status, or the JSON response cannot be decoded.
    pub async fn list_messages(
        &self,
        creds: TwilioCreds<'_>,
        to: &str,
        from: &str,
        page_size: u32,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            tracing::debug!(
                twilio.page_size = page_size,
                "sending twilio message list request"
            );
            let sensitive_values = [creds.account_sid, creds.auth_token, to, from];
            let mut url =
                self.endpoint_url(&["2010-04-01", "Accounts", creds.account_sid, "Messages.json"])?;
            let page_size = page_size.to_string();
            url.query_pairs_mut()
                .append_pair("To", to)
                .append_pair("From", from)
                .append_pair("PageSize", page_size.as_str());
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            self.read_page(resp, &sensitive_values, &url, creds.account_sid)
                .await
        }
        .instrument(request_span(&self.base_url, "list_messages", "GET"))
        .await
    }

    /// Fetch a subsequent page by the `next_page_uri` Twilio returned (an absolute
    /// path on the API host, e.g. `/2010-04-01/Accounts/.../Messages.json?Page=1&…`).
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if `next_page_uri` is invalid, points at another
    /// origin, is not a Messages page for the credential account, the HTTP
    /// request fails, Twilio returns a non-2xx status, or the JSON response
    /// cannot be decoded.
    pub async fn list_page_uri(
        &self,
        creds: TwilioCreds<'_>,
        next_page_uri: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            tracing::debug!("sending twilio message page request");
            let sensitive_values = [creds.account_sid, creds.auth_token, next_page_uri];
            let url = self.page_uri_url(next_page_uri, creds.account_sid)?;
            let resp = self
                .http
                .get(url.clone())
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            self.read_page(resp, &sensitive_values, &url, creds.account_sid)
                .await
        }
        .instrument(request_span(&self.base_url, "list_page_uri", "GET"))
        .await
    }

    async fn read_page(
        &self,
        resp: reqwest::Response,
        sensitive_values: &[&str],
        current_url: &Url,
        account_sid: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireListResponse = decode_2xx(resp, sensitive_values).await?;
        let next_page_uri = parsed.next_page_uri.filter(|u| !u.is_empty());
        if let Some(next_page_uri) = next_page_uri.as_ref() {
            let next_url = self.page_uri_url(next_page_uri, account_sid)?;
            validate_next_page_query(current_url, &next_url)?;
        }
        let page = TwilioMessagePage {
            messages: parsed
                .messages
                .into_iter()
                .map(WireMessage::into_message)
                .collect(),
            next_page_uri,
        };
        tracing::debug!(
            twilio.message_count = page.messages.len(),
            twilio.has_next_page = page.next_page_uri.is_some(),
            "twilio message page decoded"
        );
        Ok(page)
    }

    fn endpoint_url(&self, segments: &[&str]) -> Result<Url, TwilioError> {
        endpoint_url_from_base(&self.base_url, segments)
    }

    fn page_uri_url(&self, next_page_uri: &str, account_sid: &str) -> Result<Url, TwilioError> {
        page_uri_url_from_base(&self.base_url, next_page_uri, account_sid)
    }
}

/// Decode a 2xx JSON body or map a non-2xx/transport failure to [`TwilioError`].
async fn decode_2xx<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    sensitive_values: &[&str],
) -> Result<T, TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        let body = match read_limited_response_text(resp).await {
            Ok(body) => body,
            Err(e) => reqwest_error_message(&e),
        };
        let body = sanitize_diagnostic(body, sensitive_values);
        tracing::warn!(
            http.status_code = status.as_u16(),
            response.body_len = body.len(),
            "twilio api error"
        );
        return Err(TwilioError::Api {
            status: status.as_u16(),
            body: truncate(body),
        });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| transport_error(&e, sensitive_values))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        let message = sanitize_diagnostic(e.to_string(), sensitive_values);
        tracing::warn!(error = %message, "failed to decode twilio response");
        TwilioError::Decode(message)
    })
}

async fn read_limited_response_text(mut resp: reqwest::Response) -> Result<String, reqwest::Error> {
    let mut body = Vec::new();
    while body.len() <= MAX_DIAGNOSTIC_BODY_BYTES {
        let Some(chunk) = resp.chunk().await? else {
            break;
        };
        let remaining = MAX_DIAGNOSTIC_BODY_BYTES + 1 - body.len();
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn normalize_base_url(raw: String) -> Result<Url, String> {
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

fn endpoint_url_from_base(base_url: &Url, segments: &[&str]) -> Result<Url, TwilioError> {
    let mut url = base_url.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned()))?;
        path.pop_if_empty();
        path.extend(segments);
    }
    Ok(url)
}

fn page_uri_url_from_base(
    base_url: &Url,
    next_page_uri: &str,
    account_sid: &str,
) -> Result<Url, TwilioError> {
    let url = if next_page_uri.starts_with('/') && !next_page_uri.starts_with("//") {
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
        let path_and_query = next_page_uri.strip_prefix('/').unwrap_or(next_page_uri);
        if path_and_query.contains('#') {
            return Err(TwilioError::InvalidResponseMetadata(
                "next_page_uri included a fragment".to_owned(),
            ));
        }
        let (path_part, query) = path_and_query
            .split_once('?')
            .map_or((path_and_query, None), |(path, query)| (path, Some(query)));
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| TwilioError::InvalidBaseUrl("base URL cannot be a base".to_owned()))?;
            path.clear();
            path.extend(base_prefix.iter().map(String::as_str));
            path.extend(path_part.split('/').filter(|segment| !segment.is_empty()));
        }
        url.set_query(query);
        url.set_fragment(None);
        url
    } else {
        base_url
            .join(next_page_uri)
            .map_err(|e| TwilioError::InvalidResponseMetadata(e.to_string()))?
    };
    if url.scheme() != base_url.scheme()
        || url.host_str() != base_url.host_str()
        || url.port_or_known_default() != base_url.port_or_known_default()
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri changed API origin".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri included embedded credentials".to_owned(),
        ));
    }
    if url.fragment().is_some() {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri included a fragment".to_owned(),
        ));
    }
    validate_messages_page_uri(base_url, &url, account_sid)?;
    Ok(url)
}

fn validate_messages_page_uri(
    base_url: &Url,
    page_url: &Url,
    account_sid: &str,
) -> Result<(), TwilioError> {
    let base_prefix: Vec<&str> = base_url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    let page_segments: Vec<&str> = page_url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    let Some(api_segments) = page_segments.strip_prefix(base_prefix.as_slice()) else {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri left the configured API base path".to_owned(),
        ));
    };
    if api_segments.len() != 4
        || api_segments[0] != "2010-04-01"
        || api_segments[1] != "Accounts"
        || api_segments[2] != account_sid
        || api_segments[3] != "Messages.json"
    {
        return Err(TwilioError::InvalidResponseMetadata(
            "next_page_uri is not a Messages page for this account".to_owned(),
        ));
    }
    validate_messages_page_query_keys(page_url)?;
    Ok(())
}

fn validate_messages_page_query_keys(page_url: &Url) -> Result<(), TwilioError> {
    let mut seen = Vec::new();
    for (key, _) in page_url.query_pairs() {
        if !matches!(
            key.as_ref(),
            "To" | "From" | "PageSize" | "Page" | "PageToken"
        ) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri has unsupported query parameter {key:?}"
            )));
        }
        if seen.iter().any(|candidate| candidate == key.as_ref()) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri repeated query parameter {key:?}"
            )));
        }
        seen.push(key.into_owned());
    }
    Ok(())
}

fn validate_next_page_query(current_url: &Url, next_url: &Url) -> Result<(), TwilioError> {
    for key in ["To", "From", "PageSize"] {
        if query_values(current_url, key) != query_values(next_url, key) {
            return Err(TwilioError::InvalidResponseMetadata(format!(
                "next_page_uri changed {key} query parameter"
            )));
        }
    }
    Ok(())
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

fn sanitize_diagnostic(s: String, sensitive_values: &[&str]) -> String {
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
            if matches!(sep, Some('=') | Some(':')) && is_sensitive_key(&s[key_start..key_end]) {
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
        let ch = s[i..]
            .chars()
            .next()
            .expect("index is inside a valid utf-8 string");
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
        let ch = s[i..]
            .chars()
            .next()
            .expect("index is inside a valid utf-8 string");
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
        let ch = s[i..]
            .chars()
            .next()
            .expect("index is inside a valid utf-8 string");
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
            | "body"
            | "to"
            | "from"
            | "url"
            | "uri"
            | "nextpageuri"
    )
}

fn normalized_key(key: &str) -> String {
    key.to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
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
        let ch = s[i..]
            .chars()
            .next()
            .expect("index is inside a valid utf-8 string");
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
        let ch = s[i..]
            .chars()
            .next()
            .expect("index is inside a valid utf-8 string");
        if ch.is_ascii_whitespace() || matches!(ch, '&' | ',' | '}' | ']' | '"' | '\'') {
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
    #![allow(clippy::unwrap_used)]

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use rcgen::CertifiedKey;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::{
        endpoint_url_from_base, normalize_base_url, page_uri_url_from_base, sanitize_diagnostic,
    };
    use crate::TwilioError;

    #[derive(Clone)]
    struct MockResponse {
        status: u16,
        body: String,
        content_length: Option<usize>,
    }

    impl MockResponse {
        fn json(body: impl Into<String>) -> Self {
            Self {
                status: 200,
                body: body.into(),
                content_length: None,
            }
        }

        fn truncated(status: u16, body: impl Into<String>, content_length: usize) -> Self {
            Self {
                status,
                body: body.into(),
                content_length: Some(content_length),
            }
        }
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl RecordedRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    struct HttpsMockServer {
        base_url: String,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
    }

    impl HttpsMockServer {
        async fn start(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let acceptor = tls_acceptor();
            let expected_requests = responses.len();
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let task_responses = Arc::clone(&responses);
            let task_requests = Arc::clone(&requests);

            tokio::spawn(async move {
                for _ in 0..expected_requests {
                    let (stream, _) = listener.accept().await.unwrap();
                    let acceptor = acceptor.clone();
                    let responses = Arc::clone(&task_responses);
                    let requests = Arc::clone(&task_requests);

                    tokio::spawn(async move {
                        let mut stream = acceptor.accept(stream).await.unwrap();
                        let request = read_http_request(&mut stream).await.unwrap();
                        let response = {
                            let mut responses = responses.lock().unwrap();
                            responses.pop_front().unwrap()
                        };
                        {
                            let mut requests = requests.lock().unwrap();
                            requests.push(request);
                        }
                        write_http_response(&mut stream, response).await.unwrap();
                    });
                }
            });

            Self {
                base_url: format!("https://{addr}"),
                requests,
            }
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn tls_acceptor() -> TlsAcceptor {
        let CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec![
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
        ])
        .unwrap();
        let cert_chain = vec![cert.der().clone()];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .unwrap();

        TlsAcceptor::from(Arc::new(config))
    }

    async fn read_http_request<S: AsyncRead + Unpin>(
        stream: &mut S,
    ) -> std::io::Result<RecordedRequest> {
        let mut raw = Vec::new();
        let mut chunk = [0; 1024];
        while header_end(&raw).is_none() {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
        }

        let header_end = header_end(&raw).unwrap();
        let header_text = String::from_utf8_lossy(&raw[..header_end]);
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().unwrap();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap().to_owned();
        let path = request_parts.next().unwrap().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
            })
            .collect();
        let content_length = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.parse::<usize>().unwrap())
            .unwrap_or(0);
        let mut body = raw[header_end + 4..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(content_length);

        Ok(RecordedRequest {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    fn header_end(raw: &[u8]) -> Option<usize> {
        raw.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response<S: AsyncWrite + Unpin>(
        stream: &mut S,
        response: MockResponse,
    ) -> std::io::Result<()> {
        let reason = if response.status == 200 {
            "OK"
        } else {
            "Error"
        };
        let content_length = response.content_length.unwrap_or(response.body.len());
        let headers = format!(
            "HTTP/1.1 {} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            response.status, content_length
        );
        stream.write_all(headers.as_bytes()).await?;
        stream.write_all(response.body.as_bytes()).await?;
        stream.shutdown().await
    }

    fn test_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .unwrap()
    }

    fn test_creds() -> super::TwilioCreds<'static> {
        super::TwilioCreds {
            account_sid: "AC123",
            auth_token: "token",
        }
    }

    fn assert_basic_auth(request: &RecordedRequest) {
        assert_eq!(
            request.header("authorization"),
            Some("Basic QUMxMjM6dG9rZW4=")
        );
    }

    #[tokio::test]
    async fn public_create_and_fetch_methods_send_expected_requests() {
        let server = HttpsMockServer::start(vec![
            MockResponse::json(
                r#"{"sid":"SMcreated","status":"queued","body":"hello world","direction":"outbound-api"}"#,
            ),
            MockResponse::json(
                r#"{"sid":"SMfetched","status":"delivered","body":"hello world","direction":"outbound-api"}"#,
            ),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();

        let created = client
            .create_message(creds, "+15551234567", "+15557654321", "hello world")
            .await
            .unwrap();
        let fetched = client.fetch_message(creds, "SM fetch/123").await.unwrap();

        assert_eq!(created.sid, "SMcreated");
        assert_eq!(created.status, "queued");
        assert_eq!(fetched.sid, "SMfetched");
        assert_eq!(fetched.status, "delivered");
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/2010-04-01/Accounts/AC123/Messages.json");
        assert_eq!(
            requests[0].body,
            "To=%2B15551234567&From=%2B15557654321&Body=hello+world"
        );
        assert_basic_auth(&requests[0]);
        assert_eq!(requests[1].method, "GET");
        assert_eq!(
            requests[1].path,
            "/2010-04-01/Accounts/AC123/Messages/SM%20fetch%2F123.json"
        );
        assert_basic_auth(&requests[1]);
    }

    #[tokio::test]
    async fn public_list_methods_send_expected_requests_and_follow_page_uri() {
        let next_page_uri = "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&PageSize=2&Page=1&PageToken=abc";
        let first_body = format!(
            r#"{{
                "messages": [
                    {{"sid":"SMfirst","status":"sent","body":"one","direction":"outbound-api"}}
                ],
                "next_page_uri": "{next_page_uri}"
            }}"#
        );
        let server = HttpsMockServer::start(vec![
            MockResponse::json(first_body),
            MockResponse::json(
                r#"{"messages":[{"sid":"SMsecond","status":"delivered","body":"two","direction":"outbound-api"}],"next_page_uri":null}"#,
            ),
        ])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();
        let creds = test_creds();

        let first = client
            .list_messages(creds, "+15551234567", "+15557654321", 2)
            .await
            .unwrap();
        let second = client
            .list_page_uri(creds, first.next_page_uri.as_deref().unwrap())
            .await
            .unwrap();

        assert_eq!(first.messages[0].sid, "SMfirst");
        assert_eq!(first.next_page_uri.as_deref(), Some(next_page_uri));
        assert_eq!(second.messages[0].sid, "SMsecond");
        assert!(second.next_page_uri.is_none());
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&PageSize=2"
        );
        assert_basic_auth(&requests[0]);
        assert_eq!(requests[1].method, "GET");
        assert_eq!(requests[1].path, next_page_uri);
        assert_basic_auth(&requests[1]);
    }

    #[tokio::test]
    async fn non_success_response_keeps_api_status_when_body_stream_fails() {
        let server = HttpsMockServer::start(vec![MockResponse::truncated(
            429,
            r#"{"message":"rate"#,
            128,
        )])
        .await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();

        let err = client
            .fetch_message(test_creds(), "SM123")
            .await
            .expect_err("truncated 429 should be classified as an API error");

        assert!(matches!(err, TwilioError::Api { status: 429, .. }));
    }

    #[tokio::test]
    async fn success_response_body_stream_failure_is_transport_not_decode() {
        let server =
            HttpsMockServer::start(vec![MockResponse::truncated(200, r#"{"sid":"SM"#, 128)]).await;
        let client = super::TwilioClient::try_new(test_http_client(), &server.base_url).unwrap();

        let err = client
            .fetch_message(test_creds(), "SM123")
            .await
            .expect_err("truncated 200 body should be classified as transport");

        assert!(
            matches!(err, TwilioError::Transport(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn normalizes_base_url_with_trailing_slash() {
        let url = normalize_base_url(" https://api.twilio.com/ ".to_owned()).unwrap();
        assert_eq!(url.as_str(), "https://api.twilio.com/");

        let url = normalize_base_url("https://api.twilio.com/proxy".to_owned()).unwrap();
        assert_eq!(url.as_str(), "https://api.twilio.com/proxy/");
    }

    #[test]
    fn rejects_unusable_base_urls() {
        assert!(normalize_base_url("".to_owned()).is_err());
        assert!(normalize_base_url("http://api.twilio.com".to_owned()).is_err());
        assert!(normalize_base_url("file:///tmp/twilio".to_owned()).is_err());
        assert!(normalize_base_url("https://sid:token@api.twilio.com".to_owned()).is_err());
        assert!(normalize_base_url("https://api.twilio.com?x=1".to_owned()).is_err());
        assert!(normalize_base_url("https://api.twilio.com#frag".to_owned()).is_err());
    }

    #[test]
    fn endpoint_join_encodes_path_segments() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy".to_owned()).unwrap();
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
            .unwrap()
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC%2F123/Messages/SM%20123.json"
        );
    }

    #[test]
    fn page_uri_join_rejects_origin_changes() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy".to_owned()).unwrap();
        assert_eq!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?Page=1",
                "AC123",
            )
            .unwrap()
            .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages.json?Page=1"
        );
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "https://example.test/2010-04-01/Accounts/AC123/Messages.json",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Calls.json?Page=1",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC999/Messages.json?Page=1",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?Page=1&Unexpected=1",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "https://user:pass@api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?Page=1#frag",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?Page=1#frag",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
        assert!(matches!(
            page_uri_url_from_base(
                &base_url,
                "/2010-04-01/Accounts/AC123/Messages.json?Page=1&Page=2",
                "AC123",
            ),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
    }

    #[test]
    fn next_page_query_preserves_stable_filters() {
        let base_url = normalize_base_url("https://api.twilio.com/proxy".to_owned()).unwrap();
        let current_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&PageSize=50&Page=0",
            "AC123",
        )
        .unwrap();
        let next_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321&PageSize=50&Page=1&PageToken=abc",
            "AC123",
        )
        .unwrap();
        assert!(super::validate_next_page_query(&current_url, &next_url).is_ok());

        let changed_filter_url = page_uri_url_from_base(
            &base_url,
            "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15550000000&From=%2B15557654321&PageSize=50&Page=1&PageToken=abc",
            "AC123",
        )
        .unwrap();
        assert!(matches!(
            super::validate_next_page_query(&current_url, &changed_filter_url),
            Err(TwilioError::InvalidResponseMetadata(_))
        ));
    }

    #[test]
    fn wire_messages_tolerate_optional_twilio_fields() {
        let msg = serde_json::from_str::<super::WireMessage>(
            r#"{"sid":null,"status":null,"direction":"outbound-api"}"#,
        )
        .unwrap()
        .into_message();

        assert_eq!(msg.sid, "");
        assert_eq!(msg.status, "");
        assert_eq!(msg.body, "");
        assert_eq!(msg.direction, "outbound-api");
    }

    #[test]
    fn wire_list_response_requires_messages_field() {
        let result = serde_json::from_str::<super::WireListResponse>(
            r#"{"next_page_uri":"/2010-04-01/Accounts/AC123/Messages.json?Page=1"}"#,
        );
        assert!(result.is_err());
        let err = result.err().unwrap();

        assert!(err.to_string().contains("missing field `messages`"));
    }

    #[test]
    fn wire_decoding_ignores_unknown_twilio_fields() {
        let parsed = serde_json::from_str::<super::WireListResponse>(
            r#"{
                "messages": [{
                    "sid": "SM123",
                    "status": "sent",
                    "body": "hello",
                    "direction": "outbound-api",
                    "date_created": "Tue, 28 Jun 2026 20:00:00 +0000",
                    "account_sid": "AC123",
                    "to": "+15551234567",
                    "from": "+15557654321",
                    "uri": "/2010-04-01/Accounts/AC123/Messages/SM123.json"
                }],
                "next_page_uri": null,
                "page_size": 50
            }"#,
        )
        .unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].sid.as_deref(), Some("SM123"));
    }

    #[test]
    fn creds_debug_redacts_auth_token() {
        let creds = super::TwilioCreds {
            account_sid: "AC123",
            auth_token: "super-secret-token",
        };

        let rendered = format!("{creds:?}");

        assert!(rendered.contains("AC123"));
        assert!(!rendered.contains("super-secret-token"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn response_debug_redacts_message_payloads_and_page_uris() {
        let page = super::TwilioMessagePage {
            messages: vec![super::TwilioMessage {
                sid: "SMsecret".to_owned(),
                status: "sent".to_owned(),
                body: "hello from the sms body".to_owned(),
                direction: "outbound-api".to_owned(),
                date_created: None,
            }],
            next_page_uri: Some(
                "/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567".to_owned(),
            ),
        };

        let rendered = format!("{page:?}");

        for leaked in [
            "SMsecret",
            "hello from the sms body",
            "/2010-04-01/Accounts/AC123/Messages.json",
            "%2B15551234567",
        ] {
            assert!(
                !rendered.contains(leaked),
                "debug output leaked {leaked:?}: {rendered}"
            );
        }
        assert!(rendered.contains("sent"));
        assert!(rendered.contains("outbound-api"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_known_secrets_auth_headers_and_message_fields() {
        let diagnostic = concat!(
            "url=https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?",
            "To=%2B15551234567&From=%2B15557654321&Body=hello&AuthToken=abc123 ",
            "Authorization: Basic dXNlcjpwYXNz ",
            r#"Authorization: "Basic cXVvdGVkOnNlY3JldA==" "#,
            r#"json={"password":"pw","api_key":"key123","body":"secret text"} "#,
            "raw=super-secret-token bearer abc.def.ghi"
        );

        let redacted = sanitize_diagnostic(diagnostic.to_owned(), &["super-secret-token"]);

        for leaked in [
            "api.twilio.com",
            "2010-04-01",
            "Accounts",
            "%2B15551234567",
            "%2B15557654321",
            "hello",
            "abc123",
            "dXNlcjpwYXNz",
            "cXVvdGVkOnNlY3JldA",
            "pw",
            "key123",
            "secret text",
            "super-secret-token",
            "abc.def.ghi",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_urls_even_without_known_values() {
        let redacted = sanitize_diagnostic(
            "transport failed for https://api.twilio.com/2010-04-01/Accounts/AC123/Messages.json?To=%2B15551234567&From=%2B15557654321".to_owned(),
            &[],
        );

        for leaked in [
            "https://api.twilio.com",
            "2010-04-01",
            "Accounts",
            "AC123",
            "%2B15551234567",
            "%2B15557654321",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn diagnostics_redact_operation_specific_request_values() {
        let diagnostic = concat!(
            "request failed for https://api.twilio.com/2010-04-01/Accounts/ACsecret/Messages/",
            "SMsecret.json?To=%2B15551234567&From=%2B15557654321 with body hello+world"
        );

        let redacted = sanitize_diagnostic(
            diagnostic.to_owned(),
            &[
                "ACsecret",
                "SMsecret",
                "%2B15551234567",
                "%2B15557654321",
                "hello+world",
            ],
        );

        for leaked in [
            "ACsecret",
            "SMsecret",
            "%2B15551234567",
            "%2B15557654321",
            "hello+world",
        ] {
            assert!(
                !redacted.contains(leaked),
                "diagnostic leaked {leaked:?}: {redacted}"
            );
        }
    }
}
