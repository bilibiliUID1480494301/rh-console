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
    /// 登录时探测到的站点角色。只读站（非 admin）在界面上要禁用写操作。
    #[serde(default)]
    role: String,
}

#[derive(Serialize)]
struct Probe {
    status: String, // online | auth_failed | unreachable | incompatible | third_party
    role: Option<String>, // 该账号在站点上的角色：admin | vip | user
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
async fn admin_call(
    base_url: String,
    username: String,
    password: String,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    // 只放行 /api/ 下的路径：这个命令拿着管理员 cookie，不能变成任意请求器
    if !path.starts_with("/api/") {
        return Err("只允许 /api/ 下的路径".into());
    }
    let verb = method.to_uppercase();
    if !matches!(verb.as_str(), "GET" | "POST" | "PUT" | "DELETE") {
        return Err(format!("不支持的方法 {verb}"));
    }
    let base = base_of(&base_url);
    let cookie = login_cookie(&base, &username, &password).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{base}{path}");
    let mut req = match verb.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        _ => client.delete(&url),
    }
    .header("Cookie", cookie);
    if verb != "GET" {
        if let Some(payload) = body {
            req = req.json(&payload);
        }
    }
    let resp = req.send().await.map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("HTTP {status}：{text}"));
    }
    serde_json::from_str(&text).or_else(|_| Ok(serde_json::json!({"raw": text})))
}

// ---- 远程命令（SSH） -------------------------------------------------------
// 安全模型：
// 1. 预设动作的命令串写在代码里（SSH_PRESETS），UI 只能选 id，不能传命令；
// 2. 自由命令必须由用户在「高级模式」里逐条确认，且每站点单独放行；
// 3. 命令通过 ssh 的 argv 直传（不做本地 shell 拼接），无非必要转义；
// 4. 每次执行都写本地审计（时间/主机/命令/退出码/耗时），便于事后追。
pub const SSH_PRESETS: &[(&str, &str, &str, bool)] = &[
    ("version", "版本", "python3 -m relayhub.gateway --version 2>/dev/null || hubrelay --version", false),
    ("env", "环境自检", "uname -srm; cat /etc/os-release 2>/dev/null | head -2; if [ -f /.dockerenv ]; then echo 容器环境; fi; if command -v systemctl >/dev/null 2>&1; then echo 有systemd; else echo 无systemd; fi; if command -v python3 >/dev/null 2>&1; then python3 -V; else echo 无python3; fi", false),
    ("probe", "健康探测", "curl -s -o /dev/null -m 3 -w '%{http_code}' http://127.0.0.1:8799/v1/messages || echo 不通", false),
    ("ports", "监听端口", "if command -v ss >/dev/null 2>&1; then ss -ltn 2>/dev/null | grep -E ':(8799|18802)' || echo 没看到8799/18802在听; elif command -v netstat >/dev/null 2>&1; then netstat -ltn 2>/dev/null | grep -E ':(8799|18802)'; else echo 容器里没有ss/netstat，可试 cat /proc/net/tcp; fi", false),
    ("disk", "磁盘与内存", "df -h / 2>/dev/null; df -h /opt/hubrelay 2>/dev/null; free -m 2>/dev/null || echo 无free", false),
    ("logs", "最近日志", "if command -v journalctl >/dev/null 2>&1; then journalctl -u relayhub -n 100 --no-pager; elif [ -f /var/log/relayhub.log ]; then tail -n 100 /var/log/relayhub.log; else echo 没有本地日志：容器部署的日志由平台收集，请在该平台的日志页查看; fi", false),
    ("restart", "重启服务", "if command -v systemctl >/dev/null 2>&1; then systemctl restart relayhub && echo 已重启systemd; else echo 容器或非 systemd 部署：没有通用重启命令，请用该平台的重启或重新部署; fi", true),
];

fn ssh_audit(app: &tauri::AppHandle, entry: &serde_json::Value) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("ssh-audit.jsonl");
    let mut line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    line.push('\n');
    use std::io::Write;
    let mut fh = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    fh.write_all(line.as_bytes()).map_err(|e| e.to_string())
}


/// ssh 只认 `-p PORT` + 裸主机，不认 `user@host:port`。
/// 而端口转发/穿透场景给出的恰恰是 `user@127.0.0.1:2222`——必须自己拆。
/// IPv6 字面量请写成 `[::1]:22` 或 `[::1]`。
fn split_host_port(target: &str) -> Result<(String, Option<u16>), String> {
    let Some(idx) = target.rfind(':') else {
        return Ok((target.to_string(), None));
    };
    let (head, tail) = (&target[..idx], &target[idx + 1..]);
    if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
        return Ok((target.to_string(), None));   // 尾巴不是数字：不是端口，原样交给 ssh
    }
    // 冒号前还有冒号 = 无括号 IPv6（root@::1），那个 "1" 是地址的一部分，不是端口
    let head_is_bracketed_v6 = head.starts_with('[') && head.ends_with(']');
    if head.contains(':') && !head_is_bracketed_v6 {
        return Ok((target.to_string(), None));
    }
    let port: u16 = tail.parse().map_err(|_| format!("端口不合法：{tail}"))?;
    if head.is_empty() {
        return Err("缺少主机名".into());
    }
    Ok((head.to_string(), Some(port)))
}

fn ssh_check_host(host: &str) -> Result<(), String> {
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || "@.-_:[]".contains(c)) {
        return Err("host 只允许字母数字与 @ . - _ : [ ]".into());
    }
    Ok(())
}

fn ssh_check_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("命令不能为空".into());
    }
    if command.len() > 2000 {
        return Err("命令太长（上限 2000 字节）".into());
    }
    if command.contains('\0') {
        return Err("命令含非法字符".into());
    }
    Ok(())
}

/// 执行一条远端命令。argv 直传：不做本地 shell 拼接，避免本地注入。
/// 拼 ssh 参数。单独拆出来是为了能断言「端口对不对、命令有没有被本地 shell 碰过」，
/// 不必真连一台远端机器。
fn ssh_args(target: &str, key_path: &str, command: &str) -> Result<Vec<String>, String> {
    let (host, port) = split_host_port(target)?;
    let mut args: Vec<String> = vec![
        "-i".into(), key_path.to_string(),
        "-o".into(), "BatchMode=yes".into(),
        "-o".into(), "ConnectTimeout=10".into(),
        "-o".into(), "StrictHostKeyChecking=accept-new".into(),
    ];
    if let Some(p) = port {
        args.push("-p".into());
        args.push(p.to_string());
    }
    args.push(host);
    args.push(command.to_string());
    Ok(args)
}

fn ssh_exec(target: &str, key_path: &str, command: &str) -> Result<(i32, String, String, u128), String> {
    let started_at = std::time::Instant::now();
    let args = ssh_args(target, key_path, command)?;
    let out = std::process::Command::new("ssh")
        .args(&args)
        .output()
        .map_err(|e| format!("调用 ssh 失败：{e}"))?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr, started_at.elapsed().as_millis()))
}

#[tauri::command]
async fn ssh_preset(
    app: tauri::AppHandle,
    host: String,
    key_path: String,
    preset: String,
) -> Result<serde_json::Value, String> {
    ssh_check_host(&host)?;
    let (_, label, command, needs_confirm) = SSH_PRESETS
        .iter()
        .find(|(id, ..)| *id == preset)
        .copied()
        .ok_or_else(|| format!("未知预设 {preset}"))?;
    let (code, stdout, stderr, ms) = ssh_exec(&host, &key_path, command)?;
    ssh_audit(&app, &serde_json::json!({
        "ts": now_secs(), "host": host, "kind": "preset", "preset": preset,
        "label": label, "needs_confirm": needs_confirm, "command": command,
        "exit": code, "ms": ms,
    }))?;
    Ok(serde_json::json!({
        "ok": code == 0, "exit": code, "stdout": stdout,
        "stderr": truncate_tail(&stderr, 4000), "ms": ms, "label": label,
    }))
}

#[tauri::command]
async fn ssh_run(
    app: tauri::AppHandle,
    host: String,
    key_path: String,
    command: String,
) -> Result<serde_json::Value, String> {
    ssh_check_host(&host)?;
    ssh_check_command(&command)?;
    let (code, stdout, stderr, ms) = ssh_exec(&host, &key_path, &command)?;
    ssh_audit(&app, &serde_json::json!({
        "ts": now_secs(), "host": host, "kind": "custom", "command": command,
        "exit": code, "ms": ms,
    }))?;
    Ok(serde_json::json!({
        "ok": code == 0, "exit": code, "stdout": stdout,
        "stderr": truncate_tail(&stderr, 4000), "ms": ms,
    }))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn truncate_tail(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let skip = text.chars().count() - limit;
    format!(
        "…（前面省略 {} 字）\n{}",
        skip,
        text.chars().skip(skip).collect::<String>()
    )
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

/// 行尾归一：签名按字节验，清单经 HTTP 回来常带 CRLF（reqwest 的 text() 不翻译换行，
/// 不像 Python 的 read_text）。两侧必须用同一套规则，否则同一份清单两种验签结论。
fn normalize_manifest(manifest: &str) -> String {
    manifest.replace("\r\n", "\n").replace('\r', "\n")
}

fn verify_signature(manifest: &str, signature_b64: &str) -> Result<(), String> {
    verify_with_pubkey(RELEASE_PUBKEY, manifest, signature_b64)
}

fn verify_with_pubkey(public_key_b64: &str, manifest: &str, signature_b64: &str) -> Result<(), String> {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let engine = base64::engine::general_purpose::STANDARD;
    let key_bytes = engine.decode(public_key_b64).map_err(|e| e.to_string())?;
    let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| "公钥长度不对".to_string())?;
    let key = VerifyingKey::from_bytes(&key_arr).map_err(|e| e.to_string())?;

    let sig_bytes = engine.decode(signature_b64.trim()).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    let normalized = normalize_manifest(manifest);
    key.verify(normalized.as_bytes(), &signature)
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

const GH_REPO: &str = "bilibiliUID1480494301/relay-hub";
const GH_API: &str = "https://api.github.com";

/// 发布资产下载器：先直连 github.com，被挡时回落 api.github.com 的资产端点。
///
/// 回落只换传输路径，**验签强度不变**；一次直连失败后不再重试，
/// 否则三个资产各等一次超时，部署要白等一分半。
struct ReleaseDownloader {
    client: reqwest::Client,
    version: String,
    prefer_api: bool,
    index: Option<std::collections::HashMap<String, String>>,
}

impl ReleaseDownloader {
    fn new(client: reqwest::Client, version: &str) -> Self {
        Self { client, version: version.to_string(), prefer_api: false, index: None }
    }

    async fn asset(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let direct = format!(
            "https://github.com/{GH_REPO}/releases/download/v{}/{name}",
            self.version
        );
        if !self.prefer_api {
            match self.client.get(&direct).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string());
                }
                _ => self.prefer_api = true,
            }
        }
        if self.index.is_none() {
            self.index = Some(self.load_index().await?);
        }
        let url = self
            .index
            .as_ref()
            .and_then(|m| m.get(name).cloned())
            .ok_or_else(|| format!("API 的发布资产里没有 {name}"))?;
        let resp = self
            .client
            .get(url)
            .header("Accept", "application/octet-stream")
            .send()
            .await
            .map_err(|e| format!("API 下载 {name} 失败：{e}"))?;
        if !resp.status().is_success() {
            return Err(format!("API 下载 {name} 失败：HTTP {}", resp.status()));
        }
        resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
    }

    async fn load_index(&self) -> Result<std::collections::HashMap<String, String>, String> {
        let resp = self
            .client
            .get(format!("{GH_API}/repos/{GH_REPO}/releases/tags/v{}", self.version))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| format!("查询 release 失败：{e}"))?;
        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut map = std::collections::HashMap::new();
        for asset in body.get("assets").and_then(|a| a.as_array()).into_iter().flatten() {
            let (Some(n), Some(u)) = (asset.get("name").and_then(|v| v.as_str()), asset.get("url").and_then(|v| v.as_str()))
            else { continue };
            map.insert(n.to_string(), u.to_string());
        }
        Ok(map)
    }
}

#[tauri::command]
async fn remote_deploy(
    host: String,
    key_path: String,
    version: String,
    python: String,
    mode: Option<String>,
    data_dir: Option<String>,
) -> Result<String, String> {
    let mode = mode.unwrap_or_else(|| "pip".into());
    let data_dir = data_dir.unwrap_or_else(|| "/opt/hubrelay".into());
    if mode != "pip" && mode != "docker" {
        return Err("mode 只能是 pip 或 docker".into());
    }
    if !data_dir.starts_with('/') || !data_dir.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.')) {
        return Err("data_dir 必须是以 / 开头的绝对路径".into());
    }
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
        // GitHub API 对没有 UA 的请求直接 403，回落请求必须带上
        .user_agent("rh-console")
        .build()
        .map_err(|e| e.to_string())?;
    let mut dl = ReleaseDownloader::new(client, &version);

    // 直连被墙时自动回落 api.github.com；拿回来的字节照旧要验签 + 比哈希
    let manifest = String::from_utf8_lossy(&dl.asset("SHA256SUMS").await?)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let signature = String::from_utf8_lossy(&dl.asset("SHA256SUMS.sig").await?).to_string();
    verify_signature(&manifest, &signature)?;

    let expected = pick_wheel_hash(&manifest, &version)
        .ok_or_else(|| format!("签名清单里没有 {version} 的 wheel"))?;
    let name = format!("hubrelay-{version}-py3-none-any.whl");
    let bytes = dl.asset(&name).await?;
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

    // 部署前先记下远端当前版本，健康检查不过时用来回滚
    let prev = std::process::Command::new("ssh")
        .args(["-i", &key_path, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
               "-o", "StrictHostKeyChecking=accept-new", &host,
               &format!("{py} -m relayhub.gateway --version 2>/dev/null | head -1")])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let remote = if mode == "docker" {
        format!(
            "set -e; mkdir -p {data_dir}{{/data,/pkg}}; mv {remote_tmp} {data_dir}/pkg/{name}; docker rm -f hubrelay >/dev/null 2>&1 || true; docker run -d --name hubrelay --restart unless-stopped   -p 8799:8799 -v {data_dir}/data:/data -v {data_dir}/pkg/{name}:/tmp/pkg.whl:ro   python:3.12-slim sh -c 'pip install --no-cache-dir /tmp/pkg.whl && hubrelay serve --host 0.0.0.0 --port 8799 --public'; sleep 8; docker exec hubrelay python -c \"import urllib.request,sys;sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8799/healthz',timeout=5).status==200 else 1)\""
        )
    } else {
        format!(
            "{py} -m pip install --no-deps --force-reinstall --no-input {remote_tmp} && rm -f {remote_tmp} && {py} -m relayhub.gateway --version"
        )
    };
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
        // 装完起不来：pip 模式下尝试回滚到部署前版本，把现场情况一并报出来
        let mut msg = format!(
            "远程{}失败（{}）：{}{}",
            if mode == "docker" { "容器启动" } else { "安装" },
            ssh.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&ssh.stderr),
            stdout
        );
        if mode == "pip" {
            if let Some(prev_version) = prev.as_ref() {
                if let Some(v) = prev_version.split_whitespace().last() {
                    let _ = std::process::Command::new("ssh")
                        .args(["-i", &key_path, "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                               "-o", "StrictHostKeyChecking=accept-new", &host,
                               &format!("{py} -m pip install --no-deps --force-reinstall --no-input relayhub=={v}")])
                        .output();
                    msg.push_str(&format!("
已尝试回滚到 {v}，请核实远端状态。"));
                }
            }
        }
        return Err(msg);
    }
    Ok(format!("已验签并部署 {version} → {host}（{mode}）
{}", stdout.trim()))
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
        role: None,
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
    // 探测角色：站点有三级（user / vip / admin）。
    // 先问 /api/user/me 拿准确角色；拿不到再退回「能否读 /api/users」判管理员。
    if out.status == "online" {
        if let Ok(cookie) = login_cookie(&base, &username, &password).await {
            let me = client
                .get(format!("{base}/api/state"))
                .header("Cookie", cookie.clone())
                .send()
                .await;
            let mut role: Option<String> = None;
            if let Ok(resp) = me {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        role = v.get("role").and_then(|x| x.as_str()).map(|s| s.to_string());
                    }
                }
            }
            if role.is_none() {
                let resp = client
                    .get(format!("{base}/api/users"))
                    .header("Cookie", cookie)
                    .send()
                    .await;
                role = Some(match resp {
                    Ok(r) if r.status().is_success() => "admin".to_string(),
                    _ => "user".to_string(),
                });
            }
            out.role = role;
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
        role: probe.role.clone().unwrap_or_else(|| "user".into()),
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
            admin_call,
            ssh_preset,
            ssh_run,
            doctor_local,
            remote_deploy,
            site_remove,
            env_detect,
            instance_create,
            overview_all,
            site_users,
            site_set_role
        ])
        .run(tauri::generate_context!())
        .expect("failed to start relay-hub console");
}

#[cfg(test)]
mod tests {

    /// 真连一次本机测试 sshd（默认忽略；显式 --ignored 才跑）。
    /// 起服务端：python E:/dev/.tmp-asar/sshtest_server.py（127.0.0.1:2222，只认 sshtest_clientkey）
    /// 注意：假远端是 MSYS bash，复杂复合命令偶发 fork 失败返回 255——
    /// 那是测试夹具的问题，不代表产品行为，所以这里只断言协议层可判定的部分。
    #[test]
    #[ignore = "需要本机 127.0.0.1:2222 的测试 sshd"]
    fn ssh_exec_end_to_end() {
        let key = "E:/dev/.tmp-asar/sshtest_clientkey";

        // 1) 退出码必须原样透传（我们靠它判断成败）
        let (code, out, err, ms) =
            ssh_exec("test@127.0.0.1:2222", key, "echo hello-e2e; exit 7").unwrap();
        assert_eq!(code, 7, "退出码没透传：out={out} err={err}");
        assert!(out.contains("hello-e2e"), "stdout 丢了：{out}");
        assert!(ms > 0, "耗时应有值");

        // 2) 两条流都要回收（假远端的 stdout/stderr 会串，故只断言内容都在）
        let (code2, out2, err2, _) =
            ssh_exec("test@127.0.0.1:2222", key, "echo to-stdout; echo to-stderr 1>&2").unwrap();
        assert_eq!(code2, 0);
        let both = format!("{out2}{err2}");
        assert!(both.contains("to-stdout"), "stdout 内容丢了：{both}");
        assert!(both.contains("to-stderr"), "stderr 内容丢了：{both}");

        // 3) 端口写法：无端口的 target 仍走默认 22（这里应连接失败而非 panic）
        let plain = ssh_exec("test@127.0.0.1", key, "true");
        assert!(plain.is_err() || plain.map(|(c, ..)| c != 0).unwrap_or(true),
                "不该意外连上默认端口的服务");
    }


    #[test]
    fn ssh_args_carry_port_and_verbatim_command() {
        let args = ssh_args("root@127.0.0.1:2222", "C:/keys/id_ed25519", "systemctl status relayhub").unwrap();
        let rendered = args.join(" ");
        assert!(rendered.contains("-p 2222"), "端口没传成 -p：{rendered}");
        // 命令必须原样作为最后一个 argv，不能被本机 shell 解释
        assert_eq!(args.last().unwrap(), "systemctl status relayhub");
        assert_eq!(args[args.len() - 2], "root@127.0.0.1");
        // 无端口时不出现 -p
        let plain = ssh_args("root@1.2.3.4", "k", "true").unwrap().join(" ");
        assert!(!plain.contains("-p"), "不该凭空多出 -p：{plain}");
        // 带管道/重定向的命令照原样传，不做本地拼接
        let complex = "a | b > c 2>&1";
        let a2 = ssh_args("h", "k", complex).unwrap();
        assert_eq!(a2.last().unwrap(), complex);
    }


    #[test]
    fn host_port_is_split_for_ssh() {
        // 端口转发场景：user@127.0.0.1:2222 必须拆成 host + -p 2222
        assert_eq!(split_host_port("root@127.0.0.1:2222").unwrap(),
                   ("root@127.0.0.1".to_string(), Some(2222)));
        assert_eq!(split_host_port("root@1.2.3.4").unwrap(),
                   ("root@1.2.3.4".to_string(), None));
        // 无括号的 IPv6 里冒号不是端口：原样返回，交给 ssh 处理
        assert_eq!(split_host_port("root@::1").unwrap(),
                   ("root@::1".to_string(), None));
        assert_eq!(split_host_port("[::1]:22").unwrap(),
                   ("[::1]".to_string(), Some(22)));
        assert!(split_host_port("root@host:abc").is_ok());   // 不当端口，原样
        assert!(split_host_port("root@host:99999").is_err()); // 越界
        assert!(split_host_port(":22").is_err());             // 没主机
    }


    #[test]
    fn ssh_presets_are_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for (id, label, command, needs_confirm) in SSH_PRESETS {
            assert!(ids.insert(*id), "预设 id 重复：{id}");
            assert!(!label.is_empty(), "{id} 没有中文标签");
            assert!(!command.trim().is_empty(), "{id} 命令为空");
            assert!(!command.contains('\u{0}'), "{id} 命令含 NUL");
        }
        let restart = SSH_PRESETS.iter().find(|(id, ..)| *id == "restart").unwrap();
        assert!(restart.3, "restart 必须标记 needs_confirm");
        assert!(SSH_PRESETS.iter().filter(|(id, ..)| *id != "restart").count() >= 4);
    }

    #[test]
    fn ssh_command_validation() {
        assert!(ssh_check_command("   ").is_err());
        assert!(ssh_check_command("").is_err());
        assert!(ssh_check_command("ls\u{0}").is_err());
        assert!(ssh_check_command(&"a".repeat(2001)).is_err());
        assert!(ssh_check_command("systemctl status relayhub").is_ok());
    }

    #[test]
    fn ssh_host_validation() {
        assert!(ssh_check_host("root@1.2.3.4").is_ok());
        assert!(ssh_check_host("user@host:2222").is_ok());
        assert!(ssh_check_host("host; rm -rf /").is_err());
        assert!(ssh_check_host("").is_err());
    }

    #[test]
    fn truncate_keeps_the_tail() {
        let text: String = (0..100).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let cut = truncate_tail(&text, 10);
        assert!(cut.contains("前面省略 90 字"));
        assert!(cut.ends_with(&text[text.len() - 10..]));
        assert_eq!(truncate_tail("short", 10), "short");
    }

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
    fn crlf_manifest_verifies_against_lf_signature() {
        // 发布方签 LF，清单回来可能带 CRLF：验签前必须归一，否则新旧资产结论不一致
        use base64::Engine;
        use ed25519_dalek::{Signer, SigningKey};

        let engine = base64::engine::general_purpose::STANDARD;
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let pub_b64 = engine.encode(signing.verifying_key().to_bytes());

        let lf = "aa  hubrelay-0.12.31-py3-none-any.whl\nbb  hubrelay-0.12.31.tar.gz\n";
        let sig = engine.encode(signing.sign(lf.as_bytes()).to_bytes());

        assert!(verify_with_pubkey(&pub_b64, lf, &sig).is_ok());
        assert!(verify_with_pubkey(&pub_b64, &lf.replace('\n', "\r\n"), &sig).is_ok());
        // 内容真被改过还是要拒绝
        assert!(verify_with_pubkey(&pub_b64, "cc  hubrelay-0.12.31-py3-none-any.whl\n", &sig).is_err());
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


// ---- 多站点总览：一次把所有站的用量拉齐（网页版做不到，它一次只能登一个站） ----

#[derive(serde::Deserialize)]
struct SiteRef {
    id: String,
    label: String,
    base_url: String,
    username: String,
}

#[derive(serde::Serialize)]
struct SiteOverview {
    id: String,
    label: String,
    base_url: String,
    ok: bool,
    status: String,
    requests: u64,
    failed: u64,
    tokens_in: u64,
    tokens_out: u64,
    latency_ms: u64,
    detail: Option<String>,
}

fn pick_u64(v: &serde_json::Value, path: &str) -> u64 {
    v.pointer(path).and_then(|x| x.as_u64()).unwrap_or(0)
}

#[tauri::command]
async fn overview_all(sites: Vec<SiteRef>, days: Option<u32>) -> Result<Vec<SiteOverview>, String> {
    let window = days.unwrap_or(1).max(1);
    let mut out = Vec::with_capacity(sites.len());
    for site in sites {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("site:{}", site.id))
            .map_err(|e| e.to_string())?;
        let password = match entry.get_password() {
            Ok(p) => p,
            Err(_) => {
                out.push(SiteOverview {
                    id: site.id, label: site.label, base_url: site.base_url,
                    ok: false, status: "no_credential".into(),
                    requests: 0, failed: 0, tokens_in: 0, tokens_out: 0, latency_ms: 0,
                    detail: Some("本机没有保存该站口令".into()),
                });
                continue;
            }
        };
        let base = base_of(&site.base_url);
        let started = std::time::Instant::now();
        let fetched = async {
            let cookie = login_cookie(&base, &site.username, &password).await?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .map_err(|e| e.to_string())?;
            let resp = client
                .get(format!("{base}/api/usage?days={window}"))
                .header("Cookie", cookie)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status().as_u16()));
            }
            resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())
        }
        .await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match fetched {
            Ok(v) => {
                // 实际形状：{"requests":{"window":{requests,ok,failed,tokens_in,tokens_out,...}}}
                let w = v.pointer("/requests/window").cloned().unwrap_or(serde_json::Value::Null);
                out.push(SiteOverview {
                    id: site.id, label: site.label, base_url: site.base_url,
                    ok: true, status: "online".into(),
                    requests: pick_u64(&w, "/requests"),
                    failed: pick_u64(&w, "/failed"),
                    tokens_in: pick_u64(&w, "/tokens_in"),
                    tokens_out: pick_u64(&w, "/tokens_out"),
                    latency_ms,
                    detail: None,
                });
            }
            Err(e) => out.push(SiteOverview {
                id: site.id, label: site.label, base_url: site.base_url,
                ok: false, status: "error".into(),
                requests: 0, failed: 0, tokens_in: 0, tokens_out: 0, latency_ms,
                detail: Some(e),
            }),
        }
    }
    Ok(out)
}


// ---- 权限分配：在这台控制台里改站内用户的角色 ----
// 站点侧四层：owner(站长) / admin(管理员) / vip / user。
// 守卫放 Rust 侧：前端隐藏按钮不算数，这里再判一次。

#[derive(serde::Serialize, serde::Deserialize)]
struct UserRow {
    user_id: String,
    username: String,
    role: String,
    enabled: bool,
    quota: i64,
    used: i64,
}

async fn site_session(site_id: &str, app: &tauri::AppHandle) -> Result<(String, String, String, String), String> {
    let site = load_sites(app)
        .into_iter()
        .find(|s| s.id == site_id)
        .ok_or_else(|| "没有这个站点".to_string())?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("site:{}", site.id))
        .map_err(|e| e.to_string())?;
    let password = entry.get_password().map_err(|_| "本机没有保存该站口令".to_string())?;
    let base = base_of(&site.base_url);
    let cookie = login_cookie(&base, &site.username, &password).await?;
    Ok((base, site.username, password, cookie))
}

async fn my_scopes(base: &str, cookie: &str) -> Vec<String> {
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match client.get(format!("{base}/api/state")).header("Cookie", cookie.to_string()).send().await {
        Ok(r) if r.status().is_success() => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("scopes").and_then(|x| x.as_array()).map(|arr| {
                arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect::<Vec<_>>()
            }))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[tauri::command]
async fn site_users(app: tauri::AppHandle, site_id: String) -> Result<Vec<UserRow>, String> {
    let (base, _u, _p, cookie) = site_session(&site_id, &app).await?;
    let scopes = my_scopes(&base, &cookie).await;
    if !scopes.iter().any(|s| s == "user:read") {
        return Err("当前账号没有 user:read，看不到用户列表".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base}/api/users"))
        .header("Cookie", cookie)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Some(items) = v.get("users").and_then(|u| u.as_array()) {
        for u in items {
            out.push(UserRow {
                user_id: u.get("user_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                username: u.get("username").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                role: u.get("role").and_then(|x| x.as_str()).unwrap_or("user").to_string(),
                enabled: u.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
                quota: u.get("quota").and_then(|x| x.as_i64()).unwrap_or(0),
                used: u.get("used").and_then(|x| x.as_i64()).unwrap_or(0),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
async fn site_set_role(
    app: tauri::AppHandle,
    site_id: String,
    user_id: String,
    role: String,
) -> Result<(), String> {
    if !["user", "vip", "admin"].contains(&role.as_str()) {
        return Err(format!("未知角色 {role}"));
    }
    let (base, _u, _p, cookie) = site_session(&site_id, &app).await?;
    let scopes = my_scopes(&base, &cookie).await;
    if !scopes.iter().any(|s| s == "role:grant") {
        return Err("当前账号没有 role:grant，只有站长能改角色".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .put(format!("{base}/api/users/{user_id}"))
        .header("Cookie", cookie)
        .json(&serde_json::json!({"role": role}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {code} {body}"));
    }
    Ok(())
}
