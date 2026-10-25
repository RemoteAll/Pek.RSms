//! 烽火短信客户端（对应 C# `Pek.Sms.FengHuo.FengHuoSmsClient`）。
//!
//! 协议：烽火批量发送接口 `{BaseUrl}/sms/api/sendMessageMass`（JSON + MD5 签名）。
//! 支持代理模式（`X-Target-Url` / `X-Token-Code`）。

use serde_json::{Map, Value, json};

use crate::client::{SmsClient, http_failure, invalid_result};
use crate::http;
use crate::message::{SmsRequest, SmsResult};
use crate::settings::{SmsData, SmsSettings};

/// 默认 API 基础地址（不含 `/sms` 后缀）。
pub const FENGHUO_DEFAULT_BASE_URL: &str = "https://aisms.aipaas.com:8443";

/// 烽火短信客户端。
#[derive(Debug, Clone)]
pub struct FengHuoSmsClient {
    config: SmsData,
    enable_proxy: bool,
    proxy_url: String,
    proxy_token: String,
}

impl FengHuoSmsClient {
    /// 服务商标识。
    pub const NAME: &'static str = "fenghuo";

    /// 以配置构造。
    pub fn new(config: SmsData) -> Self {
        Self {
            config,
            enable_proxy: false,
            proxy_url: String::new(),
            proxy_token: String::new(),
        }
    }

    /// 以配置 + 全局设置构造（代理需要全局设置）。
    pub fn with_settings(config: SmsData, settings: SmsSettings) -> Self {
        Self::from_settings_ref(config, &settings)
    }

    /// 只克隆代理三项（供 `create_client_owned` 使用，避免整份 settings 克隆）。
    pub(crate) fn from_settings_ref(config: SmsData, settings: &SmsSettings) -> Self {
        Self {
            config,
            enable_proxy: settings.enable_proxy,
            proxy_url: settings.proxy_url.clone(),
            proxy_token: settings.proxy_token.clone(),
        }
    }

    /// 实际 API 基础地址（确保以 `/sms` 结尾）。
    pub fn base_url(&self) -> String {
        let mut base = self.config.extend_view().base_url(FENGHUO_DEFAULT_BASE_URL);
        base = base.trim_end_matches('/').to_string();
        if !base.ends_with("/sms") {
            base.push_str("/sms");
        }
        base
    }
}

impl SmsClient for FengHuoSmsClient {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn send(&self, request: &SmsRequest) -> SmsResult {
        let config = &self.config;
        if config.access_key.trim().is_empty() {
            return invalid_result(Self::NAME, "用户名（AccessKey）不能为空");
        }
        if config.access_secret.trim().is_empty() {
            return invalid_result(Self::NAME, "密码（AccessSecret）不能为空");
        }
        if !request.has_mobiles() {
            return invalid_result(Self::NAME, "手机号列表为空");
        }

        let extend = config.extend_view();
        let template_id = request
            .template_id
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0)
            .or_else(|| {
                extend
                    .str("TemplateId")
                    .and_then(|text| text.trim().parse::<i64>().ok())
                    .filter(|value| *value > 0)
            });

        if template_id.is_none() && request.content.trim().is_empty() {
            return invalid_result(Self::NAME, "短信内容和模板ID不能同时为空");
        }

        let final_content = if template_id.is_none() {
            request.render_content()
        } else {
            request.content.clone()
        };
        let params: Option<Value> = if template_id.is_some() && !request.template_params.is_empty()
        {
            let map: Map<String, Value> = request
                .template_params
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect();
            Some(Value::Object(map))
        } else {
            None
        };

        let timestamp = dhrust::times::getmilltimestamp() as i64;
        let password_md5 = dhrust::sign::md5_hex(config.access_secret.trim());
        let sign = dhrust::sign::md5_hex(&format!(
            "{}{}{}",
            config.access_key.trim(),
            timestamp,
            password_md5
        ));

        let payload = json!({
            "userName": config.access_key.as_str(),
            "timestamp": timestamp,
            "sign": sign,
            "content": final_content,
            "templateId": template_id,
            "params": params,
            "phoneList": request.mobiles,
            "sendTime": request.send_time.as_str(),
            "extcode": request.extcode.as_str(),
            "callData": request.call_data.as_str(),
        })
        .to_string();

        let base = self.base_url();
        let (url, headers) = if self.enable_proxy && !self.proxy_url.trim().is_empty() {
            let mut headers = vec![
                ("X-Target-Url".to_string(), base.clone()),
                (
                    "Id".to_string(),
                    dhrust::times::getmilltimestamp().to_string(),
                ),
            ];
            if !self.proxy_token.trim().is_empty() {
                headers.push((
                    "X-Token-Code".to_string(),
                    self.proxy_token.trim().to_string(),
                ));
            }
            (
                format!(
                    "{}/api/sendMessageMass",
                    self.proxy_url.trim_end_matches('/')
                ),
                headers,
            )
        } else {
            (format!("{base}/api/sendMessageMass"), Vec::new())
        };

        let response = match http::post(
            &url,
            &headers,
            "application/json",
            payload.into_bytes(),
            config.timeout_ms(),
            config.attempts(),
            extend.insecure_tls(),
        ) {
            Ok(response) => response,
            Err(error) => return http_failure(Self::NAME, error),
        };

        let text = http::body_text(&response);
        parse_response(&text, response.status)
    }
}

fn parse_response(text: &str, status: u16) -> SmsResult {
    if text.trim().is_empty() {
        return SmsResult::failure(
            FengHuoSmsClient::NAME,
            "EmptyResponse",
            format!("响应数据为空（HTTP {status}）"),
            "",
        );
    }
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return SmsResult::failure(
            FengHuoSmsClient::NAME,
            "ParseError",
            format!("解析响应失败（HTTP {status}）"),
            text,
        );
    };

    let code = crate::client::json_i64(&value, "code");
    let message = crate::client::json_text(&value, "message");
    let msg_id = crate::client::json_text(&value, "msgId");
    let sms_count = crate::client::json_i64(&value, "smsCount") as i32;

    if code == 0 {
        let mut result = SmsResult::ok(
            FengHuoSmsClient::NAME,
            format!("发送成功，消息ID：{msg_id}，计费条数：{sms_count}"),
            text,
        );
        result.code = "0".to_string();
        result.biz_id = msg_id;
        result.fee = sms_count;
        result
    } else {
        SmsResult::failure(
            FengHuoSmsClient::NAME,
            code.to_string(),
            if message.is_empty() {
                format!("错误码：{code}")
            } else {
                format!("错误码：{code}，{message}")
            },
            text,
        )
    }
}
