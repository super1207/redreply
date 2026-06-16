# redreply-adapter 协议说明

本文是 `redreply-adapter` 的公开协议说明。它只描述 RedReply Adapter 自己的使用方式，不要求读者了解任何其它机器人协议。

`redreply-adapter` 提供一套统一的收发消息约定：

- 宿主项目用统一 API 发送消息、撤回消息、查询资料。
- 适配器把不同平台收到的消息统一转换成 RedReply 事件。
- 消息可以用纯文本、消息码字符串或消息段数组表示。
- 平台差异由适配器处理；无法跨平台统一的能力会在本文标注限制。

## 1. 基本模型

RedReply Adapter 的交互分为两个方向。

| 方向 | 接口 | 说明 |
| --- | --- | --- |
| API 调用 | `BotRegistry::call_api(platform, self_id, passive_id, json)` | 宿主调用适配器，发送消息或执行操作。 |
| 事件上报 | `AdapterHost::dispatch_event(event_json)` | 适配器收到平台事件后，上报给宿主。 |

核心调用接口：

```rust
async fn call_api(
    &self,
    platform: &str,
    self_id: &str,
    passive_id: &str,
    json: &mut serde_json::Value,
) -> AdapterResult<serde_json::Value>;
```

参数：

| 参数 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `platform` | string | 是 | 平台标识，例如 `onebot11`、`qqgroup_public`、`qqguild_public`、`kook`、`telegram`、`email`。 |
| `self_id` | string | 是 | 机器人自身 ID。不同平台含义不同，例如官方 QQ 为 AppID，邮件为邮箱地址。 |
| `passive_id` | string | 否 | 被动回复上下文 ID。通常来自事件中的 `message_id`。没有被动上下文时传空字符串。 |
| `json` | object | 是 | API 请求对象。 |

## 2. API 请求格式

所有 API 请求都使用同一个外壳：

```json
{
  "action": "send_group_msg",
  "params": {
    "group_id": "123456",
    "message": "你好"
  },
  "echo": "optional-id"
}
```

字段：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `action` | string | 是 | API 名称。 |
| `params` | object | 否 | API 参数。省略时按空对象处理。 |
| `echo` | any | 否 | 调用方自定义回声字段。支持的适配器会在返回中原样带回。 |

## 3. API 响应格式

成功响应：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {},
  "echo": "optional-id"
}
```

失败响应：

```json
{
  "status": "failed",
  "retcode": 1404,
  "message": "reason",
  "data": {},
  "echo": "optional-id"
}
```

字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `status` | string | `ok` 表示成功，`failed` 表示失败。 |
| `retcode` | int | 返回码。`0` 表示成功，`1404` 常表示当前适配器不支持或找不到上下文。 |
| `data` | any | API 数据。 |
| `message` | string | 失败说明，可能不存在。 |
| `echo` | any | 请求中的 echo，可能不存在。 |

宿主应同时处理两种失败：

- API 正常返回，但 `status` 为 `failed`。
- `call_api` 直接返回 `AdapterError`。

处理示例：

```rust
match registry.call_api(platform, self_id, passive_id, &mut req).await {
    Ok(Some(ret)) if ret["status"] == "ok" => {
        // 成功
    }
    Ok(Some(ret)) => {
        // API 层失败
    }
    Ok(None) => {
        // 没有匹配的 bot
    }
    Err(err) => {
        // 网络、平台、适配器内部错误
    }
}
```

## 4. 连接配置

连接 URL 用于创建具体适配器实例。通常由宿主配置界面生成。

| 平台 | URL 示例 | 说明 |
| --- | --- | --- |
| 官方 QQ | `qqguild_public://{"AppID":"1024","AppSecret":"xxx","Token":"xxx"}` | 官方 QQ，包含 Q 群、Q 单聊、频道私信能力。 |
| Kook | `kook://{"Token":"xxx"}` | Kook 机器人。 |
| Telegram | `telegram://{"Token":"xxx","Proxy":""}` | Telegram 机器人。 |
| 邮件 | `email://{"username":"a@example.com","password":"xxx","imap_server":"imap.example.com","imap_port":993,"imap_ssl":true,"smtp_server":"smtp.example.com","smtp_port":465,"smtp_ssl":true}` | 邮箱收发。 |
| Satori | `satori://{"uri":"127.0.0.1:5500","token":"xxx","use_tls":false}` | Satori 连接。 |
| OneBot11 | `ws://127.0.0.1:3001` 或 `wss://example.com/ws` | OneBot11 WebSocket 连接。 |

敏感字段包括 token、secret、邮箱密码等，日志和文档中应脱敏。

## 5. 平台标识

适配器注册后，宿主通过 `platform + self_id` 选择目标 bot。

| 平台能力 | platform | self_id |
| --- | --- | --- |
| 官方 QQ 频道私信 | `qqguild_public` | AppID |
| 官方 QQ Q 群 / Q 单聊 | `qqgroup_public` | AppID |
| Kook | `kook` | Kook bot id |
| Telegram | `telegram` | Telegram bot id |
| 邮件 | `email` | 邮箱地址 |
| Satori | 由连接端返回 | 由连接端返回 |
| OneBot11 | `onebot11` | 连接端上报的机器人 ID |

如果注册表中只有一个 bot，且该 bot 只暴露一个身份，宿主也可以传空 `platform` 和空 `self_id`，由 `BotRegistry` 自动选择。

## 6. 消息表示

RedReply Adapter 支持三种消息写法：

- 纯文本字符串。
- 消息码字符串。
- 消息段数组。

### 6.1 纯文本

```json
{
  "message": "你好"
}
```

如果发送字符串，默认会解析其中的消息码。若适配器支持 `auto_escape`，可以强制按纯文本发送：

```json
{
  "message": "[CQ:image,file=https://example.com/a.png]",
  "auto_escape": true
}
```

### 6.2 消息码字符串

消息码字符串也称 CQ 码字符串，由普通文本和 `[CQ:type,key=value]` 片段组成：

```text
你好[CQ:at,qq=123456][CQ:image,file=https://example.com/a.png]
```

一个消息码可以包含多个参数：

```text
[CQ:image,file=https://example.com/a.png,url=https://example.com/a.png]
```

消息码中的 `type`、参数名、参数值都按字符串处理。消息码格式至少包含一个 `key=value` 参数。

#### 6.2.1 转义规则

消息码解析分为两个区域：

- 文本区：不在 `[CQ:...]` 内的普通文本。
- 消息码区：`[CQ:type,key=value]` 内的 `type`、参数名、参数值。

文本区转义：

| 原字符 | 写法 | 说明 |
| --- | --- | --- |
| `&` | `&amp;` | 避免和转义序列冲突。 |
| `[` | `&#91;` | 避免被误认为消息码开始。 |
| `]` | `&#93;` | 文本区支持该转义。 |

消息码区转义：

| 原字符 | 写法 | 说明 |
| --- | --- | --- |
| `&` | `&amp;` | 避免和转义序列冲突。 |
| `[` | `&#91;` | 避免破坏消息码结构。 |
| `]` | `&#93;` | 避免被误认为消息码结束。 |
| `,` | `&#44;` | 避免被误认为下一个参数。 |

转义只需要按上表处理：

- 文本区只会把 `&#91;`、`&#93;`、`&amp;` 解回原字符。
- 消息码区会把 `&#91;`、`&#93;`、`&amp;`、`&#44;` 解回原字符。
- 文本区的 `&#44;` 不会被解码，会保持为字面量 `&#44;`。

分隔符规则：

- `[CQ:` 必须大写，`[cq:` 不会被识别为消息码。
- `type` 后的第一个原始逗号 `,` 用于结束 type。
- 参数名后的第一个原始等号 `=` 用于结束参数名。
- 参数值中的原始逗号 `,` 用于结束当前参数。
- 参数值中的原始右中括号 `]` 用于结束整个消息码。
- 参数值中的等号 `=` 是普通字符，不需要转义。

#### 6.2.2 转义示例

发送普通文本：

```text
今天看到 &#91;测试&#93; &amp; 记录
```

解析后的文本是：

```text
今天看到 [测试] & 记录
```

发送带逗号的图片 URL。逗号在参数值中必须写成 `&#44;`：

```text
[CQ:image,file=https://example.com/a.png?x=1&#44;y=2]
```

解析后的参数是：

```json
{
  "type": "image",
  "data": {
    "file": "https://example.com/a.png?x=1,y=2"
  }
}
```

发送文件名中带方括号的图片：

```text
[CQ:image,file=https://example.com/&#91;cover&#93;.png]
```

解析后的 `file` 是：

```text
https://example.com/[cover].png
```

发送参数值中带等号的 URL。等号在参数值中可以原样写：

```text
[CQ:image,file=https://example.com/a.png?token=a=b]
```

解析后的 `file` 是：

```text
https://example.com/a.png?token=a=b
```

文本区的逗号不需要转义，也不会解码 `&#44;`：

```text
你好,世界 &#44;
```

解析后的文本是：

```text
你好,世界 &#44;
```

解析结果示例：

```text
你好[CQ:image,file=https://example.com/a.png]
```

等价于：

```json
[
  {"type":"text","data":{"text":"你好"}},
  {"type":"image","data":{"file":"https://example.com/a.png"}}
]
```

### 6.3 消息段数组

消息段数组格式：

```json
[
  {"type": "text", "data": {"text": "111"}},
  {"type": "image", "data": {"file": "https://example.com/a.png"}},
  {"type": "text", "data": {"text": "222"}}
]
```

消息段字段：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `type` | string | 是 | 消息段类型。 |
| `data` | object | 是 | 消息段参数。 |

适配器会尽量保持消息段顺序。某些平台无法在一条消息中混合文本和媒体，此时适配器会拆成多条平台消息顺序发送。

## 7. 消息段类型

### 7.1 text 文本

```json
{"type":"text","data":{"text":"hello"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `text` | string | 是 | 文本内容。 |

### 7.2 image 图片

```json
{"type":"image","data":{"file":"https://example.com/a.png"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `file` | string | 是 | 图片资源。支持 `http://`、`https://`、`base64://...`。 |
| `url` | string | 否 | 接收事件中可能出现的图片 URL。 |

平台说明：

- 官方 QQ Q 群/Q 单聊发送图片时会走媒体接口，并设置直接发送。
- 频道私信和部分平台会下载图片后上传。
- 邮件发送图片时会生成 HTML 图片内容。

### 7.3 record 语音

```json
{"type":"record","data":{"file":"https://example.com/a.mp3"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `file` | string | 是 | 语音资源。支持 URL 或 `base64://...`。 |
| `url` | string | 否 | 接收事件中可能出现的语音 URL。 |

平台说明：

- 官方 QQ 发送语音前会调用宿主提供的 `all_to_silk` 转换能力。
- Telegram、Kook 有部分语音发送能力。

### 7.4 video 视频

```json
{"type":"video","data":{"file":"https://example.com/a.mp4"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `file` | string | 是 | 视频资源。支持 URL 或 `base64://...`。 |

平台说明：

- 官方 QQ 会走媒体接口。
- Kook 可接收和转换视频类消息。

### 7.5 file 文件

```json
{"type":"file","data":{"file":"https://example.com/a.zip"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `file` | string | 是 | 文件资源。支持 URL 或 `base64://...`。 |

平台说明：

- 官方 QQ 代码中会按文件媒体类型处理，但 Q 群文件能力受平台开放情况限制。
- Kook 接收事件中可转换文件消息。

### 7.6 at 提及

```json
{"type":"at","data":{"qq":"123456"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `qq` | string | 是 | 目标用户 ID。`all` 表示全体成员。 |
| `name` | string | 否 | 展示名。多数平台不使用。 |

平台说明：

- 官方 QQ 频道文本支持用户提及和全体提及。
- 官方 QQ Q 群/Q 单聊当前发送时忽略 at。
- 官方 QQ 接收消息时会把平台 at 标签转换为消息码。

### 7.7 reply 回复

```json
{"type":"reply","data":{"id":"message-id"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `id` | string | 是 | 要回复的消息 ID。应使用事件或发送返回中的 `message_id`。 |

说明：

- `id` 是 RedReply Adapter 对宿主暴露的 ID，不一定是平台原始消息 ID。
- 官方 QQ、Satori、Telegram、Kook 会尽量映射到平台回复能力。
- 如果消息 ID 对应的上下文已过期，回复可能失败或退化为普通发送。

### 7.8 face 表情

```json
{"type":"face","data":{"id":"14"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `id` | string | 是 | 表情 ID。 |

平台说明：

- 官方 QQ 频道场景可转换为平台表情。
- Q 群/Q 单聊发送时当前不处理。

### 7.9 poke 戳一戳

```json
{"type":"poke","data":{"qq":"123456"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `qq` | string | 是 | 目标用户 ID。 |
| `name` | string | 否 | 展示名。 |

平台说明：

- 只有支持戳一戳的平台会处理。
- 不支持的平台会忽略或返回失败。

### 7.10 music 音乐

```json
{
  "type": "music",
  "data": {
    "type": "custom",
    "url": "https://example.com",
    "audio": "https://example.com/a.mp3",
    "title": "标题",
    "content": "简介",
    "image": "https://example.com/a.png"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `type` | string | 是 | `custom` 或平台音乐类型。 |
| `id` | string | 否 | 平台音乐 ID。 |
| `url` | string | custom 时常用 | 跳转 URL。 |
| `audio` | string | custom 时常用 | 音频 URL。 |
| `title` | string | 否 | 标题。 |
| `content` | string | 否 | 简介。 |
| `image` | string | 否 | 封面。 |

平台说明：

- Kook、Telegram 有部分转换能力。
- 其它平台不保证支持。

### 7.11 json / xml 原始卡片

```json
{"type":"json","data":{"data":"{\"key\":\"value\"}"}}
```

```json
{"type":"xml","data":{"data":"<xml></xml>"}}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `data` | string | 是 | 原始 JSON 或 XML 字符串。 |

平台说明：

- 主要用于 OneBot11 或支持原始卡片的平台。
- 官方 QQ、Kook、Telegram、邮件不作为通用能力。

### 7.12 forward / node 合并消息

```json
{"type":"forward","data":{"id":"forward-id"}}
```

```json
{"type":"node","data":{"user_id":"10000","nickname":"Alice","content":"hello"}}
```

说明：

- 主要用于 OneBot11 或支持合并消息的平台。
- 当前不是 RedReply Adapter 跨平台通用能力。

### 7.13 qmarkdown 官方 QQ Markdown

```json
{
  "type": "qmarkdown",
  "data": {
    "data": "base64://eyJtYXJrZG93biI6eyJjb250ZW50IjoiIyDmoIfpopgifX0="
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `data` | string | 是 | Markdown 请求 JSON 的 base64，可带 `base64://` 前缀。 |

解码后的 JSON 示例：

```json
{
  "markdown": {
    "content": "# 标题\n正文"
  }
}
```

说明：

- 仅官方 QQ 发送链路使用。
- 解码支持 UTF-8，也兼容部分 CESU-8 字节。
- qmarkdown 会作为独立发送块处理，以保持和文本、图片的发送顺序。

## 8. 事件格式

适配器收到平台事件后，会转换成统一 JSON 字符串并交给宿主：

```rust
AdapterHost::dispatch_event(event_json)
```

### 8.1 事件通用字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `time` | int | Unix 秒级时间戳。 |
| `self_id` | string/int | 当前机器人 ID。 |
| `platform` | string | 平台标识。 |
| `post_type` | string | 事件类型：`message`、`notice`、`request`、`meta_event`。 |

### 8.2 私聊消息事件

```json
{
  "time": 1710000000,
  "self_id": "bot-id",
  "platform": "telegram",
  "post_type": "message",
  "message_type": "private",
  "sub_type": "friend",
  "message_id": "message-id",
  "user_id": "user-id",
  "message": "hello",
  "raw_message": "hello",
  "font": 0,
  "sender": {
    "user_id": "user-id",
    "nickname": "nickname",
    "remark": "nickname"
  }
}
```

字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `message_type` | string | 固定为 `private`。 |
| `sub_type` | string | 常见为 `friend`，不同平台可能不同。 |
| `message_id` | string/int | 适配器消息 ID，可用于回复或撤回。 |
| `user_id` | string/int | 发送者 ID。 |
| `message` | string/array | 消息内容，通常是消息码字符串。 |
| `raw_message` | string | 原始文本或较少处理的文本。 |
| `font` | int | 字体字段，通常为 0。 |
| `sender` | object | 发送者信息。 |

### 8.3 群消息事件

```json
{
  "time": 1710000000,
  "self_id": "bot-id",
  "platform": "qqgroup_public",
  "post_type": "message",
  "message_type": "group",
  "sub_type": "normal",
  "message_id": "message-id",
  "group_id": "group-id",
  "user_id": "user-id",
  "anonymous": null,
  "message": "hello",
  "raw_message": "hello",
  "font": 0,
  "sender": {
    "user_id": "user-id",
    "nickname": "nickname",
    "card": "",
    "sex": "unknown",
    "age": 0,
    "area": "",
    "level": "0",
    "role": "member",
    "title": ""
  }
}
```

字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `message_type` | string | 固定为 `group`。 |
| `sub_type` | string | 常见为 `normal`。 |
| `message_id` | string/int | 适配器消息 ID，可用于回复或撤回。 |
| `group_id` | string/int | 群、频道或平台等价会话 ID。 |
| `user_id` | string/int | 发送者 ID。 |
| `anonymous` | object/null | 匿名信息，多数平台为空。 |
| `message` | string/array | 消息内容。 |
| `raw_message` | string | 原始文本。 |
| `sender` | object | 群成员信息。 |

`sender.role` 表示发送者在群内的身份，常见值为 `owner`、`admin`、`member`。官方 QQ 群消息会从原始事件的 `author.member_role` 映射到此字段；平台未提供时默认为 `member`。

### 8.4 通知事件

```json
{
  "time": 1710000000,
  "self_id": "bot-id",
  "platform": "kook",
  "post_type": "notice",
  "notice_type": "group_increase",
  "group_id": "group-id",
  "user_id": "user-id"
}
```

常见通知类型：

| `notice_type` | 说明 | 常见字段 |
| --- | --- | --- |
| `group_upload` | 群文件上传 | `group_id`、`user_id`、`file` |
| `group_admin` | 管理员变动 | `sub_type`、`group_id`、`user_id` |
| `group_decrease` | 群成员减少 | `sub_type`、`group_id`、`operator_id`、`user_id` |
| `group_increase` | 群成员增加 | `sub_type`、`group_id`、`operator_id`、`user_id` |
| `group_ban` | 群禁言 | `sub_type`、`group_id`、`operator_id`、`user_id`、`duration` |
| `friend_add` | 好友添加 | `user_id` |
| `group_recall` | 群消息撤回 | `group_id`、`user_id`、`operator_id`、`message_id` |
| `friend_recall` | 私聊消息撤回 | `user_id`、`message_id` |
| `notify` | 平台提示类事件 | `sub_type` 和平台扩展字段 |

### 8.5 请求事件

```json
{
  "time": 1710000000,
  "self_id": "bot-id",
  "platform": "bridge",
  "post_type": "request",
  "request_type": "friend",
  "user_id": "10000",
  "comment": "hello",
  "flag": "request-flag"
}
```

请求类型：

| `request_type` | 说明 | 常见字段 |
| --- | --- | --- |
| `friend` | 加好友请求 | `user_id`、`comment`、`flag` |
| `group` | 加群或邀请请求 | `sub_type`、`group_id`、`user_id`、`comment`、`flag` |

多数平台当前不会上报请求事件，OneBot11 等连接可能上报。

### 8.6 元事件

```json
{
  "time": 1710000000,
  "self_id": "bot-id",
  "platform": "bridge",
  "post_type": "meta_event",
  "meta_event_type": "lifecycle"
}
```

| `meta_event_type` | 说明 |
| --- | --- |
| `lifecycle` | 生命周期事件。 |
| `heartbeat` | 心跳事件。当前大部分连接器不会把心跳上报给宿主。 |

## 9. API 列表

### 9.1 send_private_msg 发送私聊消息

```json
{
  "action": "send_private_msg",
  "params": {
    "user_id": "10000",
    "message": "hello",
    "auto_escape": false
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `user_id` | string/int | 是 | 目标用户 ID。 |
| `message` | string/array | 是 | 消息内容。 |
| `auto_escape` | bool | 否 | 为 true 时字符串不解析消息码。 |

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "message_id": "message-id"
  }
}
```

平台说明：

- 官方 QQ Q 单聊和频道私信只支持被动回复，调用时必须传入 `passive_id`。
- 邮件平台的 `user_id` 是收件人邮箱地址。

### 9.2 send_group_msg 发送群消息

```json
{
  "action": "send_group_msg",
  "params": {
    "group_id": "123456",
    "message": [
      {"type":"text","data":{"text":"hello"}}
    ],
    "auto_escape": false
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 目标群、频道或平台等价会话 ID。 |
| `message` | string/array | 是 | 消息内容。 |
| `auto_escape` | bool | 否 | 为 true 时字符串不解析消息码。 |

平台说明：

- 官方 QQ Q 群如果机器人具备全量消息能力，可以尝试主动发送。
- 官方 QQ 收到 Q 群 @ 消息时，回复会按被动消息形式发送。
- 文本、图片、语音、视频等混合消息会尽量按原顺序拆分发送。

### 9.3 send_msg 自动选择私聊或群聊

```json
{
  "action": "send_msg",
  "params": {
    "message_type": "group",
    "group_id": "123456",
    "user_id": "10000",
    "message": "hello",
    "auto_escape": false
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `message_type` | string | 否 | `private` 或 `group`。 |
| `user_id` | string/int | 私聊时必填 | 目标用户。 |
| `group_id` | string/int | 群聊时必填 | 目标群。 |
| `message` | string/array | 是 | 消息内容。 |
| `auto_escape` | bool | 否 | 是否转义消息码。 |

说明：

- 部分适配器会根据 `group_id` 是否存在自动选择群聊或私聊。
- 也可以直接调用 `send_private_msg` 或 `send_group_msg`。

### 9.4 delete_msg 撤回消息

```json
{
  "action": "delete_msg",
  "params": {
    "message_id": "message-id"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `message_id` | string/int | 是 | 要撤回的消息 ID。必须使用事件或发送返回中的 `message_id`。 |

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {}
}
```

平台说明：

- 官方 QQ 会通过内部映射找到真实消息 ID。
- 如果一次发送被拆成多条平台消息，撤回会逐条执行。
- 超过平台允许撤回时间后，平台可能返回失败。

### 9.5 get_msg 获取消息

```json
{
  "action": "get_msg",
  "params": {
    "message_id": "message-id"
  }
}
```

返回常见字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `time` | int | 发送时间。 |
| `message_type` | string | `private` 或 `group`。 |
| `message_id` | string/int | 消息 ID。 |
| `sender` | object | 发送者。 |
| `message` | string/array | 消息内容。 |

说明：只有部分连接支持此 API。

### 9.6 get_forward_msg 获取合并转发消息

```json
{
  "action": "get_forward_msg",
  "params": {
    "id": "forward-id"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `id` | string | 是 | 合并转发 ID。 |

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "message": [
      {
        "type": "node",
        "data": {
          "user_id": "10000",
          "nickname": "Alice",
          "content": "hello"
        }
      }
    ]
  }
}
```

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.7 send_like 发送好友赞

```json
{
  "action": "send_like",
  "params": {
    "user_id": "10000",
    "times": 1
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `user_id` | string/int | 是 | 目标用户 ID。 |
| `times` | int | 否 | 点赞次数，默认 1。 |

返回：无数据，成功时 `data` 通常为空对象。

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.8 get_login_info 获取机器人信息

```json
{
  "action": "get_login_info",
  "params": {}
}
```

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "user_id": "bot-id",
    "nickname": "bot-name",
    "avatar": "https://example.com/avatar.png"
  }
}
```

### 9.9 get_stranger_info 获取用户信息

```json
{
  "action": "get_stranger_info",
  "params": {
    "user_id": "10000",
    "no_cache": false
  }
}
```

返回常见字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `user_id` | string/int | 用户 ID。 |
| `nickname` | string | 昵称。 |
| `sex` | string | `male`、`female`、`unknown`。 |
| `age` | int | 年龄。 |
| `avatar` | string | 头像 URL，部分平台返回。 |

### 9.10 get_friend_list 获取好友列表

```json
{
  "action": "get_friend_list",
  "params": {}
}
```

返回元素：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `user_id` | string/int | 用户 ID。 |
| `nickname` | string | 昵称。 |
| `remark` | string | 备注。 |

说明：当前主要由部分平台支持。

### 9.11 get_group_info 获取群信息

```json
{
  "action": "get_group_info",
  "params": {
    "group_id": "123456",
    "no_cache": false
  }
}
```

返回字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `group_id` | string/int | 群 ID。 |
| `group_name` | string | 群名。 |
| `member_count` | int | 成员数。 |
| `max_member_count` | int | 最大成员数。 |

### 9.12 get_group_list 获取群列表

```json
{
  "action": "get_group_list",
  "params": {
    "groups_id": "guild-id"
  }
}
```

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": [
    {
      "group_id": "123456",
      "group_name": "群名",
      "member_count": 0,
      "max_member_count": 0
    }
  ]
}
```

说明：

- `groups_id` 是扩展参数，用于部分频道/服务器式平台。
- 没有 `groups_id` 时，部分平台会尝试从 `passive_id` 上下文中获取。

### 9.13 get_group_member_info 获取群成员信息

```json
{
  "action": "get_group_member_info",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "no_cache": false
  }
}
```

返回常见字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `group_id` | string/int | 群 ID。 |
| `user_id` | string/int | 用户 ID。 |
| `nickname` | string | 昵称。 |
| `card` | string | 群名片。 |
| `sex` | string | 性别。 |
| `age` | int | 年龄。 |
| `area` | string | 地区。 |
| `join_time` | int | 入群时间。 |
| `last_sent_time` | int | 最后发言时间。 |
| `level` | string | 等级。 |
| `role` | string | `owner`、`admin`、`member`。 |
| `title` | string | 专属头衔。 |
| `avatar` | string | 头像 URL，部分平台返回。 |
| `groups_id` | string | 扩展字段，表示上级服务器/频道组 ID。 |

### 9.14 get_group_member_list 获取群成员列表

```json
{
  "action": "get_group_member_list",
  "params": {
    "group_id": "123456"
  }
}
```

返回为群成员信息数组。当前主要由部分平台支持。

### 9.15 get_group_honor_info 获取群荣誉信息

```json
{
  "action": "get_group_honor_info",
  "params": {
    "group_id": "123456",
    "type": "all"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 群 ID。 |
| `type` | string | 是 | 荣誉类型。可用值包括 `talkative`、`performer`、`legend`、`strong_newbie`、`emotion`、`all`。 |

返回常见字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `group_id` | string/int | 群 ID。 |
| `current_talkative` | object | 当前龙王，`type` 为 `talkative` 或 `all` 时可能存在。 |
| `talkative_list` | array | 历史龙王列表。 |
| `performer_list` | array | 群聊之火列表。 |
| `legend_list` | array | 群聊炽焰列表。 |
| `strong_newbie_list` | array | 冒尖小春笋列表。 |
| `emotion_list` | array | 快乐之源列表。 |

荣誉成员对象：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `user_id` | string/int | 用户 ID。 |
| `nickname` | string | 昵称。 |
| `avatar` | string | 头像 URL。 |
| `description` | string | 荣誉描述。 |
| `day_count` | int | 持续天数，仅部分对象存在。 |

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.16 set_group_kick 踢出群成员

```json
{
  "action": "set_group_kick",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "reject_add_request": false
  }
}
```

### 9.17 set_group_ban 禁言群成员

```json
{
  "action": "set_group_ban",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "duration": 1800
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 群或频道 ID。 |
| `user_id` | string/int | 是 | 目标用户。 |
| `duration` | int/string | 否 | 禁言秒数，默认 1800。0 通常表示解除。 |

### 9.18 set_group_anonymous_ban 禁言匿名群成员

```json
{
  "action": "set_group_anonymous_ban",
  "params": {
    "group_id": "123456",
    "anonymous": {},
    "anonymous_flag": "anonymous-flag",
    "duration": 1800
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 群 ID。 |
| `anonymous` | object | 二选一 | 事件中上报的匿名用户对象。 |
| `anonymous_flag` | string | 二选一 | 匿名用户 flag。 |
| `flag` | string | 二选一 | `anonymous_flag` 的别名。 |
| `duration` | int | 否 | 禁言秒数，默认 1800。匿名禁言通常无法取消。 |

说明：如果同时提供 `anonymous` 和 `anonymous_flag`，优先使用 `anonymous`。当前主要由 OneBot11 等连接端决定是否支持。

### 9.19 set_group_whole_ban 全员禁言

```json
{
  "action": "set_group_whole_ban",
  "params": {
    "group_id": "123456",
    "enable": true
  }
}
```

### 9.20 set_group_admin 设置管理员

```json
{
  "action": "set_group_admin",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "enable": true
  }
}
```

### 9.21 set_group_anonymous 设置群匿名

```json
{
  "action": "set_group_anonymous",
  "params": {
    "group_id": "123456",
    "enable": true
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 群 ID。 |
| `enable` | bool | 否 | 是否允许匿名聊天，默认 true。 |

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.22 set_group_card 设置群名片

```json
{
  "action": "set_group_card",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "card": "新名片"
  }
}
```

### 9.23 set_group_name 设置群名

```json
{
  "action": "set_group_name",
  "params": {
    "group_id": "123456",
    "group_name": "新群名"
  }
}
```

### 9.24 set_group_leave 退出群

```json
{
  "action": "set_group_leave",
  "params": {
    "group_id": "123456",
    "is_dismiss": false
  }
}
```

### 9.25 set_group_special_title 设置群专属头衔

```json
{
  "action": "set_group_special_title",
  "params": {
    "group_id": "123456",
    "user_id": "10000",
    "special_title": "头衔",
    "duration": -1
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `group_id` | string/int | 是 | 群 ID。 |
| `user_id` | string/int | 是 | 目标用户 ID。 |
| `special_title` | string | 否 | 专属头衔。空字符串表示删除。 |
| `duration` | int | 否 | 有效期秒数，默认 -1 表示永久。 |

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.26 set_friend_add_request 处理加好友请求

```json
{
  "action": "set_friend_add_request",
  "params": {
    "flag": "request-flag",
    "approve": true,
    "remark": "备注"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `flag` | string | 是 | 请求事件中的 flag。 |
| `approve` | bool | 否 | 是否同意，默认 true。 |
| `remark` | string | 否 | 同意后的好友备注。 |

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.27 set_group_add_request 处理加群请求或邀请

```json
{
  "action": "set_group_add_request",
  "params": {
    "flag": "request-flag",
    "sub_type": "add",
    "approve": true,
    "reason": ""
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `flag` | string | 是 | 请求事件中的 flag。 |
| `sub_type` | string | 是 | `add` 或 `invite`。 |
| `type` | string | 是 | `sub_type` 的别名。 |
| `approve` | bool | 否 | 是否同意，默认 true。 |
| `reason` | string | 否 | 拒绝理由，仅拒绝时有效。 |

说明：`sub_type` 和 `type` 二选一即可。当前主要由 OneBot11 等连接端决定是否支持。

### 9.28 set_msg_emoji_like 设置消息表情回应

```json
{
  "action": "set_msg_emoji_like",
  "params": {
    "message_id": "message-id",
    "group_id": "123456",
    "emoji_id": "128077"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `message_id` | string/int | 是 | 要回应的消息 ID。 |
| `emoji_id` | string/int | 是 | 表情 ID。 |
| `group_id` | string/int | 部分连接端必填 | 群 ID。部分 OneBot11 连接端需要此字段。 |

说明：

- 当前主要用于 OneBot11 平台。
- 不同 OneBot11 连接端对此 API 的原始字段命名可能不同，适配器会尽量转换。
- 其它平台当前不支持。

### 9.29 get_cookies 获取 Cookies

```json
{
  "action": "get_cookies",
  "params": {
    "domain": "qq.com"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `domain` | string | 否 | 需要获取 cookies 的域名。 |

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "cookies": "key=value"
  }
}
```

说明：Kook 有显式实现；OneBot11 等连接端也可能支持。

### 9.30 get_csrf_token 获取 CSRF Token

```json
{
  "action": "get_csrf_token",
  "params": {}
}
```

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "token": 123456
  }
}
```

说明：当前主要由 OneBot11 等连接端决定是否支持。

### 9.31 get_credentials 获取接口凭证

```json
{
  "action": "get_credentials",
  "params": {
    "domain": "qq.com"
  }
}
```

参数：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `domain` | string | 否 | 需要获取 cookies 的域名。 |

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "cookies": "key=value",
    "csrf_token": 123456
  }
}
```

说明：这是 `get_cookies` 和 `get_csrf_token` 的组合能力。当前主要由 OneBot11 等连接端决定是否支持。

### 9.32 get_status 获取连接状态

```json
{
  "action": "get_status",
  "params": {}
}
```

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "online": true,
    "good": true
  }
}
```

### 9.33 get_version_info 获取适配器版本

```json
{
  "action": "get_version_info",
  "params": {}
}
```

返回：

```json
{
  "status": "ok",
  "retcode": 0,
  "data": {
    "app_name": "redreply-adapter",
    "app_version": "0.0.1",
    "protocol_version": "v1"
  }
}
```

## 10. 平台能力矩阵

| action | OneBot11 | 官方 QQ | Kook | Telegram | 邮件 | Satori |
| --- | --- | --- | --- | --- | --- | --- |
| `send_private_msg` | 支持 | 被动支持 | 支持 | 支持 | 支持 | 支持 |
| `send_group_msg` | 支持 | 支持，受权限限制 | 支持 | 支持 | 不支持 | 支持 |
| `send_msg` | 支持 | 可使用明确 action | 支持 | 支持 | 不支持 | 由连接端决定 |
| `delete_msg` | 支持 | 支持 | 支持 | 支持 | 不支持 | 支持 |
| `get_msg` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 支持 |
| `get_forward_msg` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `send_like` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `get_login_info` | 支持 | 支持 | 支持 | 支持 | 不支持 | 支持 |
| `get_stranger_info` | 支持 | 支持 | 支持 | 不支持 | 不支持 | 支持 |
| `get_friend_list` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `get_group_info` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `get_group_list` | 支持 | 支持 | 支持 | 不支持 | 不支持 | 支持 |
| `get_group_member_info` | 支持 | 支持 | 支持 | 不支持 | 不支持 | 支持 |
| `get_group_member_list` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `get_group_honor_info` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_kick` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `set_group_ban` | 支持 | 支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_anonymous_ban` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_card` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `set_group_name` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `set_group_leave` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `set_group_whole_ban` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_admin` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_anonymous` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_special_title` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_friend_add_request` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_group_add_request` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `set_msg_emoji_like` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `get_cookies` | 支持 | 不支持 | 支持 | 不支持 | 不支持 | 不支持 |
| `get_csrf_token` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `get_credentials` | 支持 | 不支持 | 不支持 | 不支持 | 不支持 | 不支持 |
| `get_status` | 支持 | 未显式实现 | 支持 | 支持 | 不支持 | 不支持 |
| `get_version_info` | 支持 | 未显式实现 | 支持 | 支持 | 不支持 | 不支持 |

## 11. 被动回复与 message_id

`passive_id` 是 RedReply Adapter 为被动回复设计的上下文参数。

典型流程：

1. 平台收到用户消息。
2. 适配器保存平台原始事件。
3. 适配器生成 RedReply 事件，并在事件中放入 `message_id`。
4. 宿主需要回复时，把这个 `message_id` 作为 `passive_id` 传给 `call_api`。
5. 适配器根据 `passive_id` 找到平台需要的真实上下文。

重要规则：

- 事件中的 `message_id` 可以用于被动回复。
- 发送返回中的 `data.message_id` 可以用于撤回。
- 不要假设 `message_id` 等于平台真实消息 ID。
- 上下文有过期时间，过期后回复或撤回可能失败。

## 12. 官方 QQ 行为约定

### 12.1 会话能力

| 场景 | 规则 |
| --- | --- |
| Q 群 | 可以尝试主动发送；如果机器人没有对应权限，发送会返回失败。 |
| Q 单聊 | 只支持被动回复。 |
| 频道私信 | 只支持被动回复。 |

被动回复时，宿主需要把收到事件中的 `message_id` 作为 `passive_id` 传给 `call_api`。

### 12.2 消息发送结果

官方 QQ 的消息可能需要经过平台处理后才获得最终结果。对宿主来说，只需要关注 API 调用是否成功：

- API 成功返回时，`data.message_id` 可用于后续撤回。
- API 失败时，按返回的 `status`、`retcode`、`message` 或 `AdapterError` 处理。
- 平台超频、权限不足、内容审核失败等都可能导致发送失败。

### 12.3 富媒体消息

官方 QQ 支持发送图片、语音、视频等富媒体消息。不同媒体类型会受平台权限、格式和大小限制影响。

语音发送需要宿主提供可用的音频转换能力；如果无法转换为平台需要的格式，发送会失败。

### 12.4 多段消息顺序

对于：

```json
[
  {"type":"text","data":{"text":"111"}},
  {"type":"image","data":{"file":"https://example.com/a.png"}},
  {"type":"text","data":{"text":"222"}}
]
```

适配器会尽量按顺序发送为：

1. `111`
2. 图片
3. `222`

宿主不需要关心底层拆分细节。后续撤回时，传入发送返回中的 `message_id` 即可。

### 12.5 撤回

官方 QQ 支持撤回机器人已发送的消息，但受平台时间窗口和权限限制影响。

宿主调用 `delete_msg` 时，只需要传 RedReply Adapter 事件或发送结果中的 `message_id`。适配器会自行处理不同会话类型的撤回方式。

## 13. 官方 QQ Markdown

官方 QQ Markdown 通过 `qmarkdown` 消息段发送。

消息码写法：

```text
[CQ:qmarkdown,data=base64内容]
```

消息段写法：

```json
{
  "type": "qmarkdown",
  "data": {
    "data": "base64://base64内容"
  }
}
```

base64 解码后应是请求 JSON：

```json
{
  "markdown": {
    "content": "# 标题\n## 小标题\n正文"
  }
}
```

发送时适配器会构造 Markdown 消息。实际展示效果由官方 QQ 服务端决定。

## 14. 使用示例

### 14.1 发送群文本

```rust
let mut req = serde_json::json!({
    "action": "send_group_msg",
    "params": {
        "group_id": "group-id",
        "message": "你好"
    }
});

let ret = registry
    .call_api("qqgroup_public", "appid", "", &mut req)
    .await?;
```

### 14.2 发送图文混合消息

```rust
let mut req = serde_json::json!({
    "action": "send_group_msg",
    "params": {
        "group_id": "group-id",
        "message": [
            {"type":"text","data":{"text":"111"}},
            {"type":"image","data":{"file":"https://example.com/a.png"}},
            {"type":"text","data":{"text":"222"}}
        ]
    }
});
```

### 14.3 被动回复

```rust
let passive_id = event["message_id"].as_str().unwrap_or("");

let mut req = serde_json::json!({
    "action": "send_private_msg",
    "params": {
        "user_id": event["user_id"],
        "message": "收到"
    }
});

let ret = registry
    .call_api("qqgroup_public", "appid", passive_id, &mut req)
    .await?;
```

### 14.4 回复某条消息

```rust
let mut req = serde_json::json!({
    "action": "send_group_msg",
    "params": {
        "group_id": "group-id",
        "message": [
            {"type":"reply","data":{"id":"event-message-id"}},
            {"type":"text","data":{"text":"这是一条回复"}}
        ]
    }
});
```

### 14.5 撤回发送结果

```rust
let mut req = serde_json::json!({
    "action": "delete_msg",
    "params": {
        "message_id": send_ret["data"]["message_id"]
    }
});

let ret = registry
    .call_api("qqgroup_public", "appid", "", &mut req)
    .await?;
```

## 15. 兼容性说明

- 本文描述 RedReply Adapter 当前公开协议。
- 不同平台能力不同，适配器无法保证所有 action 在所有平台都可用。
- 官方 QQ 的主动消息、Markdown、富媒体、撤回都受服务端权限、审核、频率限制影响。
- `passive_id` 和消息映射有过期时间，长时间后回复或撤回可能失败。
