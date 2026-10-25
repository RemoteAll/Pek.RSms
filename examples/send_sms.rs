//! 示例：从 `Config/Sms.toml` 读取配置并发送一条短信。
//!
//! 用法（PowerShell）：
//! ```powershell
//! cargo run --example send_sms -- 0 13800138000 "SMS_123" code=123456
//! ```
//! 参数：`<SmsType 0~3> <手机号> <模板ID> [key=value ...]`
//! 若模板 ID 传 `-`，则用后续参数作为内容直发（`content=...`）。

use std::env;

use pek_rsms::{SmsRequest, SmsType, Smser};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let sms_type = args
        .first()
        .and_then(|value| value.parse::<i32>().ok())
        .and_then(SmsType::from_i32)
        .unwrap_or(SmsType::DomesticNotification);
    let mobiles = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "13800138000".to_string());
    let template = args.get(2).cloned().unwrap_or_else(|| "-".to_string());

    let mut request = SmsRequest::new(mobiles);
    if template == "-" {
        let content = args
            .iter()
            .skip(3)
            .find_map(|item| item.strip_prefix("content="))
            .unwrap_or("Pek.RSms 测试短信 {%code%}");
        request = request.with_content(content);
    } else {
        request = request.with_template(template, Vec::<(String, String)>::new());
    }
    for item in args.iter().skip(3) {
        if let Some((key, value)) = item.split_once('=')
            && key != "content"
        {
            request = request.param(key, value);
        }
    }

    let smser = Smser::open_in(".");
    for note in smser.take_notes() {
        println!("配置提示：{note}");
    }
    println!("配置文件：{:?}", smser.config_path());
    println!(
        "可用账号：{}",
        smser
            .settings()
            .enabled_for_type(sms_type)
            .iter()
            .map(|item| format!("{}({})", item.label(), item.name))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let result = smser.send(sms_type, &request)?;
    println!("发送成功：{}", result.message);
    Ok(())
}
