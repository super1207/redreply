# KOOK 与 Telegram 原生接口

这些命令使用红色问答中已连接的原生机器人账号。在对应平台的聊天或事件脚本中，平台和机器人 ID 来自当前事件。在 CRON 等没有消息来源的脚本中，可先通过 `【设置来源】` 指定机器人。

```text
【设置来源@机器人平台@telegram】
【设置来源@机器人ID@123456789】
```

把示例 ID 换成自己的机器人 ID。KOOK 的平台名为 `kook`。

## 命令说明

### Telegram调用

【Telegram调用@<font color="red">Bot API 方法</font>@<font color="red">参数 JSON 对象</font>@<font color="red">上传文件 JSON 对象</font>】

按 [Telegram Bot API](https://core.telegram.org/bots/api) 的字段名发送请求。方法名区分大小写。参数可省略。支持数组、布尔值和嵌套对象。文件 ID 和 URL 可直接填写在普通参数里。

需要上传本地内容时，第三个参数按表单字段名提供文件，例如 `{"photo":{"filename":"cover.png","base64":"……"}}`。Base64 可通过 `【BASE64编码@【读文件@路径】】` 得到。若原生参数需要引用另一个表单字段，使用 Telegram 的 `attach://字段名`。附件表单字段和普通参数的字段名应各自唯一。

### KOOK调用

【KOOK调用@<font color="red">HTTP 方法</font>@<font color="red">接口路径</font>@<font color="red">参数 JSON 对象</font>@<font color="red">上传文件 JSON 对象</font>】

按 [KOOK 开发者文档](https://developer.kookapp.cn/doc/reference) 选择 `GET` 或 `POST`。路径可写 `voice/list` 或 `/api/v3/voice/list`。GET 的参数作为查询参数，POST 默认发送 JSON。上传文件使用 POST 和与 Telegram 相同的文件对象格式。例如 `asset/create` 使用文件字段 `file`。

### 返回对象

| 字段 | 含义 |
| --- | --- |
| `status` | `ok` 或 `failed`。 |
| `retcode` | 成功为 `0`，平台错误保留原始错误码，本地调用失败为 `-1`。 |
| `data` | Telegram 的 `result` 或 KOOK 的 `data`，可用 `【取元素】` 读取。 |
| `message` | 平台返回的说明。 |
| `parameters` | Telegram 错误响应附带的参数，例如限流后的 `retry_after`。 |

原生调用使用平台原始 ID。Telegram 原生方法返回消息时，以及 KOOK 创建频道或私聊消息时，外层还提供 `redreply_message_id`，可用于 RedReply 的 OneBot 消息操作。脚本中的原生参数按 JSON 发送，例如 `show_alert` 填 JSON 的 `true` 或 `false`。

## Telegram 菜单与按钮

### 设置命令菜单

下面把 `/start` 和 `/help` 加入默认命令列表。命令字段省略斜杠。可通过 `scope` 指定范围，通过 `language_code` 设置某种语言。对应的查询和清除方法为 `getMyCommands` 和 `deleteMyCommands`。

```text
【定义变量@结果@【Telegram调用@setMyCommands@【@{
  "commands":[
    {"command":"start","description":"开始使用"},
    {"command":"help","description":"查看帮助"}
  ]
}】】】
【取元素@【变量@结果】@retcode】
```

用 `setChatMenuButton` 设置私聊输入框旁的菜单按钮。可选类型为 `commands`、`web_app` 或 `default`。省略 `chat_id` 时设置机器人默认按钮。用 `getChatMenuButton` 查询当前设置。

```text
【隐藏@【Telegram调用@setChatMenuButton@【@{"menu_button":{"type":"commands"}}】】】
```

### 发送行内键盘

下面使用当前聊天 ID。私聊中取发送者 ID，群聊中取群 ID。行内按钮还可使用 `url`、`web_app` 等官方按钮字段。

```text
【定义变量@聊天ID@【判空@【群ID】@【发送者ID】】】
【定义变量@参数@【文本替换@【@{
  "chat_id":"CHAT_ID","text":"请选择操作",
  "reply_markup":{"inline_keyboard":[[
    {"text":"下一页","callback_data":"next"},
    {"text":"帮助","url":"https://core.telegram.org/bots"}
  ]]}
}】@CHAT_ID@【变量@聊天ID】】】
【隐藏@【Telegram调用@sendMessage@【变量@参数】】】
```

### 接收按钮回调

新建「事件触发」脚本，关键词填 `notice:telegram:callback_query`。`【取元素@【事件内容】@telegram@data】` 是按钮的 `callback_data`。点击后应调用 `answerCallbackQuery` 结束客户端的等待状态。需要更新按钮时使用 `editMessageReplyMarkup`，传入回调中原始的 `message.chat.id` 和 `message.message_id`，或传入 `inline_message_id`。

```text
【定义变量@回调ID@【取元素@【事件内容】@telegram@id】】
【定义变量@回调参数@【文本替换@【@{"callback_query_id":"CALLBACK_ID","text":"已收到"}】@CALLBACK_ID@【变量@回调ID】】】
【隐藏@【Telegram调用@answerCallbackQuery@【变量@回调参数】】】
```

Telegram 的群聊和私聊消息继续走原来的消息触发，原始消息在 `【事件内容】` 的 `telegram` 字段。其余收到的更新可用 `notice:telegram:更新类型` 触发，例如 `inline_query`、`channel_post`、`poll_answer`。管理台的「订阅更新」接受逗号分隔的更新类型。直接编辑连接配置时，可用 `AllowedUpdates` 字符串数组指定订阅范围。例如 `["message","callback_query","chat_member"]`。默认使用 Telegram 的空数组订阅规则。适配器使用 `getUpdates` 轮询，同一个 Token 只应有一个收件程序；需要继续用本适配器接收更新时，应保持 webhook 关闭。

## KOOK 卡片与共享播放

### 发送卡片按钮

通过 `message/create` 发送频道卡片。`type` 为 `10`，`content` 是卡片数组序列化后的字符串。私聊可使用 `direct-message/create` 并按官方文档填写目标。下面用 `【转文本】` 为卡片内容生成 JSON 字符串。

```text
【定义变量@卡片@【@[
  {"type":"card","theme":"primary","size":"lg","modules":[
    {"type":"action-group","elements":[
      {"type":"button","theme":"primary","text":{"type":"plain-text","content":"确认"},
       "click":"return-val","value":"confirm"}
    ]}
  ]}
]】】
【定义变量@参数@【@{"type":10,"target_id":】【转文本@【群ID】】【@,"content":】【转文本@【变量@卡片】】【@}】】
【隐藏@【KOOK调用@POST@message/create@【变量@参数】】】
```

按钮事件使用「事件触发」关键词 `notice:kook:message_btn_click`。通过 `【取元素@【事件内容】@kook@value】` 读取按钮值，`【发送者ID】` 是点击者。原始 KOOK 消息 ID 可从 `kook_message_id` 读取。KOOK 的其他系统事件使用 `notice:kook:事件类型`，事件体位于 `kook`，网关原始事件位于 `kook_event`。

### KOOK共享播放

【KOOK共享播放@<font color="red">语音频道ID</font>@<font color="red">音源</font>@<font color="red">音量百分比</font>@<font color="red">频道密码</font>】

机器人加入指定语音频道，把音频推送给频道成员。音源可以是直连 HTTP(S) 地址或 RedReply 所在机器上的绝对文件路径。音量为 `0` 至 `100`，默认为 `100`。密码频道可填写最后一个参数。运行机器的 PATH 需包含支持 `libopus` 的 FFmpeg，机器人需要频道的加入和语音权限，媒体服务器的 UDP 端口需可达。

```text
【定义变量@播放结果@【KOOK共享播放@语音频道ID@https://example.org/song.mp3@65】】
【取元素@【变量@播放结果】@retcode】
```

每个机器人同时推送一条音源，也可以依次播放音源数组。命令在进程启动后返回；可用 `【KOOK播放状态】` 查询 `data.state`，状态为 `idle`、`playing`、`stopping`、`finished`、`stopped` 或 `failed`。`【KOOK停止播放】` 停止推流并离开频道。音源播完或适配器断开后也会离开频道。播放结束可用 `notice:kook:voice_playback` 触发脚本，通过 `kook.state` 查看结果，通过 `kook.channel_id` 读取语音频道 ID。

脚本若自行管理语音推流，可用 `【KOOK调用@GET@voice/list】` 和官方的 `voice/join`、`voice/keep-alive`、`voice/leave`。由红色问答管理的播放进行中，应通过 `【KOOK停止播放】` 结束当前推流。[KOOK 语音接口](https://developer.kookapp.cn/doc/http/voice) · [卡片消息](https://developer.kookapp.cn/doc/cardmessage) · [按钮事件](https://developer.kookapp.cn/doc/event/user)

## 进阶用法

### 沿用 RedLang 通用命令

- KOOK 和 Telegram 均可用 `【文件】`、`【视频】` 发送本地文件、字节集或 HTTP(S) 文件地址。Telegram 把文件和视频分别作为文档和视频发送。KOOK 使用文件或视频卡片。
- 两个平台的 `【表情回应@表情@消息ID】` 均可省略消息 ID，对当前消息执行。Telegram 接受平台支持的 Unicode 表情，KOOK 还接受官方自定义表情 ID。第三个参数填 `假` 可删除回应。KOOK 删除时需要指定表情，Telegram 会清除机器人在该消息上的回应。
- `【撤回】` 接受 RedReply 消息 ID。KOOK 根据消息来源选择频道或私聊删除接口，Telegram 根据消息 ID 映射找到对应聊天。
- Telegram 还可通过现有群成员信息查询获取用户及管理员身份，原始数据仍可从返回对象的 `telegram` 字段读取。
- Telegram 论坛话题、私聊话题和频道私信话题中的普通输出和 `【输出流】` 会保留原话题。原生调用可从 `【事件内容】` 读取 `message_thread_id` 或 `direct_messages_topic_id` 并显式传给 Bot API。话题回调也提供这些字段。

```text
【表情回应@👍】
【表情回应@👍@@假】
```

### Telegram 行内查询、投票和文件

用 BotFather 启用 inline mode。新建「事件触发」脚本，关键词为 `notice:telegram:inline_query`。用户的查询文本在 `telegram.query`。以下示例返回一条可选的文章结果。

```text
【定义变量@查询ID@【取元素@【事件内容】@telegram@id】】
【定义变量@参数@【@{"inline_query_id":】【转文本@【变量@查询ID】】【@,"cache_time":0,"results":[{"type":"article","id":"help","title":"帮助","input_message_content":{"message_text":"欢迎使用红色问答"}}]}】】
【隐藏@【Telegram调用@answerInlineQuery@【变量@参数】】】
```

通过 `sendPoll` 发起投票。要接收每位投票者的答案，发送非匿名投票并订阅 `poll_answer`，使用事件关键词 `notice:telegram:poll_answer`。从 `telegram.option_ids` 读取选项下标。

```text
【定义变量@聊天ID@【判空@【群ID】@【发送者ID】】】
【定义变量@参数@【@{"chat_id":】【转文本@【变量@聊天ID】】【@,"question":"下一次活动选哪天？","options":[{"text":"周六"},{"text":"周日"}],"is_anonymous":false}】】
【隐藏@【Telegram调用@sendPoll@【变量@参数】】】
```

【Telegram下载文件@<font color="red">file_id</font>】

通过 Telegram 的 `getFile` 获取下载路径，再使用当前机器人连接下载，返回字节集。下载上限为 20 MB，返回值可交给 `【写文件】` 或 `【文件】`。下面示例适用于收到文档的 Telegram 消息。

```text
【定义变量@文件ID@【取元素@【事件内容】@telegram@document@file_id】】
【写文件@【应用目录】telegram-download.bin@【Telegram下载文件@【变量@文件ID】】】
```

话题管理可调用 `createForumTopic`、`editForumTopic`、`closeForumTopic` 或 `reopenForumTopic`。Mini App、媒体组和消息回应可使用 `answerWebAppQuery`、`sendMediaGroup` 和 `setMessageReaction`。更新订阅、权限和字段以官方文档为准。[行内机器人](https://core.telegram.org/bots/inline) · [Telegram Bot API](https://core.telegram.org/bots/api)

### KOOK 播放列表和频道事件

原有 `【KOOK共享播放】` 的音源参数也接受 `【数组】`，列表最多 32 首。每首结束后释放语音连接，下一首使用新推流端口。原有停止命令会停止整份列表。状态和结束事件包含从 1 开始的 `track` 和总曲目数 `total`。

```text
【定义变量@音源@【数组@https://example.org/first.mp3@https://example.org/second.mp3】】
【隐藏@【KOOK共享播放@语音频道ID@【变量@音源】@65】】
```

用 `notice:kook:joined_channel` 和 `notice:kook:exited_channel` 处理语音成员进出。用 `notice:kook:added_reaction` 和 `notice:kook:deleted_reaction` 处理频道回应。此类事件的 `【发送者ID】` 是实际操作人，`【群ID】` 是发生事件的频道 ID。表情事件中，原消息可通过 `【消息ID】` 用于 RedReply 命令，原始 KOOK ID 位于 `kook_message_id`。

查询语音频道成员使用 `channel/user-list`。拥有对应权限的机器人还可使用 `channel/move-user` 移动成员、`channel/kickout` 踢出语音成员、`message/pin` 置顶消息，或通过 `guild-role/grant` 分配身份组。

```text
【定义变量@参数@【@{"channel_id":】【转文本@【群ID】】【@}】】
【定义变量@结果@【KOOK调用@GET@channel/user-list@【变量@参数】】】
【取长度@【取元素@【变量@结果】@data】】
```

[KOOK 频道接口](https://developer.kookapp.cn/doc/http/channel) · [频道事件](https://developer.kookapp.cn/doc/event/channel) · [用户事件](https://developer.kookapp.cn/doc/event/user)
