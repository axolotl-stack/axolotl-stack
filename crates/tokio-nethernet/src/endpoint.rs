//! HTTP signaling port of df-mc/go-nethernet `endpoint/{client,handler,status}.go`.
//! Offers and complete SDP answers use one HTTP round trip; trickle ICE is disabled.
//! Optional credential providers are called per negotiation, as in upstream.

use crate::signaling::{
    Credentials, Signal, SignalErrorCode, Signaling, SignalingChannel, signal_type,
};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Incoming};
use hyper_util::rt::{TokioIo, TokioTimer};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::{TlsAcceptor, rustls};

/// Source: go-nethernet endpoint/handler.go:maxSDPBodySize; also Client.Signal.
pub const MAX_SDP_BODY_SIZE: usize = 1 << 20;
/// Source: go-nethernet endpoint/handler.go:HandlerConfig.New.
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(15);
/// Source: go-nethernet endpoint/handler.go:HandlerConfig.serve.
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Source: go-nethernet endpoint/handler.go:HandlerConfig.serve.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Source: go-nethernet endpoint/client.go:Client.Status / Client.Signal.
const USER_AGENT: &str = "libhttpclient/1.0.0.0";
/// Local resource limits, not wire constants.
const SIGNAL_QUEUE_CAPACITY: usize = 256;
const MAX_SUBSCRIBERS: usize = 64;
const MAX_PENDING: usize = 1024;
const MAX_HTTP_CONNECTIONS: usize = 1024;
const MAX_NETWORK_ID_BYTES: usize = 256;

/// Supplies current credentials for each ICE negotiation, without a refresh timer.
/// Source: endpoint/client.go:ClientConfig.Credentials and handler.go:HandlerConfig.Credentials.
#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn credentials(&self) -> anyhow::Result<Credentials>;
}

/// Server status JSON from go-nethernet endpoint/status.go:Status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    #[serde(rename = "name")]
    pub server_name: String,
    pub protocol: i64,
    pub version: String,
    #[serde(rename = "level")]
    pub level_name: String,
    #[serde(rename = "players")]
    pub player_count: i64,
    #[serde(rename = "maxPlayers")]
    pub max_player_count: i64,
    #[serde(rename = "gameType")]
    pub game_type: i64,
}

impl Status {
    /// Source: endpoint/status.go:Status.RakNet (game types 0/1/2, port 19132).
    pub fn raknet(&self) -> Vec<u8> {
        let game_type = match self.game_type {
            0 => "Survival",
            1 => "Creative",
            2 => "Adventure",
            _ => "Unknown",
        };
        format!(
            "MCPE;{};{};{};{};{};{};{};{};{};19132;19132;0;",
            self.server_name,
            self.protocol,
            self.version,
            self.player_count,
            self.max_player_count,
            rand::random::<u64>() >> 1,
            self.level_name,
            game_type,
            self.game_type
        )
        .into_bytes()
    }

    /// Source: endpoint/status.go:RakNetPongData / parseGameType.
    pub fn from_raknet(data: &[u8]) -> anyhow::Result<Self> {
        let mut fields = std::str::from_utf8(data)?.split(';');
        let mut next = || {
            fields
                .next()
                .ok_or_else(|| anyhow::anyhow!("malformed pong data"))
        };
        next()?;
        let server_name = next()?.to_owned();
        let protocol = next()?.parse()?;
        let version = next()?.to_owned();
        let player_count = next()?.parse()?;
        let max_player_count = next()?.parse()?;
        next()?;
        let level_name = next()?.to_owned();
        let game_type = match next()?.trim().to_ascii_lowercase().as_str() {
            "survival" => 0,
            "creative" => 1,
            "adventure" => 2,
            _ => anyhow::bail!("invalid game type"),
        };
        Ok(Self {
            server_name,
            protocol,
            version,
            level_name,
            player_count,
            max_player_count,
            game_type,
        })
    }
}

/// Configuration from endpoint/client.go:ClientConfig.
pub struct ClientConfig {
    pub http_client: reqwest::Client,
    pub credentials: Option<Arc<dyn CredentialProvider>>,
    /// Empty generates a random uint64, as in ClientConfig.New.
    pub network_id: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            credentials: None,
            network_id: String::new(),
        }
    }
}

impl ClientConfig {
    pub fn build(mut self) -> Arc<Client> {
        if self.network_id.is_empty() {
            self.network_id = rand::random::<u64>().to_string();
        }
        Arc::new(Client {
            config: self,
            subscribers: Mutex::new(Vec::new()),
        })
    }
}

/// HTTP endpoint client. Each receiver is an independent broadcast subscription.
pub struct Client {
    config: ClientConfig,
    subscribers: Mutex<Vec<mpsc::Sender<Signal>>>,
}

impl Client {
    pub fn new() -> Arc<Self> {
        ClientConfig::default().build()
    }

    /// Source: endpoint/client.go:Client.Status.
    pub async fn status(&self, address: &str) -> anyhow::Result<Status> {
        let mut url = parse_url(address)?;
        url.set_path("/v1/join");
        let response = self
            .config
            .http_client
            .get(url)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;
        anyhow::ensure!(
            response.status() == StatusCode::OK,
            "GET /v1/join: {}",
            response.status()
        );
        let body = read_response(response).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub async fn ping(&self, address: &str) -> anyhow::Result<Vec<u8>> {
        Ok(self.status(address).await?.raknet())
    }

    fn notify(&self, signal: Signal) {
        let mut subscribers = self.subscribers.lock();
        subscribers.retain(|sender| !sender.is_closed());
        for sender in subscribers.iter() {
            // Upstream Notifier reports non-admission; it must never block signaling.
            let _ = sender.try_send(signal.clone());
        }
    }
}

/// Source: endpoint/client.go:parseURL. The explicit port is checked before URL
/// normalization, which otherwise strips the default HTTP/HTTPS port.
fn parse_url(address: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(address)?;
    let (_, remainder) = address
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("invalid endpoint URL"))?;
    let authority = remainder.split(['?', '#']).next().unwrap_or_default();
    let explicit_port = authority
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok());
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && !authority.contains('/')
            && explicit_port.is_some(),
        "network ID must be an HTTP/HTTPS URL with port and no path"
    );
    Ok(url)
}

async fn read_response(mut response: reqwest::Response) -> anyhow::Result<Vec<u8>> {
    if let Some(length) = response.content_length() {
        anyhow::ensure!(
            length <= MAX_SDP_BODY_SIZE as u64,
            "response exceeds {MAX_SDP_BODY_SIZE} bytes"
        );
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            chunk.len() <= MAX_SDP_BODY_SIZE.saturating_sub(body.len()),
            "response exceeds {MAX_SDP_BODY_SIZE} bytes"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[async_trait]
impl Signaling for Client {
    /// Source: endpoint/client.go:Client.Signal.
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let mut url = parse_url(&signal.network_id)?;
        match signal.typ.as_str() {
            signal_type::OFFER => {
                anyhow::ensure!(
                    signal.data.len() <= MAX_SDP_BODY_SIZE,
                    "SDP offer exceeds {MAX_SDP_BODY_SIZE} bytes"
                );
                url.path_segments_mut()
                    .map_err(|_| anyhow::anyhow!("invalid endpoint URL"))?
                    .clear()
                    .push("v1")
                    .push("join")
                    .push(&self.config.network_id);
                let response = self
                    .config
                    .http_client
                    .post(url)
                    .header("Content-Type", "application/sdp")
                    .header("User-Agent", USER_AGENT)
                    .body(signal.data)
                    .send()
                    .await?;
                anyhow::ensure!(
                    response.status() == StatusCode::OK,
                    "POST /v1/join: {}",
                    response.status()
                );
                let body = read_response(response).await?;
                anyhow::ensure!(!body.is_empty(), "missing SDP answer in response body");
                let data = String::from_utf8(body)?;
                // Go ParseUint accepts only unsigned decimal digits (no '+' prefix).
                if data.bytes().all(|b| b.is_ascii_digit())
                    && let Ok(code) = data.parse::<u32>()
                {
                    anyhow::bail!("negotiation failed with error code: {code}");
                }
                self.notify(Signal {
                    typ: signal_type::ANSWER.into(),
                    connection_id: signal.connection_id,
                    data,
                    network_id: signal.network_id,
                });
                Ok(())
            }
            signal_type::ERROR => Ok(()),
            signal_type::CANDIDATE => {
                anyhow::bail!("nethernet/endpoint: trickle ICE is not supported")
            }
            _ => anyhow::bail!("nethernet/endpoint: unknown signal type: {}", signal.typ),
        }
    }

    async fn credentials(&self) -> anyhow::Result<Option<Credentials>> {
        match &self.config.credentials {
            Some(provider) => provider.credentials().await.map(Some),
            None => Ok(Some(Credentials::default())),
        }
    }

    fn disable_trickle_ice(&self) -> bool {
        true
    }
    fn network_id(&self) -> String {
        self.config.network_id.clone()
    }
    fn set_pong_data(&self, _data: &[u8]) {
        panic!("nethernet/endpoint: Client.PongData: unsupported");
    }
}

#[async_trait]
impl SignalingChannel for Client {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        let mut subscribers = self.subscribers.lock();
        subscribers.retain(|sender| !sender.is_closed());
        if subscribers.len() == MAX_SUBSCRIBERS {
            return None;
        }
        let (sender, receiver) = mpsc::channel(SIGNAL_QUEUE_CAPACITY);
        subscribers.push(sender);
        Some(receiver)
    }
}

/// Configuration from endpoint/handler.go:HandlerConfig.
pub struct HandlerConfig {
    pub negotiation_timeout: Duration,
    pub credentials: Option<Arc<dyn CredentialProvider>>,
    pub network_id: String,
    pub disable_pong_data: bool,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        Self {
            negotiation_timeout: NEGOTIATION_TIMEOUT,
            credentials: None,
            network_id: String::new(),
            disable_pong_data: false,
        }
    }
}

impl HandlerConfig {
    pub fn build(mut self) -> Arc<Handler> {
        if self.network_id.is_empty() {
            self.network_id = rand::random::<u64>().to_string();
        }
        let (offers, receiver) = mpsc::channel(SIGNAL_QUEUE_CAPACITY);
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Arc::new(Handler {
            config: self,
            pending: Mutex::new(HashMap::new()),
            offers: RwLock::new(offers),
            receiver: Mutex::new(Some(receiver)),
            registered: AtomicBool::new(false),
            status: RwLock::new(Bytes::new()),
            server: Mutex::new(None),
            address: RwLock::new(None),
            shutdown,
        })
    }

    pub async fn serve(self, address: impl ToSocketAddrs) -> anyhow::Result<Arc<Handler>> {
        let listener = TcpListener::bind(address).await?;
        let handler = self.build();
        handler.start_server(listener, None)?;
        Ok(handler)
    }

    /// Source: endpoint/handler.go:HandlerConfig.ServeTLS.
    pub async fn serve_tls(
        self,
        address: impl ToSocketAddrs,
        certificate: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> anyhow::Result<Arc<Handler>> {
        let mut certificate = std::io::BufReader::new(std::fs::File::open(certificate)?);
        let mut key = std::io::BufReader::new(std::fs::File::open(key)?);
        let certificates =
            rustls_pemfile::certs(&mut certificate).collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut key)?
            .ok_or_else(|| anyhow::anyhow!("missing TLS private key"))?;
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certificates, key)?;
        let listener = TcpListener::bind(address).await?;
        let handler = self.build();
        handler.start_server(listener, Some(TlsAcceptor::from(Arc::new(tls))))?;
        Ok(handler)
    }
}

type ConnectionKey = (String, u64);

/// HTTP signaling handler with one listener and bounded pending negotiations.
pub struct Handler {
    config: HandlerConfig,
    pending: Mutex<HashMap<ConnectionKey, mpsc::Sender<Signal>>>,
    offers: RwLock<mpsc::Sender<Signal>>,
    receiver: Mutex<Option<mpsc::Receiver<Signal>>>,
    registered: AtomicBool,
    status: RwLock<Bytes>,
    server: Mutex<Option<JoinHandle<()>>>,
    address: RwLock<Option<SocketAddr>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Handler {
    pub fn new() -> Arc<Self> {
        HandlerConfig::default().build()
    }

    pub async fn serve(address: impl ToSocketAddrs) -> anyhow::Result<Arc<Self>> {
        HandlerConfig::default().serve(address).await
    }

    pub async fn serve_tls(
        address: impl ToSocketAddrs,
        certificate: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> anyhow::Result<Arc<Self>> {
        HandlerConfig::default()
            .serve_tls(address, certificate, key)
            .await
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self.address.read()
    }
    pub fn is_closed(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// No-op for an unbound handler, as endpoint/handler.go:Handler.Close.
    pub fn close(&self) {
        if let Some(task) = self.server.lock().take() {
            self.shutdown.send_replace(true);
            task.abort();
            self.pending.lock().clear();
        }
    }

    /// Source: endpoint/handler.go:Handler.Status.
    pub fn set_status(&self, status: Status) -> anyhow::Result<()> {
        let data = serde_json::to_vec(&status)?;
        anyhow::ensure!(
            data.len() <= MAX_SDP_BODY_SIZE,
            "status exceeds local size limit"
        );
        *self.status.write() = Bytes::from(data);
        Ok(())
    }

    fn start_server(
        self: &Arc<Self>,
        listener: TcpListener,
        tls: Option<TlsAcceptor>,
    ) -> anyhow::Result<()> {
        *self.address.write() = Some(listener.local_addr()?);
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let Ok((stream, _)) = result else {
                            if let Some(handler) = weak.upgrade() { handler.shutdown.send_replace(true); }
                            break;
                        };
                        if connections.len() >= MAX_HTTP_CONNECTIONS { continue; }
                        let handler = weak.clone();
                        let tls = tls.clone();
                        connections.spawn(async move {
                            if let Some(tls) = tls {
                                if let Ok(Ok(stream)) = tokio::time::timeout(READ_TIMEOUT, tls.accept(stream)).await {
                                    serve_connection(stream, handler).await;
                                }
                            } else {
                                serve_connection(stream, handler).await;
                            }
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        *self.server.lock() = Some(task);
        Ok(())
    }

    /// Source: endpoint/handler.go:Handler.ServeHTTP / handlePing / handleOffer.
    /// This can also be mounted in an application's HTTP/TLS server.
    pub async fn handle_request<B>(&self, request: Request<B>) -> Response<Full<Bytes>>
    where
        B: Body<Data = Bytes> + Unpin,
        B::Error: std::fmt::Display,
    {
        let path = request.uri().path();
        if path == "/v1/join" && matches!(*request.method(), Method::GET | Method::HEAD) {
            let status = self.status.read().clone();
            let content_type = (!status.is_empty()).then_some("application/json");
            // Hyper suppresses HEAD body bytes while retaining the GET content length.
            return response(StatusCode::OK, content_type, status);
        }
        if path == "/v1/join" {
            let mut result = text_response(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed\n");
            result
                .headers_mut()
                .insert("allow", http::HeaderValue::from_static("GET, HEAD"));
            return result;
        }
        let Some(network_id) = path
            .strip_prefix("/v1/join/")
            .filter(|value| !value.is_empty() && !value.contains('/'))
        else {
            return text_response(StatusCode::NOT_FOUND, "404 page not found\n");
        };
        if request.method() != Method::POST {
            let mut result = text_response(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed\n");
            result
                .headers_mut()
                .insert("allow", http::HeaderValue::from_static("POST"));
            return result;
        }
        let Ok(network_id) = percent_encoding::percent_decode_str(network_id).decode_utf8() else {
            return text_response(StatusCode::BAD_REQUEST, "Network ID must be uint64");
        };
        if network_id.is_empty()
            || network_id.len() > MAX_NETWORK_ID_BYTES
            || !network_id.bytes().all(|b| b.is_ascii_digit())
            || network_id.parse::<u64>().is_err()
        {
            return text_response(StatusCode::BAD_REQUEST, "Network ID must be uint64");
        }
        let network_id = network_id.into_owned();
        let body = match tokio::time::timeout(READ_TIMEOUT, read_offer(request.into_body())).await {
            Ok(Ok(body)) if !body.is_empty() => body,
            Ok(Err(ReadOfferError::TooLarge)) => {
                return text_response(StatusCode::PAYLOAD_TOO_LARGE, "SDP offer is too large");
            }
            _ => {
                return text_response(StatusCode::BAD_REQUEST, "Missing SDP offer in request body");
            }
        };
        let Ok(offer) = String::from_utf8(body) else {
            return text_response(StatusCode::BAD_REQUEST, "Missing SDP offer in request body");
        };
        match self.negotiate(network_id, offer).await {
            Ok(signal) if signal.typ == signal_type::ANSWER => response(
                StatusCode::OK,
                Some("application/sdp"),
                Bytes::from(signal.data),
            ),
            Ok(signal) if signal.typ == signal_type::ERROR => response(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("text/plain; charset=utf-8"),
                Bytes::from(signal.data),
            ),
            Ok(_) => text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "An error has occurred while handling this request",
            ),
            Err(NegotiateError::NotAdmitted) => {
                text_response(StatusCode::SERVICE_UNAVAILABLE, "Service unavailable")
            }
            Err(NegotiateError::Timeout) => {
                text_response(StatusCode::BAD_GATEWAY, "Timed out waiting for answer")
            }
            Err(NegotiateError::Unavailable) => text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "An error has occured while handling this request",
            ),
        }
    }

    async fn negotiate(&self, network_id: String, offer: String) -> Result<Signal, NegotiateError> {
        if !self.registered.load(Ordering::Acquire) {
            return Err(NegotiateError::Unavailable);
        }
        let notifier = self.offers.read().clone();
        if notifier.is_closed() {
            return Err(NegotiateError::Unavailable);
        }
        let (sender, mut receiver) = mpsc::channel(1);
        let key = {
            let mut pending = self.pending.lock();
            if pending.len() >= MAX_PENDING {
                return Err(NegotiateError::NotAdmitted);
            }
            let mut key = (network_id, rand::random::<u64>());
            while pending.contains_key(&key) {
                key.1 = rand::random();
            }
            pending.insert(key.clone(), sender);
            key
        };
        let mut guard = PendingNegotiation {
            handler: self,
            key,
            notifier,
            cancel: false,
        };
        let signal = Signal {
            typ: signal_type::OFFER.into(),
            connection_id: guard.key.1,
            network_id: guard.key.0.clone(),
            data: offer,
        };
        guard
            .notifier
            .try_send(signal)
            .map_err(|_| NegotiateError::NotAdmitted)?;
        guard.cancel = true;
        match tokio::time::timeout(self.config.negotiation_timeout, receiver.recv()).await {
            Ok(Some(signal)) => {
                guard.cancel = false;
                Ok(signal)
            }
            Ok(None) => Err(NegotiateError::Unavailable),
            Err(_) => Err(NegotiateError::Timeout),
        }
    }
}

async fn serve_connection<T>(stream: T, handler: std::sync::Weak<Handler>)
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = hyper::service::service_fn(move |request: Request<Incoming>| {
        let handler = handler.upgrade();
        async move {
            Ok::<_, Infallible>(match handler {
                Some(handler) => handler.handle_request(request).await,
                None => text_response(StatusCode::SERVICE_UNAVAILABLE, "Service unavailable"),
            })
        }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .keep_alive(false)
        .max_buf_size(64 * 1024)
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

impl Drop for Handler {
    fn drop(&mut self) {
        if let Some(task) = self.server.get_mut().take() {
            task.abort();
        }
    }
}

enum NegotiateError {
    NotAdmitted,
    Timeout,
    Unavailable,
}
enum ReadOfferError {
    TooLarge,
    Invalid,
}

async fn read_offer<B>(mut body: B) -> Result<Vec<u8>, ReadOfferError>
where
    B: Body<Data = Bytes> + Unpin,
{
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| ReadOfferError::Invalid)?;
        if let Ok(chunk) = frame.into_data() {
            if chunk.len() > MAX_SDP_BODY_SIZE.saturating_sub(data.len()) {
                return Err(ReadOfferError::TooLarge);
            }
            data.extend_from_slice(&chunk);
        }
    }
    Ok(data)
}

struct PendingNegotiation<'a> {
    handler: &'a Handler,
    key: ConnectionKey,
    notifier: mpsc::Sender<Signal>,
    cancel: bool,
}

impl Drop for PendingNegotiation<'_> {
    fn drop(&mut self) {
        self.handler.pending.lock().remove(&self.key);
        if self.cancel {
            // Source: endpoint/handler.go:Handler.negotiate, including cancellation.
            let _ = self.notifier.try_send(Signal::error(
                self.key.1,
                self.key.0.clone(),
                SignalErrorCode::NegotiationTimeoutWaitingForResponse,
            ));
        }
    }
}

fn response(
    status: StatusCode,
    content_type: Option<&'static str>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("connection", http::HeaderValue::from_static("close"));
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert("content-type", http::HeaderValue::from_static(content_type));
    }
    response
}

fn text_response(status: StatusCode, text: &'static str) -> Response<Full<Bytes>> {
    response(
        status,
        Some("text/plain"),
        Bytes::from_static(text.as_bytes()),
    )
}

#[async_trait]
impl Signaling for Handler {
    /// Source: endpoint/handler.go:Handler.Signal.
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        anyhow::ensure!(
            signal.typ != signal_type::CANDIDATE,
            "disable trickle ICE in listener configuration"
        );
        anyhow::ensure!(
            signal.data.len() <= MAX_SDP_BODY_SIZE,
            "SDP answer exceeds {MAX_SDP_BODY_SIZE} bytes"
        );
        let key = (signal.network_id.clone(), signal.connection_id);
        let pending = self.pending.lock();
        let sender = pending
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("unexpected connection ID: {}/{}", key.0, key.1))?;
        sender
            .try_send(signal)
            .map_err(|_| anyhow::anyhow!("channel buffer is full or negotiation closed"))
    }

    async fn credentials(&self) -> anyhow::Result<Option<Credentials>> {
        match &self.config.credentials {
            Some(provider) => provider.credentials().await.map(Some),
            None => Ok(Some(Credentials::default())),
        }
    }

    async fn closed(&self) {
        let mut shutdown = self.shutdown.subscribe();
        while !*shutdown.borrow_and_update() {
            if shutdown.changed().await.is_err() {
                break;
            }
        }
    }

    fn disable_trickle_ice(&self) -> bool {
        true
    }
    fn network_id(&self) -> String {
        self.config.network_id.clone()
    }
    fn set_pong_data(&self, data: &[u8]) {
        if self.config.disable_pong_data {
            return;
        }
        match Status::from_raknet(data).and_then(|status| self.set_status(status)) {
            Ok(()) => {}
            Err(error) => tracing::warn!(%error, "invalid endpoint pong data"),
        }
    }
}

#[async_trait]
impl SignalingChannel for Handler {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        let mut initial = self.receiver.lock();
        let receiver = if let Some(receiver) = initial.take() {
            receiver
        } else {
            let mut offers = self.offers.write();
            if !offers.is_closed() {
                return None;
            }
            let (sender, receiver) = mpsc::channel(SIGNAL_QUEUE_CAPACITY);
            *offers = sender;
            receiver
        };
        self.registered.store(true, Ordering::Release);
        Some(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn head_status_preserves_get_content_length() {
        let handler = Handler::serve("127.0.0.1:0").await.unwrap();
        handler
            .set_status(Status {
                server_name: "example".into(),
                ..Default::default()
            })
            .unwrap();
        let url = format!("http://{}/v1/join", handler.local_addr().unwrap());
        let client = reqwest::Client::new();
        let get = client.get(&url).send().await.unwrap();
        let expected = get.content_length().unwrap();
        let head = client.head(&url).send().await.unwrap();
        assert_eq!(
            head.headers()["content-length"]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap(),
            expected
        );
        assert!(head.bytes().await.unwrap().is_empty());
        handler.close();
    }

    #[tokio::test]
    async fn subscriptions_are_bounded_and_dropped_receivers_unregister() {
        let client = Client::new();
        let mut subscriptions = Vec::new();
        for _ in 0..MAX_SUBSCRIBERS {
            subscriptions.push(client.take_signal_receiver().await.unwrap());
        }
        assert!(client.take_signal_receiver().await.is_none());
        drop(subscriptions);
        assert!(client.take_signal_receiver().await.is_some());
        let handler = Handler::new();
        let receiver = handler.take_signal_receiver().await.unwrap();
        assert!(handler.take_signal_receiver().await.is_none());
        drop(receiver);
        assert!(handler.take_signal_receiver().await.is_some());
    }

    /// endpoint/client_test.go:TestClientSignalRejectsOversizedSDPAnswer,
    /// plus endpoint/client.go:Client.Signal's empty/numeric answer checks.
    #[tokio::test]
    async fn client_rejects_empty_numeric_and_oversized_answers() {
        for body in [
            Vec::new(),
            b"37".to_vec(),
            vec![b'x'; MAX_SDP_BODY_SIZE + 1],
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let body = Bytes::from(body);
                let service = hyper::service::service_fn(move |request: Request<Incoming>| {
                    assert_eq!(request.method(), Method::POST);
                    assert_eq!(request.uri().path(), "/v1/join/123");
                    assert_eq!(request.headers()["user-agent"], USER_AGENT);
                    assert_eq!(request.headers()["content-type"], "application/sdp");
                    let body = body.clone();
                    async move {
                        Ok::<_, Infallible>(response(StatusCode::OK, Some("application/sdp"), body))
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
            let client = ClientConfig {
                network_id: "123".into(),
                ..Default::default()
            }
            .build();
            assert!(
                client
                    .signal(Signal {
                        typ: signal_type::OFFER.into(),
                        connection_id: 42,
                        network_id: format!("http://{address}"),
                        data: "offer".into()
                    })
                    .await
                    .is_err()
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn close_wakes_context_waiters() {
        let handler = Handler::serve("127.0.0.1:0").await.unwrap();
        handler.close();
        handler.closed().await;
        assert!(handler.is_closed());
    }

    #[tokio::test]
    async fn credentials_are_requested_each_time_and_errors_propagate() {
        struct Provider(std::sync::atomic::AtomicU32);
        #[async_trait]
        impl CredentialProvider for Provider {
            async fn credentials(&self) -> anyhow::Result<Credentials> {
                let count = self.0.fetch_add(1, Ordering::Relaxed);
                anyhow::ensure!(count < 2, "credential refresh failed");
                Ok(Credentials {
                    expiration_seconds: count,
                    ice_servers: Vec::new(),
                })
            }
        }
        let client = ClientConfig {
            credentials: Some(Arc::new(Provider(std::sync::atomic::AtomicU32::new(0)))),
            ..Default::default()
        }
        .build();
        assert_eq!(
            client
                .credentials()
                .await
                .unwrap()
                .unwrap()
                .expiration_seconds,
            0
        );
        assert_eq!(
            client
                .credentials()
                .await
                .unwrap()
                .unwrap()
                .expiration_seconds,
            1
        );
        assert!(client.credentials().await.is_err());
    }

    fn offer_request(body: impl Into<Bytes>) -> Request<Full<Bytes>> {
        Request::post("/v1/join/123")
            .body(Full::new(body.into()))
            .unwrap()
    }

    /// go-nethernet endpoint/client_test.go:TestHandlerServesSDPAnswer.
    #[tokio::test]
    async fn handler_serves_sdp_answer_and_cleans_pending() {
        let handler = Handler::new();
        let mut signals = handler.take_signal_receiver().await.unwrap();
        let request = Request::post("/v1/join/%31%32%33")
            .body(Full::new(Bytes::from_static(b"offer")))
            .unwrap();
        let response = handler.handle_request(request);
        let answer = async {
            let mut signal = signals.recv().await.unwrap();
            assert_eq!(signal.network_id, "123");
            assert_eq!(signal.typ, signal_type::OFFER);
            signal.typ = signal_type::ANSWER.into();
            signal.data = "v=0\r\ns=-\r\n".into();
            handler.signal(signal).await.unwrap();
        };
        let (response, ()) = tokio::join!(response, answer);
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "application/sdp");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "v=0\r\ns=-\r\n"
        );
        assert!(handler.pending.lock().is_empty());
    }

    /// go-nethernet endpoint/client_test.go:TestHandlerRejectsOversizedSDPOffer.
    #[tokio::test]
    async fn handler_rejects_bad_offers_and_full_admission_queue() {
        let handler = Handler::new();
        assert_eq!(
            handler
                .handle_request(offer_request(vec![b'x'; MAX_SDP_BODY_SIZE + 1]))
                .await
                .status(),
            413
        );
        assert_eq!(
            handler.handle_request(offer_request("")).await.status(),
            400
        );
        let request = Request::post("/v1/join/not-a-number")
            .body(Full::new(Bytes::from_static(b"offer")))
            .unwrap();
        assert_eq!(handler.handle_request(request).await.status(), 400);
        let _receiver = handler.take_signal_receiver().await.unwrap();
        for id in 0..SIGNAL_QUEUE_CAPACITY {
            handler
                .offers
                .read()
                .try_send(Signal {
                    typ: signal_type::OFFER.into(),
                    connection_id: id as u64,
                    data: String::new(),
                    network_id: "123".into(),
                })
                .unwrap();
        }
        assert_eq!(
            handler
                .handle_request(offer_request("offer"))
                .await
                .status(),
            503
        );
        assert!(handler.pending.lock().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn negotiation_timeout_cleans_pending_and_notifies_listener() {
        let handler = Handler::new();
        let mut signals = handler.take_signal_receiver().await.unwrap();
        let response = handler.handle_request(offer_request("offer")).await;
        assert_eq!(response.status(), 502);
        let offer = signals.recv().await.unwrap();
        let error = signals.recv().await.unwrap();
        assert_eq!(error.connection_id, offer.connection_id);
        assert_eq!(error.data, "14");
        assert_eq!(error.typ, signal_type::ERROR);
        assert!(handler.pending.lock().is_empty());
    }

    /// go-nethernet endpoint/client_test.go:TestClientSignalDeliversSDPAnswer.
    #[tokio::test]
    async fn client_delivers_answer_to_independent_subscribers() {
        let handler = Handler::serve("127.0.0.1:0").await.unwrap();
        let mut offers = handler.take_signal_receiver().await.unwrap();
        let client = ClientConfig {
            network_id: "123".into(),
            ..Default::default()
        }
        .build();
        let mut one = client.take_signal_receiver().await.unwrap();
        let mut two = client.take_signal_receiver().await.unwrap();
        let address = format!("http://{}", handler.local_addr().unwrap());
        let send = client.signal(Signal {
            typ: signal_type::OFFER.into(),
            connection_id: 42,
            data: "offer".into(),
            network_id: address.clone(),
        });
        let answer = async {
            let mut signal = offers.recv().await.unwrap();
            assert_eq!(signal.network_id, "123");
            signal.typ = signal_type::ANSWER.into();
            signal.data = "v=0\r\ns=-\r\n".into();
            handler.signal(signal).await.unwrap();
        };
        let (result, ()) = tokio::join!(send, answer);
        result.unwrap();
        for receiver in [&mut one, &mut two] {
            let signal = receiver.recv().await.unwrap();
            assert_eq!(signal.connection_id, 42);
            assert_eq!(signal.network_id, address);
            assert_eq!(signal.data, "v=0\r\ns=-\r\n");
        }
        handler.close();
        assert!(handler.is_closed());
    }

    #[tokio::test]
    async fn server_drop_does_not_leave_an_idle_connection_owner_cycle() {
        let handler = Handler::serve("127.0.0.1:0").await.unwrap();
        let stream = tokio::net::TcpStream::connect(handler.local_addr().unwrap())
            .await
            .unwrap();
        tokio::task::yield_now().await;
        let weak = Arc::downgrade(&handler);
        drop(handler);
        assert!(weak.upgrade().is_none());
        drop(stream);
    }

    #[tokio::test]
    async fn ping_method_mismatch_is_405() {
        let handler = Handler::new();
        let request = Request::post("/v1/join")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let response = handler.handle_request(request).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()["allow"], "GET, HEAD");
    }

    #[tokio::test]
    async fn dropping_offer_future_cleans_pending_and_notifies_listener() {
        use futures::FutureExt;
        let handler = Handler::new();
        let mut signals = handler.take_signal_receiver().await.unwrap();
        let mut negotiation = Box::pin(handler.handle_request(offer_request("offer")));
        assert!(negotiation.as_mut().now_or_never().is_none());
        let offer = signals.recv().await.unwrap();
        drop(negotiation);
        let error = signals.recv().await.unwrap();
        assert_eq!(error.connection_id, offer.connection_id);
        assert_eq!(error.data, "14");
        assert!(handler.pending.lock().is_empty());
    }

    #[test]
    fn endpoint_urls_require_explicit_port_and_no_path() {
        for address in [
            "http://localhost:80",
            "https://localhost:443",
            "http://[::1]:19132",
        ] {
            assert!(parse_url(address).is_ok(), "{address}");
        }
        for address in [
            "http://localhost",
            "ftp://localhost:123",
            "http://localhost:80/",
            "http://localhost:80/path",
        ] {
            assert!(parse_url(address).is_err(), "{address}");
        }
    }

    /// Wire fixture from endpoint/status.go:Status / Status.RakNet / RakNetPongData.
    #[test]
    fn status_json_and_raknet_round_trip() {
        let status: Status = serde_json::from_str(r#"{"name":"Server","protocol":818,"version":"1.21","level":"World","players":2,"maxPlayers":10,"gameType":1}"#).unwrap();
        let pong = status.raknet();
        assert!(
            std::str::from_utf8(&pong)
                .unwrap()
                .ends_with(";World;Creative;1;19132;19132;0;")
        );
        assert_eq!(Status::from_raknet(&pong).unwrap(), status);
        assert_eq!(serde_json::to_value(status).unwrap()["maxPlayers"], 10);
    }
}
