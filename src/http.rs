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
use dhrust::net::http_client::{HttpClientOptions, HttpResponse, blocking_request_auto_with};

/// POST 请求（带重试）。
///
/// - `attempts` 为总尝试次数（`<=1` 只发一次）；
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

    let attempts = attempts.max(1);
    let mut last_error = String::new();
    for attempt in 0..attempts {
        match blocking_request_auto_with(
            "POST",
            url,
            &refs,
            Some(content_type),
            body.clone(),
            options.clone(),
        ) {
            Ok(response) => return Ok(response),
            Err(err) => {
                last_error = err.to_string();
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }
    Err(last_error)
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
