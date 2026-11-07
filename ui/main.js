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
    if (msg.type === "state.changed") {
      const bar = document.getElementById("live");
      if (bar) bar.textContent = "服务端状态已变化 · " + new Date().toLocaleTimeString();
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
