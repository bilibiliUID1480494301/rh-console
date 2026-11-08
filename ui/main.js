const $ = (id) => document.getElementById(id);
const invoke = window.__TAURI__?.core?.invoke;
let lastProbe = null;

const LABEL = {
  online: "在线",
  auth_failed: "凭证无效",
  unreachable: "无法连接",
  incompatible: "服务端过旧",
  third_party: "第三方 / 旧版",
};

function renderProbe(p) {
  lastProbe = p;
  const dot = `<span class="dot ${p.status}"></span>`;
  let extra = "";
  if (p.status === "online") extra = ` · api ${p.api_compat} · ${p.latency_ms}ms`;
  if (p.detail) extra += ` · ${p.detail}`;
  $("probe").innerHTML = `${dot}${LABEL[p.status] || p.status}${extra}`;
  $("save").disabled = !(p.status === "online" || p.status === "third_party");
}

async function test() {
  if (!invoke) { $("probe").textContent = "未运行在 Tauri 中"; return; }
  $("probe").textContent = "探测中…";
  $("save").disabled = true;
  try {
    renderProbe(await invoke("site_test", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
    }));
  } catch (e) {
    $("probe").textContent = "探测失败：" + e;
  }
}

async function save() {
  try {
    await invoke("site_add", {
      label: "", baseUrl: $("base").value,
      username: $("user").value, password: $("pass").value,
    });
    $("pass").value = "";
    $("probe").textContent = "已保存（凭证已写入系统钥匙串）";
    checkSecurity();
    invoke("subscribe_events", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
    }).catch(() => {});
    $("save").disabled = true;
    loadSites();
  } catch (e) {
    $("probe").textContent = "保存失败：" + e;
  }
}

async function loadSites() {
  if (!invoke) return;
  const sites = await invoke("site_list");
  const box = $("sites");
  if (!sites.length) { box.innerHTML = '<div class="hint">还没有站点</div>'; return; }
  box.innerHTML = "";
  for (const s of sites) {
    const row = document.createElement("div");
    row.className = "site";
    row.innerHTML = `<div><span class="dot"></span>${s.label}<br><small>${s.base_url} · ${s.username || ""}</small></div>`;
    const del = document.createElement("button");
    del.textContent = "移除";
    del.onclick = async () => { await invoke("site_remove", { id: s.id }); loadSites(); };
    row.appendChild(del);
    box.appendChild(row);
  }
}

$("test").onclick = test;
$("save").onclick = save;
$("create").onclick = async () => {
  if (!invoke) { $("createMsg").textContent = "未运行在 Tauri 中"; return; }
  $("create").disabled = true;
  try {
    const env = await invoke("env_detect");
    $("createMsg").textContent = env.detail;
    if (!env.python) { $("create").disabled = false; return; }

    const user = $("user").value.trim();
    const pass = $("pass").value;
    if (!user || pass.length < 8) {
      $("createMsg").textContent = "请先填管理员账号与密码（密码至少 8 位）";
      return;
    }
    const port = 18802;
    const token = "rc_" + crypto.randomUUID().replace(/-/g, "");
    const info = await invoke("instance_create", {
      port, token, home: "", username: user, password: pass,
    });
    $("base").value = info.base_url;
    $("probe").textContent = `已启动（pid ${info.pid}），等待就绪…`;
    await new Promise((r) => setTimeout(r, 1500));
    await test();
  } catch (e) {
    $("createMsg").textContent = "启动失败：" + e;
  } finally {
    $("create").disabled = false;
  }
};
const listen = window.__TAURI__?.event?.listen;
if (listen) {
  listen("relayhub://event", (e) => {
    let msg;
    try { msg = JSON.parse(e.payload); } catch { return; }
    const bar = document.getElementById("live");
    const notify2 = (title, body) => {
      const n = window.__TAURI__?.notification;
      if (!n) return;
      n.isPermissionGranted().then((ok) => (ok ? true : n.requestPermission()))
        .then((ok) => { if (ok) n.sendNotification({ title, body }); })
        .catch(() => {});
    };

    if (msg.type === "channel.cooldown") {
      if (bar) bar.textContent = `渠道 ${msg.id} 进入冷却（${msg.tier}）· ` + new Date().toLocaleTimeString();
      notify2("relay-hub · 渠道冷却", `渠道 ${msg.id} 进入 ${msg.tier} 冷却`);
    } else if (msg.type === "channel.recovered") {
      if (bar) bar.textContent = `渠道 ${msg.id} 已恢复 · ` + new Date().toLocaleTimeString();
      notify2("relay-hub · 渠道恢复", `渠道 ${msg.id} 已退出冷却`);
    }

    if (msg.type === "state.changed") {
      const notify = window.__TAURI__?.notification;
      if (notify) {
        notify.isPermissionGranted().then((ok) => {
          if (!ok) return notify.requestPermission();
        }).then((ok) => {
          if (ok) notify.sendNotification({ title: "relay-hub", body: "上游状态发生变化" });
        }).catch(() => {});
      }
    }
  });
}

if (!invoke) { $("probe").textContent = "未运行在 Tauri 中（浏览器预览模式）"; }
loadSites();

// ---- 插件用量 ----
let plugRows = [];

async function loadPlugins() {
  if (!invoke) return;
  const days = parseInt($("days").value, 10) || 7;
  $("plug-msg").textContent = "读取中…";
  try {
    const data = await invoke("plugin_costs", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value, days,
    });
    plugRows = data.plugins || [];
    $("plug-msg").textContent = plugRows.length ? `${plugRows.length} 个插件 · ${data.days} 天` : "没有记录";
    renderPlugins();
  } catch (e) {
    $("plug-msg").textContent = "读取失败：" + e;
  }
}

function renderPlugins() {
  const box = $("plug-out");
  if (!plugRows.length) { box.innerHTML = '<div class="hint">暂无数据</div>'; return; }
  const head = "<tr><th>插件</th><th>请求</th><th>失败</th><th>失败率</th><th>输入</th><th>输出</th><th>平均延迟</th><th>版本</th></tr>";
  const body = plugRows.map((r) => `<tr>
    <td>${r.plugin}</td><td>${r.requests}</td><td>${r.failed}</td>
    <td>${r.fail_rate}%</td><td>${r.tokens_in}</td><td>${r.tokens_out}</td>
    <td>${r.avg_latency_ms}ms</td><td>${r.version || "-"}</td></tr>`).join("");
  box.innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:13px">
    <thead>${head}</thead><tbody>${body}</tbody></table>`;
}

function exportCsv() {
  if (!plugRows.length) { $("plug-msg").textContent = "没有可导出的数据"; return; }
  const cols = ["plugin", "requests", "failed", "fail_rate", "tokens_in", "tokens_out", "avg_latency_ms", "version"];
  const lines = [cols.join(",")].concat(
    plugRows.map((r) => cols.map((c) => JSON.stringify(r[c] ?? "")).join(",")),
  );
  const blob = new Blob([lines.join(String.fromCharCode(10))], { type: "text/csv;charset=utf-8" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `plugin-usage-${$("days").value || 7}d.csv`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 2000);
}

$("plug-load").onclick = loadPlugins;
$("plug-csv").onclick = exportCsv;

// ---- 渠道（只读） ----
async function loadChannels() {
  if (!invoke) return;
  $("ch-msg").textContent = "读取中…";
  try {
    const st = await invoke("admin_get", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/state",
    });
    const keys = st.keys || st.channels || [];
    $("ch-msg").textContent =
      `${keys.length} 个渠道 · 协议 ${(st.protocols || []).length} 种 · 版本 ${st.version}`;
    renderChannels(keys);
  } catch (e) {
    $("ch-msg").textContent = "读取失败：" + e;
  }
}

function renderChannels(keys) {
  const box = $("ch-out");
  if (!keys.length) { box.innerHTML = '<div class="hint">没有渠道</div>'; return; }
  const rows = keys.map((k) => `<tr>
    <td>${k.key_id || ""}</td><td>${k.label || ""}</td>
    <td>${k.protocol || ""}</td>
    <td>${k.cooling_down_for > 0 ? "冷却 " + Math.round(k.cooling_down_for) + "s"
            : (k.enabled === false ? "已停用" : (k.available === false ? "不可用" : "正常"))}</td>
    <td>${(k.models || []).length}</td></tr>`).join("");
  box.innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:13px">
    <thead><tr><th>ID</th><th>标签</th><th>协议</th><th>状态</th><th>模型数</th></tr></thead>
    <tbody>${rows}</tbody></table>`;
}

$("ch-load").onclick = loadChannels;

// ---- 令牌（只读） ----
async function loadTokens() {
  if (!invoke) return;
  $("tk-msg").textContent = "读取中…";
  try {
    const data = await invoke("admin_get", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/tokens",
    });
    const list = data.tokens || [];
    $("tk-msg").textContent = `${list.length} 个令牌`;
    const box = $("tk-out");
    if (!list.length) { box.innerHTML = '<div class="hint">没有令牌</div>'; return; }
    box.innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:13px">
      <thead><tr><th>名称</th><th>尾号</th><th>状态</th><th>已用</th></tr></thead><tbody>${
        list.map((t) => `<tr><td>${t.name || t.token_id}</td><td>…${t.tail || ""}</td>
          <td>${t.disabled ? "停用" : "启用"}</td><td>${t.used ?? t.requests ?? "-"}</td></tr>`).join("")
      }</tbody></table>`;
  } catch (e) {
    $("tk-msg").textContent = "读取失败：" + e;
  }
}
$("tk-load").onclick = loadTokens;

// ---- 安全公告 / 版本检查 ----
function renderSecurity(d) {
  const box = document.getElementById("sec");
  if (!box) return;
  const affected = (d.advisories || []).filter((a) => a.affects_current);
  const parts = [];
  if (affected.length) {
    const a = affected[0];
    const id = a.cve || a.ghsa || "未知公告";
    parts.push(`<div style="padding:8px 12px;border-radius:8px;background:#dc2626;color:#fff">
      当前版本 ${d.current} 受 ${id} 影响（${a.severity}）· 修复版本 ${a.patched || "未知"}</div>`);
  } else if ((d.advisories || []).length) {
    parts.push(`<div style="padding:8px 12px;border-radius:8px;background:#16a34a;color:#fff">
      已知公告均不影响当前版本（${d.advisories.length} 条）</div>`);
  }
  if (d.update && d.update.behind) {
    parts.push(`<div style="padding:8px 12px;border-radius:8px;background:#d97706;color:#fff;margin-top:6px">
      有新版本 ${d.update.latest}（当前 ${d.update.current}）· 建议 pip install -U hubrelay</div>`);
  }
  if (d.error) {
    parts.push(`<div class="hint" style="margin-top:6px">部分检查失败：${d.error}</div>`);
  }
  box.innerHTML = parts.join("");
}

async function checkSecurity() {
  if (!invoke) return;
  try {
    renderSecurity(await invoke("admin_get", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/security",
    }));
  } catch (e) {
    const box = document.getElementById("sec");
    if (box) box.innerHTML = `<div class="hint">安全检查失败：${e}</div>`;
  }
}
