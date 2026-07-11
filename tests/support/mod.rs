#![allow(dead_code, clippy::unwrap_used, clippy::missing_panics_doc)]

use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};

use rcgen::CertifiedKey;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
#[cfg(feature = "sync")]
use twilio2::BlockingTwilioClient;
#[cfg(feature = "async")]
use twilio2::TwilioClient;
#[cfg(feature = "async")]
use twilio2::TwilioClientConfig;
use twilio2::{TwilioAuth, TwilioConfig};

#[derive(Clone)]
pub struct MockResponse {
    status: u16,
    body: Vec<u8>,
    content_type: String,
    content_length: Option<usize>,
    headers: Vec<(String, String)>,
}

impl MockResponse {
    pub fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    pub fn status_json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    pub fn created_json(body: impl Into<String>) -> Self {
        Self::status_json(201, body)
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: Vec::new(),
            content_type: "application/json".to_owned(),
            content_length: None,
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub struct HttpsMockServer {
    pub base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl HttpsMockServer {
    pub async fn start(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("https://{addr}");
        let acceptor = tls_acceptor();
        let expected_requests = responses.len();
        let responses = responses
            .into_iter()
            .map(|mut response| {
                let body =
                    String::from_utf8_lossy(&response.body).replace("__BASE_URL__", &base_url);
                response.body = body.into_bytes();
                response
            })
            .collect::<VecDeque<_>>();
        let responses = Arc::new(Mutex::new(responses));
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

        Self { base_url, requests }
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[cfg(feature = "async")]
pub fn test_http_client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    install_test_crypto_provider();

    builder.danger_accept_invalid_certs(true).no_proxy()
}

#[cfg(feature = "sync")]
pub fn test_agent() -> ureq::Agent {
    install_test_crypto_provider();

    let builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .tls_config(test_tls_config());
    ureq::Agent::new_with_config(builder.build())
}

pub fn test_creds() -> &'static TwilioAuth {
    static CREDS: LazyLock<TwilioAuth> = LazyLock::new(|| TwilioAuth::auth_token("AC123", "token"));
    &CREDS
}

#[cfg(feature = "async")]
pub fn client_for(server: &HttpsMockServer) -> TwilioClient {
    TwilioClient::from_config_with_http_builder(
        TwilioClientConfig::new().base_urls(twilio_config(&server.base_url)),
        test_http_client,
    )
    .unwrap()
}

#[cfg(feature = "sync")]
pub fn blocking_client_for(server: &HttpsMockServer) -> BlockingTwilioClient {
    BlockingTwilioClient::try_with_config(test_agent(), twilio_config(&server.base_url)).unwrap()
}

pub fn twilio_config(base_url: &str) -> TwilioConfig {
    TwilioConfig::new()
        .rest_base_url(base_url)
        .messaging_base_url(base_url)
        .pricing_base_url(base_url)
        .content_base_url(base_url)
        .accounts_base_url(format!("{base_url}/v1"))
}

pub async fn unused_https_base_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("https://{addr}")
}

fn tls_acceptor() -> TlsAcceptor {
    install_test_crypto_provider();

    let CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
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
        .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
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
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let content_length = response.content_length.unwrap_or(response.body.len());
    let mut headers = format!(
        "HTTP/1.1 {} {reason}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n",
        response.status, response.content_type, content_length
    );
    for (name, value) in &response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

fn install_test_crypto_provider() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[cfg(feature = "sync")]
fn test_tls_config() -> ureq::tls::TlsConfig {
    let builder = ureq::tls::TlsConfig::builder().disable_verification(true);
    #[cfg(all(
        feature = "native-tls",
        not(feature = "rustls"),
        not(feature = "rustls-no-provider")
    ))]
    let builder = builder.provider(ureq::tls::TlsProvider::NativeTls);
    builder.build()
}
