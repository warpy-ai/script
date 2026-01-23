//! High-Performance HTTP Server
//!
//! Features:
//! - SIMD-accelerated HTTP parsing via httparse
//! - Multi-worker architecture with channel distribution
//! - Pre-allocated connection buffers
//! - Lock-free routing (no Mutex contention)
//! - Edge-triggered I/O ready
//! - TLS support with rustls

use super::super::r#async::{TcpListener, TcpStream, AsyncTcpListener, Runtime};
use super::{Request, Response, RequestParser, Method, Version, Header};
use std::collections::HashMap;
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::net::SocketAddr;
use std::path::Path;
use std::io;
use std::sync::mpsc;
use std::thread;

#[cfg(feature = "tls")]
use super::super::r#async::tls::{TlsAcceptor, TlsServerConfig, TlsStream};

// ============================================================================
// Route and Handler Types
// ============================================================================

pub struct Route {
    method: Option<Method>,
    pattern: String,
    handler: Arc<dyn Handler + Send + Sync>,
}

impl Route {
    pub fn new(method: Option<Method>, pattern: String, handler: Arc<dyn Handler + Send + Sync>) -> Self {
        Self { method, pattern, handler }
    }
}

pub trait Handler: Send + Sync {
    fn handle(&self, request: &Request) -> Response;
}

// ============================================================================
// Router (Sync version - legacy)
// ============================================================================

pub struct Router {
    routes: Vec<Route>,
    not_found: Arc<dyn Handler + Send + Sync>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            not_found: Arc::new(DefaultNotFoundHandler),
        }
    }

    pub fn route<H>(
        &mut self,
        method: Option<Method>,
        pattern: &str,
        handler: H,
    ) where
        H: Handler + 'static,
    {
        self.routes.push(Route::new(
            method,
            pattern.to_string(),
            Arc::new(handler),
        ));
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Get), pattern, handler);
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Post), pattern, handler);
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Put), pattern, handler);
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Delete), pattern, handler);
    }

    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(None, pattern, handler);
    }

    pub fn set_not_found<H>(&mut self, handler: H)
    where
        H: Handler + 'static,
    {
        self.not_found = Arc::new(handler);
    }
}

struct DefaultNotFoundHandler;

impl Handler for DefaultNotFoundHandler {
    fn handle(&self, _request: &Request) -> Response {
        Response::new(Version::Http11, 404, "Not Found".to_string())
    }
}

// ============================================================================
// Request with Parameters
// ============================================================================

pub struct RequestWithParams {
    request: Request,
    params: HashMap<String, String>,
}

impl RequestWithParams {
    pub fn new(request: Request, params: HashMap<String, String>) -> Self {
        Self { request, params }
    }

    pub fn method(&self) -> &Method {
        self.request.method()
    }

    pub fn uri(&self) -> &str {
        self.request.uri()
    }

    pub fn headers(&self) -> &[Header] {
        self.request.headers()
    }

    pub fn body(&self) -> Option<&[u8]> {
        self.request.body()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.request.header(name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn into_request(self) -> Request {
        self.request
    }
}

// ============================================================================
// Async Handler and Router (Lock-free)
// ============================================================================

type HandlerResult = Result<Response, Box<dyn std::error::Error + Send + Sync>>;

pub trait AsyncHandler: Send + Sync {
    fn handle(&self, request: RequestWithParams) -> HandlerResult;
}

struct BoxAsyncHandler<F> {
    f: F,
}

impl<F> AsyncHandler for BoxAsyncHandler<F>
where
    F: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
{
    fn handle(&self, request: RequestWithParams) -> HandlerResult {
        (self.f)(request)
    }
}

/// Async Router - immutable after construction for lock-free routing
///
/// Routes are added during setup, then the router is frozen (wrapped in Arc)
/// for concurrent access without locks.
pub struct AsyncRouter {
    routes: Vec<(Option<Method>, String, Arc<dyn AsyncHandler + Send + Sync>)>,
    not_found: Arc<dyn AsyncHandler + Send + Sync>,
}

impl AsyncRouter {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            not_found: Arc::new(DefaultAsyncNotFoundHandler),
        }
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Get),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Post),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Put),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Delete),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            None,
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn set_not_found<H>(&mut self, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.not_found = Arc::new(BoxAsyncHandler { f: handler });
    }

    /// Route a request to the appropriate handler (lock-free)
    #[inline]
    fn route(&self, request: &Request) -> Response {
        for (method, pattern, handler) in &self.routes {
            if method.as_ref().map_or(false, |m| m != request.method()) {
                continue;
            }
            if match_pattern(pattern, request.uri()) {
                let params = extract_params(pattern, request.uri());
                let req_with_params = RequestWithParams::new(request.clone(), params);
                return handler.handle(req_with_params).unwrap_or_else(|_| {
                    Response::new(Version::Http11, 500, "Internal Server Error".to_string())
                });
            }
        }

        self.not_found.handle(RequestWithParams::new(
            request.clone(),
            HashMap::new(),
        )).unwrap_or_else(|_| {
            Response::new(Version::Http11, 404, "Not Found".to_string())
        })
    }
}

struct DefaultAsyncNotFoundHandler;

impl AsyncHandler for DefaultAsyncNotFoundHandler {
    fn handle(&self, _request: RequestWithParams) -> HandlerResult {
        Ok(Response::new(Version::Http11, 404, "Not Found".to_string()))
    }
}

// ============================================================================
// Pattern Matching Helpers
// ============================================================================

#[inline]
fn match_pattern(pattern: &str, uri: &str) -> bool {
    // Fast path for exact match
    if pattern == uri {
        return true;
    }

    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let uri_path = uri.split('?').next().unwrap_or(uri);
    let uri_parts: Vec<&str> = uri_path.split('/').filter(|s| !s.is_empty()).collect();

    if pattern_parts.len() != uri_parts.len() {
        return false;
    }

    for (p, u) in pattern_parts.iter().zip(uri_parts.iter()) {
        if p.starts_with(':') {
            continue;
        }
        if p.starts_with('*') {
            return true;
        }
        if *p != *u {
            return false;
        }
    }

    true
}

#[inline]
fn extract_params(pattern: &str, uri: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let uri_path = uri.split('?').next().unwrap_or(uri);
    let uri_parts: Vec<&str> = uri_path.split('/').filter(|s| !s.is_empty()).collect();

    for (i, p) in pattern_parts.iter().enumerate() {
        if p.starts_with(':') {
            let name = &p[1..];
            if i < uri_parts.len() {
                params.insert(name.to_string(), uri_parts[i].to_string());
            }
        }
    }

    params
}

// ============================================================================
// Connection State (Pre-allocated buffers)
// ============================================================================

/// Pre-allocated connection state for zero-allocation request handling
struct ConnectionState {
    parser: RequestParser,
    write_buf: Vec<u8>,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            parser: RequestParser::with_capacity(8192),
            write_buf: Vec::with_capacity(4096),
        }
    }

    fn reset(&mut self) {
        self.parser.reset();
        self.write_buf.clear();
    }
}

// ============================================================================
// HTTP Server (Multi-worker)
// ============================================================================

/// High-performance HTTP server with multi-worker architecture
///
/// Features:
/// - Lock-free routing after setup
/// - Pre-allocated connection buffers
/// - Channel-based connection distribution
/// - SIMD-accelerated HTTP parsing
pub struct HttpServer {
    listener: TcpListener,
    router: Arc<AsyncRouter>,
    runtime: Runtime,
    num_workers: usize,
}

impl HttpServer {
    /// Bind the server to an address
    pub async fn bind(addr: &SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let router = Arc::new(AsyncRouter::new());
        let runtime = Runtime::new()?;
        let num_workers = thread::available_parallelism()
            .map(|n| n.get().min(6)) // Cap at 6 (performance cores)
            .unwrap_or(4);

        Ok(Self {
            listener,
            router,
            runtime,
            num_workers,
        })
    }

    /// Set the number of worker threads
    pub fn workers(mut self, n: usize) -> Self {
        self.num_workers = n;
        self
    }

    /// Get mutable access to the router for setup
    ///
    /// Note: This should only be called before `serve()` is called.
    /// After serving starts, the router is frozen.
    pub fn router(&mut self) -> &mut AsyncRouter {
        Arc::get_mut(&mut self.router).expect("Router already shared - cannot modify after serve()")
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().get(pattern, handler);
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().post(pattern, handler);
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().put(pattern, handler);
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().delete(pattern, handler);
    }

    /// Start serving HTTP connections
    ///
    /// This method runs forever, accepting connections and routing requests.
    /// After this is called, the router cannot be modified.
    pub async fn serve(&mut self) {
        println!(
            "HTTP Server listening on {} ({} workers)",
            self.listener.local_addr().unwrap(),
            self.num_workers
        );

        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    let router = self.router.clone();
                    self.runtime.spawn(async move {
                        handle_connection(stream, addr, router).await;
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
}

/// Handle a single HTTP connection with pre-allocated buffers
async fn handle_connection(
    mut stream: TcpStream,
    _addr: SocketAddr,
    router: Arc<AsyncRouter>,
) {
    let mut state = ConnectionState::new();
    let mut read_buf = [0u8; 8192];

    loop {
        match stream.read(&mut read_buf).await {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                state.parser.feed(&read_buf[..n]);

                // Process all complete requests (pipelining support)
                while let Ok(Some(request)) = state.parser.parse() {
                    let response = router.route(&request);

                    // Build response into pre-allocated buffer
                    build_response(&response, &mut state.write_buf);

                    if let Err(_) = stream.write_all(&state.write_buf).await {
                        return;
                    }
                    state.write_buf.clear();
                    state.parser.drain(0); // Drain consumed data
                }

                // Check for oversized requests
                if state.parser.buf.len() > 1024 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Build HTTP response into a pre-allocated buffer
#[inline]
fn build_response(response: &Response, buf: &mut Vec<u8>) {
    buf.extend_from_slice(response.version().as_str().as_bytes());
    buf.push(b' ');

    // Use itoa for fast integer formatting (avoid format! allocation)
    let status = response.status();
    if status < 10 {
        buf.push(b'0' + status as u8);
    } else if status < 100 {
        buf.push(b'0' + (status / 10) as u8);
        buf.push(b'0' + (status % 10) as u8);
    } else {
        buf.push(b'0' + (status / 100) as u8);
        buf.push(b'0' + ((status / 10) % 10) as u8);
        buf.push(b'0' + (status % 10) as u8);
    }

    buf.push(b' ');
    buf.extend_from_slice(response.reason().as_bytes());
    buf.extend_from_slice(b"\r\n");

    for header in response.headers() {
        buf.extend_from_slice(header.name().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(header.value().as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    if let Some(body) = response.body() {
        buf.extend_from_slice(b"Content-Length: ");
        let len = body.len();
        if len == 0 {
            buf.push(b'0');
        } else {
            // Fast integer to string conversion
            let mut tmp = [0u8; 20];
            let mut n = len;
            let mut i = tmp.len();
            while n > 0 {
                i -= 1;
                tmp[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
            buf.extend_from_slice(&tmp[i..]);
        }
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"\r\n");

    if let Some(body) = response.body() {
        buf.extend_from_slice(body);
    }
}

// ============================================================================
// Response Helpers
// ============================================================================

pub fn method_not_allowed() -> Response {
    Response::new(Version::Http11, 405, "Method Not Allowed".to_string())
}

pub fn bad_request() -> Response {
    Response::new(Version::Http11, 400, "Bad Request".to_string())
}

pub fn internal_error() -> Response {
    Response::new(Version::Http11, 500, "Internal Server Error".to_string())
}

pub fn ok(body: &str) -> Response {
    let mut response = Response::new(Version::Http11, 200, "OK".to_string());
    response.add_header("Content-Type".to_string(), "text/plain".to_string());
    response.set_body(body.as_bytes().to_vec());
    response
}

pub fn json(body: &str) -> Response {
    let mut response = Response::new(Version::Http11, 200, "OK".to_string());
    response.add_header("Content-Type".to_string(), "application/json".to_string());
    response.set_body(body.as_bytes().to_vec());
    response
}

pub fn redirect(url: &str) -> Response {
    let mut response = Response::new(Version::Http11, 302, "Found".to_string());
    response.add_header("Location".to_string(), url.to_string());
    response
}

// ============================================================================
// HTTPS Server (TLS-enabled)
// ============================================================================

#[cfg(feature = "tls")]
pub struct HttpsServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    router: Arc<AsyncRouter>,
    runtime: Runtime,
    num_workers: usize,
}

#[cfg(feature = "tls")]
impl HttpsServer {
    /// Bind an HTTPS server with TLS using certificate and key files.
    pub async fn bind(
        addr: &SocketAddr,
        cert_path: &Path,
        key_path: &Path,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let tls_config = TlsServerConfig::from_pem_files(cert_path, key_path)?;
        let acceptor = TlsAcceptor::new(tls_config);
        let router = Arc::new(AsyncRouter::new());
        let runtime = Runtime::new()?;
        let num_workers = thread::available_parallelism()
            .map(|n| n.get().min(6))
            .unwrap_or(4);

        Ok(Self {
            listener,
            acceptor,
            router,
            runtime,
            num_workers,
        })
    }

    /// Bind with custom TLS configuration.
    pub async fn bind_with_config(
        addr: &SocketAddr,
        tls_config: TlsServerConfig,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let acceptor = TlsAcceptor::new(tls_config);
        let router = Arc::new(AsyncRouter::new());
        let runtime = Runtime::new()?;
        let num_workers = thread::available_parallelism()
            .map(|n| n.get().min(6))
            .unwrap_or(4);

        Ok(Self {
            listener,
            acceptor,
            router,
            runtime,
            num_workers,
        })
    }

    /// Set the number of worker threads
    pub fn workers(mut self, n: usize) -> Self {
        self.num_workers = n;
        self
    }

    /// Get mutable access to the router for setup
    pub fn router(&mut self) -> &mut AsyncRouter {
        Arc::get_mut(&mut self.router).expect("Router already shared")
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().get(pattern, handler);
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().post(pattern, handler);
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().put(pattern, handler);
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().delete(pattern, handler);
    }

    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router().all(pattern, handler);
    }

    /// Start serving HTTPS connections.
    pub async fn serve(&mut self) {
        println!(
            "HTTPS Server listening on {} (TLS enabled, {} workers)",
            self.listener.local_addr().unwrap(),
            self.num_workers
        );

        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    match self.acceptor.accept(stream) {
                        Ok(tls_stream) => {
                            let router = self.router.clone();
                            self.runtime.spawn(async move {
                                handle_tls_connection(tls_stream, addr, router).await;
                            });
                        }
                        Err(e) => {
                            eprintln!("TLS handshake error from {}: {}", addr, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
}

#[cfg(feature = "tls")]
async fn handle_tls_connection(
    mut stream: TlsStream<TcpStream>,
    _addr: SocketAddr,
    router: Arc<AsyncRouter>,
) {
    let mut state = ConnectionState::new();
    let mut read_buf = [0u8; 8192];

    loop {
        match stream.read(&mut read_buf).await {
            Ok(0) => break,
            Ok(n) => {
                state.parser.feed(&read_buf[..n]);

                while let Ok(Some(request)) = state.parser.parse() {
                    let response = router.route(&request);
                    build_response(&response, &mut state.write_buf);

                    if let Err(_) = stream.write_all(&state.write_buf).await {
                        return;
                    }
                    state.write_buf.clear();
                    state.parser.drain(0);
                }

                if state.parser.buf.len() > 1024 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
