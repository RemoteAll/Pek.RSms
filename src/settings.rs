//! 短信配置（对应 C# `Pek.Sms.SmsSettings` / `SmsData`）。
//!
//! - 配置一律 TOML：`Config/Sms.toml`，字段名与 C# `Sms` 配置保持一致；
//! - 读写复用 [`dhrust::config::TomlConfig`]：缺失生成、缺键带注释补齐、类型容错、损坏备份重建、外部改动热加载；
//! - 数组表 `[[Data]]` 的写回复用 [`TomlConfig::save_with_arrays`]（保留注释与排版）。

use std::path::{Path, PathBuf};

use dhrust::config::TomlConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SmsError;
use crate::message::SmsType;

/// 默认配置相对路径（相对程序基目录）。
pub const SMS_CONFIG_FILE: &str = "Config/Sms.toml";

/// 配置模板（内嵌；带全部字段中文注释）。
pub const SMS_CONFIG_TEMPLATE: &str = include_str!("../res/Sms.toml.template");

/// 短信账号（对应 C# `Pek.Sms.SmsData`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct SmsData {
    /// 惟一标识
    pub code: String,
    /// 服务商标识：aliyun / tencent / fenghuo / lianlu
    pub name: String,
    /// 显示名称
    pub display_name: String,
    /// 短信类型：0 国内通知，1 国际通知，2 国内营销，3 国际营销
    pub sms_type: i32,
    /// 是否默认（与 SmsType 搭配）
    pub is_default: bool,
    /// 是否启用（未启用账号不参与发送与兜底）
    pub is_enabled: bool,
    /// AccessKey / AccessKeyId / AppId / 用户名
    pub access_key: String,
    /// AccessKeySecret / AppKey / 密码
    pub access_secret: String,
    /// 签名
    pub sign_name: String,
    /// 排序（越小越先；同一类型失败后自动切换下一个）
    pub order: i32,
    /// 请求超时（毫秒；默认 60000）
    pub timeout: u64,
    /// 是否使用 HTTPS
    pub security: bool,
    /// 重试次数（默认 3，含首次尝试）
    pub retry_times: u32,
    /// 扩展字段说明（仅作文档/面板展示）
    pub extend_fields: String,
    /// 扩展内容（JSON 字符串）
    pub extend_data: String,
}

impl Default for SmsData {
    fn default() -> Self {
        Self {
            code: String::new(),
            name: String::new(),
            display_name: String::new(),
            sms_type: 0,
            is_default: false,
            is_enabled: false,
            access_key: String::new(),
            access_secret: String::new(),
            sign_name: String::new(),
            order: 0,
            timeout: 60_000,
            security: false,
            retry_times: 3,
            extend_fields: String::new(),
            extend_data: String::new(),
        }
    }
}

impl SmsData {
    /// 日志/错误展示用标识（Code 为空时用服务商名称）。
    pub fn label(&self) -> &str {
        if !self.code.trim().is_empty() {
            &self.code
        } else {
            &self.name
        }
    }

    /// 类型化短信类型（未知数值返回 `None`）。
    pub fn sms_type_kind(&self) -> Option<SmsType> {
        SmsType::from_i32(self.sms_type)
    }

    /// 请求超时（毫秒；0 按默认 60000）。
    pub fn timeout_ms(&self) -> u64 {
        if self.timeout == 0 {
            60_000
        } else {
            self.timeout
        }
    }

    /// 重试次数（0/1 都按 1 次尝试）。
    pub fn attempts(&self) -> u32 {
        self.retry_times.max(1)
    }

    /// 解析 `ExtendData`（JSON 对象；为空或非法时返回空表）。
    pub fn extend_map(&self) -> serde_json::Map<String, Value> {
        if self.extend_data.trim().is_empty() {
            return serde_json::Map::new();
        }
        match serde_json::from_str::<Value>(&self.extend_data) {
            Ok(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }

    /// 读取扩展项。
    pub fn extend_value(&self, key: &str) -> Option<Value> {
        self.extend_map().get(key).cloned()
    }

    /// 读取扩展字符串（数字/布尔自动转字符串）。
    pub fn extend_str(&self, key: &str) -> Option<String> {
        self.extend_value(key).and_then(|value| match value {
            Value::String(text) => Some(text),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        })
    }

    /// 读取扩展布尔值（兼容 `"true"/"1"/"yes"` 文本）。
    pub fn extend_bool(&self, key: &str) -> Option<bool> {
        self.extend_value(key).and_then(|value| match value {
            Value::Bool(flag) => Some(flag),
            Value::Number(number) => number.as_i64().map(|n| n != 0),
            Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        })
    }

    /// 读取扩展整数。
    pub fn extend_i64(&self, key: &str) -> Option<i64> {
        self.extend_value(key).and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        })
    }

    /// 扩展配置的 BaseUrl（未配置时用传入默认值）。
    pub fn base_url(&self, default: &str) -> String {
        self.extend_str("BaseUrl")
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    /// 是否跳过 TLS 证书校验（`ExtendData.InsecureTls` / `IgnoreSsl`；默认 false）。
    ///
    /// 说明：C# 客户端统一 `.IgnoreSsl()`；Rust 版默认校验证书（更安全），
    /// 自签/内网环境显式配置 `"InsecureTls": true` 即可。
    pub fn insecure_tls(&self) -> bool {
        self.extend_bool("InsecureTls")
            .or_else(|| self.extend_bool("IgnoreSsl"))
            .unwrap_or(false)
    }
}

/// 短信设置（对应 C# `Pek.Sms.SmsSettings`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct SmsSettings {
    /// 短信账号
    pub data: Vec<SmsData>,
    /// 是否启用代理（目前仅烽火支持）
    pub enable_proxy: bool,
    /// 代理服务器地址
    pub proxy_url: String,
    /// 代理认证令牌（`X-Token-Code`）
    pub proxy_token: String,
}

impl SmsSettings {
    /// 默认配置路径（`base/Config/Sms.toml`）。
    pub fn default_path(base: impl AsRef<Path>) -> PathBuf {
        base.as_ref().join(SMS_CONFIG_FILE)
    }

    /// 加载配置（缺失时按内嵌模板生成）。
    pub fn load(path: impl AsRef<Path>) -> TomlConfig<SmsSettings> {
        TomlConfig::load(path, SMS_CONFIG_TEMPLATE)
    }

    /// 从程序基目录加载 `Config/Sms.toml`。
    pub fn load_from_base(base: impl AsRef<Path>) -> TomlConfig<SmsSettings> {
        Self::load(Self::default_path(base))
    }

    /// 获取默认配置（对齐 C# `FindDefault`：同类型 `IsDefault` 优先，否则同类型第一条）。
    pub fn find_default(&self, sms_type: SmsType) -> Option<&SmsData> {
        let code = sms_type.as_i32();
        self.data
            .iter()
            .find(|e| e.sms_type == code && e.is_default)
            .or_else(|| self.data.iter().find(|e| e.sms_type == code))
    }

    /// 根据 Code 查找（大小写不敏感）。
    pub fn find_by_code(&self, code: &str) -> Option<&SmsData> {
        self.data.iter().find(|e| e.code.eq_ignore_ascii_case(code))
    }

    /// 根据服务商名称与类型查找（名称大小写不敏感）。
    pub fn find_by_name_and_type(&self, name: &str, sms_type: SmsType) -> Option<&SmsData> {
        let code = sms_type.as_i32();
        self.data
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name) && e.sms_type == code)
    }

    /// 获取某服务商的所有配置（名称大小写不敏感）。
    pub fn find_by_name(&self, name: &str) -> Vec<&SmsData> {
        self.data
            .iter()
            .filter(|e| e.name.eq_ignore_ascii_case(name))
            .collect()
    }

    /// 某类型的已启用账号，按 `Order` 升序（同 Order 时默认账号优先）。
    ///
    /// 这是多账号兜底的执行顺序（对齐 C#《参数规范》的 Order 机制）。
    pub fn enabled_for_type(&self, sms_type: SmsType) -> Vec<&SmsData> {
        let code = sms_type.as_i32();
        let mut list: Vec<&SmsData> = self
            .data
            .iter()
            .filter(|e| e.sms_type == code && e.is_enabled)
            .collect();
        list.sort_by_key(|e| (e.order, !e.is_default));
        list
    }

    /// 某服务商 + 某类型的已启用账号，按 `Order` 升序。
    pub fn enabled_for_name_and_type(&self, name: &str, sms_type: SmsType) -> Vec<&SmsData> {
        let code = sms_type.as_i32();
        let mut list: Vec<&SmsData> = self
            .data
            .iter()
            .filter(|e| e.sms_type == code && e.is_enabled && e.name.eq_ignore_ascii_case(name))
            .collect();
        list.sort_by_key(|e| (e.order, !e.is_default));
        list
    }
}

/// 配置文件句柄：持有 [`TomlConfig<SmsSettings>`]，支持保存（含 `[[Data]]` 数组表）与热加载。
pub struct SmsSettingsFile {
    config: TomlConfig<SmsSettings>,
}

impl SmsSettingsFile {
    /// 按路径加载。
    pub fn load(path: impl AsRef<Path>) -> Self {
        Self {
            config: SmsSettings::load(path),
        }
    }

    /// 从程序基目录加载 `Config/Sms.toml`。
    pub fn load_from_base(base: impl AsRef<Path>) -> Self {
        Self {
            config: SmsSettings::load_from_base(base),
        }
    }

    /// 配置路径。
    pub fn path(&self) -> &Path {
        self.config.path()
    }

    /// 当前配置。
    pub fn settings(&self) -> &SmsSettings {
        self.config.value()
    }

    /// 当前配置（可变；修改后调用 [`Self::save`]）。
    pub fn settings_mut(&mut self) -> &mut SmsSettings {
        self.config.value_mut()
    }

    /// 是否首次生成（或损坏后重建）。
    pub fn is_new(&self) -> bool {
        self.config.is_new()
    }

    /// 取走加载/保存期间的提示。
    pub fn take_notes(&mut self) -> Vec<String> {
        self.config.take_notes()
    }

    /// 外部修改时热加载；返回提示文本（空 = 未变化）。
    pub fn reload_if_changed(&mut self) -> Vec<String> {
        self.config.reload_if_changed()
    }

    /// 保存配置（标量 + `[[Data]]` 数组表；保留注释与排版）。
    pub fn save(&mut self) -> Result<(), SmsError> {
        let entries: Vec<Value> = self
            .config
            .value()
            .data
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|e| SmsError::Config(format!("序列化账号失败：{e}")))?;
        self.config
            .save_with_arrays(&[("Data", &entries)])
            .map_err(|e| SmsError::Config(format!("保存短信配置失败：{e}")))
    }
}
