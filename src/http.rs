//! HTTP 请求辅助（统一超时/重试/TLS 选项；复用 `dhrust::net::http_client`）。
//!
//! 使用 `blocking_request_auto_with`：不在 tokio 上下文时直接调用（省线程 + 运行时），
//! 在 tokio 上下文内才 offthread；dhrust 已固化“运行时内 block_on”防护。

#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
use std::time::Duration;

#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
use dhrust::net::http_client::{
    HttpClientOptions, HttpResponse, RetryPolicy, blocking_request_with_retry,
};

/// POST 请求（带重试）。
///
/// - `attempts` 为总尝试次数（`<=1` 只发一次）；
/// - 传输错误与 408/425/429/5xx 会重试，其余 4xx 不重试；
/// - 失败重试间隔 200ms（仅失败时发生，不对空闲路径产生开销）。
#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
pub(crate) fn post(
    url: &str,
    headers: &[(String, String)],
    content_type: &str,
    body: Vec<u8>,
    timeout_ms: u64,
    attempts: u32,
    insecure_tls: bool,
) -> Result<HttpResponse, String> {
    let refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let options = HttpClientOptions {
        timeout: Duration::from_millis(timeout_ms.max(1000)),
        insecure_tls,
        ca_pem: None,
    };
    let policy = RetryPolicy::new(attempts)
        .with_delay(Duration::from_millis(200))
        .with_statuses([408, 425, 429, 500, 502, 503, 504]);

    blocking_request_with_retry(
        "POST",
        url,
        &refs,
        Some(content_type),
        body,
        options,
        &policy,
    )
    .map_err(|error| error.to_string())
}

/// 响应体文本（UTF-8 有损）。
#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
pub(crate) fn body_text(response: &HttpResponse) -> String {
    response.body_text()
}

/// 响应状态是否为 2xx。
#[cfg(feature = "aliyun")]
pub(crate) fn is_success_status(status: u16) -> bool {
    (200..300).contains(&status)
}
