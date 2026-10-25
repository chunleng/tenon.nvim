use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use bytes::Bytes;
use rand::Rng;
use rig::http_client::{
    DynHttpClient, Error, HttpClientExt, LazyBody, MultipartForm, Request, ReqwestClient, Response,
    Result, StatusCode, StreamingResponse,
};

/// Max time to establish a connection to a provider.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Max time between body chunks on an open connection. Long LLM streams stay alive as long as the
/// provider keeps sending; only a provider that goes silent mid-stream triggers this.
const READ_TIMEOUT: Duration = Duration::from_secs(100);

/// Upper bound on a server-requested `Retry-After` delay so a hostile or misbehaving provider can't
/// stall requests indefinitely.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Retry policy for transient transport failures.
#[derive(Clone, Copy)]
struct RetryConfig {
    /// Max retries after the initial attempt.
    max_retries: u32,
    /// Backoff base; doubles after every retry.
    base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
        }
    }
}

impl RetryConfig {
    /// Zero backoff so tests run retry loops instantly.
    #[cfg(test)]
    fn zero_delay() -> Self {
        Self {
            base_delay: Duration::ZERO,
            ..Self::default()
        }
    }

    /// Backoff before the next retry, or `None` when `error` must surface to
    /// the caller instead. Transport-class failures (connection errors,
    /// timeouts) and transient HTTP statuses (429, 5xx) are retried; other
    /// status failures carry a response the caller may need to inspect.
    fn next_backoff(&self, error: &Error, retries_used: u32) -> Option<Duration> {
        if retries_used >= self.max_retries {
            return None;
        }
        match error.non_success_status() {
            // 429 (rate limit) and 5xx (server errors) are transient by
            // nature; other client errors (400, 404, ...) won't change on
            // retry.
            Some(status)
                if status != StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() =>
            {
                None
            }
            Some(_) => Some(
                self.retry_after(error)
                    .unwrap_or_else(|| self.delay(retries_used)),
            ),
            None => Some(self.delay(retries_used)),
        }
    }

    /// Delay requested by the failed response's `Retry-After` header, capped
    /// at `MAX_RETRY_AFTER`. Only the delta-seconds form is honored; a missing
    /// or malformed header (including the HTTP-date form) falls back to
    /// exponential backoff.
    fn retry_after(&self, error: &Error) -> Option<Duration> {
        let seconds = error
            .non_success_headers()?
            .get(http::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
    }

    /// Exponential backoff with jitter: 50%-100% of `base_delay * 2^retry_index`,
    /// so concurrent clients don't retry in lockstep.
    fn delay(&self, retry_index: u32) -> Duration {
        if self.base_delay.is_zero() {
            return Duration::ZERO;
        }
        let exponential = self.base_delay.as_millis() << retry_index;
        let half = exponential / 2;
        let jittered = half + rand::rng().random_range(0..=half);
        Duration::from_millis(jittered as u64)
    }
}

/// HTTP transport that retries transient transport failures (connection
/// errors, timeouts) so a brief network blip doesn't surface as an error.
///
/// Buffered sends retry through the whole response body. Streaming sends
/// retry only up to the response head: once body bytes flow to the caller, a
/// retry would duplicate already-consumed content, so mid-stream failures
/// surface as errors.
#[derive(Clone)]
pub struct RetryingClient {
    inner: DynHttpClient,
    retry: RetryConfig,
}

impl RetryingClient {
    fn new(inner: DynHttpClient) -> Self {
        Self {
            inner,
            retry: RetryConfig::default(),
        }
    }
}

impl HttpClientExt for RetryingClient {
    #[allow(clippy::manual_async_fn)]
    fn send<T, U>(
        &self,
        _req: Request<T>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        async move {
            Err(Error::instance(std::io::Error::other(
                "buffered send is not implemented: Tenon only streams LLM responses",
            )))
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn send_multipart<U>(
        &self,
        _req: Request<MultipartForm>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        async move {
            Err(Error::instance(std::io::Error::other(
                "buffered multipart send is not implemented: Tenon only streams LLM responses",
            )))
        }
    }

    fn send_streaming<T>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = Result<StreamingResponse>> + Send
    where
        T: Into<Bytes> + Send,
    {
        let inner = self.inner.clone();
        let retry = self.retry;
        let (parts, body) = req.into_parts();
        let body: Bytes = body.into();
        // A cloneable template so every retry attempt can send a fresh copy.
        let template = Request::from_parts(parts.clone(), body);
        async move {
            let mut retries_used = 0u32;
            loop {
                // Only the pre-stream phase (up to the response head) is
                // retried; the stream itself is handed to the caller as-is.
                match inner.send_streaming(template.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => match retry.next_backoff(&e, retries_used) {
                        Some(delay) => {
                            retries_used += 1;
                            tokio::time::sleep(delay).await;
                        }
                        None => return Err(e),
                    },
                }
            }
        }
    }
}

/// Process-wide HTTP transport shared by every provider client. Unlike
/// rig-reqwest's default shared client, this one carries timeouts (so a
/// hanging provider fails with an error instead of blocking forever) and
/// retries transient transport failures.
static SHARED: LazyLock<RetryingClient> = LazyLock::new(|| {
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        // Builder failure is limited to TLS backend init; fall back to the
        // plain client (same behavior as rig's default shared client).
        .unwrap_or_else(|_| reqwest::Client::new());
    RetryingClient::new(DynHttpClient::new(ReqwestClient::from(client)))
});

pub fn shared() -> RetryingClient {
    SHARED.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::StreamExt;
    use rig::http_client::{BoxedStream, HeaderMap, StatusCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn wiremock_serves_stubbed_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let response = reqwest::get(server.uri()).await;
        let response = response.expect("mock server should be reachable");
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("stubbed body"), "ok");
    }

    /// Stub transport that fails its first `fail_times` attempts, then
    /// succeeds with a 200 and body "ok". `status` switches the failure from
    /// a transport error to the given HTTP status.
    #[derive(Clone)]
    struct StubInner {
        attempts: Arc<AtomicUsize>,
        fail_times: usize,
        status: Option<StatusCode>,
    }

    impl StubInner {
        /// Records one attempt and returns the error to fail it with, if any.
        fn attempt(&self) -> Option<Error> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) < self.fail_times {
                return Some(match self.status {
                    Some(status) => Error::non_success_with_details(
                        status,
                        HeaderMap::new(),
                        "stub failure".to_string(),
                    ),
                    None => Error::instance(std::io::Error::other("connection reset by peer")),
                });
            }
            None
        }
    }

    impl HttpClientExt for StubInner {
        fn send<T, U>(
            &self,
            _req: Request<T>,
        ) -> impl Future<Output = Result<Response<LazyBody<U>>>> + Send + 'static
        where
            T: Into<Bytes> + Send,
            U: From<Bytes> + Send + 'static,
        {
            let failure = self.attempt();
            async move {
                if let Some(e) = failure {
                    return Err(e);
                }
                let body: LazyBody<U> =
                    Box::pin(async { Ok::<U, Error>(U::from(Bytes::from_static(b"ok"))) });
                Response::builder()
                    .status(200)
                    .body(body)
                    .map_err(Error::Protocol)
            }
        }

        fn send_multipart<U>(
            &self,
            _req: Request<MultipartForm>,
        ) -> impl Future<Output = Result<Response<LazyBody<U>>>> + Send + 'static
        where
            U: From<Bytes> + Send + 'static,
        {
            let failure = self.attempt();
            async move {
                if let Some(e) = failure {
                    return Err(e);
                }
                let body: LazyBody<U> =
                    Box::pin(async { Ok::<U, Error>(U::from(Bytes::from_static(b"ok"))) });
                Response::builder()
                    .status(200)
                    .body(body)
                    .map_err(Error::Protocol)
            }
        }

        fn send_streaming<T>(
            &self,
            _req: Request<T>,
        ) -> impl Future<Output = Result<StreamingResponse>> + Send
        where
            T: Into<Bytes> + Send,
        {
            let failure = self.attempt();
            async move {
                if let Some(e) = failure {
                    return Err(e);
                }
                let first_chunk: Result<Bytes> = Ok(Bytes::from_static(b"ok"));
                let body: BoxedStream = Box::pin(futures::stream::once(async move { first_chunk }));
                Response::builder()
                    .status(200)
                    .body(body)
                    .map_err(Error::Protocol)
            }
        }
    }

    fn stub_client(
        attempts: Arc<AtomicUsize>,
        fail_times: usize,
        status: Option<StatusCode>,
    ) -> RetryingClient {
        RetryingClient {
            inner: DynHttpClient::new(StubInner {
                attempts,
                fail_times,
                status,
            }),
            retry: RetryConfig::zero_delay(),
        }
    }

    fn stub_request() -> Request<Bytes> {
        Request::builder()
            .method("POST")
            .uri("http://stub")
            .body(Bytes::from_static(b"{}"))
            .expect("static request parts are valid")
    }

    #[tokio::test]
    async fn transport_failure_is_retried_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), 2, None);

        let response = client
            .send_streaming(stub_request())
            .await
            .expect("third attempt should succeed");
        assert_eq!(response.status(), 200);
        let mut body = response.into_body();
        let chunk = body
            .next()
            .await
            .expect("stubbed stream yields a chunk")
            .expect("stubbed chunk decodes");
        assert_eq!(chunk, Bytes::from_static(b"ok"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn persistent_transport_failure_errors_after_max_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), usize::MAX, None);

        let result = client.send_streaming(stub_request()).await;
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("persistent failure should surface as an error"),
        };
        // 1 initial attempt + 3 retries, then the error surfaces.
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        assert!(error.non_success_status().is_none());
    }

    #[tokio::test]
    async fn non_retryable_status_error_is_not_retried() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), 1, Some(StatusCode::BAD_REQUEST));

        let result = client.send_streaming(stub_request()).await;
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("status error should surface immediately"),
        };
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(error.non_success_status(), Some(StatusCode::BAD_REQUEST));
    }

    #[tokio::test]
    async fn server_error_5xx_is_retried_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), 1, Some(StatusCode::BAD_GATEWAY));

        let response = client
            .send_streaming(stub_request())
            .await
            .expect("second attempt should succeed");
        assert_eq!(response.status(), 200);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn persistent_429_errors_after_max_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(
            attempts.clone(),
            usize::MAX,
            Some(StatusCode::TOO_MANY_REQUESTS),
        );

        let result = client.send_streaming(stub_request()).await;
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("persistent 429 should surface as an error"),
        };
        // 1 initial attempt + 3 retries, then the error surfaces.
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        assert_eq!(
            error.non_success_status(),
            Some(StatusCode::TOO_MANY_REQUESTS)
        );
    }

    #[tokio::test]
    async fn streaming_429_is_retried_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), 1, Some(StatusCode::TOO_MANY_REQUESTS));

        let response = client
            .send_streaming(stub_request())
            .await
            .expect("second attempt should succeed");
        assert_eq!(response.status(), 200);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    fn status_error_with_retry_after(seconds: &str) -> Error {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_str(seconds).expect("test header value is valid"),
        );
        Error::non_success_with_details(
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            "rate limited".to_string(),
        )
    }

    #[test]
    fn retry_after_header_is_honored_over_exponential_backoff() {
        let retry = RetryConfig::default();
        let error = status_error_with_retry_after("2");
        assert_eq!(retry.next_backoff(&error, 0), Some(Duration::from_secs(2)));
    }

    #[test]
    fn retry_after_header_is_capped() {
        let retry = RetryConfig::default();
        let error = status_error_with_retry_after("9999");
        assert_eq!(
            retry.retry_after(&error),
            Some(MAX_RETRY_AFTER),
            "a hostile Retry-After must be capped"
        );
    }

    #[test]
    fn retry_after_falls_back_on_missing_or_malformed_header() {
        let retry = RetryConfig::default();
        // Missing header: no Retry-After at all.
        let no_header = Error::non_success_with_details(
            StatusCode::TOO_MANY_REQUESTS,
            HeaderMap::new(),
            "rate limited".to_string(),
        );
        assert_eq!(retry.retry_after(&no_header), None);
        // HTTP-date form and garbage: not delta-seconds, so no Retry-After.
        for malformed in ["Wed, 21 Oct 2015 07:28:00 GMT", "soon"] {
            let error = status_error_with_retry_after(malformed);
            assert_eq!(retry.retry_after(&error), None);
        }
    }

    #[test]
    fn retryable_429_without_retry_after_uses_exponential_backoff() {
        let retry = RetryConfig::default();
        let error = Error::non_success_with_details(
            StatusCode::TOO_MANY_REQUESTS,
            HeaderMap::new(),
            "rate limited".to_string(),
        );
        // First retry: 50%-100% of the 500ms base.
        let backoff = retry
            .next_backoff(&error, 0)
            .expect("429 without Retry-After is still retried");
        assert!(backoff >= Duration::from_millis(250) && backoff <= Duration::from_millis(500));
    }

    fn wiremock_addr(server: &MockServer) -> SocketAddr {
        server
            .uri()
            .trim_start_matches("http://")
            .parse()
            .expect("wiremock binds on 127.0.0.1")
    }

    /// A `RetryingClient` over a fresh reqwest client with zero backoff, for
    /// end-to-end tests against a local mock server.
    fn e2e_client() -> RetryingClient {
        RetryingClient {
            inner: DynHttpClient::new(ReqwestClient::from(reqwest::Client::new())),
            retry: RetryConfig::zero_delay(),
        }
    }

    /// TCP proxy that resets the first connection before any response bytes,
    /// then forwards later connections to `upstream` unchanged.
    async fn spawn_reset_then_forward_proxy(
        upstream: SocketAddr,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("proxy binds");
        let addr = listener.local_addr().expect("proxy address");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                let connection = counter.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::spawn(async move {
                    if connection == 1 {
                        // Provider resets the connection before sending anything.
                        drop(client);
                        return;
                    }
                    let Ok(mut upstream) = TcpStream::connect(upstream).await else {
                        return;
                    };
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                });
            }
        });
        (addr, connections)
    }

    #[tokio::test]
    async fn transient_connection_reset_is_absorbed_by_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let (proxy_addr, connections) =
            spawn_reset_then_forward_proxy(wiremock_addr(&server)).await;

        let response = e2e_client()
            .send_streaming(
                Request::builder()
                    .method("GET")
                    .uri(format!("http://{proxy_addr}/"))
                    .body(Bytes::new())
                    .expect("proxy address is a valid uri"),
            )
            .await
            .expect("transient reset should be retried");
        assert_eq!(response.status(), 200);
        let mut body = response.into_body();
        let chunk = body
            .next()
            .await
            .expect("stream yields a chunk")
            .expect("chunk decodes");
        assert_eq!(chunk, Bytes::from_static(b"ok"));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rate_limited_request_is_retried_end_to_end() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let response = e2e_client()
            .send_streaming(
                Request::builder()
                    .method("GET")
                    .uri(server.uri())
                    .body(Bytes::new())
                    .expect("mock server uri is valid"),
            )
            .await
            .expect("429 should be retried");
        assert_eq!(response.status(), 200);
    }

    fn head_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|pos| pos + 4)
    }

    /// TCP proxy that forwards the response head plus only half the body,
    /// then closes the connection: the client sees a truncated stream.
    async fn spawn_partial_response_proxy(upstream: SocketAddr) -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("proxy binds");
        let addr = listener.local_addr().expect("proxy address");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let Ok(mut upstream) = TcpStream::connect(upstream).await else {
                        return;
                    };
                    // Relay the (small, bodyless) request upstream.
                    let mut buf = [0u8; 4096];
                    let mut request = Vec::new();
                    loop {
                        let Ok(n) = client.read(&mut buf).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buf[..n]);
                        if request.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    if upstream.write_all(&request).await.is_err() {
                        return;
                    }
                    // Read until the full response head is buffered, then
                    // forward the head plus only part of the body.
                    let mut response = Vec::new();
                    loop {
                        let Ok(n) = upstream.read(&mut buf).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        response.extend_from_slice(&buf[..n]);
                        if let Some(head_end) = head_end(&response) {
                            let _ = client.write_all(&response[..head_end]).await;
                            let body = &response[head_end..];
                            let partial = if body.is_empty() {
                                match upstream.read(&mut buf).await {
                                    Ok(n) if n > 0 => &buf[..n / 2],
                                    _ => return,
                                }
                            } else {
                                &body[..body.len() / 2]
                            };
                            let _ = client.write_all(partial).await;
                            // Dropping the socket closes the connection mid-body.
                            return;
                        }
                    }
                });
            }
        });
        (addr, connections)
    }

    #[tokio::test]
    async fn mid_stream_failure_surfaces_without_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("0123456789abcdefghijklmnopqrstuv"),
            )
            .mount(&server)
            .await;
        let (proxy_addr, connections) = spawn_partial_response_proxy(wiremock_addr(&server)).await;

        let response = e2e_client()
            .send_streaming(
                Request::builder()
                    .method("GET")
                    .uri(format!("http://{proxy_addr}/"))
                    .body(Bytes::new())
                    .expect("proxy address is a valid uri"),
            )
            .await
            .expect("response head should arrive");
        let mut stream = response.into_body();

        let mut ok_chunks = 0;
        let mut saw_error = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(_) => ok_chunks += 1,
                Err(_) => saw_error = true,
            }
        }
        assert!(
            ok_chunks >= 1,
            "partial body bytes should arrive before the failure"
        );
        assert!(saw_error, "truncated stream should surface an error");
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "mid-stream failures must not be retried"
        );
    }

    /// Tenon only streams LLM responses, so the buffered paths are stubbed
    /// out: they must fail fast without touching the inner transport.
    #[tokio::test]
    async fn buffered_sends_surface_not_implemented_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let client = stub_client(attempts.clone(), 0, None);

        let send_result = client.send::<Bytes, Bytes>(stub_request()).await;
        assert!(send_result.is_err(), "buffered send is not implemented");

        let multipart_request = Request::builder()
            .method("POST")
            .uri("http://stub")
            .body(MultipartForm::new())
            .expect("static request parts are valid");
        let multipart_result = client.send_multipart::<Bytes>(multipart_request).await;
        assert!(
            multipart_result.is_err(),
            "buffered multipart send is not implemented"
        );

        assert_eq!(
            attempts.load(Ordering::SeqCst),
            0,
            "buffered sends must not reach the inner transport"
        );
    }
}
