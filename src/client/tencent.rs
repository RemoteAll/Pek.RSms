//! 腾讯云短信客户端（对应 C# `Pek.Sms.TencentCloud.TencentSmsClient`）。
//!
//! 协议：腾讯云短信 SendSms（`TC3-HMAC-SHA256` 签名，JSON 提交，API 2021-01-11）。
//! 文档：<https://cloud.tencent.com/document/product/382/52077>

use serde_json::{Map, Value, json};

use crate::client::{SmsClient, http_failure, invalid_result, json_text};
use crate::http;
use crate::message::{SendStatus, SmsRequest, SmsResult};
use crate::settings::{SmsData, SmsSettings};

/// 默认 Host（参与签名）。
pub const TENCENT_HOST: &str = "sms.tencentcloudapi.com";
/// 服务名（参与签名）。
pub const TENCENT_SERVICE: &str = "sms";
/// API 版本。
pub const TENCENT_API_VERSION: &str = "2021-01-11";
/// 默认地域。
pub const TENCENT_REGION: &str = "ap-guangzhou";

/// 腾讯云短信客户端。
#[derive(Debug, Clone)]
pub struct TencentSmsClient {
    config: SmsData,
}

impl TencentSmsClient {
    /// 服务商标识。
    pub const NAME: &'static str = "tencent";

    /// 以配置构造。
    pub fn new(config: SmsData) -> Self {
        Self { config }
    }

    /// 以配置 + 全局设置构造（腾讯云暂不使用代理，保留统一签名）。
    pub fn with_settings(config: SmsData, _settings: SmsSettings) -> Self {
        Self::new(config)
    }
}

impl SmsClient for TencentSmsClient {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn send(&self, request: &SmsRequest) -> SmsResult {
        let config = &self.config;
        if config.access_key.trim().is_empty() {
            return invalid_result(Self::NAME, "SecretId（AccessKey）不能为空");
        }
        if config.access_secret.trim().is_empty() {
            return invalid_result(Self::NAME, "SecretKey（AccessSecret）不能为空");
        }
        if config.sign_name.trim().is_empty() {
            return invalid_result(Self::NAME, "签名（SignName）不能为空");
        }
        if !request.has_mobiles() {
            return invalid_result(Self::NAME, "收信人为空");
        }

        let sdk_app_id = config.extend_str("SdkAppId").unwrap_or_default();
        if sdk_app_id.trim().is_empty() {
            return invalid_result(Self::NAME, "ExtendData.SdkAppId 不能为空");
        }
        let template_id = if request.template_id.trim().is_empty() {
            config.extend_str("TemplateId").unwrap_or_default()
        } else {
            request.template_id.clone()
        };
        if template_id.trim().is_empty() {
            return invalid_result(Self::NAME, "模板 ID（TemplateId）不能为空");
        }

        let country_code = config
            .extend_str("CountryCode")
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "+86".to_string());
        let phones: Vec<String> = request
            .mobiles
            .iter()
            .map(|phone| {
                let phone = phone.trim();
                if phone.starts_with('+') {
                    phone.to_string()
                } else {
                    format!("{country_code}{phone}")
                }
            })
            .collect();

        let mut body = Map::new();
        body.insert("SdkAppId".into(), json!(sdk_app_id));
        body.insert("SignName".into(), json!(config.sign_name.as_str()));
        body.insert("TemplateId".into(), json!(template_id));
        body.insert("PhoneNumberSet".into(), json!(phones));
        body.insert("TemplateParamSet".into(), json!(request.template_values()));
        if !request.extcode.trim().is_empty() {
            body.insert("ExtendCode".into(), json!(request.extcode.as_str()));
        }
        if !request.session_context.trim().is_empty() {
            body.insert(
                "SessionContext".into(),
                json!(request.session_context.as_str()),
            );
        }
        if !request.sender_id.trim().is_empty() {
            body.insert("SenderId".into(), json!(request.sender_id.as_str()));
        }
        let payload = Value::Object(body).to_string();

        let now = dhrust::times::now_utc();
        let timestamp = now.and_utc().timestamp();
        let date = now.format("%Y-%m-%d").to_string();
        let url = config.base_url("https://sms.tencentcloudapi.com");
        let host = config
            .extend_str("Host")
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| url_authority(&url).unwrap_or_else(|| TENCENT_HOST.to_string()));
        let region = config
            .extend_str("Region")
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| TENCENT_REGION.to_string());
        let authorization = generate_authorization(
            config.access_key.trim(),
            config.access_secret.trim(),
            &payload,
            timestamp,
            &date,
            &host,
            TENCENT_SERVICE,
        );

        let headers = vec![
            ("X-TC-Action".to_string(), "SendSms".to_string()),
            ("X-TC-Region".to_string(), region),
            ("X-TC-Timestamp".to_string(), timestamp.to_string()),
            ("X-TC-Version".to_string(), TENCENT_API_VERSION.to_string()),
            ("Authorization".to_string(), authorization),
        ];

        let response = match http::post(
            &url,
            &headers,
            "application/json; charset=utf-8",
            payload.into_bytes(),
            config.timeout_ms(),
            config.attempts(),
            config.insecure_tls(),
        ) {
            Ok(response) => response,
            Err(error) => return http_failure(Self::NAME, error),
        };

        let text = http::body_text(&response);
        parse_response(&text, response.status)
    }
}

/// 从 URL 提取 authority（`host[:port]`）。
fn url_authority(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or_default().trim();
    if authority.is_empty() {
        None
    } else {
        Some(authority.to_string())
    }
}

/// 生成 TC3-HMAC-SHA256 `Authorization`（供测试/排查）。
pub fn generate_authorization(
    secret_id: &str,
    secret_key: &str,
    payload: &str,
    timestamp: i64,
    date: &str,
    host: &str,
    service: &str,
) -> String {
    const ALGORITHM: &str = "TC3-HMAC-SHA256";
    const SIGNED_HEADERS: &str = "content-type;host";

    let hashed_payload = dhrust::sign::sha256_hex(payload.as_bytes());
    let canonical_headers = format!("content-type:application/json; charset=utf-8\nhost:{host}\n");
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{SIGNED_HEADERS}\n{hashed_payload}");
    let credential_scope = format!("{date}/{service}/tc3_request");
    let hashed_canonical_request = dhrust::sign::sha256_hex(canonical_request.as_bytes());
    let string_to_sign =
        format!("{ALGORITHM}\n{timestamp}\n{credential_scope}\n{hashed_canonical_request}");

    let secret_date =
        dhrust::sign::hmac_sha256(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let secret_service = dhrust::sign::hmac_sha256(&secret_date, service.as_bytes());
    let secret_signing = dhrust::sign::hmac_sha256(&secret_service, b"tc3_request");
    let signature = dhrust::sign::hmac_sha256_hex(&secret_signing, string_to_sign.as_bytes());

    format!(
        "{ALGORITHM} Credential={secret_id}/{credential_scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
    )
}

fn parse_response(text: &str, status: u16) -> SmsResult {
    if text.trim().is_empty() {
        return SmsResult::failure(
            TencentSmsClient::NAME,
            "EmptyResponse",
            format!("响应数据为空（HTTP {status}）"),
            "",
        );
    }
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return SmsResult::failure(
            TencentSmsClient::NAME,
            "ParseError",
            format!("解析响应失败（HTTP {status}）"),
            text,
        );
    };

    let Some(response) = value.get("Response") else {
        return SmsResult::failure(
            TencentSmsClient::NAME,
            "ParseError",
            "响应缺少 Response 字段",
            text,
        );
    };
    let request_id = json_text(response, "RequestId");

    if let Some(error) = response.get("Error") {
        return SmsResult {
            success: false,
            message: json_text(error, "Message"),
            raw: text.to_string(),
            provider: TencentSmsClient::NAME.to_string(),
            code: json_text(error, "Code"),
            request_id,
            ..SmsResult::default()
        };
    }

    let mut statuses = Vec::new();
    if let Some(items) = response.get("SendStatusSet").and_then(Value::as_array) {
        for item in items {
            statuses.push(SendStatus {
                serial_no: json_text(item, "SerialNo"),
                phone_number: json_text(item, "PhoneNumber"),
                fee: json_text(item, "Fee").parse().unwrap_or_default(),
                session_context: json_text(item, "SessionContext"),
                code: json_text(item, "Code"),
                message: json_text(item, "Message"),
                iso_code: json_text(item, "IsoCode"),
            });
        }
    }

    let success = !statuses.is_empty() && statuses.iter().all(|item| item.code == "Ok");
    let first = statuses.first();
    let code = if success {
        "Ok".to_string()
    } else {
        first.map(|item| item.code.clone()).unwrap_or_default()
    };
    let message = if success {
        "发送成功".to_string()
    } else {
        first
            .map(|item| {
                if item.message.is_empty() {
                    "发送失败".to_string()
                } else {
                    item.message.clone()
                }
            })
            .unwrap_or_else(|| format!("响应数据为空（HTTP {status}）"))
    };

    SmsResult {
        success,
        message,
        raw: text.to_string(),
        provider: TencentSmsClient::NAME.to_string(),
        code,
        request_id,
        fee: statuses.iter().map(|item| item.fee).sum(),
        send_status: statuses,
        ..SmsResult::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_matches_dotnet_golden_sample() {
        // 黄金样本由 .NET HMACSHA256/SHA256 独立算出（2026-10-25）
        let payload = r#"{"SdkAppId":"1400000000","SignName":"TestSign","TemplateId":"123456","PhoneNumberSet":["+8613800138000"],"TemplateParamSet":["123456"]}"#;
        let authorization = generate_authorization(
            "test-secret-id",
            "test-secret-key",
            payload,
            1_551_113_065,
            "2019-02-25",
            TENCENT_HOST,
            TENCENT_SERVICE,
        );
        assert_eq!(
            authorization,
            "TC3-HMAC-SHA256 Credential=test-secret-id/2019-02-25/sms/tc3_request, SignedHeaders=content-type;host, Signature=9b67bb1c1bef7d2e9a88f7c09929bb210de7efe6afee46017897330808418b60"
        );
    }
}
