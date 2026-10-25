//! # Pek.RSms（crate `pek-rsms`）
//!
//! C# `Pek.Sms` 的 Rust 对应实现：**短信发送库**（非服务、无面板），供生态内其他 Rust 项目以 path 依赖复用。
//!
//! ## 支持的服务商
//!
//! | 服务商 | 客户端 | 特性开关 | 类型 |
//! |---|---|---|---|
//! | 阿里云 | [`AliyunSmsClient`] | `aliyun` | 模板短信（`HMAC-SHA1` 签名） |
//! | 腾讯云 | [`TencentSmsClient`] | `tencent` | 模板短信（`TC3-HMAC-SHA256`） |
//! | 烽火 | [`FengHuoSmsClient`] | `fenghuo` | 内容/模板 + 代理 |
//! | 连陆 | [`LianLuSmsClient`] | `lianlu` | 内容短信 |
//!
//! ## 快速开始
//!
//! ```no_run
//! use pek_rsms::{SmsRequest, SmsType, Smser};
//!
//! let smser = Smser::open_in("."); // 读取/生成 Config/Sms.toml
//! for note in smser.take_notes() {
//!     dhrust::info!("{note}");
//! }
//!
//! // 同一 SmsType 的已启用账号按 Order 升序兜底
//! let result = smser.send(
//!     SmsType::DomesticNotification,
//!     &SmsRequest::new("13800138000").with_template("SMS_123", [("code", "123456")]),
//! )?;
//! println!("{}", result.message);
//! # Ok::<(), pek_rsms::SmsError>(())
//! ```
//!
//! ## 设计约束
//!
//! - 配置一律 TOML（`Config/Sms.toml`）：复用 `dhrust::config::TomlConfig`；
//! - 不引入异步运行时：同步 API 为主；[`Smser::send_async`] 用后台线程；
//! - HTTP 复用 `dhrust::net::http_client`（可 `spawn_blocking` 之外的任意上下文调用）；
//! - 日志复用 `dhrust::logs`：只记录发送成功/失败/账号切换；
//! - 与 C# 的差异（有意）：
//!   - 默认校验 HTTPS 证书（C# 统一 `.IgnoreSsl()`）；自签环境在 `ExtendData` 配 `"InsecureTls": true`；
//!   - 增加 `Smser` 统一入口，直接按 `SmsType + Order` 兜底（C# 依赖事件总线串联各消费者）。

pub mod client;
pub mod error;
mod http;
pub mod message;
pub mod settings;
pub mod smser;

#[cfg(feature = "aliyun")]
pub use client::AliyunSmsClient;
#[cfg(feature = "fenghuo")]
pub use client::FengHuoSmsClient;
#[cfg(feature = "lianlu")]
pub use client::LianLuSmsClient;
#[cfg(feature = "tencent")]
pub use client::TencentSmsClient;
pub use client::{SmsClient, create_client, create_client_owned};
pub use error::{AccountFailure, SmsError};
pub use message::{SendStatus, SmsRequest, SmsResult, SmsType, split_phone_numbers};
pub use settings::{
    ExtendView, SMS_CONFIG_FILE, SMS_CONFIG_TEMPLATE, SmsData, SmsSettings, SmsSettingsFile,
};
pub use smser::Smser;
