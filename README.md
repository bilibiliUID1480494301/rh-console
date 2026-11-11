# relay-hub Console

多站点 relay-hub 的**桌面运维控制台**。不做聊天，只做管理：连上你的（本地或远程）relay-hub 实例，管渠道、令牌、用量、日志。

Tauri 2 + Rust，Windows 单文件约 10 MB，不需要 Python 运行时、不需要 Docker。

## 为什么是桌面端

网页管理台一屏只能看一个站点。运维真实需求是「几个站一起看」：哪个站渠道在冷却、哪个令牌快超额、哪次 5xx 集中。桌面端把多站点聚合在一处，这是网页做不到的。

## 当前能力

| 能力 | 状态 |
|---|---|
| 多站点：地址 + 管理员账号密码，密码存系统钥匙串（Credential Manager / Keychain / libsecret） | ✅ |
| 测试连接五态：在线 / 凭证无效 / 无法连接 / 服务端过旧 / 第三方 fork | ✅ |
| 创建本地实例：检测 `python`/`hubrelay`，起进程并自动建管理员账号 | ✅ |
| 事件流：订阅 `GET /api/events`（SSE），状态变化即时刷新，不轮询 | ✅ |
| 渠道 / 令牌 / 用量 / 日志 / 插件用量 页签 | ✅ |
| 桌面通知：渠道冷却 / 恢复（SSE 推送，不轮询） | ✅ |
| 安全横幅：GHSA + CVE + 是否有新版 | ✅ |
| 一键升级：下载 → **本地 Ed25519 验签** → 安装；可回滚 | ✅ |
| 远程部署：本地验签 → scp → 远端 `pip install`（私钥只传路径、不落盘） | ✅ |
| 本机自检（doctor）与服务器状态 | ✅ |
| 站点主题（仅颜色白名单） | ✅ |

## 运行

```bash
cd src-tauri
cargo tauri dev        # 开发
cargo tauri build      # 出安装包（bundle.active=true 时）
```

前置：Rust stable（1.85+，edition 2024）、MSVC 生成工具、WebView2（Win10 1809+ 自带安装器）。

国内拉 crate 慢的话，`~/.cargo/config.toml` 配 rsproxy 稀疏索引即可（本仓库不含该配置）。

## 安全模型

- **凭证只进系统钥匙串**，`sites.json` 只存 `{id, label, base_url, username}`，密码从不落地
- 前端只通过 `invoke` 调 Rust，**不直接发 HTTP、不接触密码**
- 权限校验在 relay-hub 服务端（`0.10.x` 的 `scopes`），本客户端隐藏按钮**只是体验层**
- 版本号仅 Owner 可见（`version: "hidden"` 给 admin），`api_compat` 用于兼容性判断

## 兼容性

对接 relay-hub 官方管理 API（`api_compat = 2026-11`）。

第三方 fork 若改动管理端点路径、认证方式或响应格式，可能连不上——测试连接会把这类实例标成「第三方 / 旧版」，需自行确认后再保存。

## 目录

```
src-tauri/        Rust 后端（命令：site_list / site_test / site_add / site_remove /
                  env_detect / instance_create / subscribe_events）
ui/               原生 HTML + JS，无打包步骤
docs/DESIGN.md    设计稿：契约差距、RBAC、阶段计划
```


## 安全边界（远程部署）

远程部署不是「把密码存起来下次自动连」：

- SSH 私钥**每次手动给路径**，不写磁盘、不进钥匙串——远程 shell 的权限比面板密码高一个量级
- wheel **先在本地验签**（包内 Ed25519 公钥 + SHA256SUMS），验不过直接拒绝，不会传任何字节
- 远端只执行 `pip install --no-deps --force-reinstall`，装完 `rm` 临时文件

已知限制：本机代码路径已测（验签、哈希、版本挑选），**真实 ssh/scp 链路未在远端机器上验证过**。
