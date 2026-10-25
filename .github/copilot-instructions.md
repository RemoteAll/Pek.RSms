# Pek.RSms · 项目指令

> 全项目通用规则见用户级《跨项目接续协议》（本机 `%APPDATA%\Code\User\prompts\跨项目接续协议.instructions.md`；模板在 `tools/continuity/templates/`）。
> **开工先读（存在则读，按序）**：`.github/copilot-instructions.md` → `docs/项目接续指南.md` → `docs/用户档案.md` → `docs/项目记忆.md` → `docs/决策记录.md` 最新节。
> **机器自愈（有则快跑）**：`tools/continuity/install-global-instruction.ps1`（幂等；打开项目时 `.vscode/tasks.json` 也会自动跑）。

## 项目定位

`Pek.RSms`（crate `pek-rsms`）是 C# `Pek.Sms` 的 Rust 对应实现：**短信发送库**（非服务、无面板），
供生态内其他 Rust 项目以 path 依赖复用。支持阿里云、腾讯云、烽火、连陆四家服务商，统一参数与多账号兜底。

## 依赖约定（不要重复造轮子）

| 能力 | 复用来源 | 说明 |
|---|---|---|
| 配置（TOML 模板/补齐/热加载/备份） | `dhrust::config::TomlConfig<T>`（feature `toml`） | 不自己写读写与迁移 |
| TOML 数组表写回（`[[Data]]` 账号） | `TomlConfig::save_with_arrays` | 保留注释与排版 |
| HTTP 客户端（含 HTTPS/超时/自签） | `dhrust::net::http_client`（features `http-client` + `http-tls`） | 不引入新 HTTP 栈 |
| 日志 | `dhrust::logs` | 只记发送成功/失败/账号切换 |
| MD5 / HMAC-SHA1 / HMAC-SHA256 | `dhrust::sign` | 阿里云/腾讯云/烽火/连陆签名 |
| 随机 ID / 时间戳 | `dhrust::random` / `dhrust::times` | 请求 Nonce/时间戳 |
| URL 编码 | `dhrust::web::url_encode` | 阿里云签名规范 |

## 硬约束

- **配置一律 TOML**（`Config/Sms.toml`），PascalCase 键名与 C# `Sms` 配置对齐；读写只走 `TomlConfig`。
- **不新增异步运行时**：库提供同步 API；需要异步的消费方自行 `spawn_blocking`。
- **统一参数**：`SmsRequest` 对齐 C#《参数规范》：`mobiles/content/template_id/template_params/call_data/send_time/extcode/session_context/sender_id`。
- **多账号兜底**：同一 `SmsType` 按 `Order` 升序依次尝试，失败自动切换下一家/下一账号。
- **日志/审计只记有意义事件**：发送成功/失败、服务商与账号切换；不逐轮询打日志。

## 每轮收尾（务必）

① `docs/决策记录.md` 新增一节（**含用户原话**）；② 运行 `tools/continuity/sync-memory.ps1` 同步记忆；③ 跑回归（`cargo test` + `cargo clippy`）并如实汇报；④ 提交并推送。
