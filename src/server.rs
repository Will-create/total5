use bytes::Bytes;
use http::{HeaderName, HeaderValue, Request, Response, StatusCode};
use std::future::Future;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

const HEADER_END: &[u8] = b"\r\n\r\n";

#[derive(Debug, Clone)]
pub struct HttpLimits {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 32 * 1024,
            max_body_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub limits: HttpLimits,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub handler_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
}

#[derive(Clone)]
pub struct WebSocketSession {
    pub incoming: mpsc::Sender<Result<Vec<u8>, String>>,
    outgoing: Arc<std::sync::Mutex<Option<mpsc::Receiver<Vec<u8>>>>>,
    pub max_frame_bytes: usize,
    pub compression: bool,
}

impl WebSocketSession {
    pub fn new(
        incoming: mpsc::Sender<Result<Vec<u8>, String>>,
        outgoing: mpsc::Receiver<Vec<u8>>,
        max_frame_bytes: usize,
    ) -> Self {
        Self {
            incoming,
            outgoing: Arc::new(std::sync::Mutex::new(Some(outgoing))),
            max_frame_bytes,
            compression: false,
        }
    }

    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    fn take_outgoing(&self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.outgoing.lock().ok()?.take()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: HttpLimits::default(),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            handler_timeout: Duration::from_secs(5),
            graceful_shutdown_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
pub enum HttpError {
    Io(std::io::Error),
    Tls(String),
    BadRequest(String),
    HeaderTooLarge,
    BodyTooLarge,
    ConnectionClosed,
    Timeout,
}

impl From<std::io::Error> for HttpError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub async fn read_request<S>(
    stream: &mut S,
    limits: &HttpLimits,
) -> Result<Request<Bytes>, HttpError>
where
    S: AsyncRead + Unpin,
{
    let mut connection = HttpConnection::new(stream);
    connection.read_request(limits).await
}

/// Buffered HTTP connection state. Bytes read beyond one request remain in the
/// buffer for the next keep-alive or pipelined request.
pub struct HttpConnection<S> {
    stream: S,
    buffer: Vec<u8>,
}

struct BufferedStream<S> {
    stream: S,
    prefix: Vec<u8>,
    position: usize,
}

impl<S> AsyncRead for BufferedStream<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.position < self.prefix.len() {
            let available = &self.prefix[self.position..];
            let length = available.len().min(buffer.remaining());
            buffer.put_slice(&available[..length]);
            self.position += length;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.stream).poll_read(context, buffer)
    }
}

impl<S> AsyncWrite for BufferedStream<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.stream).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_shutdown(context)
    }
}

impl<S> HttpConnection<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(4096),
        }
    }

    pub fn into_inner(self) -> S {
        self.stream
    }

    fn into_buffered_stream(self) -> BufferedStream<S> {
        BufferedStream {
            stream: self.stream,
            prefix: self.buffer,
            position: 0,
        }
    }
}

impl<S> HttpConnection<S>
where
    S: AsyncRead + Unpin,
{
    pub async fn read_request(&mut self, limits: &HttpLimits) -> Result<Request<Bytes>, HttpError> {
        let header_end = loop {
            if let Some(index) = find_bytes(&self.buffer, HEADER_END) {
                break index + HEADER_END.len();
            }
            if self.buffer.len() >= limits.max_header_bytes {
                return Err(HttpError::HeaderTooLarge);
            }
            let mut chunk = [0_u8; 4096];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return if self.buffer.is_empty() {
                    Err(HttpError::ConnectionClosed)
                } else {
                    Err(HttpError::BadRequest("incomplete HTTP headers".to_string()))
                };
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        };

        if header_end > limits.max_header_bytes {
            return Err(HttpError::HeaderTooLarge);
        }
        let header_text = std::str::from_utf8(&self.buffer[..header_end - HEADER_END.len()])
            .map_err(|_| HttpError::BadRequest("headers are not valid UTF-8".to_string()))?;
        let mut lines = header_text.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| HttpError::BadRequest("missing request line".to_string()))?;
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts
            .next()
            .ok_or_else(|| HttpError::BadRequest("missing method".to_string()))?;
        let uri = request_parts
            .next()
            .ok_or_else(|| HttpError::BadRequest("missing URI".to_string()))?;
        let version = request_parts
            .next()
            .ok_or_else(|| HttpError::BadRequest("missing HTTP version".to_string()))?;
        if request_parts.next().is_some() || (version != "HTTP/1.1" && version != "HTTP/1.0") {
            return Err(HttpError::BadRequest("invalid request line".to_string()));
        }

        let mut builder = Request::builder().method(method).uri(uri);
        if version == "HTTP/1.0" {
            builder = builder.version(http::Version::HTTP_10);
        }
        let headers = builder
            .headers_mut()
            .ok_or_else(|| HttpError::BadRequest("invalid request".to_string()))?;
        for line in lines {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| HttpError::BadRequest("malformed header".to_string()))?;
            let name = HeaderName::from_str(name.trim())
                .map_err(|_| HttpError::BadRequest("invalid header name".to_string()))?;
            let value = HeaderValue::from_str(value.trim())
                .map_err(|_| HttpError::BadRequest("invalid header value".to_string()))?;
            headers.append(name, value);
        }

        let chunked = match headers.get(http::header::TRANSFER_ENCODING) {
            Some(value) => {
                let value = value
                    .to_str()
                    .map_err(|_| HttpError::BadRequest("invalid transfer-encoding".to_string()))?;
                if !value.trim().eq_ignore_ascii_case("chunked") {
                    return Err(HttpError::BadRequest(
                        "unsupported transfer-encoding".to_string(),
                    ));
                }
                true
            }
            None => false,
        };
        if chunked && headers.contains_key(http::header::CONTENT_LENGTH) {
            return Err(HttpError::BadRequest(
                "content-length with transfer-encoding is invalid".to_string(),
            ));
        }
        let content_length = headers
            .get(http::header::CONTENT_LENGTH)
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| HttpError::BadRequest("invalid content-length".to_string()))
            })
            .transpose()?
            .unwrap_or(0);
        if content_length > limits.max_body_bytes {
            return Err(HttpError::BodyTooLarge);
        }

        if chunked {
            let (body, request_end) = self
                .read_chunked_body(header_end, limits.max_body_bytes, limits.max_header_bytes)
                .await?;
            self.buffer.drain(..request_end);
            return builder
                .body(body)
                .map_err(|err| HttpError::BadRequest(format!("invalid request: {err}")));
        }

        let request_end = header_end + content_length;
        while self.buffer.len() < request_end {
            let remaining = request_end - self.buffer.len();
            let mut chunk = vec![0_u8; remaining.min(8192)];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(HttpError::BadRequest("incomplete request body".to_string()));
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
        let body = Bytes::copy_from_slice(&self.buffer[header_end..request_end]);
        self.buffer.drain(..request_end);

        builder
            .body(body)
            .map_err(|err| HttpError::BadRequest(format!("invalid request: {err}")))
    }

    async fn read_chunked_body(
        &mut self,
        mut cursor: usize,
        max_body_bytes: usize,
        max_trailer_bytes: usize,
    ) -> Result<(Bytes, usize), HttpError> {
        let mut body = Vec::new();
        loop {
            let line_end = self.read_until(cursor, b"\r\n", max_trailer_bytes).await?;
            let line = std::str::from_utf8(&self.buffer[cursor..line_end])
                .map_err(|_| HttpError::BadRequest("invalid chunk size".to_string()))?;
            let size = line.split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size, 16)
                .map_err(|_| HttpError::BadRequest("invalid chunk size".to_string()))?;
            cursor = line_end + 2;

            if size == 0 {
                let trailer_start = cursor;
                loop {
                    let trailer_end = self.read_until(cursor, b"\r\n", max_trailer_bytes).await?;
                    if trailer_end == cursor {
                        return Ok((Bytes::from(body), trailer_end + 2));
                    }
                    if trailer_end + 2 - trailer_start > max_trailer_bytes {
                        return Err(HttpError::HeaderTooLarge);
                    }
                    let trailer = &self.buffer[cursor..trailer_end];
                    if !trailer.contains(&b':') {
                        return Err(HttpError::BadRequest("malformed trailer".to_string()));
                    }
                    cursor = trailer_end + 2;
                }
            }

            if body.len().saturating_add(size) > max_body_bytes {
                return Err(HttpError::BodyTooLarge);
            }
            let chunk_end = cursor.checked_add(size).ok_or(HttpError::BodyTooLarge)?;
            self.read_to_length(chunk_end + 2).await?;
            if &self.buffer[chunk_end..chunk_end + 2] != b"\r\n" {
                return Err(HttpError::BadRequest(
                    "chunk data is not terminated".to_string(),
                ));
            }
            body.extend_from_slice(&self.buffer[cursor..chunk_end]);
            cursor = chunk_end + 2;
        }
    }

    async fn read_until(
        &mut self,
        start: usize,
        delimiter: &[u8],
        limit: usize,
    ) -> Result<usize, HttpError> {
        loop {
            if let Some(index) = find_bytes(&self.buffer[start..], delimiter) {
                return Ok(start + index);
            }
            if self.buffer.len().saturating_sub(start) > limit {
                return Err(HttpError::HeaderTooLarge);
            }
            let mut chunk = [0_u8; 4096];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(HttpError::BadRequest("incomplete request body".to_string()));
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    async fn read_to_length(&mut self, length: usize) -> Result<(), HttpError> {
        while self.buffer.len() < length {
            let remaining = length - self.buffer.len();
            let mut chunk = vec![0_u8; remaining.min(8192)];
            let read = self.stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(HttpError::BadRequest("incomplete request body".to_string()));
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
        Ok(())
    }
}

pub async fn write_response<S>(stream: &mut S, response: Response<Bytes>) -> Result<(), HttpError>
where
    S: AsyncWrite + Unpin,
{
    let (mut parts, body) = response.into_parts();
    if !parts.status.is_informational() && !parts.headers.contains_key(http::header::CONTENT_LENGTH)
    {
        parts.headers.insert(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&body.len().to_string())
                .map_err(|_| HttpError::BadRequest("invalid response length".to_string()))?,
        );
    }
    let reason = parts.status.canonical_reason().unwrap_or("");
    let version = match parts.version {
        http::Version::HTTP_10 => "HTTP/1.0",
        _ => "HTTP/1.1",
    };
    stream
        .write_all(format!("{version} {} {}\r\n", parts.status.as_u16(), reason).as_bytes())
        .await?;
    for (name, value) in &parts.headers {
        stream.write_all(name.as_str().as_bytes()).await?;
        stream.write_all(b": ").await?;
        stream.write_all(value.as_bytes()).await?;
        stream.write_all(b"\r\n").await?;
    }
    stream.write_all(b"\r\n").await?;
    stream.write_all(&body).await?;
    stream.flush().await?;
    Ok(())
}

pub fn error_response(status: StatusCode, message: &str) -> Response<Bytes> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Bytes::copy_from_slice(message.as_bytes()))
        .expect("static error response is valid")
}

pub async fn serve_connection<S, H, Fut>(
    stream: S,
    config: ServerConfig,
    handler: H,
) -> Result<(), HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: Fn(Request<Bytes>) -> Fut,
    Fut: Future<Output = Response<Bytes>>,
{
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    serve_connection_with_shutdown(stream, config, handler, shutdown_rx).await
}

async fn serve_connection_with_shutdown<S, H, Fut>(
    stream: S,
    config: ServerConfig,
    handler: H,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: Fn(Request<Bytes>) -> Fut,
    Fut: Future<Output = Response<Bytes>>,
{
    let mut connection = HttpConnection::new(stream);
    loop {
        let request = tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    let _ = timeout(config.write_timeout, connection.stream.shutdown()).await;
                    return Ok(());
                }
                continue;
            }
            result = timeout(config.read_timeout, connection.read_request(&config.limits)) => {
            match result {
                Err(_) => return Err(HttpError::Timeout),
                Ok(Err(HttpError::ConnectionClosed)) => return Ok(()),
                Ok(Err(err)) => {
                    let response = protocol_error_response(&err);
                    timeout(
                        config.write_timeout,
                        write_response(&mut connection.stream, response),
                    )
                    .await
                    .map_err(|_| HttpError::Timeout)??;
                    return Err(err);
                }
                Ok(Ok(request)) => request,
            }
            }
        };

        let keep_alive = request_keep_alive(&request);
        let version = request.version();
        let mut response = match timeout(config.handler_timeout, handler(request)).await {
            Ok(response) => response,
            Err(_) => error_response(StatusCode::SERVICE_UNAVAILABLE, "request timeout"),
        };
        let websocket = response.extensions_mut().remove::<WebSocketSession>();
        *response.version_mut() = version;
        let draining = *shutdown.borrow();
        if websocket.is_none() && (!keep_alive || draining) {
            response
                .headers_mut()
                .insert(http::header::CONNECTION, HeaderValue::from_static("close"));
        } else if version == http::Version::HTTP_10 {
            response.headers_mut().insert(
                http::header::CONNECTION,
                HeaderValue::from_static("keep-alive"),
            );
        }
        timeout(
            config.write_timeout,
            write_response(&mut connection.stream, response),
        )
        .await
        .map_err(|_| HttpError::Timeout)??;
        if let Some(websocket) = websocket {
            return websocket_loop(
                connection.into_buffered_stream(),
                websocket,
                config.read_timeout,
            )
            .await;
        }
        if !keep_alive || draining {
            timeout(config.write_timeout, connection.stream.shutdown())
                .await
                .map_err(|_| HttpError::Timeout)??;
            return Ok(());
        }
    }
}

pub async fn serve<H, Fut, Shutdown>(
    listener: TcpListener,
    config: ServerConfig,
    handler: H,
    shutdown: Shutdown,
) -> Result<(), HttpError>
where
    H: Fn(Request<Bytes>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Response<Bytes>> + Send + 'static,
    Shutdown: Future<Output = ()>,
{
    let handler = Arc::new(handler);
    let mut connections = JoinSet::new();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let handler = handler.clone();
                let config = config.clone();
                let shutdown = shutdown_rx.clone();
                connections.spawn(async move {
                    let _ = serve_connection_with_shutdown(
                        stream,
                        config,
                        move |request| handler(request),
                        shutdown,
                    ).await;
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }

    let _ = shutdown_tx.send(true);
    let drain = async { while connections.join_next().await.is_some() {} };
    if timeout(config.graceful_shutdown_timeout, drain)
        .await
        .is_err()
    {
        connections.shutdown().await;
    }
    Ok(())
}

/// Loads a PEM certificate chain and PKCS#1, PKCS#8, or SEC1 private key.
pub fn load_tls_config(
    certificate_path: impl AsRef<Path>,
    private_key_path: impl AsRef<Path>,
) -> Result<Arc<rustls::ServerConfig>, HttpError> {
    let certificate_file = std::fs::File::open(certificate_path).map_err(HttpError::Io)?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(certificate_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| HttpError::Tls(format!("invalid certificate PEM: {err}")))?;
    if certificates.is_empty() {
        return Err(HttpError::Tls(
            "certificate PEM contains no certificates".to_string(),
        ));
    }

    let private_key_file = std::fs::File::open(private_key_path).map_err(HttpError::Io)?;
    let private_key = rustls_pemfile::private_key(&mut BufReader::new(private_key_file))
        .map_err(|err| HttpError::Tls(format!("invalid private key PEM: {err}")))?
        .ok_or_else(|| HttpError::Tls("private key PEM contains no private key".to_string()))?;
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|err| HttpError::Tls(format!("certificate and private key are invalid: {err}")))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Serves HTTPS after the Rustls handshake and delegates every decrypted
/// connection to the same native HTTP connection loop used by [`serve`].
pub async fn serve_tls<H, Fut, Shutdown>(
    listener: TcpListener,
    tls_config: Arc<rustls::ServerConfig>,
    config: ServerConfig,
    handler: H,
    shutdown: Shutdown,
) -> Result<(), HttpError>
where
    H: Fn(Request<Bytes>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Response<Bytes>> + Send + 'static,
    Shutdown: Future<Output = ()>,
{
    let handler = Arc::new(handler);
    let acceptor = TlsAcceptor::from(tls_config);
    let mut connections = JoinSet::new();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let handler = handler.clone();
                let acceptor = acceptor.clone();
                let config = config.clone();
                let shutdown = shutdown_rx.clone();
                connections.spawn(async move {
                    let tls_stream = match timeout(config.read_timeout, acceptor.accept(stream)).await {
                        Ok(Ok(stream)) => stream,
                        _ => return,
                    };
                    let _ = serve_connection_with_shutdown(
                        tls_stream,
                        config,
                        move |request| handler(request),
                        shutdown,
                    ).await;
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }

    let _ = shutdown_tx.send(true);
    let drain = async { while connections.join_next().await.is_some() {} };
    if timeout(config.graceful_shutdown_timeout, drain)
        .await
        .is_err()
    {
        connections.shutdown().await;
    }
    Ok(())
}

pub async fn bind(addr: SocketAddr) -> Result<TcpListener, HttpError> {
    Ok(TcpListener::bind(addr).await?)
}

pub async fn connect(addr: SocketAddr) -> Result<TcpStream, HttpError> {
    Ok(TcpStream::connect(addr).await?)
}

trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncStream for T {}

pub struct WebSocketClient {
    stream: BufferedStream<Pin<Box<dyn AsyncStream>>>,
    pub protocol: Option<String>,
    max_frame_bytes: usize,
}

impl WebSocketClient {
    pub async fn connect(url: &str, protocols: &[&str]) -> Result<Self, HttpError> {
        let (secure, authority, path) = parse_websocket_url(url)?;
        let (host, port) = websocket_authority(&authority, secure)?;
        let tcp = TcpStream::connect((host.as_str(), port)).await?;
        let stream: Pin<Box<dyn AsyncStream>> = if secure {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            let server_name = rustls::pki_types::ServerName::try_from(host.clone())
                .map_err(|_| HttpError::Tls("invalid TLS server name".to_string()))?;
            let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
                .connect(server_name, tcp)
                .await
                .map_err(|err| HttpError::Tls(format!("websocket TLS handshake failed: {err}")))?;
            Box::pin(tls)
        } else {
            Box::pin(tcp)
        };
        let key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            uuid::Uuid::new_v4().as_bytes(),
        );
        let mut stream = stream;
        let mut request = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\n"
        );
        if !protocols.is_empty() {
            request.push_str(&format!(
                "Sec-WebSocket-Protocol: {}\r\n",
                protocols.join(", ")
            ));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let mut response = Vec::new();
        let header_end = loop {
            if let Some(index) = find_bytes(&response, HEADER_END) {
                break index + HEADER_END.len();
            }
            if response.len() >= 32 * 1024 {
                return Err(HttpError::HeaderTooLarge);
            }
            let mut chunk = [0_u8; 2048];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(HttpError::ConnectionClosed);
            }
            response.extend_from_slice(&chunk[..read]);
        };
        let headers = std::str::from_utf8(&response[..header_end])
            .map_err(|_| HttpError::BadRequest("websocket response is not UTF-8".to_string()))?;
        let mut lines = headers.split("\r\n");
        let status = lines.next().unwrap_or_default();
        if !status.starts_with("HTTP/1.1 101 ") {
            return Err(HttpError::BadRequest(format!(
                "websocket upgrade rejected: {status}"
            )));
        }
        let mut accept = None;
        let mut protocol = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("sec-websocket-accept") {
                accept = Some(value.trim());
            } else if name.eq_ignore_ascii_case("sec-websocket-protocol") {
                protocol = Some(value.trim().to_string());
            }
        }
        if accept != Some(crate::websocket_accept(&key).as_str()) {
            return Err(HttpError::BadRequest(
                "invalid websocket accept key".to_string(),
            ));
        }
        Ok(Self {
            stream: BufferedStream {
                stream,
                prefix: response[header_end..].to_vec(),
                position: 0,
            },
            protocol,
            max_frame_bytes: 256 * 1024,
        })
    }

    pub async fn send_text(&mut self, value: &str) -> Result<(), HttpError> {
        write_masked_ws_frame(&mut self.stream, 0x1, value.as_bytes()).await
    }

    pub async fn receive(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        loop {
            let (opcode, payload) =
                read_server_ws_frame(&mut self.stream, self.max_frame_bytes).await?;
            match opcode {
                0x1 | 0x2 => return Ok(Some(payload)),
                0x8 => {
                    write_masked_ws_frame(&mut self.stream, 0x8, &payload).await?;
                    return Ok(None);
                }
                0x9 => write_masked_ws_frame(&mut self.stream, 0xA, &payload).await?,
                0xA => {}
                _ => {
                    return Err(HttpError::BadRequest(
                        "unsupported server websocket frame".to_string(),
                    ))
                }
            }
        }
    }

    pub async fn close(&mut self) -> Result<(), HttpError> {
        write_masked_ws_frame(&mut self.stream, 0x8, &1000_u16.to_be_bytes()).await
    }
}

fn request_keep_alive(request: &Request<Bytes>) -> bool {
    let connection = request
        .headers()
        .get(http::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let has_token = |expected: &str| {
        connection
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case(expected))
    };
    match request.version() {
        http::Version::HTTP_11 => !has_token("close"),
        http::Version::HTTP_10 => has_token("keep-alive"),
        _ => false,
    }
}

fn protocol_error_response(error: &HttpError) -> Response<Bytes> {
    match error {
        HttpError::HeaderTooLarge => error_response(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request headers too large",
        ),
        HttpError::BodyTooLarge => {
            error_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large")
        }
        _ => error_response(StatusCode::BAD_REQUEST, "bad request"),
    }
}

async fn websocket_loop<S>(
    stream: S,
    session: WebSocketSession,
    read_timeout: Duration,
) -> Result<(), HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(mut outgoing) = session.take_outgoing() else {
        return Err(HttpError::BadRequest(
            "invalid websocket session".to_string(),
        ));
    };
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut fragmented = Vec::new();
    let mut fragmented_opcode = None;

    loop {
        tokio::select! {
            message = outgoing.recv() => {
                let Some(message) = message else {
                    write_ws_frame(&mut writer, 0x8, &1000_u16.to_be_bytes()).await?;
                    return Ok(());
                };
                if session.compression && !message.is_empty() {
                    let compressed = websocket_deflate(&message)?;
                    write_ws_frame_flags(&mut writer, 0x1, &compressed, true).await?;
                } else {
                    write_ws_frame(&mut writer, 0x1, &message).await?;
                }
            }
            frame = timeout(read_timeout, read_ws_frame(&mut reader, session.max_frame_bytes)) => {
                let (fin, rsv1, opcode, mut payload) = match frame {
                    Err(_) => return Err(HttpError::Timeout),
                    Ok(result) => result?,
                };
                if rsv1 {
                    if !session.compression || !fin || opcode == 0 || opcode >= 0x8 {
                        return websocket_protocol_error(&mut writer, "invalid compressed frame").await;
                    }
                    payload = websocket_inflate(&payload, session.max_frame_bytes)?;
                }
                match opcode {
                    0x0 => {
                        if fragmented_opcode.is_none() {
                            return websocket_protocol_error(&mut writer, "unexpected continuation").await;
                        }
                        if fragmented.len().saturating_add(payload.len()) > session.max_frame_bytes {
                            return websocket_too_large(&mut writer).await;
                        }
                        fragmented.extend_from_slice(&payload);
                        if fin {
                            let message = std::mem::take(&mut fragmented);
                            fragmented_opcode = None;
                            if session.incoming.send(Ok(message)).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    0x1 | 0x2 => {
                        if fragmented_opcode.is_some() {
                            return websocket_protocol_error(&mut writer, "interleaved data frame").await;
                        }
                        if fin {
                            if opcode == 0x1 && std::str::from_utf8(&payload).is_err() {
                                return websocket_protocol_error(&mut writer, "invalid UTF-8").await;
                            }
                            if session.incoming.send(Ok(payload)).await.is_err() {
                                return Ok(());
                            }
                        } else {
                            fragmented_opcode = Some(opcode);
                            fragmented = payload;
                        }
                    }
                    0x8 => {
                        write_ws_frame(&mut writer, 0x8, &payload).await?;
                        return Ok(());
                    }
                    0x9 => write_ws_frame(&mut writer, 0xA, &payload).await?,
                    0xA => {}
                    _ => return websocket_protocol_error(&mut writer, "unsupported opcode").await,
                }
            }
        }
    }
}

async fn read_ws_frame<R>(
    reader: &mut R,
    max_frame_bytes: usize,
) -> Result<(bool, bool, u8, Vec<u8>), HttpError>
where
    R: AsyncRead + Unpin,
{
    let mut head = [0_u8; 2];
    reader.read_exact(&mut head).await?;
    let fin = head[0] & 0x80 != 0;
    let rsv1 = head[0] & 0x40 != 0;
    if head[0] & 0x30 != 0 {
        return Err(HttpError::BadRequest(
            "websocket RSV bits are set".to_string(),
        ));
    }
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    if !masked {
        return Err(HttpError::BadRequest(
            "client websocket frame is not masked".to_string(),
        ));
    }
    let mut length = u64::from(head[1] & 0x7f);
    if length == 126 {
        let mut extended = [0_u8; 2];
        reader.read_exact(&mut extended).await?;
        length = u64::from(u16::from_be_bytes(extended));
    } else if length == 127 {
        let mut extended = [0_u8; 8];
        reader.read_exact(&mut extended).await?;
        length = u64::from_be_bytes(extended);
        if length & (1 << 63) != 0 {
            return Err(HttpError::BadRequest(
                "invalid websocket length".to_string(),
            ));
        }
    }
    if opcode >= 0x8 && (!fin || length > 125) {
        return Err(HttpError::BadRequest(
            "invalid websocket control frame".to_string(),
        ));
    }
    let length = usize::try_from(length).map_err(|_| HttpError::BodyTooLarge)?;
    if length > max_frame_bytes {
        return Err(HttpError::BodyTooLarge);
    }
    let mut mask = [0_u8; 4];
    reader.read_exact(&mut mask).await?;
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    Ok((fin, rsv1, opcode, payload))
}

async fn write_ws_frame<W>(writer: &mut W, opcode: u8, payload: &[u8]) -> Result<(), HttpError>
where
    W: AsyncWrite + Unpin,
{
    write_ws_frame_flags(writer, opcode, payload, false).await
}

async fn write_ws_frame_flags<W>(
    writer: &mut W,
    opcode: u8,
    payload: &[u8],
    compressed: bool,
) -> Result<(), HttpError>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(&[0x80 | if compressed { 0x40 } else { 0 } | opcode])
        .await?;
    match payload.len() {
        0..=125 => writer.write_all(&[payload.len() as u8]).await?,
        126..=65535 => {
            writer.write_all(&[126]).await?;
            writer
                .write_all(&(payload.len() as u16).to_be_bytes())
                .await?;
        }
        _ => {
            writer.write_all(&[127]).await?;
            writer
                .write_all(&(payload.len() as u64).to_be_bytes())
                .await?;
        }
    }
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

fn websocket_deflate(payload: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut compressor = flate2::Compress::new(flate2::Compression::fast(), false);
    let mut output = Vec::with_capacity(payload.len());
    compressor
        .compress_vec(payload, &mut output, flate2::FlushCompress::Sync)
        .map_err(|err| HttpError::BadRequest(format!("websocket compression failed: {err}")))?;
    if output.ends_with(&[0, 0, 255, 255]) {
        output.truncate(output.len() - 4);
    }
    Ok(output)
}

fn websocket_inflate(payload: &[u8], limit: usize) -> Result<Vec<u8>, HttpError> {
    let mut input = payload.to_vec();
    input.extend_from_slice(&[0, 0, 255, 255]);
    let mut decompressor = flate2::Decompress::new(false);
    let mut output = Vec::with_capacity(payload.len().saturating_mul(2));
    decompressor
        .decompress_vec(&input, &mut output, flate2::FlushDecompress::Sync)
        .map_err(|err| {
            HttpError::BadRequest(format!("invalid compressed websocket frame: {err}"))
        })?;
    if output.len() > limit {
        return Err(HttpError::BodyTooLarge);
    }
    Ok(output)
}

fn parse_websocket_url(url: &str) -> Result<(bool, String, String), HttpError> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("ws://") {
        (false, rest)
    } else if let Some(rest) = url.strip_prefix("wss://") {
        (true, rest)
    } else {
        return Err(HttpError::BadRequest(
            "websocket URL must use ws:// or wss://".to_string(),
        ));
    };
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    if authority.is_empty() {
        return Err(HttpError::BadRequest(
            "websocket URL has no host".to_string(),
        ));
    }
    Ok((secure, authority.to_string(), path))
}

fn websocket_authority(authority: &str, secure: bool) -> Result<(String, u16), HttpError> {
    if let Some(host) = authority.strip_prefix('[') {
        let end = host
            .find(']')
            .ok_or_else(|| HttpError::BadRequest("invalid IPv6 websocket host".to_string()))?;
        let hostname = host[..end].to_string();
        let port = host[end + 1..]
            .strip_prefix(':')
            .map(str::parse::<u16>)
            .transpose()
            .map_err(|_| HttpError::BadRequest("invalid websocket port".to_string()))?
            .unwrap_or(if secure { 443 } else { 80 });
        return Ok((hostname, port));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| {
            (host, port.parse::<u16>().ok())
        });
    if host.is_empty() {
        return Err(HttpError::BadRequest(
            "websocket URL has no host".to_string(),
        ));
    }
    Ok((
        host.to_string(),
        port.unwrap_or(if secure { 443 } else { 80 }),
    ))
}

async fn write_masked_ws_frame<W>(
    writer: &mut W,
    opcode: u8,
    payload: &[u8],
) -> Result<(), HttpError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&[0x80 | opcode]).await?;
    let random = uuid::Uuid::new_v4();
    let mask = &random.as_bytes()[..4];
    match payload.len() {
        0..=125 => writer.write_all(&[0x80 | payload.len() as u8]).await?,
        126..=65535 => {
            writer.write_all(&[0x80 | 126]).await?;
            writer
                .write_all(&(payload.len() as u16).to_be_bytes())
                .await?;
        }
        _ => {
            writer.write_all(&[0x80 | 127]).await?;
            writer
                .write_all(&(payload.len() as u64).to_be_bytes())
                .await?;
        }
    }
    writer.write_all(mask).await?;
    let masked = payload
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ mask[index % 4])
        .collect::<Vec<_>>();
    writer.write_all(&masked).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_server_ws_frame<R>(reader: &mut R, limit: usize) -> Result<(u8, Vec<u8>), HttpError>
where
    R: AsyncRead + Unpin,
{
    let mut head = [0_u8; 2];
    reader.read_exact(&mut head).await?;
    if head[0] & 0x80 == 0 || head[0] & 0x70 != 0 || head[1] & 0x80 != 0 {
        return Err(HttpError::BadRequest(
            "invalid server websocket frame".to_string(),
        ));
    }
    let opcode = head[0] & 0x0f;
    let mut length = u64::from(head[1] & 0x7f);
    if length == 126 {
        let mut bytes = [0_u8; 2];
        reader.read_exact(&mut bytes).await?;
        length = u64::from(u16::from_be_bytes(bytes));
    } else if length == 127 {
        let mut bytes = [0_u8; 8];
        reader.read_exact(&mut bytes).await?;
        length = u64::from_be_bytes(bytes);
    }
    let length = usize::try_from(length).map_err(|_| HttpError::BodyTooLarge)?;
    if length > limit {
        return Err(HttpError::BodyTooLarge);
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok((opcode, payload))
}

async fn websocket_protocol_error<W>(writer: &mut W, message: &str) -> Result<(), HttpError>
where
    W: AsyncWrite + Unpin,
{
    let mut payload = 1002_u16.to_be_bytes().to_vec();
    payload.extend_from_slice(message.as_bytes());
    write_ws_frame(writer, 0x8, &payload).await?;
    Err(HttpError::BadRequest(message.to_string()))
}

async fn websocket_too_large<W>(writer: &mut W) -> Result<(), HttpError>
where
    W: AsyncWrite + Unpin,
{
    write_ws_frame(writer, 0x8, &1009_u16.to_be_bytes()).await?;
    Err(HttpError::BodyTooLarge)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn parses_http_request_incrementally() {
        let input =
            b"POST /api/?q=1 HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
        let mut stream = std::io::Cursor::new(input.as_slice());
        let request = read_request(&mut stream, &HttpLimits::default())
            .await
            .unwrap();
        assert_eq!(request.method(), http::Method::POST);
        assert_eq!(request.uri(), "/api/?q=1");
        assert_eq!(request.body(), &Bytes::from_static(b"{\"a\":1}"));
    }

    #[tokio::test]
    async fn enforces_http_body_limit() {
        let input = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8\r\n\r\n12345678";
        let mut stream = std::io::Cursor::new(input.as_slice());
        let limits = HttpLimits {
            max_header_bytes: 1024,
            max_body_bytes: 4,
        };
        assert!(matches!(
            read_request(&mut stream, &limits).await,
            Err(HttpError::BodyTooLarge)
        ));
    }

    #[tokio::test]
    async fn serializes_http_response() {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(http::header::CONTENT_TYPE, "text/plain")
            .body(Bytes::from_static(b"hello"))
            .unwrap();
        let mut output = Vec::new();
        write_response(&mut output, response).await.unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(output.contains("content-length: 5\r\n"));
        assert!(output.ends_with("\r\n\r\nhello"));
    }

    #[tokio::test]
    async fn preserves_pipelined_request_bytes() {
        let input = b"GET /one HTTP/1.1\r\nHost: localhost\r\n\r\nGET /two HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut connection = HttpConnection::new(std::io::Cursor::new(input.as_slice()));
        let first = connection
            .read_request(&HttpLimits::default())
            .await
            .unwrap();
        let second = connection
            .read_request(&HttpLimits::default())
            .await
            .unwrap();
        assert_eq!(first.uri(), "/one");
        assert_eq!(second.uri(), "/two");
    }

    #[tokio::test]
    async fn decodes_chunked_body_and_preserves_next_request() {
        let input = b"POST /chunked HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=yes\r\npedia\r\n0\r\nX-Test: yes\r\n\r\nGET /next HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut connection = HttpConnection::new(std::io::Cursor::new(input.as_slice()));
        let first = connection
            .read_request(&HttpLimits::default())
            .await
            .unwrap();
        let second = connection
            .read_request(&HttpLimits::default())
            .await
            .unwrap();
        assert_eq!(first.body(), &Bytes::from_static(b"Wikipedia"));
        assert_eq!(second.uri(), "/next");
    }

    #[tokio::test]
    async fn enforces_decoded_chunked_body_limit() {
        let input = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut stream = std::io::Cursor::new(input.as_slice());
        let limits = HttpLimits {
            max_header_bytes: 1024,
            max_body_bytes: 4,
        };
        assert!(matches!(
            read_request(&mut stream, &limits).await,
            Err(HttpError::BodyTooLarge)
        ));
    }

    #[tokio::test]
    async fn serves_http11_keep_alive_until_close() {
        let (mut client, server) = tokio::io::duplex(4096);
        let task = tokio::spawn(serve_connection(
            server,
            ServerConfig::default(),
            |request| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Bytes::copy_from_slice(request.uri().path().as_bytes()))
                    .unwrap()
            },
        ));

        client
            .write_all(
                b"GET /one HTTP/1.1\r\nHost: localhost\r\n\r\nGET /two HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        task.await.unwrap().unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("HTTP/1.1 200 OK").count(), 2);
        assert!(output.contains("\r\n\r\n/oneHTTP/1.1 200 OK"));
        assert!(output.ends_with("\r\n\r\n/two"));
    }

    #[tokio::test]
    async fn closes_http10_without_keep_alive() {
        let (mut client, server) = tokio::io::duplex(2048);
        let task = tokio::spawn(serve_connection(
            server,
            ServerConfig::default(),
            |_| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Bytes::from_static(b"done"))
                    .unwrap()
            },
        ));
        client.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        task.await.unwrap().unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("HTTP/1.0 200 OK\r\n"));
        assert!(output.contains("connection: close\r\n"));
    }

    #[tokio::test]
    async fn shutdown_closes_an_idle_keep_alive_connection() {
        let (mut client, server) = tokio::io::duplex(2048);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(serve_connection_with_shutdown(
            server,
            ServerConfig::default(),
            |_| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Bytes::from_static(b"done"))
                    .unwrap()
            },
            shutdown_rx,
        ));

        shutdown_tx.send(true).unwrap();
        let mut output = Vec::new();
        timeout(Duration::from_secs(1), client.read_to_end(&mut output))
            .await
            .unwrap()
            .unwrap();
        task.await.unwrap().unwrap();
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn shutdown_drains_an_in_flight_response_and_closes_it() {
        let (mut client, server) = tokio::io::duplex(2048);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let started_tx = Arc::new(std::sync::Mutex::new(Some(started_tx)));
        let task = tokio::spawn(serve_connection_with_shutdown(
            server,
            ServerConfig::default(),
            move |_| {
                let started_tx = started_tx.clone();
                async move {
                    if let Some(started_tx) = started_tx.lock().unwrap().take() {
                        let _ = started_tx.send(());
                    }
                    tokio::task::yield_now().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Bytes::from_static(b"done"))
                        .unwrap()
                }
            },
            shutdown_rx,
        ));

        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        started_rx.await.unwrap();
        shutdown_tx.send(true).unwrap();

        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        task.await.unwrap().unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(output.contains("connection: close\r\n"));
        assert!(output.ends_with("\r\n\r\ndone"));
    }

    #[tokio::test]
    async fn serves_native_http_over_a_rustls_stream() {
        let certificate =
            rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let certificate_der =
            rustls::pki_types::CertificateDer::from(certificate.serialize_der().unwrap());
        let private_key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.serialize_private_key_der()),
        );
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate_der.clone()], private_key)
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate_der).unwrap();
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let (client, server) = tokio::io::duplex(8192);

        let server_task = tokio::spawn(async move {
            let tls = TlsAcceptor::from(Arc::new(server_config))
                .accept(server)
                .await
                .unwrap();
            serve_connection(tls, ServerConfig::default(), |_| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Bytes::from_static(b"secure"))
                    .unwrap()
            })
            .await
            .unwrap();
        });
        let server_name = rustls::pki_types::ServerName::try_from("localhost")
            .unwrap()
            .to_owned();
        let mut client = tokio_rustls::TlsConnector::from(Arc::new(client_config))
            .connect(server_name, client)
            .await
            .unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        server_task.await.unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(output.ends_with("\r\n\r\nsecure"));
    }

    #[tokio::test]
    async fn cancels_a_timed_out_handler_and_returns_503() {
        struct DropSignal(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_cancelled = cancelled.clone();
        let (mut client, server) = tokio::io::duplex(2048);
        let config = ServerConfig {
            handler_timeout: Duration::from_millis(10),
            ..ServerConfig::default()
        };
        let task = tokio::spawn(serve_connection(server, config, move |_| {
            let handler_cancelled = handler_cancelled.clone();
            async move {
                let _drop_signal = DropSignal(handler_cancelled);
                std::future::pending::<Response<Bytes>>().await
            }
        }));

        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        task.await.unwrap().unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(output.ends_with("\r\n\r\nrequest timeout"));
        assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn rejects_missing_tls_files() {
        let error = load_tls_config(
            "/path/that/does/not/exist/certificate.pem",
            "/path/that/does/not/exist/private-key.pem",
        )
        .unwrap_err();
        assert!(matches!(error, HttpError::Io(_)));
    }

    #[test]
    fn websocket_permessage_deflate_round_trips() {
        let payload = b"compressed websocket payload compressed websocket payload";
        let compressed = websocket_deflate(payload).unwrap();
        assert_ne!(compressed, payload);
        assert_eq!(websocket_inflate(&compressed, 1024).unwrap(), payload);
        assert!(matches!(
            websocket_inflate(&compressed, 4),
            Err(HttpError::BodyTooLarge)
        ));
    }
}
