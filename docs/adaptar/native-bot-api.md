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

### 命令菜单

- 【Telegram命令菜单@设置@命令数组@范围@语言@聊天ID@成员ID】
- 【Telegram命令菜单@查询@范围@语言@聊天ID@成员ID】
- 【Telegram命令菜单@删除@范围@语言@聊天ID@成员ID】

| 参数 | 怎么填 |
| --- | --- |
| 操作 | 必填 `设置`、`查询` 或 `删除`。 |
| 命令数组 | 设置时必填，每项是【数组@命令名@说明】，最多 100 项。名称可以带开头的斜杠，主体用 1 至 32 个小写字母、数字或下划线。说明填 1 至 256 字。 |
| 范围 | 可省略，默认对所有用户生效。可填 `默认`、`所有私聊`、`所有群聊`、`所有群管理员`、`指定聊天`、`指定群管理员` 或 `指定群成员`。 |
| 语言 | 可省略，表示该范围内没有专门语言设置时使用此菜单。简体中文常用 `zh`，英文用 `en`。 |
| 聊天 ID | `指定聊天`、`指定群管理员` 或 `指定群成员` 时必填，其余范围省略。用户名须带 `@`，脚本里写作【@@名称】。 |
| 成员 ID | 仅 `指定群成员` 时必填。 |

设置、删除成功时没有输出。查询返回命令数组，每项仍是命令名和说明，可以交给另一条设置命令。删除时填写与设置相同的范围和语言。

```text
【Telegram命令菜单@设置@【数组
  @【数组@start@开始使用】
  @【数组@help@查看帮助】
】@所有私聊@zh】
```

```text
【取长度@【Telegram命令菜单@查询@所有私聊@zh】】
```

### 私聊菜单按钮

【Telegram菜单按钮@类型@私聊ID@按钮文字@小程序链接】

| 参数 | 怎么填 |
| --- | --- |
| 类型 | 必填 `命令`、`默认`、`小程序` 或 `查询`。分别用于打开命令菜单、恢复默认、打开小程序和读取当前设置。 |
| 私聊 ID | 可省略，省略时操作机器人的默认按钮。指定单个私聊时填数字 ID。 |
| 按钮文字、小程序链接 | 类型为 `小程序` 时必填，链接用 HTTPS。其他类型省略。 |

查询返回三个位置，依次为类型、按钮文字和链接，普通按钮的后两项为空。

```text
【Telegram菜单按钮@小程序@@打开服务@https://example.org/app】
```

[Telegram 命令菜单](https://core.telegram.org/bots/api#setmycommands) · [私聊菜单按钮](https://core.telegram.org/bots/api#setchatmenubutton)

## Telegram 行内键盘与回复键盘

【Telegram键盘@按钮行数组@模式@自动缩放@用后隐藏】

| 参数 | 怎么填 |
| --- | --- |
| 按钮行数组 | 每行用一个【数组】。行内按钮用【数组@文字@值@动作】，动作默认为 `回调`，也可填 `链接` 或 `小程序`。回调值限 1 至 64 字节，链接填 HTTP(S) 或 `tg://`，小程序链接填 HTTPS。回复键盘每行直接填按钮文字。移除模式此项留空。 |
| 模式 | 可省略，默认 `行内`。可填 `回复` 或 `移除`。小程序按钮仅用于机器人私聊。 |
| 自动缩放、用后隐藏 | 仅回复模式使用，可省略，分别默认为 `真`、`假`。 |

把键盘放入【输出流】第二个参数。多段消息时键盘附在最后一段。

```text
【定义变量@按钮行@【数组
  @【数组@【数组@下一页@next】@【数组@帮助@https://core.telegram.org/bots@链接】】
  @【数组@【数组@关闭@close】】
】】
【隐藏@【输出流@请选择操作@【Telegram键盘@【变量@按钮行】】】】
```

```text
【隐藏@【输出流@请选择@【Telegram键盘@【数组@【数组@签到@帮助】】@回复】】】
【隐藏@【输出流@操作完成@【Telegram键盘@@移除】】】
```

【Telegram应答按钮@提示@是否弹窗@回调ID】用于结束按钮点击后的等待。提示可省略，最多 200 字。是否弹窗默认 `假`。在 `notice:telegram:callback_query` 事件中可省略回调 ID。用【事件内容@按钮值】读取点击值。更新现有行内键盘时，用【编辑消息@消息ID@新键盘@键盘】，将新键盘留空可清除按钮。

```text
【Telegram应答按钮@已收到】
【事件内容@按钮值】
```

[Telegram 行内键盘](https://core.telegram.org/bots/api#inlinekeyboardmarkup) · [回复键盘](https://core.telegram.org/bots/api#replykeyboardmarkup) · [按钮回调](https://core.telegram.org/bots/api#answercallbackquery)

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

【Telegram投票@题目@选项数组@是否匿名@是否多选@正确选项@话题ID】

| 参数 | 怎么填 |
| --- | --- |
| 题目 | 必填，1 至 300 字。 |
| 选项数组 | 必填，用【数组】直接列出 1 至 12 个选项，每项 1 至 100 字。 |
| 是否匿名、是否多选 | 均可省略，分别默认为 `真`、`假`。 |
| 正确选项 | 可省略，省略时发普通投票。填写从 1 开始的选项序号即可发测验。多个正确答案用【数组】，同时将是否多选填 `真`。 |
| 话题 ID | 可省略，默认沿用当前论坛话题。此命令向当前聊天发送。 |

【Telegram投票】返回 RedReply 消息 ID。【Telegram结束投票@消息ID】使用这个 ID 结束投票并返回总人数。ID 省略时使用当前消息。RedReply 消息 ID 依赖适配器的消息缓存，重启后应通过平台接口和原始消息 ID 操作旧投票。

```text
【定义变量@投票消息@【Telegram投票@下次活动选哪天？@【数组@周六@周日】@假】】
【变量@投票消息】
```

```text
【Telegram结束投票@已保存的投票消息ID】
```

非匿名投票可用 `notice:telegram:poll_answer` 接收个人答案。【事件内容@投票选项】返回从 1 开始的选项序号数组。撤回选择时返回空数组。用 `notice:telegram:poll` 接收状态并用【事件内容@投票人数】读取总人数。

[Telegram 投票](https://core.telegram.org/bots/api#sendpoll) · [结束投票](https://core.telegram.org/bots/api#stoppoll)

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
- 用 `notice:telegram:chat_join_request` 接收普通入群申请。在该脚本中使用【同意】或【拒绝】。机器人需要邀请用户的管理权限。带 `telegram.query_id` 的入群申请也可用【同意】或【拒绝】，平台要求在 10 秒内处理。
- 群聊和私聊消息走原有消息触发。其他更新走 `notice:telegram:更新类型`，原始数据位于 `telegram`。在管理台「订阅更新」中按逗号填写 `message,callback_query,inline_query,poll,poll_answer,chat_join_request` 等类型。默认采用 Telegram 的空列表订阅规则。适配器用 `getUpdates` 收件，使用该适配器时请保持同一个 Token 的 webhook 关闭。
- Mini App 应答、邀请链接、置顶和权限管理可用【Telegram调用】配合官方方法 `answerWebAppQuery`、`createChatInviteLink`、`pinChatMessage`、`setChatPermissions`。

[入群审核](https://core.telegram.org/bots/api#approvechatjoinrequest) · [更新订阅](https://core.telegram.org/bots/api#getupdates)

## 媒体组、贴纸和自定义表情

### 发送媒体组

【媒体组@<font color="red">媒体数组</font>】

传入包含 2 至 10 项的【数组】。每项按 `【数组@类型@来源@说明@文件名】` 填写，再用【输出流】发送媒体组。

| 位置 | 参数 | 填写方法 |
| --- | --- | --- |
| 1 | 类型 | 填 `图片`、`视频`、`音频`、`文件`，Telegram 还可以填 `实况照片`。 |
| 2 | 来源 | 填 HTTP(S) 链接、【读文件】返回的字节集，或 Telegram 可复用的文件 ID。实况照片这里填视频。 |
| 3 | 说明 | 可省略。KOOK 用作媒体标题或图片说明。 |
| 4 | 文件名 | 来源为字节集时必填，例如 `活动照片.png`。链接和文件 ID 省略。 |
| 5、6 | 实况照片的静态图来源和文件名 | 仅 Telegram 实况照片使用。静态图填文件 ID 或字节集，用字节集时再填文件名。实况照片的视频和静态图都须用文件 ID 或字节集。 |

```text
【定义变量@相册@【数组
  @【数组@图片@https://example.org/first.png@活动现场】
  @【数组@图片@【读文件@【应用目录】第二张.png】@@第二张.png】
】】
【隐藏@【输出流@【媒体组@【变量@相册】】】】
```

Telegram 可混发图片、视频和实况照片，文件组与音频组各自只接受同类媒体。相册继承【输出流】的目标话题，可使用【撤回】处理返回的消息 ID。KOOK 把媒体组合成一条卡片，连续无说明图片按每组最多 9 张排列。需要自定义卡片版式时使用【KOOK卡片】。Telegram 静默发送、业务连接等专用参数可按官方字段使用【Telegram调用】。

[Telegram 媒体组](https://core.telegram.org/bots/api#sendmediagroup) · [KOOK 卡片媒体](https://developer.kookapp.cn/doc/cardmessage)

### 发送 Telegram 贴纸

【贴纸@<font color="red">来源</font>@<font color="red">文件名</font>@<font color="red">关联表情</font>】

| 位置 | 参数 | 填写方法 |
| --- | --- | --- |
| 1 | 来源 | 填贴纸文件 ID、静态 WebP 的 HTTP(S) 链接，或【读文件】返回的字节集。 |
| 2 | 文件名 | 上传字节集时必填，后缀可用 `.webp`、`.tgs` 或 `.webm`。 |
| 3 | 关联表情 | 上传字节集时可选，例如 `🎉`。 |

```text
【隐藏@【输出流@【贴纸@【读文件@【应用目录】庆祝.webp】@庆祝.webp@🎉】】】
```

收到 Telegram 贴纸时，`telegram.sticker.file_id` 是可重用的文件 ID，`telegram.sticker.set_name` 是贴纸包名。用【Telegram下载文件】保存贴纸。

```text
【定义变量@贴纸文件ID@【取元素@【事件内容】@telegram@sticker@file_id】】
【隐藏@【输出流@【贴纸@【变量@贴纸文件ID】】】】
```

通过【Telegram调用】的 `getStickerSet` 查询贴纸包，用 `uploadStickerFile` 上传贴纸素材，再用 `createNewStickerSet`、`addStickerToSet` 或 `replaceStickerInSet` 修改机器人创建的包。贴纸包短名须以 `_by_机器人用户名` 结尾。KOOK 的公开机器人接口提供服务器表情和媒体卡片，图片形式可用【图片】发送。

[Telegram 贴纸与贴纸包](https://core.telegram.org/bots/api#stickers) · [KOOK KMarkdown 表情](https://developer.kookapp.cn/doc/kmarkdown)

### 自定义表情回应

【表情回应@<font color="red">表情</font>@<font color="red">消息 ID</font>@<font color="red">是否添加</font>@<font color="red">类型</font>@<font color="red">是否放大</font>@<font color="red">用户 ID</font>@<font color="red">聊天 ID</font>】

| 位置 | 参数 | 填写方法 |
| --- | --- | --- |
| 1 | 表情 | 普通回应填表情字符。Telegram 自定义回应填数字样式的自定义表情 ID，KOOK 填 `服务器ID/表情ID`。 |
| 2 | 消息 ID | 填 RedReply 消息 ID，省略时使用当前消息。 |
| 3 | 是否添加 | 默认为添加，填 `假` 删除。KOOK 删除时仍要填第 1 项，Telegram 清除自身回应时可省略第 1 项。 |
| 4 | 类型 | Telegram 的自定义回应填 `自定义`，普通回应可省略或填 `普通`。KOOK 的自定义 ID 直接填第 1 项即可。 |
| 5 | 是否放大 | Telegram 添加回应时可填 `真` 使用大动画。 |
| 6 | 用户 ID | 管理员删除该用户的回应时填写，且第 3 项填 `假`。 |
| 7 | 聊天 ID | Telegram 管理员针对以聊天身份回应的操作者时填写，与第 6 项只填其中一个。 |

```text
【表情回应@5421234567890123456@@真@自定义@真】
```

```text
【表情回应@服务器ID/表情ID】
```

Telegram 机器人可设一项回应，自定义表情须已用于该消息或被管理员明确允许。可用【Telegram调用】的 `getChat` 查询 `available_reactions`，用 `getCustomEmojiStickers` 查表情资料。KOOK 使用【KOOK调用】的 `guild-emoji/list`、`guild-emoji/create`、`guild-emoji/update`、`guild-emoji/delete` 管理服务器表情，或用 `message/reaction-list`、`direct-message/reaction-list` 查询回应列表。新表情上传使用 `emoji` 附件字段，文件为 256 KB 以内的 PNG。

[Telegram 回应](https://core.telegram.org/bots/api#setmessagereaction) · [管理员删除回应](https://core.telegram.org/bots/api#deletemessagereaction) · [KOOK 服务器表情](https://developer.kookapp.cn/doc/http/guild-emoji)

### 媒体与回应事件

| 事件关键词 | 数据 |
| --- | --- |
| `notice:telegram:media_group` | 每项媒体各触发一次。外层 `media_group_id` 可与 `chat_id` 一起分组，业务消息还应区分 `telegram.business_connection_id`。外层 `media` 提供媒体类型及可复用的文件 ID。Telegram 没有整组结束更新。 |
| `notice:telegram:sticker` | `telegram.sticker` 提供类型、文件 ID、贴纸包等信息。原消息也继续进入普通消息或原生频道、业务事件。 |
| `notice:telegram:message_reaction` | `telegram.old_reaction`、`telegram.new_reaction`。匿名操作者用外层 `actor_chat_id`。 |
| `notice:telegram:message_reaction_count` | `telegram.reactions` 是各回应及总数。匿名统计可能延迟。 |
| `notice:kook:media_group` | 含至少两项可识别媒体的卡片触发一次，外层 `media` 是媒体数组，原卡片仍在 `kook`。 |
| `notice:kook:added_reaction`、`notice:kook:deleted_reaction`、`notice:kook:private_added_reaction`、`notice:kook:private_deleted_reaction` | `emoji_id` 是回应 ID，`kook.emoji` 保留原生对象。【消息ID】指向被回应的消息。 |
| `notice:kook:added_emoji`、`notice:kook:updated_emoji`、`notice:kook:removed_emoji` | `kook.id`、`kook.name` 和【当前群组】。 |

Telegram 的两个回应更新需要机器人具备管理员权限，并在管理台的「订阅更新」中显式加入 `message_reaction,message_reaction_count`。处理媒体时订阅相应的 `message`、`channel_post` 或 `business_message`。处理回应事件时用【事件内容】读取数据，例如在 `notice:telegram:message_reaction` 中读取第一项新回应的类型，取消回应时数组为空。

```text
【取元素@【事件内容】@telegram@new_reaction@0@type】
```

[Telegram 更新类型](https://core.telegram.org/bots/api#update) · [KOOK 频道回应事件](https://developer.kookapp.cn/doc/event/channel) · [KOOK 私聊回应事件](https://developer.kookapp.cn/doc/event/direct-message) · [KOOK 服务器表情事件](https://developer.kookapp.cn/doc/event/guild)

## 编辑、转发和批量撤回

【输出流】返回 RedReply 消息 ID，可以存入变量供后续脚本操作。以下命令使用这个 ID。【Telegram调用】和【KOOK调用】的原生方法仍使用平台 ID。一个 RedReply ID 可能包含多条平台消息，例如一次输出发送多段内容。编辑需要单条消息 ID。转发和撤回可展开多段消息。

### 编辑消息

【编辑消息@<font color="red">RedReply 消息 ID</font>@<font color="red">新内容</font>@<font color="red">类型</font>@<font color="red">选项对象</font>@<font color="red">Telegram 上传文件对象</font>】

省略第一个参数时使用当前消息。类型省略时为 `文本`。返回对象包含 `retcode`、`data` 和 `message`。

| 平台 | 类型与参数 |
| --- | --- |
| Telegram | `文本` 和 `说明` 接受文本。`键盘` 接受含 `inline_keyboard` 的对象，留空清除按钮。`媒体` 接受 InputMedia 对象，可用 `attach://字段名` 引用上传文件。 |
| KOOK | `文本` 接受 KMarkdown。`卡片` 接受卡片数组。适配器按原消息选择频道或私聊编辑接口。 |

```text
【定义变量@消息ID@【输出流@活动开始时间待定】】
【定义变量@结果@【编辑消息@【变量@消息ID】@活动将于周六开始】】
【取元素@【变量@结果】@retcode】
```

Telegram 可通过选项对象传 `parse_mode`、`entities`、`reply_markup` 等方法字段。行内模式发出的消息用【Telegram调用】直接传 `inline_message_id`。实时位置、业务清单和组件数据等专用编辑方法也通过【Telegram调用】传官方字段。

```text
【定义变量@结果@【编辑消息@机器人消息ID@【Telegram键盘@【数组@【数组
  @【数组@查看活动@event-details】
】】】@键盘】】
【取元素@【变量@结果】@retcode】
```

KOOK 只允许编辑原类型为 KMarkdown 或卡片的消息。通过普通输出发送的新文本使用 KMarkdown。旧的纯文本消息或独立图片仍受平台编辑限制。编辑卡片时将【KOOK卡片】作为新内容传入。

```text
【定义变量@结果@【编辑消息@卡片消息ID@【KOOK卡片@报名已截止】@卡片】】
【取元素@【变量@结果】@retcode】
```

[Telegram 编辑接口](https://core.telegram.org/bots/api#updating-messages) · [KOOK 频道消息](https://developer.kookapp.cn/doc/http/message) · [KOOK 私聊消息](https://developer.kookapp.cn/doc/http/direct-message)

### 转发消息

【转发消息@<font color="red">RedReply 消息 ID 或数组</font>@<font color="red">目标 ID</font>@<font color="red">目标类型</font>@<font color="red">选项对象</font>】

源 ID 省略时使用当前消息。目标省略时使用当前会话，跨会话时填写目标 ID，KOOK 私聊同时指定 `私聊`。Telegram 可使用聊天 ID 或 `【@@频道用户名】`。该命令转发已存在的消息，【合并转发】仍用于生成现有的合并消息节点。

- **Telegram** 使用 `forwardMessage` 或 `forwardMessages`。批量最多 100 条，需来自同一普通聊天，按平台消息 ID 升序发送。服务消息和受保护消息由 Telegram 限制。选项对象中 `复制=真` 时用 `copyMessage` 或 `copyMessages`，消息不带原转发来源。可用 `静默=真`、`话题ID` 和 `话题类型`。数字目标的返回对象提供 `redreply_message_ids`，可逐条编辑或撤回。批量接口可能跳过不能转发的消息。
- **KOOK** 公开接口提供消息读取和发送。适配器读取原消息内容后重新发送文字、KMarkdown、卡片和常见附件，由机器人署名，卡片按钮的回调也会发给重发卡片的机器人。发送失败时，`data.sent` 保留此前已发送的 RedReply 消息 ID，可避免重试时重复发送。

```text
【定义变量@结果@【转发消息@【数组@原消息一@原消息二】@目标频道ID@频道】】
【取元素@【变量@结果】@retcode】
```

```text
【定义变量@结果@【转发消息@【消息ID】@-1001234567890@群聊@【对象
  @复制@真@静默@真@话题ID@15
】】】
【取长度@【取元素@【变量@结果】@redreply_message_ids】】
```

[Telegram 转发](https://core.telegram.org/bots/api#forwardmessages) · [Telegram 复制](https://core.telegram.org/bots/api#copymessages) · [KOOK 频道消息](https://developer.kookapp.cn/doc/http/message)

### 批量撤回

【撤回@<font color="red">RedReply 消息 ID 数组</font>】

Telegram 按原聊天分组，每次原生请求最多 100 条，使用 `deleteMessages`。业务消息按聊天和业务连接分组，使用 `deleteBusinessMessages`。KOOK 按每条消息的频道或私聊类型选择删除接口。两端均去重，单项失败后继续处理剩余请求。返回 `data.failed` 为失败请求数，`data.results` 内每项含目标和平台响应。Telegram 跳过已不存在的消息，因此成功表示该批请求被平台接受。单条文本 ID 的【撤回】用法保持原样。

```text
【定义变量@结果@【撤回@【数组@消息一ID@消息二ID】】】
【取元素@【变量@结果】@data@failed】
```

[Telegram 批量删除](https://core.telegram.org/bots/api#deletemessages) · [Telegram 业务消息删除](https://core.telegram.org/bots/api#deletebusinessmessages) · [KOOK 私聊删除](https://developer.kookapp.cn/doc/http/direct-message)

### 编辑、删除和转发事件

| 事件关键词 | 可读取的字段 |
| --- | --- |
| `notice:kook:updated_message`、`notice:kook:updated_private_message` | `kook.content` 为更新内容，`kook_message_id` 是平台原始 ID，【消息ID】是 RedReply ID。私聊的 `kook.chat_code` 标识会话。 |
| `notice:kook:deleted_message`、`notice:kook:deleted_private_message` | `kook.msg_id` 为已删除的原始 ID，仍兼容旧有 `notice:group_recall` 和 `notice:friend_recall`。 |
| `notice:telegram:edited_message`、`notice:telegram:edited_channel_post`、`notice:telegram:edited_business_message` | `telegram.text` 或 `telegram.caption` 为编辑后的内容，【消息ID】可关联已缓存的原消息。 |
| `notice:telegram:forwarded_message` | 收到带 `forward_origin` 的消息时触发。原始来源在 `telegram.forward_origin`，同一消息仍走相应的普通消息或频道、业务消息事件。 |
| `notice:telegram:deleted_business_messages` | `telegram.message_ids` 是原生 ID 列表，外层 `message_ids` 是 RedReply ID 列表，`telegram.business_connection_id` 标识业务连接。 |

Telegram Bot API 的删除更新仅覆盖已连接的业务账户。普通聊天或频道的删除以调用结果为准。KOOK 重新发送会作为机器人普通消息出现。自定义 Telegram 更新订阅时，填写原始更新名，例如 `message,channel_post,business_message,edited_message,edited_channel_post,edited_business_message,deleted_business_messages`。

以下示例用于普通群聊的 `notice:telegram:forwarded_message` 脚本，可读取来源类型并用【输出流】回复。

```text
【定义变量@来源@【取元素@【事件内容】@telegram@forward_origin@type】】
【隐藏@【输出流@转发来源类型：【变量@来源】】】
```

[Telegram 更新类型](https://core.telegram.org/bots/api#update) · [KOOK 频道消息事件](https://developer.kookapp.cn/doc/event/channel) · [KOOK 私聊消息事件](https://developer.kookapp.cn/doc/event/direct-message)

## Telegram 群与频道管理

Telegram 聊天 ID 可从【群ID】或事件中的 `telegram.chat.id` 取得。以用户名指定频道时，用原始字符串保留前导 `@`，例如 `【@@example_channel】`。管理接口返回的 `retcode` 为 `0` 时操作成功。

### 聊天资料、成员与置顶

| 用途 | 接口与参数 |
| --- | --- |
| 查询聊天资料 | `getChat` 使用 `chat_id`，返回 `data.title`、`data.description`、`data.permissions` 等可用字段。 |
| 更新标题、简介、头像 | `setChatTitle` 用 `title`，`setChatDescription` 用 `description`，`setChatPhoto` 用字节集上传字段 `photo`。删除头像用 `deleteChatPhoto`。 |
| 查询成员、管理员与人数 | `getChatMember`、`getChatAdministrators`、`getChatMemberCount`。现有【权限】可查询 Telegram 群里的发送者角色，【BOT权限】可查询机器人的角色。 |
| 移除与封禁 | `banChatMember` 使用整数 `user_id`。解封用 `unbanChatMember`，若只想操作已被封禁的成员，设置 `only_if_banned` 为真。 |
| 退出 | `leaveChat` 使用 `chat_id`，由机器人退出目标群或频道。 |

```text
【定义变量@结果@【Telegram调用@setChatDescription@【对象
  @chat_id@【接口值@整数@【群ID】】
  @description@这里发布活动安排和通知
】】】
【取元素@【变量@结果】@retcode】
```

【置顶消息】默认操作当前消息，也可以传【输出流】返回的 RedReply 消息 ID。用 `【置顶消息@@假】` 取消当前消息的置顶。第三个参数填 `真` 时 Telegram 静默置顶。该命令也可用于 KOOK 文字频道。清空某个 Telegram 话题的置顶列表用 `unpinAllForumTopicMessages`。

```text
【置顶消息】
```

[Telegram 聊天资料](https://core.telegram.org/bots/api#getchat) · [管理员查询](https://core.telegram.org/bots/api#getchatadministrators) · [置顶](https://core.telegram.org/bots/api#pinchatmessage)

### 默认权限、成员限制与管理员

`setChatPermissions` 设置普通成员的默认权限。先用 `getChat` 查看 `data.permissions`，并在写入时填入要保留的其他权限。下例显式设置一套只允许发文本的权限。独立权限模式会分别处理各项发送权限。

```text
【定义变量@结果@【Telegram调用@setChatPermissions@【对象
  @chat_id@【接口值@整数@【群ID】】
  @use_independent_chat_permissions@【接口值@逻辑@真】
  @permissions@【对象
    @can_send_messages@【接口值@逻辑@真】
    @can_send_photos@【接口值@逻辑@假】
    @can_send_videos@【接口值@逻辑@假】
    @can_send_polls@【接口值@逻辑@假】
    @can_send_other_messages@【接口值@逻辑@假】
    @can_add_web_page_previews@【接口值@逻辑@假】
  】
】】】
【取元素@【变量@结果】@retcode】
```

- 在超级群用【禁言@300】暂停当前发送者的消息发送权限 300 秒。时长范围为 30 秒至 366 天。【禁言@0】把发送权限恢复为群默认值，其他权限沿用成员当前设置。机器人需有 `can_restrict_members`。需要对某个成员逐项控制权限时，用 `restrictChatMember`，传 `user_id`、`permissions` 和可选的 Unix 秒 `until_date`。
- 在超级群或频道用 `promoteChatMember` 管理管理员权限。聊天类型决定可用字段，例如超级群的 `can_manage_topics`、频道的 `can_post_messages`。只填写机器人有权授予的权限。撤销管理员时按官方接口传入各项布尔权限为假。
- 在 `notice:telegram:chat_member` 读取 `telegram.old_chat_member`、`telegram.new_chat_member` 和 `telegram.from`。需显式订阅 `chat_member` 并具备管理员身份。机器人自身变更使用 `notice:telegram:my_chat_member`。

```text
【定义变量@结果@【Telegram调用@promoteChatMember@【对象
  @chat_id@【接口值@整数@【群ID】】
  @user_id@【接口值@整数@目标用户ID】
  @can_manage_topics@【接口值@逻辑@真】
】】】
【取元素@【变量@结果】@retcode】
```

[默认权限](https://core.telegram.org/bots/api#setchatpermissions) · [成员限制](https://core.telegram.org/bots/api#restrictchatmember) · [管理员权限](https://core.telegram.org/bots/api#promotechatmember)

### 邀请链接与审核

- 【邀请链接@创建@目标ID@目标类型@有效秒数@上限@名称@是否审核】
- 【邀请链接@编辑@链接@目标ID@目标类型@有效秒数@上限@名称@是否审核】
- 【邀请链接@撤销@链接@目标ID@目标类型】

| 参数 | Telegram 填法 |
| --- | --- |
| 操作 | 必填 `创建`、`编辑` 或 `撤销`。创建和编辑返回链接。撤销成功时没有输出。 |
| 链接 | 编辑或撤销时必填，填写机器人创建的完整邀请链接。 |
| 目标 ID、目标类型 | 可省略，默认当前群或频道。目标可填聊天 ID 或用户名，类型可填 `群聊` 或 `频道`。 |
| 有效秒数 | 可省略。创建时默认无有效期限制，填 `0` 也表示无有效期限制。正整数从调用时开始计时。 |
| 上限 | 可省略，填写 1 至 99999。开启审核时省略。 |
| 名称、是否审核 | 均可省略，名称最多 32 字，创建时默认无需审核。编辑时可填 `假` 关闭审核。 |

```text
【邀请链接@创建@@@3600@@活动报名@真】
```

```text
【邀请链接@撤销@已保存的邀请链接】
```

机器人须具备邀请权限。在 `notice:telegram:chat_join_request` 中用【同意】或【拒绝】处理申请。主邀请和付费订阅邀请可通过【Telegram调用】使用官方专用接口。

[Telegram 邀请链接](https://core.telegram.org/bots/api#createchatinvitelink)

### 论坛话题与频道私信话题

论坛超级群和已启用话题模式的私聊使用 `createForumTopic` 创建话题，保存 `data.message_thread_id`，再用 `editForumTopic`、`closeForumTopic`、`reopenForumTopic` 或 `deleteForumTopic` 管理。其中关闭和重开用于论坛超级群。创建或编辑时可选用 `getForumTopicIconStickers` 返回的自定义图标。话题创建、编辑、关闭、重开以及 General 话题的隐藏、取消隐藏，可用 `notice:telegram:forum_topic_created`、`notice:telegram:forum_topic_edited`、`notice:telegram:forum_topic_closed`、`notice:telegram:forum_topic_reopened`、`notice:telegram:general_forum_topic_hidden`、`notice:telegram:general_forum_topic_unhidden`。原始消息在 `telegram`。可以用 `editGeneralForumTopic`、`closeGeneralForumTopic`、`reopenGeneralForumTopic`、`hideGeneralForumTopic` 和 `unhideGeneralForumTopic` 管理 General 话题。

下例在当前群创建话题，成功后用【输出流】发往新话题。第三个参数为正整数话题 ID。频道私信则在第四个参数填 `频道私信`，第三个参数传原生 `direct_messages_topic_id` 对应的 ID。

```text
【定义变量@结果@【Telegram调用@createForumTopic@【对象
  @chat_id@【接口值@整数@【群ID】】@name@周末活动
】】】
【判等@【取元素@【变量@结果】@retcode】@0@
  【取元素@【变量@结果】@message】
  @【隐藏@【输出流@请在此话题报名@@【取元素@【变量@结果】@data@message_thread_id】】】
】
```

若要关闭触发当前脚本的论坛话题，用该事件的 `message_thread_id`。重开时将方法改为 `reopenForumTopic`。

```text
【定义变量@结果@【Telegram调用@closeForumTopic@【对象
  @chat_id@【接口值@整数@【群ID】】
  @message_thread_id@【接口值@整数@【取元素@【事件内容】@message_thread_id】】
】】】
【取元素@【变量@结果】@retcode】
```

[话题接口](https://core.telegram.org/bots/api#createforumtopic) · [话题服务消息](https://core.telegram.org/bots/api#message)

## KOOK 服务器与频道管理

在 KOOK，【群ID】是频道 ID，【当前群组】是服务器 ID。在 CRON 或私聊中管理服务器时可显式填服务器 ID。【取群列表@服务器ID】会遍历分页并返回文字频道，省略时使用当前服务器或从当前频道查询所属服务器。完整频道类型和分组信息用 `channel/list` 查询。

### 频道与成员

| 用途 | KOOK 接口 |
| --- | --- |
| 服务器及成员查询 | GET `guild/list`、`guild/view`、`guild/user-list`。成员列表可用 `search` 或整数 `role_id` 筛选。 |
| 频道查询 | GET `channel/list` 传 `guild_id`，GET `channel/view` 传 `target_id`。 |
| 创建、更新、删除频道 | POST `channel/create`、`channel/update`、`channel/delete`。更新和删除用 `channel_id`。 |
| 成员昵称、踢出、离开 | POST `guild/nickname`、`guild/kickout`、`guild/leave`。踢出用 `target_id`。 |
| 封禁与解除 | GET `blacklist/list`、POST `blacklist/create` 和 `blacklist/delete`。写入用 `guild_id` 和 `target_id`。 |
| 语音闭麦、静音 | GET `guild-mute/list`、POST `guild-mute/create` 和 `guild-mute/delete`。整数 `type` 为 1 时闭麦，为 2 时静音。 |

```text
【定义变量@结果@【KOOK调用@POST@channel/update@【对象
  @channel_id@【群ID】
  @topic@周末活动报名处
  @slow_mode@【接口值@整数@5000】
】】】
【取元素@【变量@结果】@retcode】
```

`slow_mode` 在更新接口里以毫秒计，只能使用官方列出的档位。频道增删改事件用 `notice:kook:added_channel`、`notice:kook:updated_channel`、`notice:kook:deleted_channel`。脚本可直接读取【群ID】和【当前群组】。频道置顶变动事件用 `notice:kook:pinned_message` 和 `notice:kook:unpinned_message`。

频道或群聊改名也可以沿用【OB调用】的 `set_group_name`。下例在 KOOK 修改当前频道名，在 Telegram 修改当前群或频道标题。

```text
【定义变量@结果@【OB调用@【对象@action@set_group_name@params@【对象
  @group_id@【群ID】@group_name@活动公告
】】】】
【取元素@【变量@结果】@retcode】
```

[KOOK 频道](https://developer.kookapp.cn/doc/http/channel) · [KOOK 服务器](https://developer.kookapp.cn/doc/http/guild) · [黑名单](https://developer.kookapp.cn/doc/http/blacklist) · [频道事件](https://developer.kookapp.cn/doc/event/channel)

### 身份组与频道权限覆盖

- 服务器角色用 `guild-role/list` 查询，`guild-role/create`、`guild-role/update`、`guild-role/delete` 管理，`guild-role/grant`、`guild-role/revoke` 分配和撤销。此处的 `role_id` 为整数，`user_id` 为文本。
- 频道权限用 `channel-role/index` 查询，`channel-role/create`、`channel-role/update`、`channel-role/delete` 管理覆盖。用 `channel-role/sync` 同步父分组权限。`type` 是 `role_id` 或 `user_id`，对应的 `value` 为文本。权限掩码 `allow` 和 `deny` 用整数。
- 【权限】和【BOT权限】按服务器所有者和身份组的管理员权限位判断。要查询单项权限，读取角色位掩码和频道覆盖规则。身份组变动用 `notice:kook:added_role`、`notice:kook:updated_role`、`notice:kook:deleted_role`，从【当前群组】取服务器 ID。

下面修改已存在的频道权限覆盖，允许指定身份组查看频道，禁止发布消息。先用 `channel-role/index` 核对并合并需要保留的其他权限位。

```text
【定义变量@结果@【KOOK调用@POST@channel-role/update@【对象
  @channel_id@【群ID】@type@role_id@value@702
  @allow@【接口值@整数@2048】@deny@【接口值@整数@4096】
】】】
【取元素@【变量@结果】@retcode】
```

[身份组与权限位](https://developer.kookapp.cn/doc/http/guild-role) · [频道权限](https://developer.kookapp.cn/doc/http/channel) · [身份组事件](https://developer.kookapp.cn/doc/event/guild-role)

### 邀请链接

- 【邀请链接@创建@目标ID@目标类型@有效秒数@次数】
- 【邀请链接@撤销@链接或邀请码@目标ID@目标类型】
- 【邀请链接@列表@目标ID@目标类型@页码】
- 【邀请链接@受邀人数@链接或邀请码@服务器ID】

| 参数 | KOOK 填法 |
| --- | --- |
| 操作 | 必填 `创建`、`撤销`、`列表` 或 `受邀人数`。创建返回链接，列表返回当页链接数组，受邀人数返回总数。 |
| 目标类型、目标 ID | 类型可省略，默认 `频道`，也可填 `服务器`。ID 省略时按类型取当前频道或当前服务器。 |
| 有效秒数 | 可省略，默认 604800 秒。可填 `0` 永久，或 `1800`、`3600`、`21600`、`43200`、`86400`、`604800`。 |
| 次数 | 可省略，默认不限。可填 `-1`、`1`、`5`、`10`、`25`、`50` 或 `100`。 |
| 链接或邀请码 | 撤销或查询受邀人数时必填。 |
| 页码、服务器 ID | 列表页码默认 `1`。查询受邀人数时，服务器 ID 可省略。 |

```text
【邀请链接@创建@@@3600@5】
```

```text
【取长度@【邀请链接@列表】】
```

```text
【邀请链接@撤销@已保存的邀请链接】
```

[KOOK 邀请接口](https://developer.kookapp.cn/doc/http/invite)

### 帖子频道与分区

GET `category/list` 查询分区，GET `thread/list` 查询帖子，GET `thread/view` 查询详情。POST `thread/create` 使用服务器、帖子频道和标题，主楼用卡片数组。GET `thread/post` 查询回复，必填 `channel_id`、`thread_id`、`order` 和 `page`，后者按官方字段类型传文本。POST `thread/delete` 用 `thread_id` 删除整帖，传 `post_id` 时只删该条评论或回复。

```text
【定义变量@帖子@【KOOK调用@POST@thread/create@【对象
  @channel_id@帖子频道ID@guild_id@【当前群组】@title@活动报名
  @content@【数组@【对象@type@card@modules@【数组@【对象@type@section@text@请在本帖回复报名】】】】
】】】
【取元素@【变量@帖子】@data@id】
```

POST `thread/reply` 的 `content` 是回复文本。回复某一楼时再传 `reply_id`，填写该楼的评论或回复 ID。

```text
【定义变量@回复@【KOOK调用@POST@thread/reply@【对象
  @channel_id@帖子频道ID@thread_id@帖子ID@content@本次活动将在周六举行
】】】
【取元素@【变量@回复】@retcode】
```

[KOOK 帖子接口](https://developer.kookapp.cn/doc/http/thread)

## KOOK 卡片、文件与共享播放

### 发送交互卡片

【KOOK卡片@正文@按钮行数组@主题@大小】

| 参数 | 怎么填 |
| --- | --- |
| 正文 | 填显示的文本，有按钮时可省略。 |
| 按钮行数组 | 可省略。每行用一个【数组】，每行 1 至 4 个按钮。每个按钮用【数组@文字@值@动作】。动作默认为 `回调`，值是点击后发送给脚本的文本。动作填 `链接` 时，值填 HTTP(S) 链接。 |
| 主题 | 可省略，默认 `蓝色`。也可填 `绿色`、`红色`、`黄色`、`青色`、`灰色` 或 `无边框`。 |
| 大小 | 可省略，默认 `大`，也可填 `小`。 |

用【输出流】发送卡片，可用【编辑消息】更新原卡片。正文及每行按钮各占一个模块，每张简写卡片最多 50 个模块。

```text
【定义变量@按钮行@【数组@【数组
  @【数组@确认@confirm】
  @【数组@说明@https://developer.kookapp.cn@链接】
】】】
【隐藏@【输出流@【KOOK卡片@请选择操作@【变量@按钮行】】】】
```

按钮回调在 `notice:kook:message_btn_click` 触发。用【事件内容@按钮值】读取点击值，用【消息ID】获取 RedReply 消息 ID。需要更复杂的布局时，【KOOK卡片】也接受官方卡片数组。

[KOOK 卡片规范](https://developer.kookapp.cn/doc/cardmessage) · [按钮事件](https://developer.kookapp.cn/doc/event/user)

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
【隐藏@【输出流@【KOOK卡片@下次活动选哪天？@【数组@【数组
  @【数组@周六@poll-weekend-sat】
  @【数组@周日@poll-weekend-sun】
】】】】】
```

用 `notice:kook:message_btn_click` 建立事件脚本，用同一张卡片的原生消息 ID 和投票人 ID 保存选择。下例把每张卡片作为一个配置节，同一人再次点击会覆盖原选择。

```text
【定义变量@选择@【事件内容@按钮值】】
【定义变量@投票ID@【事件内容@平台消息ID】】
【加锁@kook-votes.ini】
【判等@【变量@选择】@poll-weekend-sat@
  【判等@【变量@选择】@poll-weekend-sun@@【写配置@【应用目录】kook-votes.ini@【变量@投票ID】@【发送者ID】@周日】】
  @【写配置@【应用目录】kook-votes.ini@【变量@投票ID】@【发送者ID】@周六】
】
【解锁@kook-votes.ini】
```

【加锁】和【解锁】避免同包中的投票事件同时写配置。统计脚本使用同一个锁，可用 `【读配置键@【应用目录】kook-votes.ini@原生消息ID】` 读取各人的选择，再用【循环】按选项计数。[KOOK 按钮事件](https://developer.kookapp.cn/doc/event/user)
