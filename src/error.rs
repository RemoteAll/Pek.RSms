//! 错误类型（对应 C# `Pek.Sms` 的异常口径，但用 `Result` 返回）。

use std::fmt;

/// 单个账号的发送失败信息（聚合错误用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFailure {
    /// 账号 Code（未填写时用服务商名称）
    pub code: String,
    /// 服务商标识
    pub provider: String,
    /// 服务商错误码
    pub provider_code: String,
    /// 失败原因
    pub message: String,
    /// 服务商原始响应
    pub raw: String,
}

impl fmt::Display for AccountFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.code, self.provider)?;
        if !self.provider_code.is_empty() {
            write!(f, " {}", self.provider_code)?;
        }
        write!(f, "：{}", self.message)
    }
}

/// 短信错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmsError {
    /// 配置问题（无可用账号、配置读写失败等）
    Config(String),
    /// 参数不合法
    InvalidArgument(String),
    /// 不支持的服务商
    UnsupportedProvider(String),
    /// 指定账号/服务商发送失败
    Provider {
        provider: String,
        code: String,
        message: String,
        raw: String,
    },
    /// 所有可用账号均发送失败（对齐 C# 事件兜底后的失败口径）
    AllFailed {
        attempts: usize,
        failures: Vec<AccountFailure>,
    },
}

impl fmt::Display for SmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmsError::Config(msg) => write!(f, "短信配置错误：{msg}"),
            SmsError::InvalidArgument(msg) => write!(f, "短信参数错误：{msg}"),
            SmsError::UnsupportedProvider(name) => write!(f, "不支持的短信服务商：{name}"),
            SmsError::Provider {
                provider,
                code,
                message,
                ..
            } => {
                if code.is_empty() {
                    write!(f, "{provider} 发送失败：{message}")
                } else {
                    write!(f, "{provider} 发送失败（{code}）：{message}")
                }
            }
            SmsError::AllFailed { attempts, failures } => {
                write!(f, "所有 {attempts} 个短信账号均发送失败")?;
                for (i, item) in failures.iter().enumerate() {
                    write!(f, "{}#{i}: {item}", if i == 0 { "：" } else { "；" })?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SmsError {}
