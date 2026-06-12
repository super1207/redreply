# redreply-adapter

`redreply-adapter` 是红色问答的聊天平台适配器 crate。它把“连接不同聊天协议”和“宿主业务如何收发事件、调用 API”这层边界抽了出来，方便红色问答主项目以及其它 Rust 项目复用同一套协议适配能力。

这个 crate 目前提供两类内容：

- 适配器抽象：`BotConnectTrait`、`BotRegistry`、`AdapterResult`、`AdapterError`、`BotHandle`。
- 已内置的协议实现：OneBot11、Satori、官方 QQ、Kook、邮件、Telegram 等。

## 适合做什么

- 在自己的项目里接入红色问答已经支持的聊天平台协议。
- 写一个新的协议适配器，然后统一注册到 `BotRegistry`。
- 让宿主项目只关心“收到事件”和“调用 API”，协议连接、鉴权、WebSocket/HTTP 细节交给适配器。
- 复用红色问答的 CQ 消息段解析、事件转换、官方 QQ 群聊/私聊发送等基础能力。

## 安装

当前 crate 仍在仓库内开发，建议先使用 path 依赖：

```toml
[dependencies]
redreply-adapter = { path = "crates/redreply-adapter" }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

如果你的项目不在红色问答 workspace 内，把 `path` 改成实际路径即可：

```toml
redreply-adapter = { path = "../redreply/crates/redreply-adapter" }
```

## 核心概念

### BotConnectTrait

所有协议连接器都实现 `BotConnectTrait`：

```rust
use redreply_adapter::{async_trait, AdapterResult, BotConnectTrait};

#[async_trait]
pub trait BotConnectTrait: Send + Sync {
    async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        json: &mut serde_json::Value,
    ) -> AdapterResult<serde_json::Value>;

    fn get_platform_and_self_id(&self) -> Vec<(String, String)>;
    fn get_alive(&self) -> bool;
    async fn connect(&mut self) -> AdapterResult<()>;
    async fn disconnect(&mut self);
}
```

字段含义：

- `platform`：平台名，例如 `onebot11`、`qqgroup_public`、`qqguild_public`。
- `self_id`：机器人自身 ID。不同协议的含义不同，通常是 bot id、appid 或账号。
- `passive_id`：被动上下文 ID。适配器可以用它保存回复消息所需的事件 ID、消息 ID 或会话信息。
- `json`：API 调用参数，通常使用 OneBot 风格的 JSON。

### BotRegistry

`BotRegistry` 用来按连接 URL 管理多个 bot，并按 `(platform, self_id)` 分发 API 调用。

常用方法：

- `BotRegistry::new()`：创建注册表。
- `insert(url, bot)`：注册一个已经连接或准备连接的 bot。
- `contains_url(url)`：检查某个连接 URL 是否已注册。
- `removable_urls(configured_urls)`：找出配置中已不存在或已经不存活的 bot。
- `disconnect_and_remove(url)`：断开并移除指定 bot。
- `call_api(platform, self_id, passive_id, json)`：找到匹配 bot 并调用 API。

`call_api` 找不到匹配 bot 时会返回 `Ok(None)`，宿主项目可以自行决定是报错、记录警告，还是返回空结果。

## 宿主能力注入

部分协议需要把收到的事件交给宿主项目，也需要使用宿主提供的日志、应用目录、音频转 silk 等能力。这些能力通过 `host::AdapterHost` 注入。

```rust
use std::sync::Arc;
use redreply_adapter::{
    AdapterResult,
    host::{AdapterHost, set_host},
};

struct MyHost;

impl AdapterHost for MyHost {
    fn log(&self, msg: &str) {
        println!("{msg}");
    }

    fn warn(&self, msg: &str) {
        eprintln!("{msg}");
    }

    fn dispatch_event(&self, event_json: &str) -> AdapterResult<()> {
        println!("收到事件: {event_json}");
        Ok(())
    }

    fn app_dir(&self) -> AdapterResult<String> {
        Ok("./".to_owned())
    }

    fn all_to_silk(&self, input: &[u8]) -> AdapterResult<Vec<u8>> {
        // 如果宿主暂时没有 silk 转码能力，可以先返回原始数据。
        // 但发送语音时，目标平台可能仍然要求真实 silk 数据。
        Ok(input.to_vec())
    }
}

fn init_adapter_host() {
    let _ = set_host(Arc::new(MyHost));
}
```

注意事项：

- `set_host` 内部使用 `OnceLock`，一个进程里只能成功设置一次。
- 如果不设置 host，日志和事件派发会静默跳过，但 `all_to_silk` 会报错。
- 内置协议收到事件后，会调用 `dispatch_event(event_json)`。这里的 `event_json` 是红色问答/OneBot 风格事件 JSON 字符串。

## 使用内置协议

内置协议连接器都提供 `build(url: &str) -> Self`，通常流程是：

1. 调用 `build` 创建连接器。
2. 调用 `connect().await` 建立连接或完成初始化。
3. 调用 `BotRegistry::insert(url, bot).await` 注册。
4. 后续通过 `BotRegistry::call_api` 分发 API。

示例：

```rust
use redreply_adapter::{
    AdapterResult,
    BotConnectTrait,
    BotRegistry,
    onebot11::OneBot11Connect,
};

#[tokio::main]
async fn main() -> AdapterResult<()> {
    let registry = BotRegistry::new();
    let url = "ws://127.0.0.1:3001";

    let mut bot = OneBot11Connect::build(url);
    bot.connect().await?;
    registry.insert(url.to_owned(), bot).await;

    let mut req = serde_json::json!({
        "action": "send_msg",
        "params": {
            "message_type": "private",
            "user_id": "10000",
            "message": "你好"
        }
    });

    let ret = registry
        .call_api("onebot11", "10000", "", &mut req)
        .await?;

    println!("API 返回: {ret:?}");
    Ok(())
}
```

如果注册表里只有一个 bot，并且这个 bot 只返回一个 `(platform, self_id)`，`call_api("", "", "", &mut json)` 会自动选中它。

## 内置协议与 URL 格式

连接 URL 是红色问答主项目目前使用的配置格式。其它项目可以直接沿用，也可以自己包装一层配置后再生成 URL。

| 协议 | 连接器 | URL 示例 |
| --- | --- | --- |
| OneBot11 | `onebot11::OneBot11Connect` | `ws://127.0.0.1:3001` 或 `wss://example.com/ws` |
| Satori | `satoriv1::Satoriv1Connect` | `satori://{"uri":"127.0.0.1:5500","token":"xxx","use_tls":false}` |
| 官方 QQ | `qqguild_public::QQGuildPublicConnect` | `qqguild_public://{"AppID":"1024","AppSecret":"xxx","Token":"xxx"}` |
| Kook | `kook::KookConnect` | `kook://{"Token":"xxx"}` |
| 邮件 | `email::EmailConnect` | `email://{"username":"a@example.com","password":"xxx","imap_server":"imap.example.com","imap_port":993,"imap_ssl":true,"smtp_server":"smtp.example.com","smtp_port":465,"smtp_ssl":true}` |
| Telegram | `telegram::TeleTramConnect` | `telegram://{"Token":"xxx","Proxy":""}` |

补充说明：

- `qqguild_private::QQGuildPrivateConnect` 仍作为兼容模块保留，当前红色问答配置界面统一使用 `官方QQ`，也就是 `qqguild_public://...`。
- 官方 QQ 的频道私信、Q 单聊主要按被动消息处理；Q 群如果具备全量消息能力，可以收到 `GROUP_MESSAGE_CREATE` 并发送主动群消息。
- 某些平台对主动消息、撤回、富媒体、语音格式有额外权限或审核限制，适配器会尽量按平台返回值处理，但最终行为仍以平台服务端为准。

## 主项目集成范式

宿主项目通常会维护一个全局或应用级 `BotRegistry`，并定期把配置中的 URL 与当前已连接 bot 对齐。

```rust
use redreply_adapter::{
    AdapterResult,
    BotConnectTrait,
    BotRegistry,
    email::EmailConnect,
    kook::KookConnect,
    onebot11::OneBot11Connect,
    qqguild_public::QQGuildPublicConnect,
    satoriv1::Satoriv1Connect,
    telegram::TeleTramConnect,
};

async fn connect_one(registry: &BotRegistry, url: String) -> AdapterResult<()> {
    if registry.contains_url(&url).await {
        return Ok(());
    }

    if url.starts_with("ws://") || url.starts_with("wss://") {
        let mut bot = OneBot11Connect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else if url.starts_with("satori://") {
        let mut bot = Satoriv1Connect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else if url.starts_with("qqguild_public://") {
        let mut bot = QQGuildPublicConnect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else if url.starts_with("kook://") {
        let mut bot = KookConnect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else if url.starts_with("email://") {
        let mut bot = EmailConnect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else if url.starts_with("telegram://") {
        let mut bot = TeleTramConnect::build(&url);
        bot.connect().await?;
        registry.insert(url, bot).await;
    } else {
        return Err(format!("未知适配器 URL: {url}").into());
    }

    Ok(())
}
```

清理配置里已经删除的连接：

```rust
use redreply_adapter::BotRegistry;

async fn sync_removed(registry: &BotRegistry, configured_urls: &[String]) {
    let removable = registry.removable_urls(configured_urls).await;
    for url in removable {
        registry.disconnect_and_remove(&url).await;
    }
}
```

## API 调用约定

内置协议的 `call_api` 接收 `serde_json::Value`，不同协议支持的 action 会有差异。红色问答主项目主要使用 OneBot 风格 API，例如：

```rust
let mut req = serde_json::json!({
    "action": "send_msg",
    "params": {
        "message_type": "group",
        "group_id": "123456",
        "message": "hello"
    }
});

let ret = registry
    .call_api("qqgroup_public", "1024", "", &mut req)
    .await?;
```

常见要点：

- `platform + self_id` 用于选中具体 bot。
- `passive_id` 可以传空字符串；需要被动回复上下文的平台会在内部或事件链路中处理。
- 返回值是协议适配器归一化后的 JSON。平台原始错误通常会作为 `AdapterError` 返回。
- 如果你要给自定义协议扩展 action，建议保持 OneBot 风格的 `action + params` 外壳，便于与红色问答现有调用链兼容。

## 事件派发约定

协议连接器收到消息、通知或请求事件后，会转换为宿主可处理的 JSON 字符串，并调用：

```rust
host::dispatch_event(event_json)
```

宿主项目需要在 `AdapterHost::dispatch_event` 中完成自己的业务派发。例如红色问答主项目会把事件交给规则引擎。

事件 JSON 大体遵循 OneBot 风格，通常包含：

- `post_type`：事件类型，例如 `message`。
- `message_type`：消息类型，例如 `private`、`group`。
- `platform`：适配器平台名。
- `self_id`：机器人自身 ID。
- `user_id` / `group_id`：事件来源。
- `message` / `raw_message`：消息内容。

不同协议会保留各自需要的扩展字段。宿主项目不要假设所有协议都有完全相同的字段。

## 编写自定义适配器

自定义适配器只需要实现 `BotConnectTrait`，然后注册到 `BotRegistry`。

```rust
use redreply_adapter::{async_trait, AdapterResult, BotConnectTrait, BotRegistry};

struct MyAdapter {
    alive: bool,
    self_id: String,
}

impl MyAdapter {
    fn new(self_id: impl Into<String>) -> Self {
        Self {
            alive: false,
            self_id: self_id.into(),
        }
    }
}

#[async_trait]
impl BotConnectTrait for MyAdapter {
    async fn call_api(
        &self,
        _platform: &str,
        _self_id: &str,
        _passive_id: &str,
        json: &mut serde_json::Value,
    ) -> AdapterResult<serde_json::Value> {
        let action = json
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        match action {
            "get_login_info" => Ok(serde_json::json!({
                "user_id": self.self_id,
                "nickname": "my-adapter"
            })),
            _ => Err(format!("不支持的 action: {action}").into()),
        }
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        vec![("my-platform".to_owned(), self.self_id.clone())]
    }

    fn get_alive(&self) -> bool {
        self.alive
    }

    async fn connect(&mut self) -> AdapterResult<()> {
        self.alive = true;
        Ok(())
    }

    async fn disconnect(&mut self) {
        self.alive = false;
    }
}

async fn register_custom_adapter() -> AdapterResult<BotRegistry> {
    let registry = BotRegistry::new();
    let mut bot = MyAdapter::new("bot-001");

    bot.connect().await?;
    registry.insert("my://bot-001".to_owned(), bot).await;

    Ok(registry)
}
```

如果自定义适配器会主动接收事件，可以在连接任务中调用 `redreply_adapter::host::dispatch_event`：

```rust
use redreply_adapter::{AdapterResult, host};

fn emit_message_event() -> AdapterResult<()> {
    let event = serde_json::json!({
        "post_type": "message",
        "message_type": "private",
        "platform": "my-platform",
        "self_id": "bot-001",
        "user_id": "user-001",
        "message": "hello"
    });

    host::dispatch_event(&event.to_string())
}
```

## 常见注意事项

- 这个 crate 是从红色问答主项目中新抽出的适配器层，接口会优先服务红色问答现有协议，后续仍可能继续整理。
- `BotRegistry` 只负责保存、查找、移除 bot，不负责完整的重连策略。重连循环应由宿主项目控制。
- `get_alive()` 的语义由具体适配器决定；宿主通常用它判断是否需要移除并重连。
- 官方 QQ、Telegram、Kook 等平台能力会受到机器人权限、服务端审核、主动消息额度等限制。
- 发送语音时，某些协议需要 silk 格式；如果宿主没有提供可用的 `all_to_silk`，相关 API 可能失败。
- URL 中包含 token、secret、邮箱密码等敏感信息，日志输出时请自行脱敏。

## redreply 主项目中的用法

红色问答主项目当前在 `src/botconn/mod.rs` 中完成三件事：

1. 实现 `AdapterHost`，把日志、事件派发、应用目录、音频转 silk 注入给适配器。
2. 维护一个全局 `BotRegistry`。
3. 根据配置里的连接 URL 创建对应内置协议连接器，并把 `call_api` 统一转发给 `BotRegistry`。

其它项目可以照这个结构集成，也可以把 `BotRegistry` 放进自己的应用状态、服务容器或 actor 中。
