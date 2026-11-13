# 框架与插件架构

红色问答 v1.1 起采用「框架核心 + 插件」架构：框架负责连接适配器与管理插件，
脚本引擎、TTS、浏览器等能力都是普通插件。

## 分层

```
┌────────────────────────── 框架特殊层（redreply-bot） ──────────────────────────┐
│ PluginManager       插件注册 / 启用禁用 / 启停 / 状态                          │
│ AdapterManager      适配器子系统：工厂注册、连接对账、退避重试、API 路由       │
│ EventBus            事件总线（优先级 / 消费语义 / 按插件回收）                 │
│ ServiceRegistry     服务注册表（按名字 + 类型取用）                            │
│ HttpService         单端口 HTTP：/ 管理台、/api/*、插件路由、旧路径兜底        │
│ Logger / Config     日志与配置                                                 │
└──────────────────────────────────────────────────────────────────────────────┘
        ▲ 编译期注册适配器工厂              ▲ Plugin trait + 服务/路由/事件
┌───────┴───────────────────────┐   ┌──────┴─────────────────────────────────────┐
│ redreply-adapter（特殊 SDK）  │   │ 普通插件层                                  │
│ onebot11 / ovo / satori /     │   │ redlang（脚本引擎） tts browser mqtt pgsql  │
│ qqguild / kook / telegram /   │   │                                            │
│ email / yunhu                 │   │                                            │
└───────────────────────────────┘   └────────────────────────────────────────────┘
```

## Workspace 布局

```
crates/redreply-bot        框架核心（本文件描述的特殊层）
crates/redreply-adapter    适配器 SDK（trait + 全部协议实现，特殊层）
crates/redreply            主程序（bin `redlang`）：组装宿主、适配器与插件
crates/redreply-common     共享工具层：mytool/文件锁/代理缓存 + 服务端口 trait
crates/redlang             宿主库：仅平台能力（连接、会话、HTTP 兜底、配置、资源）
crates/plugin-redlang      RedLang 全部内容：解释器 core、命令库、脚本管理、
                           事件/定时/网络触发、脚本 API（/redlang/*）、
                           Python 环境与 /pyserver 桥
crates/plugin-tts          TTS 插件（注册 `tts` 服务）
crates/plugin-browser      浏览器插件（sidecar DLL，注册 `browser` 服务）
crates/plugin-mqtt         MQTT 分布式插件（自带客户端与状态，注册 `mqtt` 服务）
crates/plugin-pgsql        PostgreSQL 持久化插件（自带连接状态，注册 `pgsql` 服务）
crates/plugin-onebot       OneBot 反向接口插件（含事件转换、ID 映射、WS 服务与
                           事件总线订阅，不依赖 redlang 插件）
crates/plugin-tianwang     ChatGPT 聊天机器人插件（OpenAI 兼容接口，群聊 @/前缀触发）
crates/redreply-browser    浏览器 sidecar（cdylib）
crates/msedge-tts          Edge TTS 库
```

宿主机实现位于根 crate `redlang`（lib）：插件 crate 依赖宿主提供的
`cqapi`/`httpserver`/`session_store` 等能力；跨插件共享的工具与服务端口
trait 位于 `redreply-common`（宿主再导出为 `redlang::{mytool, plugin_ports}`）；
主程序通过 `redlang::set_framework_starter` 注入组装好的框架。

redlang 插件内部仍保持解耦：`core` 模块（解释器本体，不依赖宿主）通过
`core::{set_logger, set_cancellation, set_script_runner}` 接收宿主钩子；
脚本触发统一来自框架事件总线（本地适配器与 mqtt 插件发布的远端事件），
脚本 HTTP API 由插件自行注册在 `/redlang/*`。

## 插件契约

```rust
pub trait Plugin: Send + Sync + 'static {
    fn meta(&self) -> PluginMeta;                        // 名字/版本/提供的服务/依赖的服务
    fn register(&self, ctx: &PluginContext) -> Result<()>; // 注册服务、HTTP 路由、事件订阅
    fn start(&self, ctx: &PluginContext) -> Result<()>;    // 启动后台任务
    fn stop(&self, ctx: &PluginContext) -> Result<()>;     // 停止后台任务
}
```

- 插件通过 `PluginContext` 访问框架：`services()`、`events()`、`http()`、`adapters()`、`logger()`、`runtime()`、`menus()`。
- 插件可调用 `ctx.register_menu(MenuItem::new(id, label, url).order(n))` 注册自己的管理页面入口；
  该入口显示在框架管理台「插件」列表中它自己的卡片内（不进入框架全局导航），
  并随插件停用自动回收。redlang 插件即通过该机制提供「脚本管理」「运行日志」入口。
- 插件停用时框架按 owner 自动回收它注册的服务、路由、菜单与事件订阅。
- 插件间不直接依赖：能力调用走服务名（如 redlang 通过 `tts`、`browser`、`mqtt`、`pgsql`
  取用，服务端口 trait 定义在 `redlang::plugin_ports`）；事件消费走事件总线订阅。
  `onebot` 插件即通过自己的订阅完成反向接口转发，不要求 redlang 插件存在。
- 插件缺失时按语义自动跳过：服务未注册则调用方明确报错或跳过，事件无人订阅则自然丢弃。
- 启用状态持久化在 `config.json` 的 `disabled_plugins` 数组；管理台可随时启停。

TTS 服务由 `plugin-tts` 实现，通过共享的 `TtsService` 端口提供合成参数、
在线人声查询和试听合成。`plugin-redlang` 只负责参数解析及结果转换，
音频仍交给原有 `【语音】` 命令和适配器处理。调用方式见[文字转语音](tts/README.md)。

## 事件流

```
平台 → 适配器 → AdapterDeps::dispatch ─┐
                                      ├→ EventBus ─┬─ onebot 插件：事件转换 → /onebot 订阅者
MQTT 远端事件 → mqtt 插件 ────────────┘           ├─ redlang 插件：do_1207_event → 脚本执行
                                                  │    ├─ 落库：pgsql 服务（插件未启用则跳过）
                                                  │    └─ 跨实例发布：mqtt 服务（插件未启用则跳过）
                                                  └─ chatgpt 插件：AI 对话（忽略 MQTT 远端事件）

外部程序 → /onebot/{platform}/{self_id} → onebot 插件 → cq_call_api → 适配器 → 平台
脚本回复 → cq_call_api → 框架 AdapterManager::call_api → 适配器 → 平台
远端调用 → cq_call_api(remote_id) → mqtt 服务 → 目标实例
```

OneBot 转发只依赖事件总线：redlang 插件停用时脚本不再执行，但外部 OneBot
客户端仍能收到平台事件；MQTT 远端事件由 mqtt 插件发布到总线后同样会被转发。

## 适配器（特殊层）

适配器不实现 `Plugin`，而是实现 `AdapterFactory`，由 app 在组装时编译期注册：

| scheme | 协议 |
| --- | --- |
| `ws://` `wss://` | OneBot11 |
| `ovo://` | OneBot115 |
| `satori://` | Satori v1 |
| `qqguild_private://` `qqguild_public://` | QQ 频道 |
| `kook://` `telegram://` `email://` `yunhu://` | 其他平台 |

连接目标 URL 由 `AdapterManager` 与 `config.json` 的 `ws_urls` 对账：新增连接、
断线重连（5s→60s 退避）、被移除连接的清理都是自动的。管理台「适配器」页可直接增删。

## 路径规划

| 路径 | 归属 | 说明 |
| --- | --- | --- |
| `/` `/index.html` | 框架 | 管理台（同一个页面：插件/适配器/服务/日志） |
| `/api/*` | 框架 | 管理 API |
| `/login` `/login.html` `/heartbeat` `/favicon.ico` | 框架 | 登录、保活、图标 |
| `/docs/*` | 框架 | 帮助文档（docsify） |
| `/onebot/{platform}/{self_id}` | onebot 插件 | 对外 OneBot 反向接口 |
| `/watch_log` | 框架 | 日志 WebSocket |
| `/pyserver` | redlang 插件 | Python sidecar WebSocket |
| `/mqtt/` `/mqtt/api/*` | mqtt 插件 | MQTT 配置页与读写 API（重启生效） |
| `/pgsql/` `/pgsql/api/*` | pgsql 插件 | PostgreSQL 配置页与读写 API（重启生效） |
| `/redlang/` 及 `/redlang/*.html` | redlang 插件 | 脚本编辑台静态页面 |
| `/redlang/api/*` | redlang 插件 | 脚本管理 API |
| `/redlang/user/{包名}/{自定义路径}` | redlang 插件 | 脚本网络触发 |

旧版根路径（`/index.html`、`/get_code`、`/user/...` 等）已移除，不再提供。

## 当前迁移状态

- [x] 框架核心：插件管理、事件总线、服务注册表、单端口 HTTP、管理台
- [x] 适配器特殊层：全部协议接入框架连接管理
- [x] redlang / tts / browser / mqtt / pgsql / onebot 以插件接入；redlang 通过 `tts`、`browser` 服务调用能力
- [x] 插件启用状态与适配器连接的持久化
- [x] redlang 编辑台、脚本 API 与网络触发全部迁移到 `/redlang/*`（旧根路径已移除）
- [x] 每个插件都是独立 crate（`crates/plugin-*`），主程序为独立 bin 包 `crates/redreply`
- [x] RedLang 全部并入 `crates/plugin-redlang`：解释器 core 模块 + 标准命令库 +
      脚本管理 + 事件/定时/网络触发 + 脚本 API；宿主仅保留平台能力并通过钩子回调
- [x] 共享工具层抽到 `crates/redreply-common`：mytool、文件锁、代理缓存、服务端口 trait
- [x] Python 环境与 `/pyserver` 桥迁入 redlang 插件（宿主不再持有 Python 逻辑）
- [x] MQTT 客户端与状态迁入 mqtt 插件（注册 `mqtt` 服务，宿主远程调用走服务端口）
- [x] PostgreSQL 连接与落库迁入 pgsql 插件（注册 `pgsql` 服务，redlang 落库走服务端口）
- [x] OneBot 反向接口全部迁入 onebot 插件：`onebot11s` 事件转换、`onebot_store`
      ID 映射与 `/onebot` WS 服务；转发改为插件自行订阅事件总线，不再依赖 redlang 服务端口
