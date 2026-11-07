# relay-hub Console — 设计稿 v0.1

> 结论先说：**登录页 / 权限 / 版本可见性 / 公告可见性 这四块，客户端全部只能做“体验层”，安全边界必须在 relay-hub 服务端。**
> 因此顺序是：先补服务端契约（0.10.0），再写客户端登录页。先画 UI 会返工。

## 一、契约差距（必须先补的服务端改动）

| 你的需求 | relay-hub 现状（0.9.15） | 需要补 |
|---|---|---|
| 版本号仅 Owner 可见 | `/api/state` 对任何管理员返回版本 | 按角色判定，非 Owner 返回 `"version": "hidden"` |
| 版本 ≠ 兼容性 | 只有 `server_version` | 新增 `api_compat`（如 `2026-11`），所有已认证用户可读 |
| 公告需认证（未认证 401） | 0.9.15 我刚把 `GET /api/announcement` 做成**公开** | 拆两个：`/api/announcement`（需认证）+ `/public/announcements`（只列显式公开项） |
| 三层角色 + 细粒度权限 | 只有 `admin` / `normal` 两档 | 用户加 `role` 与 `scopes[]`；令牌带 `scopes` |
| 授权/写操作审计 | `audit.jsonl` 已记录部分事件 | 补 `role.grant` / `channel.update` / `token.issue` / `config.export` |
| 日志脱敏 | 请求日志只记元数据，Key 显尾 4 位 | 复用现有逻辑，新增 `logs:read_raw` 才给原文 |

**这六项不补，客户端写了也是假安全**——UI 藏按钮挡不住 curl。

## 二、RBAC 服务端设计（建议）

角色：

- `owner`：隐式拥有全部 scope，不可被降级（只能转让）。
- `admin`：`scopes` 从 owner 逐项授予，默认空。
- `viewer`：固定 `usage:read` + `logs:read`（脱敏）。

scope 命名（资源:动作）：

```
channel:read  channel:write  channel:reset_cooldown
token:read    token:create   token:revoke
usage:read
logs:read     logs:read_raw
site:version
role:grant
config:export
```

落点：

- `users.json` 加 `role`、`scopes`；`tokens.json` 的令牌继承所属用户的 scope（可再收窄，不可放宽）。
- 鉴权中间件统一产出 `Principal{user_id, role, scopes}`，路由用 `require("channel:write")` 声明式校验，**默认拒绝**。
- `site:version` 缺失时，`/api/state` 的 `version` 置 `"hidden"`；`api_compat` 不受限。
- 审计：所有写操作 + 授权变更写 `audit.jsonl`，带 `principal`、`scope`、`target`、`result`。

## 三、客户端技术选型（确认）

| 项 | 选择 | 理由 |
|---|---|---|
| 壳 | Tauri 2（已验证 `cargo check` 通过） | 无 Python 运行时、体积小、WebView2 已有 |
| 前端 | **Svelte 5 + Vite + TS** | 无虚拟 DOM 运行时，产物小；只 `invoke`，不碰 HTTP/凭证 |
| HTTP | `reqwest`（rustls） | 不在前端放 token |
| 凭证 | `keyring`（Windows Credential Manager / Keychain / libsecret） | 服务名 `relayhub-console`，账户 `site:<id>` |
| 本地库 | `rusqlite`（bundled） | 存站点元数据、日志缓存、用量快照；免外部依赖 |
| 时间 | `chrono` | 注意时区：展示本地，存储 UTC |
| 构建 | `tauri build`，`bundle.active=true` 后再开 | MVP 先 `cargo run` |

**keyring 失败回退**：Linux 无 Secret Service 时，落到 `~/.config/relayhub-console/secrets.json`（0600，写盘即提示「未加密存储」）；Windows/macOS 不回退，直接报错让用户修环境。

## 四、登录页（P0 第一屏）

结构：

```
标语：管理你的本地或远程 relay-hub 实例
副标：不代理流量、不存储上游 Key
[地址]  https://…
[认证]  Token（管理面 token / admin 账号）
[测试连接]  ← 必须先于保存
  ├─ 200 + api_compat 匹配      → 允许保存，显示「在线 · <api_compat>」
  ├─ 401                        → 「凭证无效」
  ├─ 网络错误 / 超时            → 「无法连接（检查地址/防火墙/是否 https）」
  ├─ api_compat 低于最低要求    → 「服务端版本过旧，请升级」
  └─ 响应缺 api_compat 字段     → 「第三方或旧版，可能不兼容」→ 需二次确认才能保存
次路径：还没有 relay-hub？创建一个实例
  └─ 检测 python3 / hubrelay → 填端口/监听/Token/数据目录/开机自启 → 起进程 → 回填连接信息
问号：基于官方管理 API 开发；fork 改动端点/认证/响应格式可能无法连接
```

「测试连接」顺序：`GET /healthz`（免认证，判可达）→ `GET /api/state`（带凭证，判认证与 `api_compat`）→ 通过才落库。

## 五、公告

- 客户端**只调** `/public/announcements`（登录前）与 `/api/announcement`（登录后）。
- 未认证访问 `/api/announcement` 返回 **401**，不是 200 + 空。
- 公开公告需在发布时显式勾「允许公开」，字段脱敏后才进 `/public/announcements`。

## 六、权限在客户端的表现

| 层 | 做什么 |
|---|---|
| 服务端 | 真正的校验（`require(scope)`），越权回 403 |
| 客户端 | 按 `Principal.scopes` 隐藏/禁用按钮，**只为减少误操作** |
| 文档 | 明确写「客户端最小权限是 UI 约束，不是安全边界」 |

首次连接后调 `/api/whoami` 拿 `role + scopes`，缓存到内存（不写盘），每次重连刷新。

## 七、阶段与验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| S0 | relay-hub 0.10.0：RBAC 骨架、`api_compat`、版本脱敏、公告拆分、审计补全 | `pytest` 覆盖越权 403；curl 验证非 owner 拿不到 version |
| S1 | 客户端骨架：登录页 + 站点库 + 钥匙串 + 测试连接五态 | 五种状态各点一次，凭证可从凭据管理器验证存在 |
| S2 | P0 页签：渠道（启停/重置冷却）、令牌（创建/吊销/明文只显示一次）、用量、日志 | 每页签一次真实操作 + 审计日志有记录 |
| S3 | Doctor 自检可视化 | `hubrelay doctor` 输出映射成勾选清单 |
| S4 | P1：插件分账（CSV）、桌面通知、配置导入导出 | 分账合计与 `pluginlogs` 手算一致 |

## 八、明确不做

- 不做流量代理（客户端只读管理面）
- 不内置 Python 运行时（创建实例调本机 Python，找不到就引导安装）
- 不在客户端做权限校验（服务端没有就写文档说明，不假装有）
