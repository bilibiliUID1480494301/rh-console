#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

const KEYRING_SERVICE: &str = "relayhub-console";
const MIN_API_COMPAT: &str = "2026-11";

#[derive(Serialize, Deserialize, Clone)]
struct Site {
    id: String,
    label: String,
    base_url: String,
    username: String,
}

#[derive(Serialize)]
struct Probe {
    status: String, // online | auth_failed | unreachable | incompatible | third_party
    api_compat: Option<String>,
    version: Option<String>,
    latency_ms: u64,
    detail: Option<String>,
}

fn base_of(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('/');
    if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    }
}

fn sites_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("sites.json"))
}

fn load_sites(app: &tauri::AppHandle) -> Vec<Site> {
    match sites_path(app) {
        Ok(p) => std::fs::read_to_string(p)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_sites(app: &tauri::AppHandle, sites: &[Site]) -> Result<(), String> {
    let body = serde_json::to_string_pretty(sites).map_err(|e| e.to_string())?;
    std::fs::write(sites_path(app)?, body).map_err(|e| e.to_string())
}

#[tauri::command]
async fn subscribe_events(
    app: tauri::AppHandle,
    base_url: String,
    username: String,
    password: String,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tauri::Emitter;

    let base = base_of(&base_url);
    let cookie = login_cookie(&base, &username, &password).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base}/api/events"))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|e| format!("订阅失败：{e}"))?;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("流中断：{e}"))?;
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(idx) = buf.find("

") {
            let frame: String = buf.drain(..idx + 2).collect();
            for line in frame.lines() {
                if let Some(payload) = line.strip_prefix("data: ") {
                    let _ = app.emit("relayhub://event", payload.to_string());
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn site_list(app: tauri::AppHandle) -> Vec<Site> {
    load_sites(&app)
}

async fn login_cookie(base: &str, username: &str, password: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{base}/login"))
        .json(&serde_json::json!({"username": username, "password": password}))
        .send()
        .await
        .map_err(|e| format!("登录请求失败：{e}"))?;
    if resp.status().as_u16() == 401 {
        return Err("账号或密码不正确".into());
    }
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| v.split(';').next().map(str::to_string))
        .ok_or_else(|| "服务端没有下发会话 cookie".to_string())?;
    Ok(cookie)
}

#[tauri::command]
async fn site_test(base_url: String, username: String, password: String) -> Probe {
    let base = base_of(&base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .expect("client");
    let started = std::time::Instant::now();
    let mut out = Probe {
        status: "unreachable".into(),
        api_compat: None,
        version: None,
        latency_ms: 0,
        detail: None,
    };

    if client.get(format!("{base}/healthz")).send().await.is_err() {
        out.detail = Some("健康检查无响应".into());
        return out;
    }

    let cookie = match login_cookie(&base, &username, &password).await {
        Ok(c) => c,
        Err(msg) => {
            out.status = "auth_failed".into();
            out.detail = Some(msg);
            return out;
        }
    };
    let resp = client
        .get(format!("{base}/api/state"))
        .header("Cookie", cookie)
        .send()
        .await;
    out.latency_ms = started.elapsed().as_millis() as u64;

    let Ok(resp) = resp else {
        out.detail = Some("请求失败".into());
        return out;
    };
    if resp.status().as_u16() == 401 {
        out.status = "auth_failed".into();
        out.detail = Some("凭证无效".into());
        return out;
    }

    let compat_header = resp
        .headers()
        .get("x-api-compat")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    match resp.json::<serde_json::Value>().await {
        Ok(body) => {
            out.api_compat = compat_header.or_else(|| {
                body.get("api_compat")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
            out.version = body
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            out.status = match out.api_compat.as_deref() {
                None => "third_party",
                Some(c) if c < MIN_API_COMPAT => "incompatible",
                Some(_) => "online",
            }
            .into();
        }
        Err(e) => {
            out.status = "third_party".into();
            out.detail = Some(format!("响应不是预期 JSON：{e}"));
        }
    }
    out
}

#[tauri::command]
async fn site_add(
    app: tauri::AppHandle,
    label: String,
    base_url: String,
    username: String,
    password: String,
) -> Result<Site, String> {
    let probe = site_test(base_url.clone(), username.clone(), password.clone()).await;
    if probe.status == "unreachable" || probe.status == "auth_failed" {
        return Err(probe.detail.unwrap_or_else(|| "连接失败".into()));
    }
    let site = Site {
        id: uuid::Uuid::new_v4().to_string(),
        label: if label.trim().is_empty() {
            base_of(&base_url)
        } else {
            label.trim().to_string()
        },
        base_url: base_of(&base_url),
        username,
    };
    keyring::Entry::new(KEYRING_SERVICE, &format!("site:{}", site.id))
        .map_err(|e| e.to_string())?
        .set_password(&password)
        .map_err(|e| format!("写入钥匙串失败：{e}"))?;
    let mut sites = load_sites(&app);
    sites.push(site.clone());
    save_sites(&app, &sites)?;
    Ok(site)
}

#[tauri::command]
fn site_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut sites = load_sites(&app);
    sites.retain(|s| s.id != id);
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &format!("site:{id}")) {
        let _ = entry.delete_credential();
    }
    save_sites(&app, &sites)
}

#[derive(Serialize)]
struct EnvReport {
    python: Option<String>,
    hubrelay: Option<String>,
    detail: String,
}

fn probe_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(if text.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else {
        text
    })
}

#[tauri::command]
fn env_detect() -> EnvReport {
    let python = probe_cmd("python", &["--version"]).or_else(|| probe_cmd("python3", &["--version"]));
    let hubrelay = probe_cmd("hubrelay", &["--version"]);
    let detail = match (&python, &hubrelay) {
        (None, _) => "未找到 python / python3，请先安装 Python 3.10+".to_string(),
        (Some(py), None) => format!("{py}（未安装 hubrelay，可用 pip install hubrelay 或直接 python -m relayhub.gateway）"),
        (Some(py), Some(h)) => format!("{py} · hubrelay {h}"),
    };
    EnvReport { python, hubrelay, detail }
}

#[derive(Serialize)]
struct InstanceInfo {
    base_url: String,
    pid: u32,
}

#[tauri::command]
async fn instance_create(
    port: u16,
    token: String,
    home: String,
    username: String,
    password: String,
) -> Result<InstanceInfo, String> {
    let py = probe_cmd("python", &["--version"]).map(|_| "python".to_string())
        .or_else(|| probe_cmd("python3", &["--version"]).map(|_| "python3".to_string()))
        .ok_or_else(|| "未找到 python/python3".to_string())?;

    let mut cmd = std::process::Command::new(py);
    cmd.args(["-m", "relayhub.gateway", "serve", "--port", &port.to_string(), "--token", &token]);
    if !home.trim().is_empty() {
        cmd.args(["--home", &home]);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd.spawn().map_err(|e| format!("启动失败：{e}"))?;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    // 等它起来（最多约 12 秒）
    let mut ready = false;
    for _ in 0..30 {
        if client
            .get(format!("{base}/healthz"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    if !ready {
        return Err("实例 12 秒内没有就绪，请检查 Python 环境或端口占用".into());
    }

    // 用启动时的临时 token 建管理员账号；账号密码来自表单
    let resp = client
        .post(format!("{base}/api/users"))
        .header("X-Admin-Token", &token)
        .json(&serde_json::json!({
            "username": username, "password": password, "role": "admin", "quota": -1
        }))
        .send()
        .await
        .map_err(|e| format!("创建管理员账号失败：{e}"))?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        return Err(format!("创建管理员账号失败（HTTP {code}）：{detail}"));
    }

    Ok(InstanceInfo {
        base_url: base,
        pid: child.id(),
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            site_list,
            site_test,
            site_add,
            subscribe_events,
            site_remove,
            env_detect,
            instance_create
        ])
        .run(tauri::generate_context!())
        .expect("failed to start relay-hub console");
}
