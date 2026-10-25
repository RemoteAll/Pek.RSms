//! 统一短信请求/结果模型（对应 C# `Pek.Sms.SmsResult` 与《短信事件参数统一规范》）。

/// 短信类型（对应 C# `SmsData.SmsType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmsType {
    /// 国内通知类
    #[default]
    DomesticNotification,
    /// 国际通知类
    InternationalNotification,
    /// 国内营销类
    DomesticMarketing,
    /// 国际营销类
    InternationalMarketing,
}

impl SmsType {
    /// 全部类型（按 0~3 顺序）。
    pub const ALL: [SmsType; 4] = [
        SmsType::DomesticNotification,
        SmsType::InternationalNotification,
        SmsType::DomesticMarketing,
        SmsType::InternationalMarketing,
    ];

    /// 配置中的数值（0~3）。
    pub fn as_i32(self) -> i32 {
        match self {
            SmsType::DomesticNotification => 0,
            SmsType::InternationalNotification => 1,
            SmsType::DomesticMarketing => 2,
            SmsType::InternationalMarketing => 3,
        }
    }

    /// 由配置数值解析；未知值返回 `None`。
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(SmsType::DomesticNotification),
            1 => Some(SmsType::InternationalNotification),
            2 => Some(SmsType::DomesticMarketing),
            3 => Some(SmsType::InternationalMarketing),
            _ => None,
        }
    }

    /// 中文说明。
    pub fn label(self) -> &'static str {
        match self {
            SmsType::DomesticNotification => "国内通知",
            SmsType::InternationalNotification => "国际通知",
            SmsType::DomesticMarketing => "国内营销",
            SmsType::InternationalMarketing => "国际营销",
        }
    }
}

/// 统一短信请求（对齐 C#《短信事件参数统一规范》）。
///
/// 同一份请求会依次交给多个服务商尝试；各服务商只取自己支持的字段：
///
/// | 字段 | 阿里云 | 腾讯云 | 烽火 | 连陆 |
/// |---|---|---|---|---|
/// | `mobiles` | PhoneNumbers | PhoneNumberSet | phoneList | mobile |
/// | `content` | —（需模板） | —（需模板） | 无模板时使用 | 必填 |
/// | `template_id` | TemplateCode | TemplateId | templateId | — |
/// | `template_params` | TemplateParam(JSON) | TemplateParamSet(数组) | params/占位符替换 | — |
/// | `call_data` | — | — | callData | — |
/// | `send_time` | — | — | sendTime | — |
/// | `extcode` | — | ExtendCode | extcode | — |
/// | `session_context` | — | SessionContext | — | — |
/// | `sender_id` | — | SenderId | — | — |
#[derive(Debug, Clone, Default)]
pub struct SmsRequest {
    /// 手机号列表（构造时会拆分 `,` `;` `|` `，` `；` 并去重/去空白）
    pub mobiles: Vec<String>,
    /// 短信内容（无模板直发时使用）
    pub content: String,
    /// 模板 ID / 模板 Code
    pub template_id: String,
    /// 模板参数（保持插入顺序；腾讯云按顺序转数组）
    pub template_params: Vec<(String, String)>,
    /// 用户回传数据（烽火）
    pub call_data: String,
    /// 定时发送时间（烽火，格式 `yyyy-MM-dd HH:mm:ss`）
    pub send_time: String,
    /// 扩展码
    pub extcode: String,
    /// 会话上下文（腾讯云）
    pub session_context: String,
    /// 国际/港澳台 SenderId（腾讯云）
    pub sender_id: String,
    /// 业务自定义 ID（阿里云 OutId）
    pub out_id: String,
}

impl SmsRequest {
    /// 以手机号串构造（多个用 `,` `;` `|` `，` `；` 分隔）。
    pub fn new(mobiles: impl Into<String>) -> Self {
        Self {
            mobiles: split_phone_numbers(&mobiles.into()),
            ..Self::default()
        }
    }

    /// 以手机号列表构造（自动去空白/去重）。
    pub fn from_numbers(numbers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut mobiles = Vec::new();
        for number in numbers {
            for part in split_phone_numbers(&number.into()) {
                if !mobiles.contains(&part) {
                    mobiles.push(part);
                }
            }
        }
        Self {
            mobiles,
            ..Self::default()
        }
    }

    /// 设置短信内容。
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// 设置模板与参数。
    pub fn with_template<P, K, V>(mut self, template_id: impl Into<String>, params: P) -> Self
    where
        P: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.template_id = template_id.into();
        self.template_params = params
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// 追加一个模板参数（保持顺序）。
    pub fn param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.template_params.push((key.into(), value.into()));
        self
    }

    /// 设置用户回传数据（烽火）。
    pub fn with_call_data(mut self, value: impl Into<String>) -> Self {
        self.call_data = value.into();
        self
    }

    /// 设置定时发送时间（烽火）。
    pub fn with_send_time(mut self, value: impl Into<String>) -> Self {
        self.send_time = value.into();
        self
    }

    /// 设置扩展码。
    pub fn with_extcode(mut self, value: impl Into<String>) -> Self {
        self.extcode = value.into();
        self
    }

    /// 设置会话上下文（腾讯云）。
    pub fn with_session_context(mut self, value: impl Into<String>) -> Self {
        self.session_context = value.into();
        self
    }

    /// 设置 SenderId（腾讯云国际/港澳台）。
    pub fn with_sender_id(mut self, value: impl Into<String>) -> Self {
        self.sender_id = value.into();
        self
    }

    /// 设置业务自定义 ID（阿里云 OutId）。
    pub fn with_out_id(mut self, value: impl Into<String>) -> Self {
        self.out_id = value.into();
        self
    }

    /// 手机号逗号拼接（阿里云/连陆口径）。
    pub fn mobiles_string(&self) -> String {
        self.mobiles.join(",")
    }

    /// 模板参数 JSON 对象字符串（阿里云 `TemplateParam`）；空参数返回 `{}`。
    pub fn template_params_json(&self) -> String {
        let map: serde_json::Map<String, serde_json::Value> = self
            .template_params
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
    }

    /// 模板参数值数组（腾讯云 `TemplateParamSet`，按插入顺序）。
    pub fn template_values(&self) -> Vec<String> {
        self.template_params
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// 按 `{%key%}` 占位符渲染内容（烽火无模板直发时使用；无参数时原样返回）。
    pub fn render_content(&self) -> String {
        let mut text = self.content.clone();
        for (key, value) in &self.template_params {
            text = text.replace(&format!("{{%{key}%}}"), value);
        }
        text
    }

    /// 补齐模板 ID：请求为空时用配置默认值（对齐 C# `FixParameters`）。
    pub fn with_default_template(mut self, template_id: &str) -> Self {
        if self.template_id.trim().is_empty() {
            self.template_id = template_id.to_string();
        }
        self
    }

    /// 是否有可用手机号。
    pub fn has_mobiles(&self) -> bool {
        !self.mobiles.is_empty()
    }
}

/// 手机号拆分：`,` `;` `|` `，` `；` 分隔，去空白、去重。
pub fn split_phone_numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in text.split([',', ';', '|', '，', '；']) {
        let number = part.trim();
        if !number.is_empty() && !out.iter().any(|existing| existing == number) {
            out.push(number.to_string());
        }
    }
    out
}

/// 腾讯云单条发送状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendStatus {
    /// 发送流水号
    pub serial_no: String,
    /// 手机号（E.164）
    pub phone_number: String,
    /// 计费条数
    pub fee: i32,
    /// 用户 session 内容
    pub session_context: String,
    /// 错误码
    pub code: String,
    /// 错误码描述
    pub message: String,
    /// ISO 国家码
    pub iso_code: String,
}

/// 统一短信发送结果（对应 C# `Pek.Sms.SmsResult`，附服务商常用字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmsResult {
    /// 是否发送成功
    pub success: bool,
    /// 信息
    pub message: String,
    /// 服务商返回的原始消息
    pub raw: String,
    /// 服务商标识（aliyun/tencent/fenghuo/lianlu）
    pub provider: String,
    /// 服务商错误码/成功码
    pub code: String,
    /// 请求 ID（阿里云 RequestId / 腾讯云 RequestId）
    pub request_id: String,
    /// 业务 ID（阿里云 BizId / 烽火 msgId / 联麓流水号等）
    pub biz_id: String,
    /// 计费条数（腾讯云/烽火）
    pub fee: i32,
    /// 腾讯云发送状态明细
    pub send_status: Vec<SendStatus>,
}

impl SmsResult {
    /// 成功结果。
    pub fn ok(
        provider: impl Into<String>,
        message: impl Into<String>,
        raw: impl Into<String>,
    ) -> Self {
        Self {
            success: true,
            message: message.into(),
            raw: raw.into(),
            provider: provider.into(),
            ..Self::default()
        }
    }

    /// 失败结果。
    pub fn failure(
        provider: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        raw: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            message: message.into(),
            raw: raw.into(),
            provider: provider.into(),
            code: code.into(),
            ..Self::default()
        }
    }

    /// 是否成功。
    pub fn is_success(&self) -> bool {
        self.success
    }
}
