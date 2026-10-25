//! 端到端测试：本地假 HTTP 服务器 + 签名/兜底/配置。
//!
//! 不依赖外部短信平台与网络，CI 可直接运行。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use pek_rsms::{
    AliyunSmsClient, FengHuoSmsClient, LianLuSmsClient, SmsClient, SmsData, SmsError, SmsRequest,
    SmsSettings, SmsSettingsFile, SmsType, Smser, TencentSmsClient,
};

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl CapturedRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

struct FakeHttpServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    handle: thread::JoinHandle<()>,
}

impl FakeHttpServer {
    fn start(status: u16, body: &str, expected_requests: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定假 HTTP 服务");
        let addr = listener.local_addr().expect("地址");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&requests);
        let response_body = body.as_bytes().to_vec();

        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let Ok(clone) = stream.try_clone() else {
                    break;
                };
                let mut reader = BufReader::new(clone);

                let mut request_line = String::new();
                if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();

                let mut headers = Vec::new();
                let mut content_length = 0usize;
                let mut chunked = false;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end_matches(['\r', '\n']).to_string();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        let key = key.trim().to_string();
                        let value = value.trim().to_string();
                        if key.eq_ignore_ascii_case("content-length") {
                            content_length = value.parse().unwrap_or(0);
                        }
                        if key.eq_ignore_ascii_case("transfer-encoding")
                            && value.to_ascii_lowercase().contains("chunked")
                        {
                            chunked = true;
                        }
                        headers.push((key, value));
                    }
                }

                let mut body_bytes = Vec::new();
                if chunked {
                    loop {
                        let mut size_line = String::new();
                        if reader.read_line(&mut size_line).unwrap_or(0) == 0 {
                            break;
                        }
                        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
                        if size == 0 {
                            let mut trailer = String::new();
                            let _ = reader.read_line(&mut trailer);
                            break;
                        }
                        let mut chunk = vec![0u8; size];
                        if reader.read_exact(&mut chunk).is_err() {
                            break;
                        }
                        body_bytes.extend_from_slice(&chunk);
                        let mut crlf = [0u8; 2];
                        let _ = reader.read_exact(&mut crlf);
                    }
                } else if content_length > 0 {
                    body_bytes.resize(content_length, 0);
                    let _ = reader.read_exact(&mut body_bytes);
                }

                sink.lock().unwrap().push(CapturedRequest {
                    method,
                    path,
                    headers,
                    body: String::from_utf8_lossy(&body_bytes).to_string(),
                });

                let reason = if status < 400 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&response_body);
                let _ = stream.flush();
            }
        });

        Self {
            addr,
            requests,
            handle,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn join(self) -> Vec<CapturedRequest> {
        let FakeHttpServer {
            handle, requests, ..
        } = self;
        let _ = handle.join();
        let mut guard = requests.lock().unwrap();
        std::mem::take(&mut *guard)
    }
}

fn account(name: &str, port: u16, order: i32, extend: &str) -> SmsData {
    SmsData {
        code: format!("{name}-{order}"),
        name: name.to_string(),
        display_name: name.to_string(),
        sms_type: 0,
        is_default: order == 10,
        is_enabled: true,
        access_key: "test-key".to_string(),
        access_secret: "test-secret".to_string(),
        sign_name: "TestSign".to_string(),
        order,
        timeout: 3000,
        security: false,
        retry_times: 1,
        extend_fields: String::new(),
        extend_data: extend.replace("{PORT}", &port.to_string()),
    }
}

#[test]
fn request_helpers_split_params_and_render_content() {
    let request = SmsRequest::new("13800138000, 13900139000;13800138000")
        .with_content("code {%code%}")
        .param("code", "123456")
        .param("product", "test");
    assert_eq!(request.mobiles, vec!["13800138000", "13900139000"]);
    assert_eq!(request.mobiles_string(), "13800138000,13900139000");
    assert_eq!(request.render_content(), "code 123456");
    assert_eq!(
        request.template_params_json(),
        r#"{"code":"123456","product":"test"}"#
    );
    assert_eq!(request.template_values(), vec!["123456", "test"]);
}

#[test]
fn settings_selection_and_order_match_csharp_rules() {
    let mut settings = SmsSettings::default();
    let mut a = account("aliyun", 1, 20, "{}");
    a.sms_type = 0;
    let mut b = account("fenghuo", 2, 10, "{}");
    b.sms_type = 0;
    b.is_default = false;
    let mut c = account("tencent", 3, 30, "{}");
    c.sms_type = 1;
    settings.data = vec![a, b, c];

    let notification = settings.enabled_for_type(SmsType::DomesticNotification);
    assert_eq!(notification.len(), 2);
    assert_eq!(notification[0].name, "fenghuo", "Order 小的优先");
    assert_eq!(notification[1].name, "aliyun");
    assert_eq!(
        settings
            .find_by_code("FENGHUO-10")
            .map(|item| item.name.as_str()),
        Some("fenghuo"),
        "Code 大小写不敏感"
    );
    assert_eq!(
        settings
            .find_by_name_and_type("TENCENT", SmsType::InternationalNotification)
            .map(|item| item.name.as_str()),
        Some("tencent")
    );
    assert!(settings.find_default(SmsType::DomesticMarketing).is_none());
}

#[test]
fn config_file_round_trip_keeps_comments_and_syncs_accounts() {
    let dir = std::env::temp_dir().join(format!("pek-rsms-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Sms.toml");

    let mut file = SmsSettingsFile::load(&path);
    assert!(file.is_new());
    assert_eq!(file.settings().data.len(), 1, "模板自带示例账号");

    let mut settings = file.settings().clone();
    settings.data = vec![
        account("fenghuo", 100, 10, r#"{"BaseUrl":"https://example.com"}"#),
        account("lianlu", 200, 40, "{}"),
    ];
    *file.settings_mut() = settings;
    file.save().expect("保存");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# Pek.RSms 短信配置"), "模板注释应保留");
    assert_eq!(text.matches("[[Data]]").count(), 2, "{text}");
    assert!(
        text.contains("fenghuo") && text.contains("lianlu"),
        "{text}"
    );

    let mut reloaded = SmsSettings::load(&path);
    assert_eq!(reloaded.value().data.len(), 2);
    assert_eq!(
        reloaded
            .value()
            .find_default(SmsType::DomesticNotification)
            .unwrap()
            .name,
        "fenghuo"
    );
    assert!(
        reloaded
            .take_notes()
            .iter()
            .all(|note| !note.contains("解析失败"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn aliyun_sends_form_and_parses_ok() {
    let server = FakeHttpServer::start(
        200,
        r#"{"RequestId":"req-aliyun","Code":"OK","Message":"OK","BizId":"biz-1"}"#,
        1,
    );
    let config = account(
        "aliyun",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}/"}"#,
    );
    let client = AliyunSmsClient::new(config);
    let request =
        SmsRequest::new("13800138000,13900139000").with_template("SMS_123", [("code", "123456")]);

    let result = client.send(&request);
    assert!(result.success, "{result:?}");
    assert_eq!(result.request_id, "req-aliyun");
    assert_eq!(result.biz_id, "biz-1");

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    let captured = &requests[0];
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/");
    assert_eq!(
        captured.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert!(
        captured.body.contains("Action=SendSms"),
        "{}",
        captured.body
    );
    assert!(
        captured
            .body
            .contains("PhoneNumbers=13800138000%2C13900139000"),
        "{}",
        captured.body
    );
    assert!(captured.body.contains("Signature="), "{}", captured.body);
    assert!(
        captured
            .body
            .contains("TemplateParam=%7B%22code%22%3A%22123456%22%7D"),
        "{}",
        captured.body
    );
}

#[test]
fn aliyun_parse_failure_returns_failure_result() {
    let server = FakeHttpServer::start(
        200,
        r#"{"RequestId":"req-aliyun-2","Code":"isv.MOBILE_NUMBER_ILLEGAL","Message":"非法手机号"}"#,
        1,
    );
    let config = account(
        "aliyun",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}/"}"#,
    );
    let client = AliyunSmsClient::new(config);
    let result = client
        .send(&SmsRequest::new("13800138000").with_template("SMS_123", Vec::<(&str, &str)>::new()));
    assert!(!result.success);
    assert_eq!(result.code, "isv.MOBILE_NUMBER_ILLEGAL");
    assert_eq!(result.message, "非法手机号");
    let _ = server.join();
}

#[test]
fn tencent_sends_json_and_parses_ok() {
    let server = FakeHttpServer::start(
        200,
        r#"{"Response":{"SendStatusSet":[{"SerialNo":"sn-1","PhoneNumber":"+8613800138000","Fee":1,"SessionContext":"sess","Code":"Ok","Message":"send success","IsoCode":"CN"}],"RequestId":"req-tencent"}}"#,
        1,
    );
    let config = account(
        "tencent",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}","Host":"127.0.0.1:{PORT}","SdkAppId":"1400000000","TemplateId":"123456"}"#,
    );
    let client = TencentSmsClient::new(config);
    let request = SmsRequest::new("13800138000")
        .with_template("123456", [("code", "654321")])
        .with_session_context("sess");

    let result = client.send(&request);
    assert!(result.success, "{result:?}");
    assert_eq!(result.request_id, "req-tencent");
    assert_eq!(result.fee, 1);
    assert_eq!(result.send_status.len(), 1);
    assert_eq!(result.send_status[0].phone_number, "+8613800138000");

    let requests = server.join();
    let captured = &requests[0];
    assert_eq!(captured.path, "/");
    assert_eq!(captured.header("x-tc-action"), Some("SendSms"));
    assert!(
        captured
            .header("authorization")
            .unwrap_or_default()
            .starts_with("TC3-HMAC-SHA256 "),
        "{:?}",
        captured.header("authorization")
    );
    assert!(
        captured.body.contains(r#""SdkAppId":"1400000000""#),
        "{}",
        captured.body
    );
    assert!(
        captured
            .body
            .contains(r#""PhoneNumberSet":["+8613800138000"]"#),
        "{}",
        captured.body
    );
    assert!(
        captured.body.contains(r#""TemplateParamSet":["654321"]"#),
        "{}",
        captured.body
    );
}

#[test]
fn fenghuo_replaces_placeholders_and_parses_ok() {
    let server = FakeHttpServer::start(
        200,
        r#"{"code":0,"message":"ok","msgId":123,"smsCount":2}"#,
        1,
    );
    let config = account(
        "fenghuo",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
    );
    let client = FengHuoSmsClient::new(config);
    let request = SmsRequest::new("13800138000,13900139000")
        .with_content("code {%code%}")
        .param("code", "123456");

    let result = client.send(&request);
    assert!(result.success, "{result:?}");
    assert_eq!(result.biz_id, "123");
    assert_eq!(result.fee, 2);

    let requests = server.join();
    let captured = &requests[0];
    assert_eq!(captured.path, "/sms/api/sendMessageMass");
    assert!(
        captured.body.contains(r#""content":"code 123456""#),
        "{}",
        captured.body
    );
    assert!(
        captured
            .body
            .contains(r#""phoneList":["13800138000","13900139000"]"#),
        "{}",
        captured.body
    );
    assert!(captured.body.contains(r#""sign":""#), "{}", captured.body);
}

#[test]
fn fenghuo_proxy_uses_target_url_and_token() {
    let server = FakeHttpServer::start(
        200,
        r#"{"code":0,"message":"ok","msgId":1,"smsCount":1}"#,
        1,
    );
    let mut config = account(
        "fenghuo",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"https://example.invalid"}"#,
    );
    config.code = "fenghuo-proxy".to_string();
    let settings = SmsSettings {
        enable_proxy: true,
        proxy_url: server.base_url(),
        proxy_token: "proxy-token".to_string(),
        ..Default::default()
    };

    let client = FengHuoSmsClient::with_settings(config, settings);
    let result = client.send(&SmsRequest::new("13800138000").with_content("hello"));
    assert!(result.success, "{result:?}");

    let requests = server.join();
    let captured = &requests[0];
    assert_eq!(captured.path, "/api/sendMessageMass");
    assert_eq!(
        captured.header("x-target-url"),
        Some("https://example.invalid/sms")
    );
    assert_eq!(captured.header("x-token-code"), Some("proxy-token"));
    assert!(captured.header("id").is_some());
}

#[test]
fn lianlu_sends_form_and_parses_text_success() {
    let server = FakeHttpServer::start(200, "提交成功", 1);
    let config = account(
        "lianlu",
        server.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
    );
    let client = LianLuSmsClient::new(config);
    let result = client.send(&SmsRequest::new("13800138000").with_content("hello world"));
    assert!(result.success, "{result:?}");
    assert!(result.message.contains("提交成功"));

    let requests = server.join();
    let captured = &requests[0];
    assert_eq!(captured.path, "/api/sms/send");
    assert!(
        captured
            .header("content-type")
            .unwrap_or_default()
            .contains("x-www-form-urlencoded"),
        "{:?}",
        captured.header("content-type")
    );
    assert!(
        captured.body.contains("userid=test-key"),
        "{}",
        captured.body
    );
    assert!(
        captured.body.contains("mobile=13800138000"),
        "{}",
        captured.body
    );
    assert!(
        captured.body.contains("msgcontent=hello%20world"),
        "{}",
        captured.body
    );
    assert!(captured.body.contains("sign="), "{}", captured.body);
}

#[test]
fn smser_falls_back_to_next_account_by_order() {
    let failing = FakeHttpServer::start(200, r#"{"code":500,"message":"fail"}"#, 1);
    let succeeding = FakeHttpServer::start(
        200,
        r#"{"code":0,"message":"ok","msgId":2,"smsCount":1}"#,
        1,
    );

    let mut first = account(
        "fenghuo",
        failing.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
    );
    first.code = "first".to_string();
    let mut second = account(
        "fenghuo",
        succeeding.addr.port(),
        20,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
    );
    second.code = "second".to_string();

    let smser = Smser::with_settings(SmsSettings {
        data: vec![first, second],
        ..Default::default()
    });
    let result = smser
        .send(
            SmsType::DomesticNotification,
            &SmsRequest::new("13800138000").with_content("hello"),
        )
        .expect("应回退到第二个账号");
    assert_eq!(result.biz_id, "2");

    assert_eq!(failing.join().len(), 1, "第一个账号应已尝试");
    assert_eq!(succeeding.join().len(), 1, "第二个账号应已尝试");
}

#[test]
fn smser_reports_all_failed_details() {
    let failing = FakeHttpServer::start(200, r#"{"code":500,"message":"fail"}"#, 1);
    let config = account(
        "fenghuo",
        failing.addr.port(),
        10,
        r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
    );
    let smser = Smser::with_settings(SmsSettings {
        data: vec![config],
        ..Default::default()
    });
    let error = smser
        .send(
            SmsType::DomesticNotification,
            &SmsRequest::new("13800138000").with_content("hello"),
        )
        .expect_err("应全部失败");
    match error {
        SmsError::AllFailed { attempts, failures } => {
            assert_eq!(attempts, 1);
            assert_eq!(failures[0].code, "fenghuo-10");
            assert_eq!(failures[0].provider, "fenghuo");
            assert!(failures[0].message.contains("500"), "{failures:?}");
        }
        other => panic!("期望 AllFailed，实际：{other:?}"),
    }
    let _ = failing.join();
}

#[test]
fn smser_without_enabled_account_reports_config_error() {
    let smser = Smser::with_settings(SmsSettings::default());
    let error = smser
        .send(
            SmsType::DomesticNotification,
            &SmsRequest::new("13800138000").with_content("hello"),
        )
        .expect_err("应报配置错误");
    assert!(matches!(error, SmsError::Config(_)), "{error:?}");
}

#[test]
fn smser_open_generates_config_and_can_send() {
    let server = FakeHttpServer::start(
        200,
        r#"{"code":0,"message":"ok","msgId":9,"smsCount":1}"#,
        1,
    );
    let dir = std::env::temp_dir().join(format!("pek-rsms-smser-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let smser = Smser::open_in(&dir);
    assert!(
        smser.config_path().unwrap().ends_with("Config\\Sms.toml")
            || smser.config_path().unwrap().ends_with("Config/Sms.toml")
    );
    assert_eq!(smser.settings().data.len(), 1);

    let settings = SmsSettings {
        data: vec![account(
            "fenghuo",
            server.addr.port(),
            10,
            r#"{"BaseUrl":"http://127.0.0.1:{PORT}"}"#,
        )],
        ..Default::default()
    };
    smser.save_settings(settings).expect("保存配置");
    let result = smser
        .send(
            SmsType::DomesticNotification,
            &SmsRequest::new("13800138000").with_content("hi"),
        )
        .expect("通过 Smser 发送");
    assert_eq!(result.biz_id, "9");

    let _ = server.join();
    let text = std::fs::read_to_string(smser.config_path().unwrap()).unwrap();
    assert!(text.contains("fenghuo"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn direct_client_missing_config_returns_failure_result() {
    let config = SmsData {
        name: "aliyun".to_string(),
        ..Default::default()
    };
    let client = AliyunSmsClient::new(config);
    let result = client
        .send(&SmsRequest::new("13800138000").with_template("SMS_1", Vec::<(&str, &str)>::new()));
    assert!(!result.success);
    assert!(result.message.contains("AccessKey"));
    assert!(!result.is_success());
}
