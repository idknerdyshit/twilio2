use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, LazyLock, Mutex};

use rcgen::CertifiedKey;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use twilio2::{TwilioAuth, TwilioClient, TwilioClientConfig, TwilioConfig};

pub(super) type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone)]
pub(super) struct MockResponse {
    status: u16,
    body: Vec<u8>,
    content_type: String,
}

impl MockResponse {
    pub(super) fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
        }
    }

    pub(super) fn created_json(body: impl Into<String>) -> Self {
        Self {
            status: 201,
            body: body.into().into_bytes(),
            content_type: "application/json".to_owned(),
        }
    }

    pub(super) fn no_content() -> Self {
        Self {
            status: 204,
            body: Vec::new(),
            content_type: "application/json".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct RecordedRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: String,
}

pub(super) struct HttpsMockServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl HttpsMockServer {
    pub(super) async fn start(responses: Vec<MockResponse>) -> ExampleResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let base_url = format!("https://{addr}");
        let acceptor = tls_acceptor()?;
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
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                let responses = Arc::clone(&task_responses);
                let requests = Arc::clone(&task_requests);

                tokio::spawn(async move {
                    let result = handle_connection(stream, acceptor, responses, requests).await;
                    if result.is_err() {}
                });
            }
        });

        Ok(Self { base_url, requests })
    }

    pub(super) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(super) fn requests(&self) -> ExampleResult<Vec<RecordedRequest>> {
        let requests = self
            .requests
            .lock()
            .map_err(|_| io::Error::other("mock request store lock was poisoned"))?;
        Ok(requests.clone())
    }
}

pub(super) fn client_for(server: &HttpsMockServer) -> ExampleResult<TwilioClient> {
    Ok(TwilioClient::from_config_with_http_builder(
        TwilioClientConfig::new().base_urls(
            TwilioConfig::new()
                .rest_base_url(server.base_url())
                .messaging_base_url(server.base_url())
                .accounts_base_url(format!("{}/v1", server.base_url())),
        ),
        |builder| builder.danger_accept_invalid_certs(true).no_proxy(),
    )?)
}

pub(super) fn creds() -> &'static TwilioAuth {
    static CREDS: LazyLock<TwilioAuth> = LazyLock::new(|| TwilioAuth::auth_token("AC123", "token"));
    &CREDS
}

pub(super) fn missing(name: &'static str) -> io::Error {
    io::Error::other(format!("missing {name}"))
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    responses: Arc<Mutex<VecDeque<MockResponse>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
) -> io::Result<()> {
    let mut stream = acceptor.accept(stream).await?;
    let request = read_http_request(&mut stream).await?;
    let response = {
        let mut responses = responses
            .lock()
            .map_err(|_| io::Error::other("mock response queue lock was poisoned"))?;
        responses
            .pop_front()
            .ok_or_else(|| io::Error::other("mock response queue was empty"))?
    };
    {
        let mut requests = requests
            .lock()
            .map_err(|_| io::Error::other("mock request store lock was poisoned"))?;
        requests.push(request);
    }
    write_http_response(&mut stream, response).await
}

fn tls_acceptor() -> ExampleResult<TlsAcceptor> {
    let CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])?;
    let cert_chain = vec![cert.der().clone()];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

async fn read_http_request<S: AsyncRead + Unpin>(stream: &mut S) -> io::Result<RecordedRequest> {
    let mut raw = Vec::new();
    let mut chunk = [0; 1024];
    while header_end(&raw).is_none() {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
    }

    let header_end =
        header_end(&raw).ok_or_else(|| io::Error::other("request headers were incomplete"))?;
    let header_text = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::other("request line was missing"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| io::Error::other("request method was missing"))?
        .to_owned();
    let path = request_parts
        .next()
        .ok_or_else(|| io::Error::other("request path was missing"))?
        .to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map_or(Ok(0), |(_, value)| value.parse::<usize>())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
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
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_http_response<S: AsyncWrite + Unpin>(
    stream: &mut S,
    response: MockResponse,
) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        _ => "Error",
    };
    let headers = format!(
        "HTTP/1.1 {} {reason}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}
