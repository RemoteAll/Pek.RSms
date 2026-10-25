//! 连陆短信客户端（对应 C# `Pek.Sms.LianLu.LianLuSmsClient`）。
//!
//! 协议：`{BaseUrl}/api/sms/send`，表单提交，MD5 签名；响应文本包含“提交成功”即成功。

use crate::client::{SmsClient, http_failure, invalid_result};
use crate::http;
use crate::message::{SmsRequest, SmsResult};
use crate::settings::{SmsData, SmsSettings};

/// 默认 API 地址。
pub const LIANLU_DEFAULT_BASE_URL: &str = "http://47.110.199.86:8081";

/// 连陆短信客户端。
#[derive(Debug, Clone)]
pub struct LianLuSmsClient {
    config: SmsData,
}

impl LianLuSmsClient {
    /// 服务商标识。
    pub const NAME: &'static str = "lianlu";

    /// 以配置构造。
    pub fn new(config: SmsData) -> Self {
        Self { config }
    }

    /// 以配置 + 全局设置构造（连陆暂不使用代理，保留统一签名）。
    pub fn with_settings(config: SmsData, _settings: SmsSettings) -> Self {
        Self::new(config)
    }
}

impl SmsClient for LianLuSmsClient {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn send(&self, request: &SmsRequest) -> SmsResult {
        let config = &self.config;
        if config.access_key.trim().is_empty() {
            return invalid_result(Self::NAME, "用户ID（AccessKey）不能为空");
        }
        if config.access_secret.trim().is_empty() {
            return invalid_result(Self::NAME, "密钥（AccessSecret）不能为空");
        }
        if config.sign_name.trim().is_empty() {
            return invalid_result(Self::NAME, "签名（SignName）不能为空");
        }
        if !request.has_mobiles() {
            return invalid_result(Self::NAME, "手机号列表为空");
        }
        let content = request.render_content();
        if content.trim().is_empty() {
            return invalid_result(Self::NAME, "短信内容不能为空（连陆不支持模板发送）");
        }

        let timestamp = dhrust::times::gettimestamp();
        let sign = dhrust::sign::md5_hex(&format!(
            "{}{}{}",
            config.access_key.trim(),
            timestamp,
            config.access_secret.trim()
        ));
        let timestamp_text = timestamp.to_string();
        let mobiles = request.mobiles_string();
        let params = [
            ("userid", config.access_key.trim()),
            ("ts", timestamp_text.as_str()),
            ("sign", sign.as_str()),
            ("mobile", mobiles.as_str()),
            ("msgcontent", content.as_str()),
            ("extnum", ""),
            ("time", ""),
            ("messageid", ""),
        ];
        let body = params
            .iter()
            .map(|(key, value)| {
                format!(
                    "{}={}",
                    dhrust::web::url_encode(key),
                    dhrust::web::url_encode(value)
                )
            })
            .collect::<Vec<_>>()
            .join("&");

        let base = config.base_url(LIANLU_DEFAULT_BASE_URL);
        let url = format!("{}/api/sms/send", base.trim_end_matches('/'));
        let response = match http::post(
            &url,
            &[],
            "application/x-www-form-urlencoded",
            body.into_bytes(),
            config.timeout_ms(),
            config.attempts(),
            config.insecure_tls(),
        ) {
            Ok(response) => response,
            Err(error) => return http_failure(Self::NAME, error),
        };

        let text = http::body_text(&response);
        if text.contains("提交成功") {
            let mut result = SmsResult::ok(Self::NAME, text.clone(), text);
            result.code = "OK".to_string();
            result
        } else {
            let message = if text.trim().is_empty() {
                format!("提交失败（HTTP {}）", response.status)
            } else {
                text.clone()
            };
            SmsResult::failure(Self::NAME, "SubmitFailed", message, text)
        }
    }
}
