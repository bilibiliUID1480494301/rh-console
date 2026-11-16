const $ = (id) => document.getElementById(id);

const esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

function table(headers, rows) {
  if (!rows.length) return '<div class="hint">暂无数据</div>';
  const head = headers.map((h) =>
    `<th style="text-align:left;padding:6px 8px;border-bottom:1px solid var(--line)">${h}</th>`).join("");
  return `<table style="border-collapse:collapse;width:100%;font-size:13px">
    <thead><tr>${head}</tr></thead><tbody>${rows.join("")}</tbody></table>`;
}

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
  $("probe").innerHTML = `${dot}${esc(LABEL[p.status] || p.status)}${extra}`;
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

const health = {};   // 站点 id -> "ok" | "warn" | "bad" | ""

async function healthDots(sites) {
  for (const s of sites) {
    try {
      const p = await invoke("site_test", { baseUrl: s.base_url, username: s.username });
      // 只有「能说清状态」的才上色：认证失败是黄（能连上但进不去），连不上是红
      health[s.id] = p && p.ok ? "ok" : (p && p.status === 401 ? "warn" : "bad");
    } catch (_) {
      health[s.id] = "bad";
    }
    const row = document.querySelector(`.site[data-id="${s.id}"] .dot`);
    if (row) row.className = "dot " + (health[s.id] || "");
  }
  renderAggregate(sites);
}

// 顶部汇总灯：一眼看出「几台在线」，不用逐条数站点列表
function renderAggregate(sites) {
  const dotEl = document.getElementById("agg-dot");
  const txtEl = document.getElementById("agg-txt");
  if (!dotEl || !txtEl) return;
  if (!sites.length) {
    dotEl.className = "dot";
    txtEl.textContent = "未连接";
    return;
  }
  const states = sites.map((s) => health[s.id] || "");
  const ok = states.filter((v) => v === "ok").length;
  const bad = states.filter((v) => v === "bad").length;
  dotEl.className = "dot " + (ok === sites.length ? "ok" : bad ? "bad" : "warn");
  txtEl.textContent = `${ok}/${sites.length} 在线`;
}

// 管理面三档（scope 矩阵）：owner 全权 / admin 只读 / viewer 更少
const ROLE_LABEL = { owner: "站长", admin: "管理员(只读)", viewer: "查看者", vip: "VIP", user: "普通用户" };
const ROLE_TIP = {
  owner: "全部权限：改配置、改角色、发令牌",
  admin: "能看不能改：渠道/令牌/用量/日志只读",
  viewer: "只看用量与日志",
  vip: "计费层面的 VIP 分组",
  user: "普通用户",
};
function roleTag(role) {
  const key = String(role || "").toLowerCase();
  if (!ROLE_LABEL[key]) return ' <small class="hint">权限未知</small>';
  return ` <small class="hint" title="${ROLE_TIP[key] || ""}">[${ROLE_LABEL[key]}]</small>`;
}
function canWrite(role) { return String(role || "").toLowerCase() === "admin"; }

async function loadSites() {
  if (!invoke) return;
  const sites = await invoke("site_list");
  const box = $("sites");
  if (!sites.length) { box.innerHTML = '<div class="hint">还没有站点</div>'; return; }
  box.innerHTML = "";
  for (const s of sites) {
    const row = document.createElement("div");
    row.className = "site";
    row.dataset.id = s.id;
    row.innerHTML = `<div><span class="dot ${health[s.id] || ""}"></span>${esc(s.label)}${roleTag(s.role)}<br><small>${esc(s.base_url)} · ${esc(s.username || "")}</small></div>`;
    const perm = document.createElement("button");
    perm.textContent = "权限";
    perm.className = "ghost";
    perm.onclick = () => openPerm(s.id, s.label, s.role);
    row.appendChild(perm);
    const del = document.createElement("button");
    del.textContent = "移除";
    del.onclick = async () => { await invoke("site_remove", { id: s.id }); loadSites(); };
    row.appendChild(del);
    box.appendChild(row);
  }
  healthDots(sites);   // 后台逐个探活，不阻塞首次渲染
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
      refreshVisible();
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
    <td>${esc(k.key_id || "")}</td><td>${esc(k.label || "")}</td>
    <td>${esc(k.protocol || "")}</td>
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
        list.map((t) => `<tr><td>${esc(t.name || t.token_id)}</td><td>…${esc(t.tail || "")}</td>
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
      有新版本 ${d.update.latest}（当前 ${d.update.current}）
      <button id="up-btn" style="margin-left:10px;padding:4px 10px">升级</button>
      <span id="up-msg" style="margin-left:8px"></span></div>`);
    const btn = document.getElementById("up-btn");
    if (btn) btn.onclick = () => doUpgrade(d.update.latest);
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
    if (box) box.innerHTML = `<div class="hint">安全检查失败：${esc(e)}</div>`;
  }
}

// 状态一变就重载当前可见的卡片，不用手点刷新
function refreshVisible() {
  const loaded = (id) => {
    const el = document.getElementById(id);
    return el && el.querySelector("table");
  };
  if (loaded("ch-out")) loadChannels();
  if (loaded("tk-out")) loadTokens();
  if (loaded("plug-out")) loadPlugins();
}

// ---- 用量（结构未知，按 key/value 平铺，不猜字段名） ----
async function loadUsage() {
  if (!invoke) return;
  $("usg-msg").textContent = "读取中…";
  try {
    const data = await invoke("admin_get", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/usage",
    });
    const flat = [];
    const walk = (obj, prefix) => {
      for (const [k, v] of Object.entries(obj || {})) {
        if (v && typeof v === "object" && !Array.isArray(v)) walk(v, prefix + k + ".");
        else flat.push([prefix + k, Array.isArray(v) ? `${v.length} 项` : String(v)]);
      }
    };
    walk(data, "");
    $("usg-msg").textContent = `${flat.length} 个字段`;
    $("usg-out").innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:13px">
      <tbody>${flat.map(([k, v]) => `<tr><td style="color:#6b7280;padding-right:12px">${esc(k)}</td><td>${esc(v)}</td></tr>`).join("")}</tbody></table>`;
  } catch (e) {
    $("usg-msg").textContent = "读取失败：" + e;
  }
}

// ---- 请求日志（列取自数据本身，字段变了也不会空白） ----
async function loadRequests() {
  if (!invoke) return;
  $("log-msg").textContent = "读取中…";
  try {
    const data = await invoke("admin_get", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/requests",
    });
    const rows = data.items || data.requests || data.rows || [];
    $("log-msg").textContent = `${rows.length} 条`;
    const box = $("log-out");
    if (!rows.length) { box.innerHTML = '<div class="hint">暂无记录</div>'; return; }
    const cols = Object.keys(rows[0]).slice(0, 8);
    box.innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:12px">
      <thead><tr>${cols.map((c) => `<th style="text-align:left;padding:4px 8px">${esc(c)}</th>`).join("")}</tr></thead>
      <tbody>${rows.slice(0, 50).map((r) => `<tr>${cols.map((c) =>
        `<td style="padding:4px 8px">${esc(String(r[c] ?? ""))}</td>`).join("")}</tr>`).join("")}</tbody></table>`;
  } catch (e) {
    $("log-msg").textContent = "读取失败：" + e;
  }
}
$("usg-load").onclick = loadUsage;
$("log-load").onclick = loadRequests;

// ---- 主题（服务端只收颜色，前端不做二次校验，交了就是服务端说了算） ----
$("thm-save").onclick = async () => {
  if (!invoke) return;
  $("thm-msg").textContent = "保存中…";
  try {
    await invoke("admin_post", {
      baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
      path: "/api/theme",
      body: { primary: $("thm-primary").value.trim(), bg: $("thm-bg").value.trim() },
    });
    $("thm-msg").textContent = "已保存（非法颜色会被服务端丢弃）";
  } catch (e) {
    $("thm-msg").textContent = "保存失败：" + e;
  }
};

// ---- 一键升级（服务端验签，失败即拒；强制跳过必须写理由并进审计） ----
// 跳过通道不能删：签名密钥轮换、自建包、上游发版事故都需要它。
// 但必须是一次显式决定——要求写理由（>=8 字），服务端记 update.forced。
async function doUpgrade(version) {
  const msg = document.getElementById("up-msg");
  const btn = document.getElementById("up-btn");
  if (btn) btn.disabled = true;
  if (msg) msg.textContent = "下载并验签中…";
  const call = (extra) => invoke("admin_post", {
    baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
    path: "/api/update/install",
    body: Object.assign({ version, apply: true }, extra || {}),
  });

  try {
    const r = await call({});
    if (msg) {
      msg.textContent = r.restart_pending
        ? `已安装 ${version}，等待 keepalive 拉起新进程`
        : `已安装 ${version}（未请求重启）`;
    }
    return;
  } catch (e) {
    const text = String(e);
    if (!text.includes("409")) {
      if (msg) msg.textContent = "升级失败：" + text;
      if (btn) btn.disabled = false;
      return;
    }
    if (msg) msg.textContent = "验签未通过";
    const NL = String.fromCharCode(10);
    const reason = window.prompt(
      "验签未通过：" + text.slice(0, 160) + NL + NL +
      "确知风险、仍要安装请输入原因（至少 8 字，会写进 update.forced 审计）：");
    if (!reason) { if (btn) btn.disabled = false; return; }
    if (reason.trim().length < 8) {
      if (msg) msg.textContent = "原因太短（至少 8 字），已取消";
      if (btn) btn.disabled = false;
      return;
    }
    try {
      await call({ force: true, reason: reason.trim() });
      if (msg) msg.textContent = `已强制安装 ${version}（未验签，已记审计）`;
    } catch (e2) {
      if (msg) msg.textContent = "强制安装也失败：" + e2;
      if (btn) btn.disabled = false;
    }
  }
}

// ---- 本机自检 ----
$("doc-run").onclick = async () => {
  if (!invoke) return;
  $("doc-msg").textContent = "运行中…";
  try {
    const checks = await invoke("doctor_local");
    const list = Array.isArray(checks) ? checks : (checks.checks || []);
    const bad = list.filter((c) => !c.ok);
    $("doc-msg").textContent = bad.length ? `${bad.length} 项未通过` : "全部通过";
    $("doc-out").innerHTML = `<table style="border-collapse:collapse;width:100%;font-size:13px">
      <tbody>${list.map((c) => `<tr>
        <td style="width:22px">${c.ok ? "✓" : "✗"}</td>
        <td style="width:150px">${c.name || ""}</td>
        <td>${c.detail || ""}${c.fix && !c.ok ? `<br><small style="color:#b45309">修复：${c.fix}</small>` : ""}</td>
      </tr>`).join("")}</tbody></table>`;
  } catch (e) {
    $("doc-msg").textContent = "自检失败：" + e;
  }
};

// ---- 中英切换 ----
const LANG_DICT = {
  "站点": "Sites", "添加站点": "Add site", "地址": "Address", "用户名": "Username",
  "密码": "Password", "保存": "Save", "测试连接": "Test", "移除": "Remove",
  "创建本地实例": "Create local station", "一键升级": "Upgrade", "远程部署": "Deploy remotely",
  "本机自检": "Local self-check", "渠道": "Channels", "令牌": "Tokens", "用量": "Usage",
  "日志": "Logs", "主题": "Theme", "服务器状态": "Server status", "运行自检": "Run check",
  "还没有站点": "No sites yet", "导出迁移包": "Export bundle",
};
let enUI = localStorage.getItem("rhc_lang") === "en";
function walkLang(node) {
  if (node.nodeType === 3) {
    const t = node.textContent.trim();
    if (LANG_DICT[t] !== undefined) {
      if (node.__zh === undefined) node.__zh = node.textContent;
      node.textContent = enUI ? LANG_DICT[t] : node.__zh;
    }
    return;
  }
  if (node.nodeType !== 1) return;
  if (node.nodeName === "SCRIPT" || node.nodeName === "STYLE") return;
  for (const c of node.childNodes) walkLang(c);
}
const langBtn = document.createElement("button");
langBtn.className = "ghost";
langBtn.textContent = enUI ? "中文" : "EN";
langBtn.onclick = () => {
  enUI = !enUI;
  localStorage.setItem("rhc_lang", enUI ? "en" : "zh");
  langBtn.textContent = enUI ? "中文" : "EN";
  walkLang(document.body);
};
(document.querySelector("header") || document.body).appendChild(langBtn);
if (enUI) walkLang(document.body);

// ---- 多站点总览 ----
function ovEsc(v) {
  return String(v == null ? "" : v).replace(/[&<>"']/g, c => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

async function reloadOverview() {
  const msg = document.getElementById("ov-msg");
  const box = document.getElementById("ov");
  if (!msg || !box) return;
  let sites = [];
  try { sites = await invoke("site_list"); } catch (e) { msg.textContent = String(e); return; }
  if (!sites.length) { box.innerHTML = '<p class="hint">先在上面保存一个站点</p>'; return; }
  msg.textContent = "拉取中…";
  try {
    const rows = await invoke("overview_all", {
      sites: sites.map(s => ({ id: s.id, label: s.label, base_url: s.base_url, username: s.username })),
      days: 1,
    });
    const sum = rows.reduce((a, r) => ({
      requests: a.requests + r.requests,
      failed: a.failed + r.failed,
      tokens_in: a.tokens_in + r.tokens_in,
      tokens_out: a.tokens_out + r.tokens_out,
    }), { requests: 0, failed: 0, tokens_in: 0, tokens_out: 0 });
    const body = rows.map(r => `<tr>
      <td>${ovEsc(r.label)}</td>
      <td>${r.ok ? "在线" : ovEsc(r.status)}</td>
      <td>${r.requests}</td>
      <td>${r.failed}</td>
      <td>${r.tokens_in}&rarr;${r.tokens_out}</td>
      <td>${r.latency_ms} ms</td>
      <td class="hint">${ovEsc(r.detail || "")}</td>
    </tr>`).join("");
    box.innerHTML = `<table><thead><tr>
      <th>站点</th><th>状态</th><th>请求(24h)</th><th>失败</th><th>tokens</th><th>延迟</th><th></th>
      </tr></thead><tbody>${body}</tbody>
      <tfoot><tr><th>合计 ${rows.length} 个站</th><th></th>
      <th>${sum.requests}</th><th>${sum.failed}</th>
      <th>${sum.tokens_in}&rarr;${sum.tokens_out}</th><th></th><th></th></tr></tfoot></table>`;
    msg.textContent = "已更新";
  } catch (e) {
    msg.textContent = "失败：" + String(e);
  }
}

const ovBtn = document.getElementById("ov-run");
if (ovBtn) ovBtn.onclick = reloadOverview;


// ---- 权限分配 ----
const ROLE_ORDER = ["user", "vip", "admin"];

async function openPerm(siteId, label, myRole) {
  const title = document.getElementById("perm-title");
  const box = document.getElementById("perm-body");
  if (!box) return;
  if (title) title.textContent = label || "";
  box.innerHTML = '<p class="hint">读取中…</p>';
  try {
    const rows = await invoke("site_users", { siteId });
    if (!rows.length) { box.innerHTML = '<p class="hint">这个站没有用户</p>'; return; }
    const canGrantAdmin = String(myRole || "").toLowerCase() === "owner";
    const head = `<tr><th>用户名</th><th>角色</th><th>启用</th><th>额度</th><th>已用</th></tr>`;
    const body = rows.map(u => {
      const opts = ROLE_ORDER.map(r =>
        `<option value="${r}"${r === u.role ? " selected" : ""}>${
          ({ user: "普通", vip: "VIP", admin: "管理员" })[r]}</option>`).join("");
      return `<tr data-uid="${ovEsc(u.user_id)}">
        <td>${ovEsc(u.username)}</td>
        <td><select ${canGrantAdmin ? "" : "data-noadmin=1"}>${opts}</select></td>
        <td>${u.enabled ? "是" : '<span class="hint">停用</span>'}</td>
        <td>${u.quota < 0 ? "无限" : u.quota}</td>
        <td>${u.used}</td>
      </tr>`;
    }).join("");
    box.innerHTML = `<table>${head}${body}</table>`;
    box.querySelectorAll("select").forEach(sel => {
      sel.onchange = async () => {
        const uid = sel.closest("tr").dataset.uid;
        const role = sel.value;
        if (role === "admin" && sel.dataset.noadmin) {
          sel.value = sel.dataset.prev || "user";
          return;
        }
        sel.disabled = true;
        try {
          await invoke("site_set_role", { siteId, userId: uid, role });
          sel.dataset.prev = role;
        } catch (e) {
          alert("改不了：" + e);
          sel.value = sel.dataset.prev || "user";
        } finally { sel.disabled = false; }
      };
      sel.dataset.prev = sel.value;
    });
  } catch (e) {
    box.innerHTML = `<p class="hint">${ovEsc(String(e))}</p>`;
  }
}


// ---- 远程部署 ----
async function runDeploy() {
  const msg = document.getElementById("dep-msg");
  const out = document.getElementById("dep-out");
  const btn = document.getElementById("dep-run");
  const mode = (document.querySelector('input[name="depmode"]:checked') || {}).value || "pip";
  const host = document.getElementById("dep-host").value.trim();
  const key = document.getElementById("dep-key").value.trim();
  const version = document.getElementById("dep-ver").value.trim();
  const dataDir = document.getElementById("dep-dir").value.trim();
  if (!host || !key || !version) { msg.textContent = "主机 / 私钥 / 版本都要填"; return; }
  btn.disabled = true;
  msg.textContent = "验签并部署中…（会先下载校验，别关窗口）";
  out.textContent = "";
  try {
    const r = await invoke("remote_deploy", {
      host, keyPath: key, version, python: "", mode, dataDir,
    });
    msg.textContent = "成功";
    out.textContent = r;
  } catch (e) {
    msg.textContent = "失败";
    out.textContent = String(e);
  } finally { btn.disabled = false; }
}
const depBtn = document.getElementById("dep-run");
if (depBtn) depBtn.onclick = runDeploy;

// ---- 接入策略 / 兑换码 / 用户 / 客户端 / 审计 ----
async function adminCall(method, path, body) {
  return invoke("admin_call", {
    baseUrl: $("base").value, username: $("user").value, password: $("pass").value,
    method, path, body: body ?? null,
  });
}

// 策略：拉黑立即生效，优先队列让指定来源排在普通请求之前
async function loadPolicy() {
  if (!invoke) return;
  $("pol-msg").textContent = "读取中…";
  try {
    renderPolicy(await adminCall("GET", "/api/policy"));
  } catch (e) { $("pol-msg").textContent = "读取失败：" + e; }
}

function renderPolicy(state) {
  const boxes = state || {};
  const rows = [];
  for (const kind of ["ips", "devices"]) {
    for (const [value, entry] of Object.entries(boxes[kind] || {})) {
      const tag = entry.blocked ? '<span style="color:#d0483c">拉黑</span>'
        : entry.priority ? '<span style="color:#2e9e6b">优先</span>' : "-";
      rows.push(`<tr>
        <td style="padding:6px 8px">${kind === "ips" ? "IP" : "身份"}</td>
        <td style="padding:6px 8px">${esc(value)}</td>
        <td style="padding:6px 8px">${tag}</td>
        <td style="padding:6px 8px">${esc(entry.note || "")}</td>
        <td style="padding:6px 8px"><button class="q" data-pol-remove="${esc(value)}" data-pol-kind="${kind === "ips" ? "ip" : "device"}">移除</button></td>
      </tr>`);
    }
  }
  $("pol-out").innerHTML = table(["类型", "值", "状态", "备注", ""], rows);
  $("pol-msg").textContent = rows.length ? `${rows.length} 条策略` : "没有策略（默认全部放行）";
}

$("pol-load").onclick = loadPolicy;
$("pol-add").onclick = async () => {
  const value = $("pol-value").value.trim();
  if (!value) { $("pol-msg").textContent = "值不能为空"; return; }
  const act = $("pol-act").value;
  const body = { kind: $("pol-kind").value, value, action: "set" };
  body[act] = true;                       // blocked / priority，互斥由后端裁决
  if ($("pol-note").value.trim()) body.note = $("pol-note").value.trim();
  $("pol-msg").textContent = "写入中…";
  try {
    renderPolicy(await adminCall("POST", "/api/policy", body));
    $("pol-value").value = "";
    $("pol-note").value = "";
    $("pol-msg").textContent = "已写入，下一请求即生效";
  } catch (e) { $("pol-msg").textContent = "写入失败：" + e; }
};
$("pol-out").addEventListener("click", async (ev) => {
  const btn = ev.target.closest("[data-pol-remove]");
  if (!btn) return;
  try {
    renderPolicy(await adminCall("POST", "/api/policy", {
      kind: btn.dataset.polKind, value: btn.dataset.polRemove, action: "remove",
    }));
    $("pol-msg").textContent = "已移除";
  } catch (e) { $("pol-msg").textContent = "移除失败：" + e; }
});

// 兑换码：明文只在生成时回显一次
async function loadCodes() {
  if (!invoke) return;
  $("code-msg").textContent = "读取中…";
  try {
    const data = await adminCall("GET", "/api/codes");
    const codes = data.codes || [];
    const rows = codes.map((c) => `<tr>
      <td style="padding:6px 8px;font-family:ui-monospace,monospace">${esc(c.code)}</td>
      <td style="padding:6px 8px">${c.credits}</td>
      <td style="padding:6px 8px">${c.used_by ? "已用 · " + esc(c.used_by) : "未用"}</td>
      <td style="padding:6px 8px">${c.created_at ? new Date(c.created_at * 1000).toLocaleString() : "-"}</td>
      <td style="padding:6px 8px">${c.used_by ? "" : `<button class="q" data-code-del="${esc(c.code)}">作废</button>`}</td>
    </tr>`);
    $("code-out").innerHTML = table(["兑换码", "额度", "状态", "创建时间", ""], rows);
    $("code-msg").textContent = `${codes.length} 个`;
  } catch (e) { $("code-msg").textContent = "读取失败：" + e; }
}

$("code-load").onclick = loadCodes;
$("code-gen").onclick = async () => {
  const count = parseInt($("code-count").value, 10) || 1;
  const credits = parseInt($("code-credits").value, 10) || 100;
  $("code-msg").textContent = "生成中…";
  try {
    const data = await adminCall("POST", "/api/codes", { count, credits });
    const fresh = (data.codes || []).map((c) => c.code).join("\n");
    $("code-new").textContent = fresh ? "本次生成（只显示一次）：\n" + fresh : "没有生成";
    $("code-msg").textContent = `已生成 ${(data.codes || []).length} 个`;
    await loadCodes();
  } catch (e) { $("code-msg").textContent = "生成失败：" + e; }
};
$("code-del-unused").onclick = async () => {
  if (!confirm("作废全部未使用的兑换码？不可恢复")) return;
  try {
    const r = await adminCall("POST", "/api/codes/destroy", { codes: [], all_unused: true });
    $("code-msg").textContent = `已作废 ${r.removed} 个`;
    await loadCodes();
  } catch (e) { $("code-msg").textContent = "作废失败：" + e; }
};
$("code-out").addEventListener("click", async (ev) => {
  const btn = ev.target.closest("[data-code-del]");
  if (!btn) return;
  try {
    await adminCall("POST", "/api/codes/destroy", { codes: [btn.dataset.codeDel] });
    await loadCodes();
  } catch (e) { $("code-msg").textContent = "作废失败：" + e; }
});

// 用户：额度 / 停用 / 删除
async function loadUsers() {
  if (!invoke) return;
  $("usr-msg").textContent = "读取中…";
  try {
    const data = await adminCall("GET", "/api/users");
    const users = data.users || [];
    const rows = users.map((u) => `<tr>
      <td style="padding:6px 8px">${esc(u.username)}</td>
      <td style="padding:6px 8px">${esc(u.role)}</td>
      <td style="padding:6px 8px">${u.quota < 0 ? "不限" : u.quota}</td>
      <td style="padding:6px 8px">${u.used ?? 0}</td>
      <td style="padding:6px 8px">${u.enabled ? "启用" : '<span style="color:#d0483c">停用</span>'}</td>
      <td style="padding:6px 8px">${esc(u.group || "-")}</td>
      <td style="padding:6px 8px">
        <button class="q" data-usr-quota="${esc(u.username)}">改额度</button>
        <button class="q" data-usr-toggle="${esc(u.username)}" data-usr-enabled="${u.enabled ? 1 : 0}">${u.enabled ? "停用" : "启用"}</button>
        <button class="q" data-usr-del="${esc(u.username)}">删除</button>
      </td></tr>`);
    $("usr-out").innerHTML = table(["用户", "角色", "额度", "已用", "状态", "分组", "操作"], rows);
    $("usr-msg").textContent =
      `${users.length} 个用户 · 允许注册：${data.allow_register ? "是" : "否"}`;
  } catch (e) { $("usr-msg").textContent = "读取失败：" + e; }
}

$("usr-load").onclick = loadUsers;
$("usr-out").addEventListener("click", async (ev) => {
  const q = ev.target.closest("[data-usr-quota]");
  const t = ev.target.closest("[data-usr-toggle]");
  const d = ev.target.closest("[data-usr-del]");
  try {
    if (q) {
      const raw = prompt(`给 ${q.dataset.usrQuota} 设置额度（-1 = 不限）`, "");
      if (raw === null) return;
      const quota = parseInt(raw, 10);
      if (Number.isNaN(quota)) { $("usr-msg").textContent = "额度必须是整数"; return; }
      await adminCall("PUT", `/api/users/${encodeURIComponent(q.dataset.usrQuota)}`, { quota });
    } else if (t) {
      await adminCall("PUT", `/api/users/${encodeURIComponent(t.dataset.usrToggle)}`,
        { enabled: t.dataset.usrEnabled !== "1" });
    } else if (d) {
      if (!confirm(`删除用户 ${d.dataset.usrDel}？不可恢复`)) return;
      await adminCall("DELETE", `/api/users/${encodeURIComponent(d.dataset.usrDel)}`, null);
    } else return;
    await loadUsers();
  } catch (e) { $("usr-msg").textContent = "操作失败：" + e; }
});

// 客户端总览：按用户/设备聚合；刻意不展示下游用户的客户端名（隐私口径：粒度到用户）
async function loadClients() {
  if (!invoke) return;
  $("cl-msg").textContent = "读取中…";
  try {
    const data = await adminCall("GET", "/api/clients");
    const list = data.clients || [];
    const rows = list.map((c) => `<tr>
      <td style="padding:6px 8px">${c.source === "app" ? "自定义 App" : "直连 API"}</td>
      <td style="padding:6px 8px">${esc(c.user || "-")}</td>
      <td style="padding:6px 8px">${esc(c.device || "-")}</td>
      <td style="padding:6px 8px">${esc(c.ver || "-")}</td>
      <td style="padding:6px 8px">${esc(c.token || "-")}</td>
      <td style="padding:6px 8px">${c.today}</td>
      <td style="padding:6px 8px">${c.failed}</td>
      <td style="padding:6px 8px">${c.online ? '<span style="color:#2e9e6b">在线</span>' : "离线"}</td>
    </tr>`);
    $("cl-out").innerHTML = table(
      ["来源", "用户", "设备", "版本", "令牌", "今日请求", "失败", "状态"], rows);
    const s = data.summary || {};
    $("cl-msg").textContent =
      `App 设备 ${s.app_devices ?? 0} · 直连来源 ${s.api_sources ?? 0}` +
      ` · 在线 ${s.online ?? 0} · 今日 ${s.today_requests ?? 0} 次`;
  } catch (e) { $("cl-msg").textContent = "读取失败：" + e; }
}
$("cl-load").onclick = loadClients;

// 审计：管理面写操作留痕
async function loadAudit() {
  if (!invoke) return;
  $("aud-msg").textContent = "读取中…";
  try {
    const limit = parseInt($("aud-limit").value, 10) || 100;
    const data = await adminCall("GET", `/api/audit?limit=${limit}`);
    const entries = (data.entries || []).slice().reverse();
    const rows = entries.map((e) => {
      const detail = Object.entries(e)
        .filter(([k]) => !["ts", "at", "event", "action"].includes(k))
        .map(([k, v]) => `${k}=${v}`).join(" · ");
      return `<tr>
        <td style="padding:6px 8px;white-space:nowrap">${e.ts ? new Date(e.ts * 1000).toLocaleString() : "-"}</td>
        <td style="padding:6px 8px">${esc(e.event || e.action || "-")}</td>
        <td style="padding:6px 8px;word-break:break-all">${esc(detail)}</td>
      </tr>`;
    });
    $("aud-out").innerHTML = table(["时间", "事件", "详情"], rows);
    $("aud-msg").textContent = `${entries.length} 条`;
  } catch (e) { $("aud-msg").textContent = "读取失败：" + e; }
}
$("aud-load").onclick = loadAudit;

// ---- 公告 / 特性开关 / 诊断 ----
async function loadAnnouncement() {
  if (!invoke) return;
  try {
    const a = await adminCall("GET", "/api/announcement");
    $("ann-text").value = a.text || "";
    $("ann-level").value = a.level || "info";
    $("ann-pinned").checked = !!a.pinned;
    const left = a.until ? Math.max(0, Math.round((a.until - Date.now() / 1000) / 3600)) : 0;
    $("ann-hours").value = left;
    $("ann-msg").textContent = a.text ? (a.until ? `生效中 · 剩余约 ${left} 小时` : "生效中 · 长期") : "当前没有公告";
  } catch (e) { $("ann-msg").textContent = "读取失败：" + e; }
}

async function saveAnnouncement(text, hours) {
  const until = hours > 0 ? Math.floor(Date.now() / 1000) + hours * 3600 : 0;
  try {
    const a = await adminCall("POST", "/api/announcement", {
      text, level: $("ann-level").value, until, pinned: $("ann-pinned").checked,
    });
    $("ann-msg").textContent = a.text ? "已发布" : "已撤销";
    await loadAnnouncement();
  } catch (e) { $("ann-msg").textContent = "发布失败：" + e; }
}

$("ann-save").onclick = () => saveAnnouncement($("ann-text").value.trim(),
  parseInt($("ann-hours").value, 10) || 0);
$("ann-clear").onclick = () => { $("ann-text").value = ""; return saveAnnouncement("", 0); };

// 特性开关：按后端返回的键动态渲染，避免硬编码漏掉新增项
async function loadFeatures() {
  if (!invoke) return;
  $("ftr-msg").textContent = "读取中…";
  try {
    const values = await adminCall("GET", "/api/features");
    renderFeatures(values);
  } catch (e) { $("ftr-msg").textContent = "读取失败：" + e; }
}

function renderFeatures(values) {
  const keys = Object.keys(values || {});
  if (!keys.length) { $("ftr-out").innerHTML = '<div class="hint">没有开关</div>'; return; }
  $("ftr-out").innerHTML = keys.map((k) => `
    <label style="display:flex;align-items:center;gap:8px;margin:8px 0;font-size:13px;color:inherit">
      <input type="checkbox" data-ftr="${esc(k)}" ${values[k] ? "checked" : ""} style="width:auto" />
      <code>${esc(k)}</code>
    </label>`).join("");
  $("ftr-msg").textContent = `${keys.length} 项`;
}

$("ftr-load").onclick = loadFeatures;
$("ftr-out").addEventListener("change", async (ev) => {
  const box = ev.target.closest("[data-ftr]");
  if (!box) return;
  try {
    renderFeatures(await adminCall("POST", "/api/features", { [box.dataset.ftr]: box.checked }));
    $("ftr-msg").textContent = "已保存";
  } catch (e) { $("ftr-msg").textContent = "保存失败：" + e; }
});

// 诊断：指纹扫描（比对模型清单变动）
$("fp-run").onclick = async () => {
  if (!invoke) return;
  $("fp-msg").textContent = "扫描中（逐个渠道拉模型清单）…";
  try {
    const r = await adminCall("POST", "/api/fingerprint", {});
    const report = r.results || [];
    const rows = (Array.isArray(report) ? report : []).map((c) => {
      const changed = (c.added || []).length + (c.removed || []).length + (c.changed || []).length;
      return `<tr>
        <td style="padding:6px 8px">${esc(c.key_id || "-")}</td>
        <td style="padding:6px 8px">${c.ok ? "正常" : '<span style="color:#d0483c">失败</span>'}</td>
        <td style="padding:6px 8px">${(c.count ?? "-")}</td>
        <td style="padding:6px 8px">${changed ? '<span style="color:#d4930d">有变动</span>' : "无变动"}</td>
        <td style="padding:6px 8px">${esc(c.error || "")}</td>
      </tr>`;
    });
    $("fp-out").innerHTML = rows.length
      ? table(["渠道", "状态", "模型数", "变动", "错误"], rows)
      : `<div class="hint">${esc(JSON.stringify(r).slice(0, 300))}</div>`;
    $("fp-msg").textContent = "扫描完成";
  } catch (e) { $("fp-msg").textContent = "扫描失败：" + e; }
};

// 诊断：服务商探活
$("pb-run").onclick = async () => {
  if (!invoke) return;
  const provider = $("pb-provider").value.trim();
  const apiKey = $("pb-key").value.trim();
  if (!provider || !apiKey) { $("pb-msg").textContent = "服务商和 Key 都要填"; return; }
  $("pb-msg").textContent = "探测中…";
  $("pb-out").textContent = "";
  try {
    const r = await adminCall("POST", "/api/probe", {
      provider, api_key: apiKey, base_url: $("pb-base").value.trim(),
    });
    $("pb-msg").textContent = r.ok === false ? "探测失败" : "探测完成";
    $("pb-out").textContent = JSON.stringify(r, null, 2);
  } catch (e) { $("pb-msg").textContent = "探测失败：" + e; }
};

// ---- 页签分组：按 data-group 把卡片收进五个面板 ----
const GROUP_TABS = [
  ["conn", "连接与部署"],
  ["ops", "运营"],
  ["bill", "计费"],
  ["ch", "渠道与流量"],
  ["sys", "系统"],
];

function buildTabs() {
  const main = document.querySelector("main");
  const nav = document.getElementById("tabs");
  if (!main || !nav) return;
  const aside = main.querySelector("aside");
  const cards = Array.from(main.querySelectorAll(":scope > section.card"));
  const panes = {};
  for (const [key, label] of GROUP_TABS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = label;
    btn.setAttribute("role", "tab");
    btn.dataset.tabK = key;
    btn.onclick = () => select(key);
    nav.appendChild(btn);
    const pane = document.createElement("div");
    pane.className = "pane";
    pane.dataset.pane = key;
    main.insertBefore(pane, aside);
    panes[key] = pane;
  }
  for (const card of cards) {
    const key = card.dataset.group || "conn";
    (panes[key] || panes.conn).appendChild(card);
  }
  function select(key) {
    for (const [k] of GROUP_TABS) {
      panes[k].hidden = k !== key;
      const btn = nav.querySelector(`[data-tab-k="${k}"]`);
      if (btn) btn.setAttribute("aria-selected", String(k === key));
    }
  }
  select("conn");
}

buildTabs();

// ---- 余额预警 ----
async function loadOversell() {
  if (!invoke) return;
  $("ovs-msg").textContent = "计算中…";
  try {
    const d = await adminCall("GET", "/api/oversell");
    const up = d.upstream || {};
    const down = d.downstream || {};
    const rows = [
      ["下游已售出余额", down.balance ?? "-"],
      ["上游余额", up.balance ?? "未知"],
      ["超卖", (down.balance ?? 0) - (up.balance ?? 0)],
      ["日均消耗", d.per_day ?? "-"],
      ["上游预计耗尽（天）", up.days ?? "-"],
      ["预警阈值（天）", d.warn_days ?? "-"],
    ];
    $("ovs-out").innerHTML = table(["指标", "值"],
      rows.map(([k, v]) => `<tr><td style="padding:6px 8px">${esc(k)}</td><td style="padding:6px 8px">${esc(v)}</td></tr>`));
    $("ovs-msg").textContent = d.note || "";
  } catch (e) { $("ovs-msg").textContent = "读取失败：" + e; }
}
$("ovs-load").onclick = loadOversell;

// ---- 网页外观（背景色/背景图/模式）----
async function loadWebui() {
  if (!invoke) return;
  $("wui-msg").textContent = "读取中…";
  try {
    const d = await adminCall("GET", "/api/webui");
    $("wui-color").value = d.bg_color || "";
    $("wui-image").value = d.bg_image || "";
    $("wui-mode").value = d.bg_mode || "static";
    $("wui-msg").textContent = "已读取";
  } catch (e) { $("wui-msg").textContent = "读取失败：" + e; }
}
$("wui-load").onclick = loadWebui;
$("wui-save").onclick = async () => {
  $("wui-msg").textContent = "保存中…";
  try {
    await adminCall("POST", "/api/webui", {
      bg_color: $("wui-color").value.trim(),
      bg_image: $("wui-image").value.trim(),
      bg_mode: $("wui-mode").value,
    });
    $("wui-msg").textContent = "已保存";
  } catch (e) { $("wui-msg").textContent = "保存失败：" + e; }
};

// ---- 站点名称 ----
$("set-save").onclick = async () => {
  const name = $("set-name").value.trim();
  if (!name) { $("set-msg").textContent = "名称不能为空"; return; }
  $("set-msg").textContent = "保存中…";
  try {
    await adminCall("PUT", "/api/settings", { site_name: name });
    $("set-msg").textContent = "已改名";
  } catch (e) { $("set-msg").textContent = "保存失败：" + e; }
};

// ---- 迁移包：导出下载 / 预演导入 ----
$("mig-export").onclick = async () => {
  const password = $("mig-pass").value;
  if (!password) { $("mig-msg").textContent = "导出口令必填"; return; }
  $("mig-msg").textContent = "导出中…";
  try {
    const bundle = await adminCall("POST", "/api/export", {
      password, include_secrets: $("mig-secrets").checked,
    });
    const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `relayhub-bundle-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    $("mig-msg").textContent = "已导出（含密钥的包不能在线导入）";
  } catch (e) { $("mig-msg").textContent = "导出失败：" + e; }
};
$("mig-dry").onclick = async () => {
  const raw = $("mig-json").value.trim();
  if (!raw) { $("mig-dry-msg").textContent = "先粘贴迁移包"; return; }
  let bundle;
  try { bundle = JSON.parse(raw); } catch (e) { $("mig-dry-msg").textContent = "不是合法 JSON"; return; }
  $("mig-dry-msg").textContent = "预演中…";
  try {
    const r = await adminCall("POST", "/api/import", { bundle, dry_run: true });
    $("mig-dry-msg").textContent =
      `预演：用户 +${r.users_added ?? 0} / 跳过 ${r.users_skipped ?? 0}，令牌 +${r.tokens_added ?? 0}`;
  } catch (e) { $("mig-dry-msg").textContent = "预演失败：" + e; }
};

// ---- 恢复码 ----
$("rec-load").onclick = async () => {
  if (!invoke) return;
  try {
    const d = await adminCall("GET", "/api/totp/recovery");
    $("rec-msg").textContent = `剩余 ${d.remaining} 个可用恢复码`;
  } catch (e) { $("rec-msg").textContent = "读取失败：" + e; }
};
$("rec-gen").onclick = async () => {
  if (!confirm("重新生成会让旧恢复码全部作废，继续？")) return;
  try {
    const d = await adminCall("POST", "/api/totp/recovery", {});
    $("rec-out").textContent = (d.codes || []).join("\n");
    $("rec-msg").textContent = d.note || "已生成（只显示这一次）";
  } catch (e) { $("rec-msg").textContent = "生成失败：" + e; }
};

// ---- 漂移与探活记录 ----
$("drf-load").onclick = async () => {
  if (!invoke) return;
  $("drf-msg").textContent = "读取中…";
  try {
    const drift = await adminCall("GET", "/api/drift");
    const probes = await adminCall("GET", "/api/probes");
    const rows = [];
    for (const [k, v] of Object.entries(drift.summary || {})) {
      rows.push(`<tr><td style="padding:6px 8px">漂移 · ${esc(k)}</td>
        <td style="padding:6px 8px">${esc(typeof v === "object" ? JSON.stringify(v) : v)}</td></tr>`);
    }
    for (const [k, v] of Object.entries(probes.summary || {})) {
      rows.push(`<tr><td style="padding:6px 8px">探活 · ${esc(k)}</td>
        <td style="padding:6px 8px">${esc(typeof v === "object" ? JSON.stringify(v) : v)}</td></tr>`);
    }
    $("drf-out").innerHTML = rows.length
      ? table(["项", "值"], rows)
      : '<div class="hint">还没有漂移或探活记录</div>';
    $("drf-msg").textContent = drift.note || probes.disclaimer || "";
  } catch (e) { $("drf-msg").textContent = "读取失败：" + e; }
};

// ---- 回滚 ----
$("upd-rollback").onclick = async () => {
  if (!confirm("回滚到上一版？服务会被重装并可能短暂中断")) return;
  $("upd-msg").textContent = "回滚中…";
  try {
    const r = await adminCall("POST", "/api/update/rollback", {});
    $("upd-msg").textContent = "已回滚" + (r && r.version ? ` 到 ${r.version}` : "");
  } catch (e) { $("upd-msg").textContent = "回滚失败：" + e; }
};

// ---- 远程命令（SSH）----
// 预设只传 id，命令串在 Rust 侧（SSH_PRESETS）；自由命令要过确认。
const SSH_PRESETS_UI = [
  ["version", "版本"],
  ["env", "环境自检"],
  ["probe", "健康探测"],
  ["ports", "监听端口"],
  ["disk", "磁盘与内存"],
  ["logs", "最近日志"],
  ["restart", "重启服务"],
];

function sshTarget() {
  const host = $("ssh-host").value.trim();
  const keyPath = $("ssh-key").value.trim();
  if (!host || !keyPath) { $("ssh-msg").textContent = "主机和私钥都要填"; return null; }
  return { host, keyPath };
}

function showSshResult(r) {
  const parts = [];
  parts.push(`退出码 ${r.exit} · ${r.ms}ms`);
  if (r.stdout) parts.push(r.stdout.trimEnd());
  if (r.stderr) parts.push("[stderr]\n" + r.stderr.trimEnd());
  $("ssh-out").textContent = parts.join("\n\n");
  $("ssh-msg").textContent = r.ok ? "完成" : "命令返回非 0";
}

async function runPreset(id, label) {
  const target = sshTarget();
  if (!target) return;
  if (id === "restart" && !confirm(`在 ${target.host} 上重启 relay-hub 服务？`)) return;
  $("ssh-msg").textContent = `${label}…`;
  $("ssh-out").textContent = "";
  try {
    showSshResult(await invoke("ssh_preset", { ...target, preset: id }));
  } catch (e) { $("ssh-msg").textContent = "失败：" + e; }
}

(function buildSshPresets() {
  const box = $("ssh-presets");
  if (!box) return;
  for (const [id, label] of SSH_PRESETS_UI) {
    const btn = document.createElement("button");
    btn.textContent = label;
    btn.onclick = () => runPreset(id, label);
    box.appendChild(btn);
  }
})();

$("ssh-adv").onchange = () => {
  const on = $("ssh-adv").checked;
  $("ssh-cmd").disabled = !on;
  $("ssh-run").disabled = !on;
  $("ssh-msg").textContent = on ? "高级模式：命令会原样交给远端 shell" : "";
};

$("ssh-run").onclick = async () => {
  const target = sshTarget();
  if (!target) return;
  const command = $("ssh-cmd").value.trim();
  if (!command) { $("ssh-msg").textContent = "命令不能为空"; return; }
  if (!confirm(`在 ${target.host} 上执行：\n\n${command}`)) return;
  $("ssh-msg").textContent = "执行中…";
  $("ssh-out").textContent = "";
  try {
    showSshResult(await invoke("ssh_run", { ...target, command }));
  } catch (e) { $("ssh-msg").textContent = "失败：" + e; }
};
