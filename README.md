# Pek.RSms

C# [`Pek.Sms`](https://github.com/PeiKeSmart/Pek.AspNetCore) 的 Rust 实现：**短信发送库**（非服务、无面板），
供生态内其他 Rust 项目以 path 依赖复用。

- crate：`pek-rsms`
- 参照源码：`G:\Code\Pek.FrameWork\Pek.Sms`
- 支持服务商：**阿里云、腾讯云、烽火、连陆**
- 统一参数与多账号 `Order` 兜底（对齐 C#《短信事件参数统一规范》）

---

## 1. 能力对照（C# → Rust）

| C# | Rust |
|---|---|
| `SmsSettings` / `SmsData` | `SmsSettings` / `SmsData` |
| `AliyunDysmsClient` | `AliyunSmsClient`（feature `aliyun`） |
| `TencentSmsClient` | `TencentSmsClient`（feature `tencent`） |
| `FengHuoSmsClient` | `FengHuoSmsClient`（feature `fenghuo`） |
| `LianLuSmsClient` | `LianLuSmsClient`（feature `lianlu`） |
| `SmsResult` | `SmsResult` |
| 各 `SmsEventConsumer` 的 `Order` 兜底链 | `Smser::send`（同一 `SmsType` 按 `Order` 升序自动兜底） |
| `EnableProxy` / `ProxyUrl` / `ProxyToken`（仅烽火） | 同名配置，`FengHuoSmsClient` 支持 |
| `SmsEvent` 事件总线 | 不提供事件总线：`Smser` 直接按配置兜底（消费方已有事件框架时也可自行串联各客户端） |

## 2. 配置（`Config/Sms.toml`）

首次运行自动按内嵌模板生成（带中文注释）；字段名与 C# `Sms` 配置保持一致：

```toml
EnableProxy = false
ProxyUrl = ""
ProxyToken = ""

[[Data]]
Code = "fenghuo_notify"
Name = "fenghuo"                 # aliyun / tencent / fenghuo / lianlu
DisplayName = "烽火通知短信"
SmsType = 0                      # 0 国内通知，1 国际通知，2 国内营销，3 国际营销
IsDefault = true
IsEnabled = false
AccessKey = ""
AccessSecret = ""
SignName = ""
Order = 10                       # 越小越先；失败自动切换下一个
Timeout = 60000                  # 毫秒
Security = true                  # 是否 HTTPS
RetryTimes = 3                   # 含首次尝试
ExtendFields = ""
ExtendData = '{"BaseUrl": "https://aisms.aipaas.com:8443"}'
```

配置读写复用 `dhrust::config::TomlConfig`：缺失生成、缺键带注释补齐、类型容错、损坏备份重建、外部修改热加载；
`[[Data]]` 账号数组写回复用 `TomlConfig::save_with_arrays`（保留注释与排版）。
配置中的密钥为明文（与 C# 一致）；需要加密保存时由消费方用 `dhrust::secret` 处理。

### 各服务商 `ExtendData` 键

| 服务商 | 键 | 说明 |
|---|---|---|
| 阿里云 | `TemplateCode` | 默认模板 Code（请求未指定时使用） |
| 阿里云 | `OutId` / `SmsUpExtendCode` | 可选扩展 |
| 腾讯云 | `SdkAppId` | 短信应用 ID（**必填**） |
| 腾讯云 | `TemplateId` | 默认模板 ID（请求未指定时使用） |
| 腾讯云 | `CountryCode` | 国家码，默认 `+86` |
| 腾讯云 | `Region` | 地域，默认 `ap-guangzhou` |
| 烽火 | `BaseUrl` | API 地址，默认 `https://aisms.aipaas.com:8443` |
| 连陆 | `BaseUrl` | API 地址，默认 `http://47.110.199.86:8081` |
| 通用 | `BaseUrl` / `Host` | 自定义网关/本地测试；腾讯云签名 `Host` 缺省时从 `BaseUrl` 的 authority 解析 |
| 通用 | `InsecureTls` | `true` 时跳过证书校验（默认校验；C# 统一 `IgnoreSsl`） |

## 3. 快速开始

```rust
use pek_rsms::{SmsRequest, SmsType, Smser};

let smser = Smser::open_in("."); // 读取/生成 Config/Sms.toml
for note in smser.take_notes() {
    dhrust::info!("{note}");
}

// 模板短信（同一 SmsType 的已启用账号按 Order 升序兜底）
let result = smser.send(
    SmsType::DomesticNotification,
    &SmsRequest::new("13800138000").with_template("SMS_123", [("code", "123456")]),
)?;
println!("{}", result.message);
# Ok::<(), pek_rsms::SmsError>(())
```

### 内容短信 / 占位符

```rust
use pek_rsms::{SmsRequest, SmsType, Smser};

let smser = Smser::open_in(".");

// 无模板直发；{%key%} 占位符会被本地替换（烽火/连陆）
let request = SmsRequest::new("13800138000,13900139000")
    .with_content("您的验证码是 {%code%}，5 分钟内有效。")
    .param("code", "123456");

let result = smser.send(SmsType::DomesticNotification, &request)?;
# Ok::<(), pek_rsms::SmsError>(())
```

### 指定账号 / 指定服务商

```rust
use pek_rsms::{SmsRequest, SmsType, Smser};

let smser = Smser::open_in(".");
let request = SmsRequest::new("13800138000").with_template("SMS_123", [("code", "123456")]);

// 按 Code 指定账号（不兜底）
let _ = smser.send_with_code("aliyun_notify", &request);

// 只在某服务商的账号之间按 Order 兜底
let _ = smser.send_with_name("fenghuo", SmsType::DomesticNotification, &request);
# Ok::<(), pek_rsms::SmsError>(())
```

### 直接使用客户端

```rust
use pek_rsms::{AliyunSmsClient, SmsClient, SmsRequest, SmsSettings};

let config = SmsSettings::load("Config/Sms.toml").value().data[0].clone();
let client = AliyunSmsClient::new(config);
let result = client.send(
    &SmsRequest::new("13800138000").with_template("SMS_123", [("code", "123456")]),
);
println!("success={} message={}", result.success, result.message);
```

## 4. 统一参数

`SmsRequest` 对齐 C#《参数规范》，同一份请求可交给所有服务商：

| 字段 | 阿里云 | 腾讯云 | 烽火 | 连陆 |
|---|---|---|---|---|
| `mobiles` | `PhoneNumbers` | `PhoneNumberSet` | `phoneList` | `mobile` |
| `content` | — | — | 无模板时使用 | 必填 |
| `template_id` | `TemplateCode` | `TemplateId` | `templateId` | — |
| `template_params` | `TemplateParam`（JSON） | `TemplateParamSet`（数组） | `params` / 占位符替换 | — |
| `call_data` | — | — | `callData` | — |
| `send_time` | — | — | `sendTime` | — |
| `extcode` | — | `ExtendCode` | `extcode` | — |
| `session_context` | — | `SessionContext` | — | — |
| `sender_id` | — | `SenderId` | — | — |
| `out_id` | `OutId` | — | — | — |

- `mobiles` 支持 `,` `;` `|` `，` `；` 分隔，自动去空白/去重；
- 腾讯云自动补国家码（`CountryCode`，默认 `+86`；已带 `+` 的号码原样使用）。

## 5. 多账号兜底

`Smser::send` 取同一 `SmsType` 的所有 `IsEnabled=true` 账号，按 `Order` 升序依次尝试：

1. 烽火（建议 `Order=10`）
2. 阿里云（`Order=20`）
3. 腾讯云（`Order=30`）
4. 连陆（`Order=40`）

某账号失败时记录失败原因并自动切换下一个；全部失败返回 `SmsError::AllFailed`（含每个账号的错误码/原始响应）。
日志只记录发送成功、账号失败与切换，不逐轮询打印。

## 6. 代理（烽火）

配置：

```toml
EnableProxy = true
ProxyUrl = "https://proxy.example.com"
ProxyToken = "your_proxy_token"
```

请求流程（与 C# 一致）：

1. POST `{ProxyUrl}/api/sendMessageMass`
2. `X-Target-Url` = 烽火 API 基础地址（含 `/sms`）
3. `X-Token-Code` = `ProxyToken`
4. 代理转发到真实平台

> 阿里云/腾讯云/连陆暂不支持代理（与 C# 当前状态一致）。

## 7. 与 C# 的差异（有意）

| 项目 | C# | Rust |
|---|---|---|
| 异步 API | `async/await` | 同步 API；`Smser::send_async` 用后台线程（库不引入 tokio 运行时） |
| 证书校验 | 统一 `.IgnoreSsl()` | 默认校验证书；自签环境配置 `InsecureTls=true`（更安全） |
| 事件总线 | `SmsEvent` + `IConsumer` 多消费者串联 | `Smser` 直接按 `SmsType + Order` 兜底（等价效果，无事件框架依赖） |
| 服务商分包 | 多个 NuGet 包 | 单 crate + Cargo features（`aliyun`/`tencent`/`fenghuo`/`lianlu`，默认全开） |
| 腾讯云签名 | 固定 Host/Region | 默认相同，可通过 `ExtendData` 覆盖（便于网关/测试） |

## 8. 开发与回归

```powershell
cargo check
cargo test
cargo clippy --all-targets
cargo zigbuild --target x86_64-unknown-linux-musl --lib

# 特性裁剪
cargo check --no-default-features --features aliyun
```

测试不依赖外网：

- 签名黄金样本（阿里云 HMAC-SHA1 / 腾讯云 TC3-HMAC-SHA256）由 .NET 独立计算后固化；
- `tests/sms_e2e.rs` 内置假 HTTP 服务器，覆盖四家请求格式/响应解析/代理/多账号兜底/配置回写。
