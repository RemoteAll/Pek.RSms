//! 短信外观（Facade）：配置 + 多账号兜底发送。
//!
//! 对应 C# `SmsSettings` + 各 `SmsEventConsumer` 的 `Order` 兜底链：
//! 同一 `SmsType` 的已启用账号按 `Order` 升序依次尝试，失败自动切换下一家。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use dhrust::config::TomlConfigStore;

use crate::client::create_client_owned;
use crate::error::{AccountFailure, SmsError};
use crate::message::{SmsRequest, SmsResult, SmsType};
use crate::settings::{SMS_CONFIG_TEMPLATE, SmsData, SmsSettings};

/// 短信外观。
///
/// ```no_run
/// use pek_rsms::{SmsRequest, SmsType, Smser};
///
/// let smser = Smser::open_in("."); // 读取/生成 Config/Sms.toml
/// let result = smser.send(
///     SmsType::DomesticNotification,
///     &SmsRequest::new("13800138000").with_template("SMS_123", [("code", "123456")]),
/// )?;
/// println!("{}", result.message);
/// # Ok::<(), pek_rsms::SmsError>(())
/// ```
pub struct Smser {
    /// 配置文件句柄 + 共享配置快照（纯内存模式 = 不读写磁盘）
    store: TomlConfigStore<SmsSettings>,
}

impl Smser {
    /// 打开指定配置文件（不存在时按模板生成）。
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self {
            store: TomlConfigStore::open(path, SMS_CONFIG_TEMPLATE),
        }
    }

    /// 打开程序基目录下的 `Config/Sms.toml`。
    pub fn open_in(base: impl AsRef<Path>) -> Self {
        Self::open(SmsSettings::default_path(base))
    }

    /// 直接以内存配置构造（不读写磁盘；测试或动态配置场景）。
    pub fn with_settings(settings: SmsSettings) -> Self {
        Self {
            store: TomlConfigStore::in_memory(settings),
        }
    }

    /// 配置文件路径（纯内存模式返回 `None`）。
    pub fn config_path(&self) -> Option<PathBuf> {
        self.store.path().map(Path::to_path_buf)
    }

    /// 当前配置快照。
    pub fn settings(&self) -> SmsSettings {
        self.store.value()
    }

    /// 加载期提示（配置生成/补齐/迁移等；纯内存模式为空）。
    pub fn take_notes(&self) -> Vec<String> {
        self.store.take_notes()
    }

    /// 外部修改配置后热加载；返回提示文本（空 = 未变化/纯内存模式）。
    ///
    /// 注意：本方法每次都会读取文件元数据（Windows 实测约 49µs），请按秒级/文件监听调用，
    /// 不要放在每条短信的发送路径上。
    pub fn reload_if_changed(&self) -> Vec<String> {
        let notes = self.store.reload_if_changed();
        for note in &notes {
            dhrust::info!("短信配置：{note}");
        }
        notes
    }

    /// 保存新配置（写回 TOML，含 `[[Data]]`）并立即生效；纯内存模式只更新内存。
    pub fn save_settings(&self, settings: SmsSettings) -> Result<(), SmsError> {
        let entries: Vec<serde_json::Value> = settings
            .data
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|error| SmsError::Config(format!("序列化账号失败：{error}")))?;
        self.store
            .save(settings, &[("Data", &entries)])
            .map_err(|error| SmsError::Config(format!("保存短信配置失败：{error}")))
    }

    /// 按 `SmsType` 发送：所有已启用账号按 `Order` 升序兜底，成功即返回。
    ///
    /// 全部失败返回 [`SmsError::AllFailed`]（含每个账号的错误详情）。
    pub fn send(&self, sms_type: SmsType, request: &SmsRequest) -> Result<SmsResult, SmsError> {
        let settings = self.settings();
        let accounts: Vec<SmsData> = settings
            .enabled_for_type(sms_type)
            .into_iter()
            .cloned()
            .collect();
        self.send_accounts(&settings, accounts, request, sms_type)
    }

    /// 指定账号 Code 发送（不兜底；失败返回 [`SmsError::Provider`]）。
    ///
    /// 与 C# `FindByCode` 一致：**不检查 `IsEnabled`**（显式指定账号即视为调用方已知晓）。
    pub fn send_with_code(&self, code: &str, request: &SmsRequest) -> Result<SmsResult, SmsError> {
        let settings = self.settings();
        let account = settings
            .find_by_code(code)
            .cloned()
            .ok_or_else(|| SmsError::Config(format!("未找到短信账号：{code}")))?;
        let label = account.label().to_string();
        let client = create_client_owned(account, &settings)?;
        let result = client.send(request);
        let mobiles = request.mobiles_string();
        if result.success {
            dhrust::info!(
                "短信发送成功：账号={} 服务商={} 手机号={}",
                label,
                result.provider,
                mobiles
            );
            Ok(result)
        } else {
            dhrust::warn!(
                "短信发送失败：账号={} 服务商={} 错误={}",
                label,
                result.provider,
                result.message
            );
            Err(SmsError::Provider {
                provider: result.provider,
                code: result.code,
                message: result.message,
                raw: result.raw,
            })
        }
    }

    /// 指定服务商 + 类型发送：该服务商的已启用账号按 `Order` 兜底。
    pub fn send_with_name(
        &self,
        name: &str,
        sms_type: SmsType,
        request: &SmsRequest,
    ) -> Result<SmsResult, SmsError> {
        let settings = self.settings();
        let accounts: Vec<SmsData> = settings
            .enabled_for_name_and_type(name, sms_type)
            .into_iter()
            .cloned()
            .collect();
        self.send_accounts(&settings, accounts, request, sms_type)
    }

    /// 异步发送：后台线程执行同步发送（库不引入异步运行时）。
    pub fn send_async(
        self: &Arc<Self>,
        sms_type: SmsType,
        request: SmsRequest,
    ) -> JoinHandle<Result<SmsResult, SmsError>> {
        let this = Arc::clone(self);
        dhrust::threading::spawn_named("pek-rsms-send", move || this.send(sms_type, &request))
    }

    fn send_accounts(
        &self,
        settings: &SmsSettings,
        accounts: Vec<SmsData>,
        request: &SmsRequest,
        sms_type: SmsType,
    ) -> Result<SmsResult, SmsError> {
        if accounts.is_empty() {
            return Err(SmsError::Config(format!(
                "没有找到可用的短信账号（类型：{}，请检查 Sms.toml 中 IsEnabled=true 的账号）",
                sms_type.label()
            )));
        }

        let mut failures = Vec::new();
        let mobiles = request.mobiles_string();
        for account in accounts {
            let label = account.label().to_string();
            let provider_name = account.name.clone();
            let client = match create_client_owned(account, settings) {
                Ok(client) => client,
                Err(error) => {
                    dhrust::warn!("短信账号 {label} 初始化失败：{error}");
                    failures.push(AccountFailure {
                        code: label,
                        provider: provider_name,
                        provider_code: "InitError".to_string(),
                        message: error.to_string(),
                        raw: String::new(),
                    });
                    continue;
                }
            };
            let result = client.send(request);
            if result.success {
                dhrust::info!(
                    "短信发送成功：服务商={} 账号={} 手机号={} 结果={}",
                    result.provider,
                    label,
                    mobiles,
                    result.message
                );
                return Ok(result);
            }
            dhrust::warn!(
                "短信账号 {}（{}）发送失败：{}，尝试切换下一个",
                label,
                result.provider,
                result.message
            );
            failures.push(AccountFailure {
                code: label,
                provider: result.provider.clone(),
                provider_code: result.code.clone(),
                message: result.message.clone(),
                raw: result.raw.clone(),
            });
        }

        Err(SmsError::AllFailed {
            attempts: failures.len(),
            failures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_if_changed_refreshes_shared_settings() {
        let dir = std::env::temp_dir().join(format!("pek-rsms-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let smser = Smser::open_in(&dir);
        let path = smser.config_path().expect("文件模式");
        let text = std::fs::read_to_string(&path).expect("读配置");
        let updated = text.replace("ProxyUrl = \"\"", "ProxyUrl = \"http://127.0.0.1:9\"");
        assert_ne!(text, updated, "模板应包含 ProxyUrl");
        std::fs::write(&path, updated).expect("写外部修改");

        let notes = smser.reload_if_changed();
        assert!(
            notes.iter().any(|note| note.contains("热加载")),
            "{notes:?}"
        );
        assert_eq!(smser.settings().proxy_url, "http://127.0.0.1:9");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
