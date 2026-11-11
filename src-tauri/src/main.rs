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
async fn plugin_costs(
    base_url: String,
    username: String,
    password: String,
    days: u32,
) -> Result<serde_json::Value, String> {
    let base = base_of(&base_url);
    let cookie = login_cookie(&base, &username, &password).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base}/api/plugin-costs?days={days}"))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("HTTP {status}：{body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("不是 JSON：{e}"))
}

#[tauri::command]
async fn admin_get(
    base_url: String,
    username: String,
    password: String,
    path: String,
) -> Result<serde_json::Value, String> {
    if !path.starts_with("/api/") {
        return Err("只允许 /api/ 下的只读路径".into());
    }
    let base = base_of(&base_url);
    let cookie = login_cookie(&base, &username, &password).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base}{path}"))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("HTTP {status}：{body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("不是 JSON：{e}"))
}

#[tauri::command]
async fn admin_post(
    base_url: String,
    username: String,
    password: String,
    path: String,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !path.starts_with("/api/") {
        return Err("只允许 /api/ 下的路径".into());
    }
    let base = base_of(&base_url);
    let cookie = login_cookie(&base, &username, &password).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{base}{path}"))
        .header("Cookie", cookie)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("HTTP {status}：{text}"));
    }
    serde_json::from_str(&text).or_else(|_| Ok(serde_json::json!({"raw": text})))
}

#[tauri::command]
fn doctor_local() -> Result<serde_json::Value, String> {
    // doctor 只对本机有意义（查的是本机 Python/端口/数据目录），远程实例不该由它代跑
    let py = probe_cmd("python", &["--version"]).map(|_| "python".to_string())
        .or_else(|| probe_cmd("python3", &["--version"]).map(|_| "python3".to_string()))
        .ok_or_else(|| "未找到 python/python3，无法运行 doctor".to_string())?;

    let out = std::process::Command::new(py)
        .args(["-m", "relayhub.gateway", "doctor", "--json"])
        .output()
        .map_err(|e| format!("doctor 启动失败：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("doctor 输出不是 JSON：{e}"))?;
    Ok(parsed)
}

const RELEASE_PUBKEY: &str = "UalueWu9iiKHBRQnANytr4J/MDTjlt3NINOjWpdQd0g=";

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn verify_signature(manifest: &str, signature_b64: &str) -> Result<(), String> {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let engine = base64::engine::general_purpose::STANDARD;
    let key_bytes = engine.decode(RELEASE_PUBKEY).map_err(|e| e.to_string())?;
    let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| "公钥长度不对".to_string())?;
    let key = VerifyingKey::from_bytes(&key_arr).map_err(|e| e.to_string())?;

    let sig_bytes = engine.decode(signature_b64.trim()).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    key.verify(manifest.as_bytes(), &signature)
        .map_err(|_| "签名与清单不匹配（拒绝部署）".to_string())
}

fn pick_wheel_hash(manifest: &str, version: &str) -> Option<String> {
    let want = format!("hubrelay-{version}-py3-none-any.whl");
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        if parts.next() == Some(want.as_str()) {
            return Some(hash.to_string());
        }
    }
    None
}

#[tauri::command]
async fn remote_deploy(
    host: String,
    key_path: String,
    version: String,
    python: String,
) -> Result<String, String> {
    // 远程部署**先在本地验签再传**：不能让远端只凭 PyPI 的 TLS 信任就装代码。
    // 私钥只收路径、不落盘：远程 shell 的权限比面板密码高一个量级。
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || "@.-_:".contains(c)) {
        return Err("host 只允许字母数字与 @ . - _ :".into());
    }
    if !version.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit())) || version.matches('.').count() != 2 {
        return Err("version 必须形如 1.2.3".into());
    }
    let py = if python.trim().is_empty() { "python".to_string() } else { python };
    let py_ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.');
    if !py.chars().all(py_ok) {
        return Err("python 解释器路径不合法".into());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let base = format!("https://github.com/bilibiliUID1480494301/relay-hub/releases/download/v{version}");

    let manifest = client.get(format!("{base}/SHA256SUMS")).send().await
        .map_err(|e| format!("下载清单失败：{e}"))?.text().await
        .map_err(|e| e.to_string())?;
    let signature = client.get(format!("{base}/SHA256SUMS.sig")).send().await
        .map_err(|e| format!("下载签名失败：{e}"))?.text().await
        .map_err(|e| e.to_string())?;
    verify_signature(&manifest, &signature)?;

    let expected = pick_wheel_hash(&manifest, &version)
        .ok_or_else(|| format!("签名清单里没有 {version} 的 wheel"))?;
    let name = format!("hubrelay-{version}-py3-none-any.whl");
    let bytes = client.get(format!("{base}/{name}")).send().await
        .map_err(|e| format!("下载 wheel 失败：{e}"))?.bytes().await
        .map_err(|e| e.to_string())?;
    if sha256_hex(&bytes) != expected {
        return Err("wheel 与签名清单的哈希不一致（拒绝部署）".into());
    }

    let dir = std::env::temp_dir().join("relayhub-deploy");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let local = dir.join(&name);
    std::fs::write(&local, &bytes).map_err(|e| e.to_string())?;

    let remote_tmp = format!("/tmp/{name}");
    let scp = std::process::Command::new("scp")
        .args([
            "-i", &key_path, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=accept-new",
            local.to_str().ok_or("本地路径不可用")?, &format!("{host}:{remote_tmp}"),
        ])
        .output()
        .map_err(|e| format!("调用 scp 失败：{e}"))?;
    if !scp.status.success() {
        return Err(format!("scp 失败：{}", String::from_utf8_lossy(&scp.stderr)));
    }

    let remote = format!(
        "{py} -m pip install --no-deps --force-reinstall --no-input {remote_tmp} && rm -f {remote_tmp} && {py} -m relayhub.gateway --version"
    );
    let ssh = std::process::Command::new("ssh")
        .args([
            "-i", &key_path, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=accept-new",
            &host, &remote,
        ])
        .output()
        .map_err(|e| format!("调用 ssh 失败：{e}"))?;
    let stdout = String::from_utf8_lossy(&ssh.stdout).to_string();
    if !ssh.status.success() {
        return Err(format!(
            "远程安装失败（{}）：{}{}",
            ssh.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&ssh.stderr),
            stdout
        ));
    }
    Ok(format!("已验签并部署 {version} → {}
{}", host, stdout.trim()))
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
            plugin_costs,
            admin_get,
            admin_post,
            doctor_local,
            remote_deploy,
            site_remove,
            env_detect,
            instance_create
        ])
        .run(tauri::generate_context!())
        .expect("failed to start relay-hub console");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_requested_wheel_only() {
        let manifest = "aa  hubrelay-1.2.3-py3-none-any.whl
bb  hubrelay-1.2.3.tar.gz
";
        assert_eq!(pick_wheel_hash(manifest, "1.2.3"), Some("aa".to_string()));
        assert_eq!(pick_wheel_hash(manifest, "9.9.9"), None);
    }

    #[test]
    fn rejects_bad_signature() {
        // 32 字节全零公钥对应的签名绝不可能是真签名：必须拒绝而不是放行
        let manifest = "aa  hubrelay-1.2.3-py3-none-any.whl
";
        let bad_sig = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
        assert!(verify_signature(manifest, bad_sig).is_err());
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
