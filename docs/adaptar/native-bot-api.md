# KOOK 与 Telegram 原生接口

在对应平台的消息或事件脚本中，红色问答自动选用触发事件的机器人。CRON 等没有来源的脚本先用【设置来源】指定机器人。将示例 ID、文件路径和音源地址替换为实际值。

```text
【设置来源@机器人平台@telegram】
【设置来源@机器人ID@123456789】
```

KOOK 的平台名为 `kook`。

## 传参与返回值

【Telegram调用@<font color="red">Bot API 方法</font>@<font color="red">参数对象</font>@<font color="red">上传文件对象</font>】

【KOOK调用@<font color="red">GET 或 POST</font>@<font color="red">接口路径</font>@<font color="red">参数对象</font>@<font color="red">上传文件对象</font>】

参数和上传文件可以省略。按官方接口字段名用【对象】传参，嵌套列表用【数组】，文件内容用【读文件】等命令得到的字节集。KOOK 路径可写 `voice/list` 或 `/api/v3/voice/list`，上传使用 POST。Telegram 方法名区分大小写。脚本文本需要保留空格或包含 `@` 时，用 RedLang 原始字符串，例如 `【@活动通知 A】` 或 `【@@channel_name】`。

普通文本保留原样。若字段明确要求数字、布尔值或空值，使用【接口值】。例如 `【接口值@整数@12】`、`【接口值@小数@1.5】`、`【接口值@逻辑@假】`、`【接口值@空值】`。对 KOOK 频道 ID、Telegram 回调值、文件 ID 等文本字段直接传文本。Telegram 的 `chat_id` 支持整数和 `@username`，数字聊天 ID 可用【接口值】显式传整数。

| 返回字段 | 含义 |
| --- | --- |
| `status` | `ok` 或 `failed`。 |
| `retcode` | 成功为 `0`，平台拒绝时保留错误码，本地请求失败为 `-1`。 |
| `data` | Telegram 的 `result` 或 KOOK 的 `data`，可用【取元素】读取。 |
| `message` | 平台或适配器返回的说明。 |
| `parameters` | Telegram 附带的错误参数，例如限流后的 `retry_after`。 |
| `redreply_message_id` | Telegram 返回消息时或 KOOK 成功创建聊天消息时提供，可用于【撤回】等命令。 |

原生调用使用平台 ID。调用失败时先读取 `retcode` 和 `message`，成功后再读取 `data`。字段和权限要求见 [Telegram Bot API](https://core.telegram.org/bots/api) 和 [KOOK 开发者文档](https://developer.kookapp.cn/doc/reference)。

## Telegram 菜单

用 `setMyCommands` 设置输入框中的命令列表。命令名省略斜杠。`scope` 可以限定范围，`language_code` 可以限定界面语言。`getMyCommands` 和 `deleteMyCommands` 使用相同的范围和语言参数。

```text
【定义变量@命令列表@【数组
  @【对象@command@start@description@开始使用】
  @【对象@command@help@description@查看帮助】
】】
【定义变量@结果@【Telegram调用@setMyCommands@【对象
  @commands@【变量@命令列表】
  @scope@【对象@type@all_private_chats】
  @language_code@zh
】】】
【取元素@【变量@结果】@retcode】
```

用 `setChatMenuButton` 设置私聊输入框旁的菜单按钮。省略 `chat_id` 会设置机器人的默认按钮。`commands` 打开命令列表，`web_app` 可指定 HTTPS Mini App，`default` 恢复默认行为。用 `getChatMenuButton` 查询设置。

```text
【隐藏@【Telegram调用@setChatMenuButton@【对象
  @menu_button@【对象@type@web_app@text@打开服务@web_app@【对象@url@https://example.org/app】】
】】】
```

[机器人命令菜单](https://core.telegram.org/bots/api#setmycommands) · [私聊菜单按钮](https://core.telegram.org/bots/api#setchatmenubutton)

## Telegram 行内键盘与回复键盘

【输出流】的第二个参数在 Telegram 上可以传键盘对象，消息仍发到当前聊天和话题。多段消息的键盘附在最后一条。行内键盘的外层数组装所有行，每行也是一个数组。每个按钮提供 `text` 和一种动作，例如 `callback_data` 或 `url`。

```text
【定义变量@键盘@【对象@inline_keyboard@【数组
  @【数组@【对象@text@下一页@callback_data@next】@【对象@text@帮助@url@https://core.telegram.org/bots】】
  @【数组@【对象@text@关闭@callback_data@close】】
】】】
【隐藏@【输出流@请选择操作@【变量@键盘】】】
```

回复键盘出现在输入框附近，点击普通文本按钮会发送一条消息，仍用消息脚本处理。可用 `remove_keyboard` 移除回复键盘。

```text
【隐藏@【输出流@请选择@【对象
  @keyboard@【数组@【数组@【对象@text@签到】@【对象@text@帮助】】】
  @resize_keyboard@【接口值@逻辑@真】
】】】
```

新建「事件触发」脚本，关键词填 `notice:telegram:callback_query`。从 `telegram.data` 读取按钮值，用 `answerCallbackQuery` 结束客户端等待。用 `editMessageReplyMarkup` 更新行内键盘时，从 `telegram.message` 取原生聊天和消息 ID，或使用事件中的 `inline_message_id`。

```text
【定义变量@回调ID@【取元素@【事件内容】@telegram@id】】
【隐藏@【Telegram调用@answerCallbackQuery@【对象@callback_query_id@【变量@回调ID】@text@已收到】】】
```

[行内键盘](https://core.telegram.org/bots/api#inlinekeyboardmarkup) · [回复键盘](https://core.telegram.org/bots/api#replykeyboardmarkup) · [按钮回调](https://core.telegram.org/bots/api#answercallbackquery)

## Telegram 查询、投票与文件

### 行内查询

在 BotFather 开启 inline mode。用户在聊天中输入机器人的用户名和搜索内容时，用事件关键词 `notice:telegram:inline_query` 接收请求。查询文本在 `telegram.query`，偏移量在 `telegram.offset`。以下示例返回一条文章。分页时在应答中提供 `next_offset`，按该值继续返回结果。

```text
【定义变量@查询ID@【取元素@【事件内容】@telegram@id】】
【隐藏@【Telegram调用@answerInlineQuery@【对象
  @inline_query_id@【变量@查询ID】
  @cache_time@【接口值@整数@0】
  @results@【数组@【对象
    @type@article@id@help@title@帮助
    @input_message_content@【对象@message_text@欢迎使用红色问答】
  】】
】】】
```

### 聊天、成员和头像查询

用 `getChat` 查询聊天资料，`getChatMember` 查指定成员，`getChatMemberCount` 查人数，`getChatAdministrators` 查管理员。`getUserProfilePhotos` 可按 `offset` 和 `limit` 查询头像。Telegram Bot API 未提供群成员全量列表或聊天历史搜索接口。已有的【BOT权限】继续查询机器人在当前群的角色。

```text
【定义变量@结果@【Telegram调用@getChatAdministrators@【对象@chat_id@【接口值@整数@【群ID】】】】】
【取长度@【取元素@【变量@结果】@data】】
```

### 发起及结束投票

用 `sendPoll` 发起投票。示例在当前群发起非匿名投票。要接收逐人答案，使用 `notice:telegram:poll_answer`，从 `telegram.option_ids` 读取从 0 开始的选项下标。用 `notice:telegram:poll` 接收投票状态；用 `stopPoll` 和返回的原生 `data.message_id` 结束投票。要发往论坛话题，可在发送参数中加上整数 `message_thread_id`。

```text
【定义变量@投票@【Telegram调用@sendPoll@【对象
  @chat_id@【接口值@整数@【群ID】】
  @question@下一次活动选哪天？
  @options@【数组@【对象@text@周六】@【对象@text@周日】】
  @is_anonymous@【接口值@逻辑@假】
】】】
【取元素@【变量@投票】@data@poll@id】
```

保存 `data.chat.id` 和 `data.message_id` 后可在另一条脚本中结束投票。下面的两个占位值替换为已保存的 ID。

```text
【定义变量@结果@【Telegram调用@stopPoll@【对象
  @chat_id@【接口值@整数@-1001234567890】
  @message_id@【接口值@整数@128】
】】】
【取元素@【变量@结果】@data@total_voter_count】
```

测验模式填写 `type=quiz`，并用整数数组 `correct_option_ids` 指定正确选项。允许多选的普通投票可设置 `allows_multiple_answers`。

### 发送、上传与下载文件

普通发送继续用【文件】和【视频】，支持本地路径、字节集和 HTTP(S) 文件地址。需要复用 Telegram `file_id`、传标题或设置附件时，用对应的原生方法。每个上传字段的值是包含 `filename` 和 `内容` 的对象。`内容` 为字节集。若方法要求引用不同名称的附件，用 `attach://字段名`；同一个字段名只放在普通参数或上传文件对象的其中一处。

```text
【定义变量@结果@【Telegram调用@sendDocument@【对象
  @chat_id@【接口值@整数@【群ID】】
  @caption@今日文件
】@【对象@document@【对象@filename@说明.pdf@内容@【读文件@【应用目录】说明.pdf】】】】】
【取元素@【变量@结果】@data@document@file_id】
```

【Telegram下载文件@<font color="red">file_id</font>】

用当前机器人下载并返回字节集，云端 Bot API 下载上限 20 MB。以下示例适用于收到 Telegram 文档的消息脚本。

```text
【定义变量@文件ID@【取元素@【事件内容】@telegram@document@file_id】】
【写文件@【应用目录】telegram-download.bin@【Telegram下载文件@【变量@文件ID】】】
```

[行内机器人](https://core.telegram.org/bots/inline) · [查询聊天](https://core.telegram.org/bots/api#getchat) · [投票](https://core.telegram.org/bots/api#sendpoll) · [文件](https://core.telegram.org/bots/api#getfile)

### 话题、审核与更新订阅

- 普通输出和【输出流】会保留当前 Telegram 话题。原生调用可从【事件内容】读取 `message_thread_id` 或 `direct_messages_topic_id` 并按方法要求传入。论坛话题可用 `createForumTopic`、`editForumTopic`、`closeForumTopic`、`reopenForumTopic` 管理。
- 用 `notice:telegram:chat_join_request` 接收普通入群申请。在该脚本中使用【同意】或【拒绝】。机器人需要邀请用户的管理权限。带验证流程的 `chat_join_request_query` 可按官方方法调用 `answerChatJoinRequestQuery`。
- 群聊和私聊消息走原有消息触发。其他更新走 `notice:telegram:更新类型`，原始数据位于 `telegram`。在管理台「订阅更新」中按逗号填写 `message,callback_query,inline_query,poll,poll_answer,chat_join_request` 等类型。默认采用 Telegram 的空列表订阅规则。适配器用 `getUpdates` 收件，使用该适配器时请保持同一个 Token 的 webhook 关闭。
- 媒体组、Mini App 应答、邀请链接、置顶和权限管理可用【Telegram调用】配合官方方法 `sendMediaGroup`、`answerWebAppQuery`、`createChatInviteLink`、`pinChatMessage`、`setChatPermissions`。

[入群审核](https://core.telegram.org/bots/api#approvechatjoinrequest) · [更新订阅](https://core.telegram.org/bots/api#getupdates)

## KOOK 卡片、文件与共享播放

### 发送交互卡片

【KOOK卡片@<font color="red">卡片数组</font>】

每个卡片是 `type=card` 的对象，`modules` 是模块数组。一条消息最多 5 张卡片，合计最多 50 个模块，每个交互模块最多 4 个按钮。用【输出流】发往当前频道或私聊。

```text
【定义变量@卡片@【数组@【对象@type@card@theme@primary@size@lg
  @modules@【数组
    @【对象@type@section@text@【对象@type@plain-text@content@请选择操作】】
    @【对象@type@action-group@elements@【数组
      @【对象@type@button@theme@primary@text@确认@click@return-val@value@confirm】
      @【对象@type@button@text@说明@click@link@value@https://developer.kookapp.cn】
    】】
  】
】】】
【隐藏@【输出流@【KOOK卡片@【变量@卡片】】】】
```

向指定目标发卡片可用 `message/create` 或 `direct-message/create`，把卡片数组直接放到 `content`。创建消息时自动使用卡片类型；用 `message/update`、`direct-message/update` 更新已有卡片，传原生 `msg_id`。帖子频道的 `thread/create` 也可用卡片数组作为 `content`。

```text
【隐藏@【KOOK调用@POST@message/create@【对象
  @target_id@频道ID
  @content@【数组@【对象@type@card@modules@【数组@【对象@type@section@text@公告内容】】】】
】】】
```

点击 `return-val` 按钮触发 `notice:kook:message_btn_click`。从 `kook.value` 取按钮值，【发送者ID】是点击者。事件中的 `kook_message_id` 是 KOOK 原生消息 ID，【消息ID】用于 RedReply 消息操作。

[KOOK 卡片规范](https://developer.kookapp.cn/doc/cardmessage) · [频道消息](https://developer.kookapp.cn/doc/http/message) · [私聊消息](https://developer.kookapp.cn/doc/http/direct-message) · [帖子](https://developer.kookapp.cn/doc/http/thread)

### 文件上传和收发

用【文件】直接发到当前频道或私聊，发送时会将文件上传至 KOOK，文件名会随上传传递。要先取得资源链接，再把它用于卡片、封面或帖子，用 `asset/create`，上传字段名为 `file`。收到带文件模块的频道卡片时可使用 `notice:group_upload`，从 `file.url`、`file.name` 取文件。频道或私聊消息中的原始卡片也可从【事件内容】的 `kook` 字段读取。

```text
【隐藏@【输出流@【文件@【应用目录】说明.pdf@活动说明.pdf】】】
```

```text
【定义变量@上传@【KOOK调用@POST@asset/create@@【对象
  @file@【对象@filename@活动说明.pdf@内容@【读文件@【应用目录】说明.pdf】】
】】】
【取元素@【变量@上传】@data@url】
```

[KOOK 媒体模块](https://developer.kookapp.cn/doc/http/asset) · [卡片文件模块](https://developer.kookapp.cn/doc/cardmessage)

### 共享播放与语音管理

【KOOK共享播放@<font color="red">语音频道ID</font>@<font color="red">音源</font>@<font color="red">音量百分比</font>@<font color="red">频道密码</font>】

机器人加入指定语音频道，把音频推送给频道成员。音源可为 HTTP(S) 直连地址、本地绝对文件路径或最多 32 首的【数组】。音量为 0 至 100，默认 100，频道密码可省略。运行机器的 PATH 需包含支持 `libopus` 的 FFmpeg，机器人需有加入和语音权限，媒体服务器的 UDP 端口需可达。

```text
【定义变量@音源@【数组@https://example.org/first.mp3@https://example.org/second.mp3】】
【隐藏@【KOOK共享播放@语音频道ID@【变量@音源】@65】】
```

【KOOK播放状态】返回 `data.state`、当前曲目 `track` 和总数 `total`。曲目从 1 开始。状态包括 `idle`、`playing`、`stopping`、`finished`、`stopped`、`failed`。【KOOK停止播放】停止整份列表并离开频道。每首开始时触发 `notice:kook:voice_track_started`，可读 `kook.track`、`kook.total` 和 `kook.channel_id`。列表结束时触发 `notice:kook:voice_playback`，可读 `kook.state` 和 `kook.channel_id`。

```text
【取元素@【KOOK播放状态】@data@state】
```

用 `voice/list` 查询机器人加入的频道，用 `channel/user-list` 查指定语音频道成员，用 `channel/move-user` 和 `channel/kickout` 管理语音成员。成员进出通知为 `notice:kook:joined_channel` 和 `notice:kook:exited_channel`。自行推流时可通过 `voice/join`、`voice/keep-alive`、`voice/leave` 管理连接。红色问答正在管理播放时用【KOOK停止播放】结束推流。

```text
【定义变量@成员@【KOOK调用@GET@channel/user-list@【对象@channel_id@语音频道ID】】】
【取长度@【取元素@【变量@成员】@data】】
```

[KOOK 语音接口](https://developer.kookapp.cn/doc/http/voice) · [频道管理](https://developer.kookapp.cn/doc/http/channel)

### 频道历史、角色与消息操作

- 用 `message/list`、`message/view` 查询频道消息，`direct-message/list`、`direct-message/view` 查询私聊消息。频道置顶用 `message/pin` 和 `message/unpin`。查询帖子用 `thread/list`、`thread/view`、`thread/post`。
- 用 `guild-role/grant`、`guild-role/revoke` 分配或移除身份组。频道权限可查询 `channel-role/index`，再按官方文档更新。
- KOOK 与 Telegram 都可继续用【撤回】和【表情回应】处理消息。KOOK 的 `notice:kook:added_reaction` 和 `notice:kook:deleted_reaction` 提供原生回应事件。

```text
【定义变量@历史@【KOOK调用@GET@message/list@【对象@target_id@【群ID】@page_size@【接口值@整数@10】】】】
【取长度@【取元素@【变量@历史】@data@items】】
```

[KOOK 官方接口目录](https://developer.kookapp.cn/doc/reference) · [频道历史](https://developer.kookapp.cn/doc/http/message) · [频道事件](https://developer.kookapp.cn/doc/event/channel)


### 用 KOOK 卡片发起投票

KOOK 的公开 Bot API 提供卡片按钮及其回调。用下面的脚本在频道发起投票。

```text
【隐藏@【输出流@【KOOK卡片@【数组@【对象@type@card@modules@【数组
  @【对象@type@section@text@下次活动选哪天？】
  @【对象@type@action-group@elements@【数组
    @【对象@type@button@text@周六@click@return-val@value@poll-weekend-sat】
    @【对象@type@button@text@周日@click@return-val@value@poll-weekend-sun】
  】】
】】】】】】
```

用 `notice:kook:message_btn_click` 建立事件脚本，用同一张卡片的原生消息 ID 和投票人 ID 保存选择。下例把每张卡片作为一个配置节，同一人再次点击会覆盖原选择。

```text
【定义变量@选择@【取元素@【事件内容】@kook@value】】
【定义变量@投票ID@【取元素@【事件内容】@kook_message_id】】
【加锁@kook-votes.ini】
【判等@【变量@选择】@poll-weekend-sat@
  【判等@【变量@选择】@poll-weekend-sun@@【写配置@【应用目录】kook-votes.ini@【变量@投票ID】@【发送者ID】@周日】】
  @【写配置@【应用目录】kook-votes.ini@【变量@投票ID】@【发送者ID】@周六】
】
【解锁@kook-votes.ini】
```

【加锁】和【解锁】避免同包中的投票事件同时写配置。统计脚本使用同一个锁，可用 `【读配置键@【应用目录】kook-votes.ini@原生消息ID】` 读取各人的选择，再用【循环】按选项计数。[KOOK 按钮事件](https://developer.kookapp.cn/doc/event/user)
