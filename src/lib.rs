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
            .field("auth_token", &"<redacted>")
            .finish()
    }
}

/// A Twilio Message resource (the fields the adapter needs). `date_created` is
/// parsed from Twilio's RFC-2822 string; a parse failure leaves it `None` (the
/// adapter treats a record with no parseable date as too old to match).
#[derive(Debug, Clone)]
pub struct TwilioMessage {
    pub sid: String,
    pub status: String,
    pub body: String,
    pub direction: String,
    pub date_created: Option<OffsetDateTime>,
}

/// One page of a Messages list: the records plus the next page URI (absolute path
/// on the API host) when more pages exist.
#[derive(Debug, Clone)]
pub struct TwilioMessagePage {
    pub messages: Vec<TwilioMessage>,
    pub next_page_uri: Option<String>,
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
            .map_err(|e| TwilioError::Transport(reqwest_error_message(&e)))?;
        let msg: WireMessage = decode_2xx(resp).await?;
        Ok(msg.into_message())
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
            .map_err(|e| TwilioError::Transport(reqwest_error_message(&e)))?;
        let msg: WireMessage = decode_2xx(resp).await?;
        Ok(msg.into_message())
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
            .map_err(|e| TwilioError::Transport(reqwest_error_message(&e)))?;
        self.read_page(resp).await
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
        let url = self.page_uri_url(next_page_uri)?;
        let resp = self
            .http
            .get(url)
            .basic_auth(creds.account_sid, Some(creds.auth_token))
            .send()
            .await
            .map_err(|e| TwilioError::Transport(reqwest_error_message(&e)))?;
        self.read_page(resp).await
    }

    async fn read_page(&self, resp: reqwest::Response) -> Result<TwilioMessagePage, TwilioError> {
        let parsed: WireListResponse = decode_2xx(resp).await?;
        Ok(TwilioMessagePage {
            messages: parsed
                .messages
                .into_iter()
                .map(WireMessage::into_message)
                .collect(),
            next_page_uri: parsed.next_page_uri.filter(|u| !u.is_empty()),
        })
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
) -> Result<T, TwilioError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(TwilioError::Api {
            status: status.as_u16(),
            body: truncate(body),
        });
    }
    resp.json()
        .await
        .map_err(|e| TwilioError::Decode(e.to_string()))
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{endpoint_url_from_base, normalize_base_url, page_uri_url_from_base};

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
}
