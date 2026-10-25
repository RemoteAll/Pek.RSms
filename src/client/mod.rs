//! 短信服务商客户端（对应 C# `Pek.Sms.<Provider>` 各包）。
//!
//! 统一入口：[`create_client`] 根据 `SmsData.Name` 创建客户端；
//! 也可直接使用各服务商类型（`AliyunSmsClient` 等）。
//!
//! 特性开关：`aliyun` / `tencent` / `fenghuo` / `lianlu`（默认全开）。

use crate::error::SmsError;
use crate::message::{SmsRequest, SmsResult};
use crate::settings::{SmsData, SmsSettings};

#[cfg(feature = "aliyun")]
pub mod aliyun;
#[cfg(feature = "fenghuo")]
pub mod fenghuo;
#[cfg(feature = "lianlu")]
pub mod lianlu;
#[cfg(feature = "tencent")]
pub mod tencent;

#[cfg(feature = "aliyun")]
pub use aliyun::AliyunSmsClient;
#[cfg(feature = "fenghuo")]
pub use fenghuo::FengHuoSmsClient;
#[cfg(feature = "lianlu")]
pub use lianlu::LianLuSmsClient;
#[cfg(feature = "tencent")]
pub use tencent::TencentSmsClient;

/// 短信客户端接口（同步；失败以 [`SmsResult::success=false`] 返回，对齐 C#）。
pub trait SmsClient: Send + Sync {
    /// 服务商标识（与配置 `Name` 对应）。
    fn name(&self) -> &'static str;

    /// 发送短信。
    fn send(&self, request: &SmsRequest) -> SmsResult;
}

/// 根据配置创建客户端（内部克隆 `config`，保留旧签名）。
pub fn create_client(
    config: &SmsData,
    settings: &SmsSettings,
) -> Result<Box<dyn SmsClient>, SmsError> {
    create_client_owned(config.clone(), settings)
}

/// 根据配置创建客户端（**接管 `config`，不再二次克隆**）。
///
/// `Smser` 已把账号克隆进 `Vec` 时使用本函数；只有烽火需要 `settings`（代理），
/// 其余三家不会克隆整份 `SmsSettings`。
pub fn create_client_owned(
    config: SmsData,
    _settings: &SmsSettings,
) -> Result<Box<dyn SmsClient>, SmsError> {
    let name = config.name.trim().to_ascii_lowercase();
    match name.as_str() {
        #[cfg(feature = "aliyun")]
        "aliyun" | "aliyun_dysms" => Ok(Box::new(AliyunSmsClient::new(config))),
        #[cfg(feature = "tencent")]
        "tencent" | "tencentcloud" => Ok(Box::new(TencentSmsClient::new(config))),
        #[cfg(feature = "fenghuo")]
        "fenghuo" => Ok(Box::new(FengHuoSmsClient::from_settings_ref(
            config, _settings,
        ))),
        #[cfg(feature = "lianlu")]
        "lianlu" => Ok(Box::new(LianLuSmsClient::new(config))),
        other => Err(SmsError::UnsupportedProvider(other.to_string())),
    }
}

/// 统一的失败结果（缺少配置项时使用）。
#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
pub(crate) fn invalid_result(provider: &str, message: impl Into<String>) -> SmsResult {
    SmsResult::failure(provider, "InvalidArgument", message, "")
}

/// 统一的 HTTP 失败结果。
#[cfg(any(
    feature = "aliyun",
    feature = "tencent",
    feature = "fenghuo",
    feature = "lianlu"
))]
pub(crate) fn http_failure(provider: &str, error: impl Into<String>) -> SmsResult {
    SmsResult::failure(provider, "HttpError", error, "")
}

/// 从 JSON 对象取字符串（数字/布尔转字符串）。
#[cfg(any(feature = "aliyun", feature = "tencent", feature = "fenghuo"))]
pub(crate) fn json_text(value: &serde_json::Value, key: &str) -> String {
    dhrust::json::get_str(value, key).unwrap_or_default()
}

/// 从 JSON 对象取整数（字符串也尝试解析）。
#[cfg(feature = "fenghuo")]
pub(crate) fn json_i64(value: &serde_json::Value, key: &str) -> i64 {
    dhrust::json::get_i64(value, key).unwrap_or_default()
}
