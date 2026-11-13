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

每个机器人同时播放一条音源。命令在进程启动后返回；可用 `【KOOK播放状态】` 查询 `data.state`，状态为 `idle`、`playing`、`stopping`、`finished`、`stopped` 或 `failed`。`【KOOK停止播放】` 停止推流并离开频道。音源播完或适配器断开后也会离开频道。播放结束可用 `notice:kook:voice_playback` 触发脚本，通过 `kook.state` 查看结果，通过 `kook.channel_id` 读取语音频道 ID。

脚本若自行管理语音推流，可用 `【KOOK调用@GET@voice/list】` 和官方的 `voice/join`、`voice/keep-alive`、`voice/leave`。由红色问答管理的播放进行中，应通过 `【KOOK停止播放】` 结束当前推流。[KOOK 语音接口](https://developer.kookapp.cn/doc/http/voice) · [卡片消息](https://developer.kookapp.cn/doc/cardmessage) · [按钮事件](https://developer.kookapp.cn/doc/event/user)
