//! 短信外观（Facade）：配置 + 多账号兜底发送。
//!
//! 对应 C# `SmsSettings` + 各 `SmsEventConsumer` 的 `Order` 兜底链：
//! 同一 `SmsType` 的已启用账号按 `Order` 升序依次尝试，失败自动切换下一家。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use crate::client::create_client;
use crate::error::{AccountFailure, SmsError};
use crate::message::{SmsRequest, SmsResult, SmsType};
use crate::settings::{SmsData, SmsSettings, SmsSettingsFile};

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
    file: Option<Mutex<SmsSettingsFile>>,
    settings: Arc<RwLock<SmsSettings>>,
}

impl Smser {
    /// 打开指定配置文件（不存在时按模板生成）。
    pub fn open(path: impl AsRef<Path>) -> Self {
        let file = SmsSettingsFile::load(path);
        let settings = Arc::new(RwLock::new(file.settings().clone()));
        Self {
            file: Some(Mutex::new(file)),
            settings,
        }
    }

    /// 打开程序基目录下的 `Config/Sms.toml`。
    pub fn open_in(base: impl AsRef<Path>) -> Self {
        Self::open(SmsSettings::default_path(base))
    }

    /// 直接以内存配置构造（不读写磁盘；测试或动态配置场景）。
    pub fn with_settings(settings: SmsSettings) -> Self {
        Self {
            file: None,
            settings: Arc::new(RwLock::new(settings)),
        }
    }

    /// 配置文件路径（纯内存模式返回 `None`）。
    pub fn config_path(&self) -> Option<PathBuf> {
        self.file.as_ref().map(|file| {
            file.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .path()
                .to_path_buf()
        })
    }

    /// 当前配置快照。
    pub fn settings(&self) -> SmsSettings {
        self.settings
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    /// 加载期提示（配置生成/补齐/迁移等；纯内存模式为空）。
    pub fn take_notes(&self) -> Vec<String> {
        self.file
            .as_ref()
            .map(|file| {
                file.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take_notes()
            })
            .unwrap_or_default()
    }

    /// 外部修改配置后热加载；返回提示文本（空 = 未变化/纯内存模式）。
    pub fn reload_if_changed(&self) -> Vec<String> {
        let Some(file_handle) = self.file.as_ref() else {
            return Vec::new();
        };
        let mut file = file_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let notes = file.reload_if_changed();
        if !notes.is_empty() {
            self.replace_settings(file.settings().clone());
            for note in &notes {
                dhrust::info!("短信配置：{note}");
            }
        }
        notes
    }

    /// 保存新配置（写回 TOML，含 `[[Data]]`）并立即生效；纯内存模式只更新内存。
    pub fn save_settings(&self, settings: SmsSettings) -> Result<(), SmsError> {
        if let Some(file_handle) = self.file.as_ref() {
            let mut file = file_handle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *file.settings_mut() = settings.clone();
            file.save()?;
        }
        self.replace_settings(settings);
        Ok(())
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
        self.send_accounts(&settings, &accounts, request, sms_type)
    }

    /// 指定账号 Code 发送（不兜底；失败返回 [`SmsError::Provider`]）。
    pub fn send_with_code(&self, code: &str, request: &SmsRequest) -> Result<SmsResult, SmsError> {
        let settings = self.settings();
        let account = settings
            .find_by_code(code)
            .cloned()
            .ok_or_else(|| SmsError::Config(format!("未找到短信账号：{code}")))?;
        let client = create_client(&account, &settings)?;
        let result = client.send(request);
        if result.success {
            dhrust::info!(
                "短信发送成功：账号={} 服务商={} 手机号={}",
                account.label(),
                result.provider,
                request.mobiles_string()
            );
            Ok(result)
        } else {
            dhrust::warn!(
                "短信发送失败：账号={} 服务商={} 错误={}",
                account.label(),
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
        self.send_accounts(&settings, &accounts, request, sms_type)
    }

    /// 异步发送：后台线程执行同步发送（库不引入异步运行时）。
    pub fn send_async(
        self: &Arc<Self>,
        sms_type: SmsType,
        request: SmsRequest,
    ) -> JoinHandle<Result<SmsResult, SmsError>> {
        let this = Arc::clone(self);
        std::thread::spawn(move || this.send(sms_type, &request))
    }

    fn send_accounts(
        &self,
        settings: &SmsSettings,
        accounts: &[SmsData],
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
        for account in accounts {
            let client = match create_client(account, settings) {
                Ok(client) => client,
                Err(error) => {
                    dhrust::warn!("短信账号 {} 初始化失败：{error}", account.label());
                    failures.push(AccountFailure {
                        code: account.label().to_string(),
                        provider: account.name.clone(),
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
                    account.label(),
                    request.mobiles_string(),
                    result.message
                );
                return Ok(result);
            }
            dhrust::warn!(
                "短信账号 {}（{}）发送失败：{}，尝试切换下一个",
                account.label(),
                result.provider,
                result.message
            );
            failures.push(AccountFailure {
                code: account.label().to_string(),
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

    fn replace_settings(&self, settings: SmsSettings) {
        if let Ok(mut guard) = self.settings.write() {
            *guard = settings;
        }
    }
}
