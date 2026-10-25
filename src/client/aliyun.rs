//! 阿里云短信客户端（对应 C# `Pek.Sms.Aliyun.AliyunDysmsClient`）。
//!
//! 协议：阿里云短信 SendSms（RPC 风格，`HMAC-SHA1` 签名，表单提交）。
//! 文档：<https://help.aliyun.com/document_detail/101414.html>

use std::collections::BTreeMap;

use crate::client::{SmsClient, http_failure, invalid_result, json_text};
use crate::http;
use crate::message::{SmsRequest, SmsResult};
use crate::settings::{SmsData, SmsSettings};

/// 阿里云短信客户端。
#[derive(Debug, Clone)]
pub struct AliyunSmsClient {
    config: SmsData,
}

impl AliyunSmsClient {
    /// 服务商标识。
    pub const NAME: &'static str = "aliyun";
    /// 单次最多收信人。
    pub const MAX_RECEIVERS: usize = 1000;

    /// 以配置构造。
    pub fn new(config: SmsData) -> Self {
        Self { config }
    }

    /// 以配置 + 全局设置构造（阿里云暂不使用代理，保留统一签名）。
    pub fn with_settings(config: SmsData, _settings: SmsSettings) -> Self {
        Self::new(config)
    }

    /// 发送验证码（模板参数 `code`）。
    pub fn send_code(&self, phone: &str, code: &str, template_id: &str) -> SmsResult {
        let request = SmsRequest::new(phone).with_template(template_id, [("code", code)]);
        self.send(&request)
    }
}

impl SmsClient for AliyunSmsClient {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn send(&self, request: &SmsRequest) -> SmsResult {
        let config = &self.config;
        if config.access_key.trim().is_empty() {
            return invalid_result(Self::NAME, "AccessKey 不能为空");
        }
        if config.access_secret.trim().is_empty() {
            return invalid_result(Self::NAME, "AccessSecret 不能为空");
        }
        if config.sign_name.trim().is_empty() {
            return invalid_result(Self::NAME, "签名（SignName）不能为空");
        }
        if !request.has_mobiles() {
            return invalid_result(Self::NAME, "收信人为空");
        }
        if request.mobiles.len() > Self::MAX_RECEIVERS {
            return invalid_result(Self::NAME, "收信人超过限制（最多 1000 个）");
        }

        let extend = config.extend_view();
        let template_id = if request.template_id.trim().is_empty() {
            extend.str("TemplateCode").unwrap_or_default()
        } else {
            request.template_id.clone()
        };
        if template_id.trim().is_empty() {
            return invalid_result(Self::NAME, "短信模板 Code 不能为空");
        }

        let mut params: BTreeMap<String, String> = BTreeMap::new();
        params.insert("RegionId".into(), "cn-hangzhou".into());
        params.insert("Action".into(), "SendSms".into());
        params.insert("Version".into(), "2017-05-25".into());
        params.insert("AccessKeyId".into(), config.access_key.clone());
        params.insert("PhoneNumbers".into(), request.mobiles_string());
        params.insert("SignName".into(), config.sign_name.clone());
        params.insert("TemplateCode".into(), template_id);
        params.insert("SignatureMethod".into(), "HMAC-SHA1".into());
        params.insert("SignatureNonce".into(), dhrust::random::token());
        params.insert("SignatureVersion".into(), "1.0".into());
        params.insert("Timestamp".into(), iso8601_utc());
        params.insert("Format".into(), "JSON".into());

        if !request.template_params.is_empty() {
            params.insert("TemplateParam".into(), request.template_params_json());
        }

        let out_id = if !request.out_id.trim().is_empty() {
            request.out_id.clone()
        } else {
            extend.str("OutId").unwrap_or_default()
        };
        if !out_id.trim().is_empty() {
            params.insert("OutId".into(), out_id);
        }
        if let Some(extend_code) = extend.str("SmsUpExtendCode")
            && !extend_code.trim().is_empty()
        {
            params.insert("SmsUpExtendCode".into(), extend_code);
        }

        let signature = api_signature(&params, config.access_secret.trim());
        params.insert("Signature".into(), signature);

        let body = dhrust::web::form_encode(
            params
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
        let url = extend.base_url(if config.security {
            "https://dysmsapi.aliyuncs.com/"
        } else {
            "http://dysmsapi.aliyuncs.com/"
        });

        let response = match http::post(
            &url,
            &[],
            "application/x-www-form-urlencoded",
            body.into_bytes(),
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

/// 阿里云签名（`POST&%2F&...` + HMAC-SHA1 + Base64）。
pub fn api_signature(params: &BTreeMap<String, String>, access_secret: &str) -> String {
    let string_to_sign = string_to_sign(params);
    let key = format!("{access_secret}&");
    dhrust::sign::hmac_sha1_base64(key.as_bytes(), string_to_sign.as_bytes())
}

/// 待签名字符串（供测试/排查）。
pub fn string_to_sign(params: &BTreeMap<String, String>) -> String {
    let canonical = dhrust::web::form_encode(
        params
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    format!("POST&%2F&{}", dhrust::web::url_encode(&canonical))
}

fn iso8601_utc() -> String {
    dhrust::times::now_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn parse_response(text: &str, status: u16) -> SmsResult {
    if text.trim().is_empty() {
        return SmsResult::failure(
            AliyunSmsClient::NAME,
            "EmptyResponse",
            format!("响应数据为空（HTTP {status}）"),
            "",
        );
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return SmsResult::failure(
            AliyunSmsClient::NAME,
            "ParseError",
            format!("解析响应失败（HTTP {status}）"),
            text,
        );
    };

    let code = json_text(&value, "Code");
    let message = json_text(&value, "Message");
    let request_id = json_text(&value, "RequestId");
    let biz_id = json_text(&value, "BizId");
    let success = code.eq_ignore_ascii_case("OK");

    let mut result = SmsResult {
        success,
        message: if message.is_empty() {
            if success {
                "发送成功".to_string()
            } else {
                format!("发送失败（HTTP {status}）")
            }
        } else {
            message
        },
        raw: text.to_string(),
        provider: AliyunSmsClient::NAME.to_string(),
        code,
        request_id,
        biz_id,
        ..SmsResult::default()
    };
    if !success && result.code.is_empty() && !http::is_success_status(status) {
        result.message = format!("HTTP {status}：{}", result.message);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_dotnet_golden_sample() {
        // 黄金样本由 .NET HMACSHA1 + RFC3986 编码独立算出（2026-10-25）
        let params = BTreeMap::from([
            ("AccessKeyId".to_string(), "testid".to_string()),
            ("Action".to_string(), "SendSms".to_string()),
            ("Format".to_string(), "JSON".to_string()),
            ("PhoneNumbers".to_string(), "13800138000".to_string()),
            ("RegionId".to_string(), "cn-hangzhou".to_string()),
            ("SignName".to_string(), "TestSign".to_string()),
            ("SignatureMethod".to_string(), "HMAC-SHA1".to_string()),
            ("SignatureNonce".to_string(), "fixed-nonce-123".to_string()),
            ("SignatureVersion".to_string(), "1.0".to_string()),
            ("TemplateCode".to_string(), "SMS_123".to_string()),
            (
                "TemplateParam".to_string(),
                r#"{"code":"123456"}"#.to_string(),
            ),
            ("Timestamp".to_string(), "2026-10-25T00:00:00Z".to_string()),
            ("Version".to_string(), "2017-05-25".to_string()),
        ]);

        assert_eq!(
            string_to_sign(&params),
            "POST&%2F&AccessKeyId%3Dtestid%26Action%3DSendSms%26Format%3DJSON%26PhoneNumbers%3D13800138000%26RegionId%3Dcn-hangzhou%26SignName%3DTestSign%26SignatureMethod%3DHMAC-SHA1%26SignatureNonce%3Dfixed-nonce-123%26SignatureVersion%3D1.0%26TemplateCode%3DSMS_123%26TemplateParam%3D%257B%2522code%2522%253A%2522123456%2522%257D%26Timestamp%3D2026-10-25T00%253A00%253A00Z%26Version%3D2017-05-25"
        );
        assert_eq!(
            api_signature(&params, "testsecret"),
            "o0MM15vdJ2+2DvfbuohX2XkKM00="
        );
    }
}
