//! `twilio2` — a thin `reqwest` client over the Twilio Programmable
//! Messaging REST API. Three surfaces: create a Message (send), fetch a Message by
//! SID (status lookup), and list Messages filtered by To/From with paging (the
//! crash-replay reconcile). It has no Sovereign dependency; Sovereign's
//! `sv_sms::twilio` adapter can wrap it as an `SmsProvider`.
//!
//! The Account SID + Auth Token are passed per call (HTTP basic auth) and never
//! stored on the client. Bodies are returned verbatim — canonical hashing for
//! reconcile is the adapter's job (it shares `sv_sms::body_hash`).

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
/// adapter treats a record with no parseable date as too old to match).
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
    #[serde(default)]
    messages: Vec<WireMessage>,
    #[serde(default)]
    next_page_uri: Option<String>,
}

const REDACTED: &str = "<redacted>";

fn redact_non_empty(value: &str) -> &str {
    if value.is_empty() { "" } else { REDACTED }
}

fn truncate(s: String) -> String {
    const MAX: usize = 2048;
    if s.len() > MAX {
        let mut end = MAX;
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
    /// Build over an injected (shared) `reqwest::Client`. `base_url` has no
    /// trailing slash (see [`DEFAULT_BASE_URL`]).
    ///
    /// # Panics
    ///
    /// Panics when `base_url` is not a valid HTTP(S) base URL. Use
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
    /// HTTP(S), lacks a host, or includes a query string or fragment.
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
                .get(url)
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
            let url =
                self.endpoint_url(&["2010-04-01", "Accounts", creds.account_sid, "Messages.json"])?;
            let page_size = page_size.to_string();
            let query = [("To", to), ("From", from), ("PageSize", page_size.as_str())];
            let resp = self
                .http
                .get(url)
                .query(&query)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            self.read_page(resp, &sensitive_values).await
        }
        .instrument(request_span(&self.base_url, "list_messages", "GET"))
        .await
    }

    /// Fetch a subsequent page by the `next_page_uri` Twilio returned (an absolute
    /// path on the API host, e.g. `/2010-04-01/Accounts/.../Messages.json?Page=1&…`).
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] if `next_page_uri` is invalid or points at another
    /// origin, the HTTP request fails, Twilio returns a non-2xx status, or the
    /// JSON response cannot be decoded.
    pub async fn list_page_uri(
        &self,
        creds: TwilioCreds<'_>,
        next_page_uri: &str,
    ) -> Result<TwilioMessagePage, TwilioError> {
        async move {
            tracing::debug!("sending twilio message page request");
            let sensitive_values = [creds.account_sid, creds.auth_token, next_page_uri];
            let url = self.page_uri_url(next_page_uri)?;
            let resp = self
                .http
                .get(url)
                .basic_auth(creds.account_sid, Some(creds.auth_token))
                .send()
                .await
                .map_err(|e| transport_error(&e, &sensitive_values))?;
            tracing::debug!(
                http.status_code = resp.status().as_u16(),
                "twilio response received"
            );
            self.read_page(resp, &sensitive_values).await
        }
        .instrument(request_span(&self.base_url, "list_page_uri", "GET"))
        .await
    }

    async fn read_page(
        &self,
        resp: reqwest::Response,
        sensitive_values: &[&str],
    ) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireListResponse = decode_2xx(resp, sensitive_values).await?;
        let page = TwilioMessagePage {
            messages: parsed
                .messages
                .into_iter()
                .map(WireMessage::into_message)
                .collect(),
            next_page_uri: parsed.next_page_uri.filter(|u| !u.is_empty()),
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

    fn page_uri_url(&self, next_page_uri: &str) -> Result<Url, TwilioError> {
        page_uri_url_from_base(&self.base_url, next_page_uri)
    }
}

/// Decode a 2xx JSON body or map a non-2xx/transport failure to [`TwilioError`].
async fn decode_2xx<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    sensitive_values: &[&str],
) -> Result<T, TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
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
    resp.json().await.map_err(|e| {
        let message = sanitize_diagnostic(e.to_string(), sensitive_values);
        tracing::warn!(error = %message, "failed to decode twilio response");
        TwilioError::Decode(message)
    })
}

fn normalize_base_url(raw: String) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("base URL is empty".to_owned());
    }

    let mut url = Url::parse(trimmed).map_err(|e| e.to_string())?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(format!(
                "unsupported scheme {scheme:?}; expected http or https"
            ));
        }
    }
    if url.host_str().is_none() {
        return Err("base URL must include a host".to_owned());
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

fn page_uri_url_from_base(base_url: &Url, next_page_uri: &str) -> Result<Url, TwilioError> {
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
            .map_err(|e| TwilioError::InvalidBaseUrl(e.to_string()))?
    };
    if url.scheme() != base_url.scheme()
        || url.host_str() != base_url.host_str()
        || url.port_or_known_default() != base_url.port_or_known_default()
    {
        return Err(TwilioError::InvalidBaseUrl(
            "next_page_uri changed API origin".to_owned(),
        ));
    }
    Ok(url)
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
    redact_authorization_schemes(&s)
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
                    out.push_str(REDACTED);
                    authorization_value_end(s, cursor)
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

    use super::{
        endpoint_url_from_base, normalize_base_url, page_uri_url_from_base, sanitize_diagnostic,
    };

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
        assert!(normalize_base_url("file:///tmp/twilio".to_owned()).is_err());
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
            page_uri_url_from_base(&base_url, "/2010-04-01/Accounts/AC123/Messages.json?Page=1",)
                .unwrap()
                .as_str(),
            "https://api.twilio.com/proxy/2010-04-01/Accounts/AC123/Messages.json?Page=1"
        );
        assert!(
            page_uri_url_from_base(
                &base_url,
                "https://example.test/2010-04-01/Accounts/AC123/Messages.json",
            )
            .is_err()
        );
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
            r#"json={"password":"pw","api_key":"key123","body":"secret text"} "#,
            "raw=super-secret-token bearer abc.def.ghi"
        );

        let redacted = sanitize_diagnostic(diagnostic.to_owned(), &["super-secret-token"]);

        for leaked in [
            "%2B15551234567",
            "%2B15557654321",
            "hello",
            "abc123",
            "dXNlcjpwYXNz",
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
