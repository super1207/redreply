# KOOK 与 Telegram 原生接口

在对应平台的消息或事件脚本中，红色问答自动选用触发事件的机器人。CRON 等没有来源的脚本先用【设置来源】指定机器人。将示例 ID、文件路径和音源地址替换为实际值。

```text
【设置来源@机器人平台@telegram】
【设置来源@机器人ID@123456789】
```

KOOK 的平台名为 `kook`。

## 传参与返回值

本页的功能命令按位置填值。省略中间参数时保留空位，末尾可直接省略。用【数组】填写列表。新查询命令直接返回结果，新管理命令的失败由脚本报告平台错误。以下两个通用入口供自行对接其他官方方法，按原生字段传参。

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

### 机器人资料

【Telegram机器人资料@查询@资料项@语言】返回文字。【Telegram机器人资料@设置@资料项@新文字@语言】修改资料。资料项必填，可填 `名称`、`简介` 或 `短简介`，长度上限分别为 64、512 和 120 字。语言可省略，默认通用资料，指定语言时填两字母代码，例如 `zh`。设置时留空文字可移除对应语言的资料。

```text
【Telegram机器人资料@设置@短简介@查看活动和通知@zh】
```

[Telegram 机器人资料](https://core.telegram.org/bots/api#setmyname)

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

在 BotFather 开启行内模式。用 `notice:telegram:inline_query` 接收查询。【事件内容@查询内容】取得用户输入，【事件内容@查询偏移】取得分页标记。

【Telegram应答查询@结果数组@缓存秒数@下一页偏移@是否个人@查询ID】

| 参数 | 怎么填 |
| --- | --- |
| 结果数组 | 必填，用【数组】填写 0 至 50 个结果。每项为【数组@结果ID@标题@发送文字或说明@简介@链接或文件ID@类型】。 |
| 结果 ID、标题 | 必填。ID 为本页不重复的 1 至 64 字节文本。标题显示在搜索结果中。 |
| 发送文字或说明 | 文章必填将要发送的文字。媒体可省略，填写附件说明。 |
| 简介、链接或文件 ID、类型 | 简介可省略。类型默认 `文章`，可填 HTTPS 链接。类型填 `图片`、`视频` 或 `文件` 时，链接位置填写对应的 Telegram 文件 ID。 |
| 缓存秒数、下一页偏移 | 可省略，默认 0 秒和空。下一页偏移最多 64 字节。 |
| 是否个人、查询 ID | 可省略，默认 `假` 和当前事件的查询 ID。从其他脚本调用时填写查询 ID。 |

```text
【Telegram应答查询@【数组@【数组@help@帮助@欢迎使用红色问答】】】
```

### 聊天、成员和头像查询

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【群信息@群ID】 | 群 ID 默认当前会话，可填数字聊天 ID 或 `【@@频道用户名】`。返回对象，可取群ID、群名、简介、类型、用户名、论坛、头像文件ID、可用回应。 |
| 【群成员@详情@用户ID@群ID】 | 用户 ID 默认发送者，群 ID 默认当前会话。返回对象，可取用户ID、昵称、用户名、身份、机器人。 |
| 【群成员@管理员@群ID@包含机器人】 | 群 ID 可省略。返回与详情同结构的数组，包括群主。最后一项填 `真` 可包含其他管理员机器人，默认由 Telegram 省略其他机器人。 |
| 【群成员@人数@群ID】 | 群 ID 可省略。返回成员总数。 |
| 【取头像@用户ID@文件ID列表@跳过数量@返回数量】 | 用户 ID 默认发送者，跳过数量默认 0，返回数量可填 1 至 100，默认 100。返回头像文件 ID 数组，每张头像取最大可用尺寸。 |

【权限】和【BOT权限】可继续查询当前会话的发送者及机器人角色。Telegram Bot API 暂无群成员全量列表和聊天历史搜索方法。

```text
【取长度@【群成员@管理员】】
```

```text
【取元素@【群成员@详情】@昵称】
```

```text
【取头像@@文件ID列表@0@1】
```

### 发起及结束投票

【Telegram投票@题目@选项数组@是否匿名@是否多选@正确选项@话题ID@商务连接ID】

| 参数 | 怎么填 |
| --- | --- |
| 题目 | 必填，1 至 300 字。 |
| 选项数组 | 必填，用【数组】直接列出 1 至 12 个选项，每项 1 至 100 字。 |
| 是否匿名、是否多选 | 均可省略，分别默认为 `真`、`假`。 |
| 正确选项 | 可省略，省略时发普通投票。填写从 1 开始的选项序号即可发测验。多个正确答案用【数组】，同时将是否多选填 `真`。 |
| 话题 ID | 可省略，默认沿用当前论坛话题。此命令向当前聊天发送。 |
| 商务连接 ID | 可省略，默认沿用同一商务会话的连接。填 `机器人` 以机器人身份发送。 |

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

普通发送用【文件】、【视频】和【输出流】。需要传附件说明、使用 Telegram 文件 ID 或取得新文件 ID 时，用【Telegram文件@来源@文件名@说明@群ID@话题ID@话题类型@商务连接ID】。

| 参数 | 怎么填 |
| --- | --- |
| 来源 | 必填，填 Telegram 文件 ID、受 Telegram 支持的 HTTP(S) 直链，或【读文件】返回的字节集。 |
| 文件名 | 来源为字节集时必填。 |
| 说明 | 可省略，填写随文件显示的文字。 |
| 群 ID | 可省略，默认当前会话。 |
| 话题 ID、话题类型 | 可省略，默认沿用同一会话的论坛或频道私信话题。指定 ID 时类型默认 `话题`，也可填 `频道私信`。 |
| 商务连接 ID | 可省略，默认沿用同一商务会话的连接。向其他商务会话发送时填写连接 ID 与正整数目标私聊 ID。填 `机器人` 使用机器人身份。 |

发送成功返回可复用的 Telegram 文件 ID。

```text
【Telegram文件@【读文件@【应用目录】说明.pdf】@说明.pdf@今日文件】
```

【Telegram下载文件@文件ID】用当前机器人下载并返回字节集。文件 ID 必填，云端 Bot API 的下载上限为 20 MB。

```text
【写文件@【应用目录】telegram-download.bin@【Telegram下载文件@【事件内容@文件ID】】】
```

### 话题、审核与更新订阅

- 【输出流】会保留当前 Telegram 话题。用【事件内容@话题ID】或【事件内容@频道私信话题ID】读取当前话题。
- 用 `notice:telegram:chat_join_request` 接收申请，在事件脚本中使用【同意】或【拒绝】。机器人需要邀请权限。交互式入群申请须在 10 秒内处理。
- 群聊和私聊消息走原有消息触发。其他更新走 `notice:telegram:更新类型`。在管理台「订阅更新」中按逗号填写类型，例如 `message,callback_query,inline_query,poll,poll_answer,chat_join_request`。适配器用轮询收件，同一机器人 Token 的 webhook 须关闭。
- 【Telegram小程序应答@查询ID@结果ID@标题@发送文字】向小程序发起的查询返回文章。四项均必填。结果 ID 填 1 至 64 字节文本。返回行内消息 ID（Telegram 提供时）。

```text
【Telegram小程序应答@小程序查询ID@help@帮助@欢迎使用红色问答】
```

[行内机器人](https://core.telegram.org/bots/inline) · [查询聊天](https://core.telegram.org/bots/api#getchat) · [入群审核](https://core.telegram.org/bots/api#approvechatjoinrequest) · [文件下载](https://core.telegram.org/bots/api#getfile)

## Telegram 商务消息与账户

在 Telegram 中给机器人开通商务模式并连接商务账户。管理台的「订阅更新」可加入 `business_connection,business_message,edited_business_message,deleted_business_messages`。各操作由商务账户单独授权。发送和编辑需要回复权限，会话通常须在最近 24 小时收到消息。平台对部分商务消息的编辑和删除另有时限。

### 发送、已读、编辑与撤回

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【输出流@内容@键盘@话题ID@话题类型@商务连接ID@商务对方ID】 | 内容填文本或原有【图片】【文件】【贴纸】【媒体组】等消息。商务私聊的键盘可省略，填写时使用行内【Telegram键盘】。话题两项留空。连接 ID 和对方 ID 可省略，默认沿用同一机器人触发的当前商务私聊。显式指定对方时，连接 ID 与正整数对方 ID 都必填。在当前对方的机器人普通私聊中发送时，第 5 项填 `机器人`。返回 RedReply 消息 ID。 |
| 【Telegram商务已读@消息ID】 | 可省略，默认当前 RedReply 消息 ID。只标记收到的商务消息，需要已读权限，且会话在最近 24 小时内活跃。 |
| 【Telegram商务已读@平台消息ID@商务会话ID@商务连接ID】 | 前两项都填正整数。连接 ID 默认当前商务事件的连接，其他脚本须显式填写。可操作只保存了平台 ID 的消息。 |
| 【编辑消息@消息ID@新内容@类型@文本格式@行内键盘】 | 用保存的 RedReply 消息 ID，省略时用当前消息。自动沿用原消息的商务连接。类型和选填项沿用编辑消息的说明。 |
| 【置顶消息@消息ID@是否置顶@是否静默】 | 自动沿用原消息的商务连接。省略参数时置顶当前消息。第 2 项填 `假` 取消置顶。 |
| 【撤回@消息ID或数组】 | 使用 RedReply 消息 ID，自动沿用原消息的商务连接。可省略为当前消息。 |
| 【撤回@平台消息ID或数组@商务连接ID】 | 两项必填。平台 ID 填正整数，最多 100 条，须来自同一商务会话。适用于保存了平台 ID 的脚本。需要删除机器人消息或删除会话消息的相应权限。 |
| 【Telegram聊天动作@动作@聊天ID@话题ID@商务连接ID】 | 动作默认 `正在输入`，也可填 `上传图片`、`录制视频`、`上传视频`、`录制语音`、`上传语音`、`上传文件`、`选择贴纸`、`查找位置`、`录制视频便笺`、`上传视频便笺`。聊天 ID 默认当前会话。话题 ID 默认当前论坛话题，商务私聊留空。连接 ID 默认沿用同一商务会话，填 `机器人` 使用机器人身份。动作状态最多显示 5 秒。 |

在 `notice:telegram:business_message` 中可直接确认已读并回复。

```text
【Telegram商务已读】
【隐藏@【输出流@已收到，我们马上处理。】】
```

```text
【隐藏@【输出流@订单已发货@@@@商务连接ID@123456789】】
```

```text
【撤回@【数组@57@58】@商务连接ID】
```

[商务连接](https://core.telegram.org/bots/api#getbusinessconnection) · [已读和删除](https://core.telegram.org/bots/api#readbusinessmessage) · [聊天动作](https://core.telegram.org/bots/api#sendchataction)

### 任务清单与事件

【Telegram任务清单@标题@任务数组@允许他人添加@允许他人勾选】生成可交给【输出流】的清单。标题必填，1 至 255 字。任务数组必填，1 至 30 项。每项可直接填 1 至 100 字的任务文字，自动使用从 1 开始的位置作为任务 ID。需要指定 ID 时用【数组@任务ID@文字】，ID 为该清单中唯一的正整数。两个开关可省略，默认 `假`。

```text
【定义变量@清单消息@【输出流@【Telegram任务清单@订单进度@【数组@确认地址@安排发货】@@真】】】
【变量@清单消息】
```

编辑时用【编辑消息@消息ID@清单数组@清单@空位@行内键盘】，消息 ID 默认当前消息。清单数组为【数组@标题@任务数组@允许他人添加@允许他人勾选】，填法同发送。已有任务填原 ID，新增任务填未使用的 ID。行内键盘可省略。

```text
【编辑消息@已保存的清单消息ID@【数组@订单进度@【数组@【数组@1@确认地址】@【数组@2@填写物流单号】】@@真】@清单】
```

| 事件关键词 | 用【事件内容@字段】读取 |
| --- | --- |
| `notice:telegram:business_connection` | `商务连接ID`、`商务连接`。后者返回账户与权限信息，结构同【Telegram商务账户@查询】。 |
| `notice:telegram:business_message`、`notice:telegram:edited_business_message` | `商务连接ID`、`商务会话ID`、`消息文字`、`清单标题`、`清单任务`。清单任务每项有任务ID、文字、完成时间、完成人ID。 |
| `notice:telegram:deleted_business_messages` | `商务连接ID`、`商务会话ID`、`已删除消息ID` 和 `已删除平台消息ID`。两个消息数组分别使用 RedReply ID 和平台 ID。 |

商务消息事件中的清单变动可用 `新增任务`、`完成任务ID`、`未完成任务ID` 读取，并用 `清单平台消息ID` 关联原清单（Telegram 提供时）。完成与未完成的字段是对应任务 ID 数组，未提供时返回空文本。

```text
【取元素@【事件内容@商务连接】@权限】
```

[Telegram 清单](https://core.telegram.org/bots/api#sendchecklist) · [编辑清单](https://core.telegram.org/bots/api#editmessagechecklist) · [更新事件](https://core.telegram.org/bots/api#update)

### 商务账户资料

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【Telegram商务账户@查询@商务连接ID】 | 返回商务连接ID、账户用户ID、账户私聊ID、姓名、姓氏、用户名、连接时间、已启用、权限。连接时间是 Unix 秒，权限为中文名称数组。 |
| 【Telegram商务账户@姓名@名字@姓氏@商务连接ID】 | 名字 1 至 64 字，必填。姓氏最多 64 字，留空清除。 |
| 【Telegram商务账户@用户名@新用户名@商务连接ID】 | 填最多 32 字的用户名，省略前导 `@`。留空移除。 |
| 【Telegram商务账户@简介@新简介@商务连接ID】 | 简介最多 140 字，留空清除。 |
| 【Telegram商务账户@头像@字节集@文件名@类型@是否公开@商务连接ID@封面秒数】 | 字节集与文件名必填。默认 `静态`，上传 JPG。填 `动态` 时上传 MPEG4，可填用于封面的非负秒数，默认 0。是否公开默认 `假`，操作主头像。 |
| 【Telegram商务账户@移除头像@是否公开@商务连接ID】 | 是否公开默认 `假`，移除主头像。填 `真` 移除公开头像。 |

这些命令的商务连接 ID 可在触发脚本的商务事件里省略。CRON 等其他脚本须填写。修改操作成功时无输出。

```text
【Telegram商务账户@简介@工作日处理订单@商务连接ID】
```

```text
【Telegram商务账户@头像@【读文件@【应用目录】头像.jpg】@头像.jpg】
```

[商务账户姓名和资料](https://core.telegram.org/bots/api#setbusinessaccountname) · [头像](https://core.telegram.org/bots/api#setbusinessaccountprofilephoto)

### 商务动态、礼物与星币

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【Telegram商务动态@发布@字节集@文件名@类型@说明@商务连接ID@有效小时@保留在主页@保护内容】 | 前两项必填。类型默认 `图片`，也可填 `视频`。说明可省，最多 2048 字。有效小时可填 6、12、24、48，默认 24。最后两项默认 `假`。返回动态 ID。 |
| 【Telegram商务动态@编辑@动态ID@字节集@文件名@类型@说明@商务连接ID】 | 动态 ID、字节集、文件名必填。类型默认图片，说明留空可清除。返回动态 ID。 |
| 【Telegram商务动态@转发@来源私聊ID@来源动态ID@有效小时@商务连接ID@保留在主页@保护内容】 | 来源两项必填，均为正整数。两个账户须由同一个机器人管理，源动态由该机器人发布。有效期和开关默认同发布。返回新动态 ID。 |
| 【Telegram商务动态@删除@动态ID@商务连接ID】 | 动态 ID 必填。 |

动态图片请用 1080×1920、10 MB 以内的图片，视频请用 720×1280、H.265 编码、30 MB 以内的 MPEG4。

```text
【Telegram商务动态@发布@【读文件@【应用目录】活动.jpg】@活动.jpg@图片@新品已上架】
```

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【Telegram商务礼物@列表@商务连接ID@偏移@每页数量@排序@排除数组】 | 偏移留空查询首页。每页数量 1 至 100，默认 100。排序默认 `日期`，也可填 `价格`。排除数组可省略，项目可填 `未展示`、`已展示`、`不限量`、`可升级限量`、`不可升级限量`、`独特礼物`、`链上礼物`。返回礼物数组、总数、下一页偏移。 |
| 【Telegram商务礼物@接收设置@显示礼物按钮@类型数组@商务连接ID】 | 开关和数组必填，开关填 `真` 或 `假`。允许的类型可填 `不限量`、`限量`、`独特礼物`、`会员订阅`、`频道转入礼物`。空数组关闭所有类型。 |
| 【Telegram商务礼物@余额@商务连接ID】 | 返回星币余额，细分数可读 `十亿分之一星币`，未提供时为 0。 |
| 【Telegram商务礼物@转星币@星币数@商务连接ID】 | 星币数必填，1 至 10000。转入当前机器人的余额。 |
| 【Telegram商务礼物@转换@持有礼物ID@商务连接ID】 | 持有礼物 ID 必填，将可转换的普通礼物换为星币。 |
| 【Telegram商务礼物@升级@持有礼物ID@保留原信息@星币数@商务连接ID】 | 持有礼物 ID 必填。是否保留原文字和发送者、接收者默认 `假`。已预付时星币数填 0，否则填所需升级价。省略星币数时不提交星币参数。 |
| 【Telegram商务礼物@转送@持有礼物ID@接收私聊ID@星币数@商务连接ID】 | 前两项必填，接收方填最近 24 小时活跃的正整数私聊 ID。免费时可省略星币数，收费时填写所需星币。 |
| 【Telegram商务礼物@删除@持有礼物ID@商务连接ID】 | 持有礼物 ID 必填。删除符合平台条件的普通礼物，不获得星币。 |

动态和礼物命令的连接 ID 默认取当前商务事件，其他脚本须填写。使用礼物列表返回的 `持有礼物ID` 进行转换、升级、转送或删除。

```text
【取元素@【Telegram商务礼物@列表@@@20@价格@【数组@未展示】】@总数】
```

[商务动态](https://core.telegram.org/bots/api#poststory) · [商务礼物](https://core.telegram.org/bots/api#getbusinessaccountgifts) · [礼物接收设置](https://core.telegram.org/bots/api#setbusinessaccountgiftsettings)

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

Telegram 可混发图片、视频和实况照片，文件组与音频组各自只接受同类媒体。相册继承【输出流】的目标话题，可使用【撤回】处理返回的消息 ID。KOOK 把媒体组合成一条卡片，连续无说明图片按每组最多 9 张排列。需要自定义卡片版式时使用【KOOK卡片】。

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

收到 Telegram 贴纸时，用【事件内容@贴纸文件ID】和【事件内容@贴纸包】读取文件 ID 和贴纸包名。用【Telegram下载文件】保存贴纸。

```text
【定义变量@贴纸文件ID@【事件内容@贴纸文件ID】】
【隐藏@【输出流@【贴纸@【变量@贴纸文件ID】】】】
```

| 命令 | 怎么填 |
| --- | --- |
| 【Telegram贴纸包@查询@包名】 | 包名必填。返回名称、标题、贴纸；贴纸数组每项有文件ID、表情、宽度、高度、贴纸包和可能的自定义表情ID。 |
| 【Telegram上传贴纸@字节集@文件名@格式@所有者ID】 | 前两项必填。格式可填静态、动画、视频，默认静态。所有者 ID 默认发送者。返回文件 ID。 |
| 【Telegram贴纸包@创建@包名@标题@贴纸数组@包类型@所有者ID】 | 包名、标题和 1 至 50 项贴纸必填。包类型默认普通，也可填自定义表情。 |
| 【Telegram贴纸包@添加@包名@贴纸@所有者ID】 | 包名和一项贴纸必填。 |
| 【Telegram贴纸包@替换@包名@贴纸@所有者ID@旧文件ID】 | 包名、新贴纸和旧文件 ID 必填。 |
| 【Telegram贴纸包@改标题@包名@新标题】 | 包名和新标题必填。 |
| 【Telegram贴纸包@移除贴纸@文件ID】、【Telegram贴纸包@删除@包名】 | 填写要移除的贴纸文件 ID 或要删除的贴纸包名。 |

创建的包名须以 `_by_机器人用户名` 结尾。所有者 ID 省略时为当前发送者。每项贴纸用【数组@文件ID@关联表情数组@格式】，表情数组填 1 至 20 项，格式同上传命令。

```text
【定义变量@素材@【Telegram上传贴纸@【读文件@【应用目录】庆祝.webp】@庆祝.webp】】
【Telegram贴纸包@添加@events_by_example_bot@【数组@【变量@素材】@【数组@🎉】】】
```

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

Telegram 机器人可设一项回应，自定义表情须已用于该消息或被管理员明确允许。

| 命令 | 怎么填 |
| --- | --- |
| 【Telegram自定义表情@ID数组】 | 填 1 至 200 个 ID。返回贴纸资料数组，可取文件ID、表情、自定义表情ID。 |
| 【KOOK服务器表情@列表@服务器ID@页码】 | 服务器 ID 默认当前服务器，页码默认 1。返回表情ID、名称、上传者ID。 |
| 【KOOK服务器表情@上传@PNG字节集@名称@服务器ID】 | 【读文件】返回的 PNG 字节集必填，限 256 KB。名称可省略由平台生成，服务器 ID 默认当前服务器。返回表情 ID。 |
| 【KOOK服务器表情@改名@表情ID@名称】、【KOOK服务器表情@删除@表情ID】 | 表情 ID 必填，改名时名称填 2 至 32 字。 |
| 【KOOK回应列表@平台消息ID@表情@范围】 | ID 默认当前消息，表情必填，可填字符或服务器表情 ID。范围可填频道或私聊，默认按当前会话类型。返回用户数组。 |

```text
【取长度@【KOOK回应列表@@👍】】
```

```text
【KOOK服务器表情@上传@【读文件@【应用目录】庆祝.png】@庆祝】
```

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

Telegram 的两个回应更新需要机器人具备管理员权限，并在管理台的「订阅更新」中显式加入 `message_reaction,message_reaction_count`。处理媒体时订阅相应的 `message`、`channel_post` 或 `business_message`。处理回应事件时用【事件内容】读取数据，例如在 `notice:telegram:message_reaction` 中读取新回应数组。每项有类型及表情，自定义回应的表情是 ID，取消回应时数组为空。

```text
【事件内容@新回应】
```

[Telegram 更新类型](https://core.telegram.org/bots/api#update) · [KOOK 频道回应事件](https://developer.kookapp.cn/doc/event/channel) · [KOOK 私聊回应事件](https://developer.kookapp.cn/doc/event/direct-message) · [KOOK 服务器表情事件](https://developer.kookapp.cn/doc/event/guild)

## 编辑、转发和批量撤回

【输出流】返回 RedReply 消息 ID，可以存入变量供后续脚本操作。以下命令使用这个 ID。【Telegram调用】和【KOOK调用】的原生方法仍使用平台 ID。一个 RedReply ID 可能包含多条平台消息，例如一次输出发送多段内容。编辑需要单条消息 ID。转发和撤回可展开多段消息。

### 编辑消息

【编辑消息@消息ID@新内容@类型@文本格式@行内键盘@目标类型】

省略第一个参数时使用当前消息。类型省略时为 `文本`。返回对象包含 `retcode`、`data` 和 `message`。

| 平台 | 类型与参数 |
| --- | --- |
| Telegram | `文本` 和 `说明` 接受文本。`键盘` 传【Telegram键盘】，新内容留空时清除按钮。`媒体` 传【数组@媒体类型@来源@说明@文件名】，填法与【媒体组】的单项相同。 |
| KOOK | `文本` 接受 KMarkdown。`卡片` 传【KOOK卡片】。适配器按原消息选择频道或私聊编辑接口。 |

```text
【定义变量@消息ID@【输出流@活动开始时间待定】】
【定义变量@结果@【编辑消息@【变量@消息ID】@活动将于周六开始】】
【取元素@【变量@结果】@retcode】
```

Telegram 可选格式填 `Markdown`（MarkdownV2）、`HTML`，或留空发普通文本。第五项可选传【Telegram键盘】生成的行内键盘。第六项留空时第一项使用 RedReply 消息 ID。要编辑 Telegram 行内消息时，第六项填 `行内`，第一项填平台行内消息 ID，媒体使用已上传的文件 ID 或支持的直链。

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

【转发消息@消息ID或数组@目标ID@目标类型@是否复制@是否静默@话题ID@话题类型】

源 ID 省略时使用当前消息。目标省略时使用当前会话，跨会话时填写目标 ID，KOOK 私聊同时指定 `私聊`。Telegram 可使用聊天 ID 或 `【@@频道用户名】`。该命令转发已存在的消息，【合并转发】仍用于生成现有的合并消息节点。

- **Telegram** 使用 `forwardMessage` 或 `forwardMessages`。批量最多 100 条，需来自同一普通聊天，按平台消息 ID 升序发送。服务消息和受保护消息由 Telegram 限制。是否复制和是否静默分别默认 `假`，需要时填 `真`。复制后省去原转发来源。话题 ID 可省略，默认同一会话的当前话题。指定话题 ID 时，话题类型默认 `话题`，也可填 `频道私信`。数字目标的返回对象提供 `redreply_message_ids`，可逐条编辑或撤回。批量接口可能跳过不能转发的消息。
- **KOOK** 公开接口提供消息读取和发送。适配器读取原消息内容后重新发送文字、KMarkdown、卡片和常见附件，由机器人署名，卡片按钮的回调也会发给重发卡片的机器人。发送失败时，`data.sent` 保留此前已发送的 RedReply 消息 ID，可避免重试时重复发送。

```text
【定义变量@结果@【转发消息@【数组@原消息一@原消息二】@目标频道ID@频道】】
【取元素@【变量@结果】@retcode】
```

```text
【定义变量@结果@【转发消息@【消息ID】@-1001234567890@群聊@真@真@15】】
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
| `notice:telegram:edited_message`、`notice:telegram:edited_channel_post`、`notice:telegram:edited_business_message` | 【事件内容@消息文字】取得编辑后的文字或媒体说明，【消息ID】关联原消息。 |
| `notice:telegram:forwarded_message` | 收到带 `forward_origin` 的消息时触发。原始来源在 `telegram.forward_origin`，同一消息仍走相应的普通消息或频道、业务消息事件。 |
| `notice:telegram:deleted_business_messages` | 用【事件内容@商务连接ID】、【事件内容@已删除平台消息ID】和【事件内容@已删除消息ID】取得连接 ID 及两种消息 ID 数组。 |

Telegram Bot API 的删除更新仅覆盖已连接的业务账户。普通聊天或频道的删除以调用结果为准。KOOK 重新发送会作为机器人普通消息出现。自定义 Telegram 更新订阅时，填写原始更新名，例如 `message,channel_post,business_message,edited_message,edited_channel_post,edited_business_message,deleted_business_messages`。

以下示例用于普通群聊的 `notice:telegram:forwarded_message` 脚本，可读取来源类型并用【输出流】回复。

```text
【定义变量@来源@【事件内容@转发来源类型】】
【隐藏@【输出流@转发来源类型：【变量@来源】】】
```

[Telegram 更新类型](https://core.telegram.org/bots/api#update) · [KOOK 频道消息事件](https://developer.kookapp.cn/doc/event/channel) · [KOOK 私聊消息事件](https://developer.kookapp.cn/doc/event/direct-message)

## Telegram 群与频道管理

Telegram 聊天 ID 可从【群ID】取得。以用户名指定频道时，用原始字符串保留前导 `@`，例如 `【@@example_channel】`。

### 聊天资料、成员与置顶

| 命令 | 怎么填 |
| --- | --- |
| 【群管理@改名@新名称@群ID】 | 新名称必填。 |
| 【群管理@简介@新简介@群ID】 | 新简介留空可清除简介。 |
| 【群管理@封禁@用户ID@群ID】、【群管理@解封@用户ID@群ID】 | 用户 ID 默认发送者。解封只作用于已封禁的成员。 |
| 【Telegram群头像@字节集@文件名@群ID】、【群管理@移除头像@群ID】 | 设置时用【读文件】取得头像字节集，文件名必填。 |
| 【群管理@退出@群ID】 | 机器人退出目标会话。 |

这些命令的群 ID 均可省略，默认当前会话。【置顶消息@消息ID@是否置顶@是否静默】使用当前消息或【输出流】返回的 ID。第 2 项默认 `真`，填 `假` 取消置顶。第 3 项默认 `假`，填 `真` 静默置顶。

```text
【群管理@简介@这里发布活动安排和通知】
【置顶消息】
```

[Telegram 聊天资料](https://core.telegram.org/bots/api#getchat) · [置顶](https://core.telegram.org/bots/api#pinchatmessage)

### 默认权限、成员限制与管理员

| 命令 | 怎么填 |
| --- | --- |
| 【Telegram群权限@查询@群ID】 | 返回当前普通成员允许权限名称的数组。 |
| 【Telegram群权限@设置@权限数组@群ID】 | 权限数组必填，用【数组】填写要允许的所有权限。 |
| 【Telegram群权限@限制@用户ID@权限数组@有效秒数@群ID】 | 权限数组必填，填该成员仍可使用的全部权限。有效秒数可省略或填 0，表示长期限制。定时可填 30 秒至 366 天。 |
| 【Telegram群权限@管理员权限@用户ID@群ID】 | 返回该成员的管理员权限名称数组。 |
| 【Telegram群权限@设管理员@用户ID@权限数组@群ID】 | 权限数组必填，至少一项实际管理权限。填写要授予的全部权限。 |
| 【Telegram群权限@撤管理员@用户ID@群ID】 | 撤销该成员的管理员权限。 |

群 ID 默认当前会话，用户 ID 默认发送者。普通成员权限可填 `文本`、`音频`、`文件`、`图片`、`视频`、`视频便笺`、`语音`、`投票`、`其他消息`、`链接预览`、`表情回应`、`修改群资料`、`邀请成员`、`置顶`、`管理话题`、`编辑标签`。普通成员权限按完整列表写入，空【数组】表示全部禁止。【禁言@300】可暂停当前发送者的发送权限 300 秒，【禁言@0】恢复群默认发送权限并保留其他个人权限。

管理员权限可填 `匿名`、`管理聊天`、`删除消息`、`管理视频聊天`、`管理成员`、`委任管理员`、`修改群资料`、`邀请成员`、`发布动态`、`编辑动态`、`删除动态`、`发布频道消息`、`编辑频道消息`、`置顶`、`管理话题`、`管理频道私信`、`管理标签`、`欢迎消息`。授予范围受机器人权限和聊天类型限制，设管理员会写入完整权限列表。

```text
【Telegram群权限@设置@【数组@文本】】
```

```text
【Telegram群权限@设管理员@123456789@【数组@管理话题】】
```

机器人具备管理员身份并订阅 `chat_member` 时可接收 `notice:telegram:chat_member`。机器人自身身份变更使用 `notice:telegram:my_chat_member`。

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

机器人须具备邀请权限。在 `notice:telegram:chat_join_request` 中用【同意】或【拒绝】处理申请。主链接和订阅邀请也沿用【邀请链接】。

| 命令 | 怎么填 |
| --- | --- |
| 【邀请链接@主链接@群ID】 | 群 ID 默认当前会话。返回新主链接，旧主链接随之失效。 |
| 【邀请链接@创建付费@星币数@名称@频道ID】 | 星币数必填，1 至 10000，每 30 天收取一次。名称可省，最多 32 字。频道 ID 默认当前频道。返回链接。 |
| 【邀请链接@编辑付费@链接@名称@频道ID】 | 链接必填，名称留空则由 Telegram 保留，填 `清空` 移除名称。频道 ID 默认当前频道。返回编辑后的链接。 |

```text
【邀请链接@创建付费@100@活动会员】
```

[Telegram 邀请链接](https://core.telegram.org/bots/api#createchatinvitelink)

### 论坛话题与频道私信话题

| 命令 | 怎么填 |
| --- | --- |
| 【Telegram话题@创建@名称@群ID@颜色@图标ID】 | 名称必填。颜色和图标 ID 可省略，颜色可填蓝色、黄色、紫色、绿色、粉色或红色。成功返回话题 ID。 |
| 【Telegram话题@编辑@话题ID@名称@群ID@图标ID】 | 名称和图标至少填一项。图标填 `移除` 可清除自定义图标。 |
| 【Telegram话题@关闭或重开或删除或清空置顶@话题ID@群ID】 | 选择其中一个操作。关闭和重开适用于论坛超级群。 |
| 【Telegram话题@改名默认@名称@群ID】 | 名称必填，操作默认话题。 |
| 【Telegram话题@关闭默认或重开默认或隐藏默认或显示默认或清空默认置顶@群ID】 | 选择其中一个操作，适用于论坛超级群的默认话题。 |
| 【Telegram话题@图标】 | 返回可用图标数组，每项可取图标ID、表情、文件ID。 |

群 ID 默认当前会话。话题 ID 默认当前论坛话题。指定其他会话时，编辑或管理已有话题须明确填写话题 ID。新建话题可在论坛超级群及启用话题模式的私聊中使用。

```text
【定义变量@话题@【Telegram话题@创建@周末活动】】
【隐藏@【输出流@请在此话题报名@@【变量@话题】】】
```

```text
【Telegram话题@关闭】
```

向频道私信话题发送时，在【输出流】第三个参数填话题 ID，第四个参数填 `频道私信`。当前 ID 可用【事件内容@频道私信话题ID】取得。话题事件使用 `notice:telegram:forum_topic_created`、`notice:telegram:forum_topic_edited`、`notice:telegram:forum_topic_closed`、`notice:telegram:forum_topic_reopened`、`notice:telegram:general_forum_topic_hidden`、`notice:telegram:general_forum_topic_unhidden`。

[话题接口](https://core.telegram.org/bots/api#createforumtopic) · [话题服务消息](https://core.telegram.org/bots/api#message)

## KOOK 服务器与频道管理

在 KOOK，【群ID】是频道 ID，【当前群组】是服务器 ID。在 CRON 或私聊中管理服务器时可显式填服务器 ID。【取群列表@服务器ID】会遍历分页并返回文字频道，省略时使用当前服务器或从当前频道查询所属服务器。完整频道类型和分组信息用 `channel/list` 查询。

### 频道与成员

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【KOOK服务器@详情@服务器ID】、【KOOK服务器@列表@页码】 | 服务器 ID 默认当前服务器，页码默认 1。查询结果有服务器ID、名称、简介、所有者ID、图标、默认频道ID、欢迎频道ID。列表返回当页数组。 |
| 【群信息@频道ID】 | 频道 ID 默认【群ID】。返回群ID、群名、简介、类型、服务器ID、分组ID、慢速秒数。 |
| 【群成员@详情@用户ID@服务器ID】 | 用户 ID 默认发送者，服务器 ID 默认【当前群组】或从当前频道解析。返回用户ID、昵称、用户名、头像、在线、机器人、身份组。 |
| 【群成员@列表@服务器ID@页码@搜索词@身份组ID@每页数量】 | 全部可省略。默认当前服务器、第 1 页和平台每页大小。可按昵称或用户名筛选，可填数字身份组 ID。每页数量可填 1 至 50。返回成员数组。 |
| 【群成员@人数@服务器ID】、【群成员@语音@频道ID】 | ID 可省略，分别返回服务器总人数和语音频道成员数组。 |
| 【KOOK频道@创建@名称@类型@服务器ID@上级分组ID@人数上限】 | 名称必填。类型默认文字，可填语音或分组。服务器 ID 可省略。上级分组 ID 可省略，创建分组时留空。语音人数上限可填 0 至 99，0 表示不限制。返回新频道 ID。 |
| 【KOOK频道@编辑@频道ID@新名称@新简介@慢速秒数@分组ID】 | 频道 ID 可省略，其他参数至少填一项。简介填 `清空` 时移除。分组 ID 填 0 移出分组。 |
| 【KOOK频道@删除@频道ID】、【KOOK频道@列表@服务器ID@页码@类型@分组ID】 | ID 默认当前频道或服务器。列表默认第 1 页和文字频道，类型可填 `语音`，分组 ID 可省略。返回与【群信息】同结构的数组。 |

慢速秒数可填 0、5、10、15、30、60、120、300、600、900、1800、3600、7200、21600，0 关闭。已有【取群列表】可继续查询所有页的文字频道。

```text
【KOOK频道@编辑@@@周末活动报名处@5】
```

```text
【KOOK频道@创建@活动语音@语音】
```

【群管理@改名@新名称@频道ID】和【群管理@简介@新简介@频道ID】也适用于 KOOK。服务器成员可用【群管理@踢出@用户ID@服务器ID】、【群管理@封禁@用户ID@服务器ID】、【群管理@解封@用户ID@服务器ID】管理；用户默认发送者。服务器 ID 默认当前服务器。机器人退出服务器用【群管理@退出@服务器ID】。

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【群管理@黑名单@服务器ID@页码@每页数量】 | 全部可省，默认当前服务器、第 1 页、每页 50 项。页码填正整数，每页 1 至 50 项。返回当页数组，每项有用户ID、昵称、用户名、头像、在线、机器人、身份组、封禁时间、原因。封禁时间为 Unix 毫秒。 |
| 【群管理@封禁@用户ID@服务器ID@原因@清理天数】 | 用户 ID 默认发送者，服务器 ID 默认当前服务器。原因可省。清理天数默认 0，可填 0 至 7，正数清理该用户最近对应天数的消息。 |
| 【群管理@解封@用户ID@服务器ID】 | 用户和服务器 ID 的省略规则同封禁。 |
| 【群管理@昵称@用户ID@新昵称@服务器ID】 | 用户 ID 默认发送者，服务器 ID 默认当前服务器。昵称填 2 至 64 字，留空清除。 |
| 【KOOK服务器@助力历史@服务器ID@页码@每页数量@开始时间@结束时间】 | 全部可省。默认当前服务器、第 1 页、每页 50 项，不限时间。时间用 Unix 秒，可只填一端。每页 1 至 50 项。由新到旧返回当前页记录，每项有用户ID、服务器ID、开始时间、结束时间和成员。需要管理服务器权限。 |

查询黑名单、封禁及解封都需要封禁用户权限。

```text
【取长度@【群管理@黑名单】】
```

```text
【群管理@封禁@目标用户ID@@重复刷屏@3】
```

```text
【群管理@昵称@目标用户ID@活动主持人】
```

```text
【KOOK服务器@助力历史@@1@20】
```

频道增删改的事件为 `notice:kook:added_channel`、`notice:kook:updated_channel`、`notice:kook:deleted_channel`。可用【群ID】和【当前群组】取得频道与服务器。

[KOOK 频道](https://developer.kookapp.cn/doc/http/channel) · [KOOK 服务器](https://developer.kookapp.cn/doc/http/guild) · [黑名单](https://developer.kookapp.cn/doc/http/blacklist)

### 身份组与频道权限覆盖

| 命令 | 怎么填 |
| --- | --- |
| 【KOOK身份组@列表@页码@服务器ID】 | 页码默认 1，服务器 ID 默认当前服务器。返回数组，每项有身份组ID、名称、颜色、权限、权限名称、可提及、单独显示。 |
| 【KOOK身份组@创建@名称@服务器ID】 | 名称可省略，使用平台默认名称。返回身份组 ID。 |
| 【KOOK身份组@编辑@身份组ID@名称@权限数组@颜色@是否单独显示@是否可提及@服务器ID】 | 身份组 ID 必填。其他配置留空不修改，至少填一项配置。颜色填 0 至 16777215 的十进制值，两个开关填 `真` 或 `假`。 |
| 【KOOK身份组@授予或移除@身份组ID@用户ID@服务器ID】 | 操作填 `授予` 或 `移除`，身份组 ID 必填，用户 ID 默认发送者。 |
| 【KOOK身份组@删除@身份组ID@服务器ID】 | 身份组 ID 必填。 |
| 【KOOK频道权限@查询或同步@频道ID】 | 操作填 `查询` 或 `同步`，频道 ID 默认当前频道。返回身份组和成员两组覆盖，每项有目标ID、允许权限和禁止权限数组，也有允许和禁止的数值。 |
| 【KOOK频道权限@创建或删除@目标类型@目标ID@频道ID】 | 类型必填 `身份组` 或 `成员`，目标 ID 必填，频道 ID 默认当前频道。 |
| 【KOOK频道权限@设置@目标类型@目标ID@允许权限数组@禁止权限数组@频道ID】 | 两个权限数组均必填，空【数组】表示该侧无覆盖。同一权限只可出现在一侧。会重写这一条覆盖，先查询可取得要保留的权限。 |

服务器 ID 省略时使用当前服务器。权限数组可填 `管理员`、`管理服务器`、`查看管理日志`、`创建服务器邀请`、`管理邀请`、`频道管理`、`踢出用户`、`封禁用户`、`管理自定义表情`、`修改服务器昵称`、`管理角色权限`、`查看频道`、`发布消息`、`管理消息`、`上传文件`、`语音连接`、`语音管理`、`提及全体成员`、`添加回应`、`跟随添加回应`、`被动连接语音频道`、`仅使用按键说话`、`使用自由麦`、`说话`、`服务器静音`、`服务器闭麦`、`修改他人昵称`、`播放伴奏`、`屏幕分享`、`回复帖子`、`开启录音`。也接受已保存的权限数值。

```text
【KOOK频道权限@设置@身份组@702@【数组@查看频道】@【数组@发布消息】】
```

```text
【KOOK身份组@授予@702@【发送者ID】】
```

【权限】与【BOT权限】可继续查询当前用户及机器人的角色。身份组事件用 `notice:kook:added_role`、`notice:kook:updated_role`、`notice:kook:deleted_role`。

[身份组与权限位](https://developer.kookapp.cn/doc/http/guild-role) · [频道权限](https://developer.kookapp.cn/doc/http/channel)

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

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【KOOK帖子@分区@频道ID】 | 返回分区数组，每项可取分区ID、名称。 |
| 【KOOK帖子@列表@频道ID@分区ID@排序@时间@每页数量】 | 分区默认综合，排序默认频道设置，也可填 `最近回复` 或 `最新创建`。每页数量默认 30，可填 1 至 50。按时间翻页时须同时指定排序，时间填上一页最后一项的活跃时间或创建时间。 |
| 【KOOK帖子@详情@帖子ID@频道ID】 | 帖子 ID 必填。返回对象，可取帖子ID、标题、封面、主楼ID、分区ID、摘要、内容、回复数、创建时间、活跃时间。列表返回同结构数组。 |
| 【KOOK帖子@发布@标题@卡片@频道ID@服务器ID@分区ID@封面链接】 | 标题及【KOOK卡片】必填，分区默认综合，可选填封面链接。服务器 ID 省略时从频道取得。返回帖子 ID。 |
| 【KOOK帖子@回复@帖子ID@回复文字@频道ID@上级回复ID】 | 帖子 ID、回复文字必填。不填上级回复 ID 时回复主楼。返回回复 ID。 |
| 【KOOK帖子@回复列表@帖子ID@频道ID@页码@顺序@上级回复ID】 | 帖子 ID 必填。页码默认 1，顺序默认 `由早到晚`，也可填 `由晚到早`。查询楼中楼时填写上级回复 ID。返回回复数组，每项有回复ID、帖子ID、上级回复ID、内容、用户ID、创建时间。 |
| 【KOOK帖子@删除@帖子ID@频道ID】、【KOOK帖子@删除回复@回复ID@频道ID】 | 帖子或回复 ID 必填。 |

所有操作的频道 ID 均可省略，默认当前帖子频道。

```text
【定义变量@帖子ID@【KOOK帖子@发布@活动报名@【KOOK卡片@请在本帖回复报名】】】
【KOOK帖子@回复@【变量@帖子ID】@本次活动将在周六举行】
```

```text
【取长度@【KOOK帖子@回复列表@帖子ID】】
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

用【文件】直接发到当前频道或私聊。需要先获得卡片、封面或帖子用的资源链接时，用【KOOK上传文件@字节集@文件名】。两项必填。字节集通过【读文件】取得。收到文件模块时可使用 `notice:group_upload`，用【事件内容@文件名】和【事件内容@文件链接】读取资料。

```text
【隐藏@【输出流@【文件@【应用目录】说明.pdf@活动说明.pdf】】】
```

```text
【KOOK上传文件@【读文件@【应用目录】说明.pdf】@活动说明.pdf】
```

[KOOK 媒体模块](https://developer.kookapp.cn/doc/http/asset)

### 共享播放与语音管理

【KOOK共享播放@语音频道ID@音源@音量百分比@频道密码】的前两项必填。音源可为 HTTP(S) 直链、本地绝对路径或最多 32 首的【数组】。音量为 0 至 100，默认 100，频道密码可省略。运行机器的 PATH 需包含支持 `libopus` 的 FFmpeg，机器人需要加入和语音权限，媒体服务器的 UDP 端口需可达。

```text
【定义变量@音源@【数组@https://example.org/first.mp3@https://example.org/second.mp3】】
【隐藏@【KOOK共享播放@语音频道ID@【变量@音源】@65】】
```

【KOOK播放状态@查询项】可填 `状态`、`当前曲目`、`曲目总数`、`频道ID` 或 `错误`，省略时返回完整状态对象。状态值有 `空闲`、`播放中`、`停止中`、`已播完`、`已停止`、`失败`。【KOOK停止播放】无参数。每首开始时触发 `notice:kook:voice_track_started`，可用【事件内容@曲目序号】和【事件内容@曲目总数】读取进度。列表结束触发 `notice:kook:voice_playback`，用【事件内容@播放状态】读取结果。

```text
【KOOK播放状态@状态】
```

| 命令 | 怎么填及返回值 |
| --- | --- |
| 【KOOK语音管理@已加入】 | 返回机器人已加入的语音频道数组。 |
| 【群成员@语音@频道ID】 | 默认当前频道，返回语音成员数组。 |
| 【KOOK语音管理@移动@用户数组@目标频道ID】 | 两项必填，用【数组】填一个或多个在线语音用户 ID，目标必须是语音频道。 |
| 【KOOK语音管理@移出@用户ID@频道ID】 | 用户 ID 默认发送者，频道 ID 默认当前频道。 |
| 【KOOK语音管理@限制列表@服务器ID】 | 服务器 ID 默认当前服务器。返回对象，`闭麦` 和 `静音` 分别为两种限制下的用户 ID 数组，无成员时为空数组。 |
| 【KOOK语音管理@闭麦@用户ID@服务器ID】、【KOOK语音管理@解除闭麦@用户ID@服务器ID】 | 用户 ID 默认发送者，服务器 ID 默认当前服务器。禁止或恢复用户在该服务器的麦克风发言。 |
| 【KOOK语音管理@静音@用户ID@服务器ID】、【KOOK语音管理@解除静音@用户ID@服务器ID】 | 用户和服务器 ID 的省略规则同闭麦。静音限制的是该用户的耳机收听。 |

服务器语音限制持续到解除，闭麦和静音分别管理。机器人需要对应的服务器闭麦或静音权限。

```text
【取长度@【群成员@语音@语音频道ID】】
```

```text
【KOOK语音管理@移动@【数组@用户一ID@用户二ID】@目标频道ID】
```

```text
【KOOK语音管理@闭麦@目标用户ID】
【取元素@【KOOK语音管理@限制列表】@闭麦】
```

```text
【KOOK语音管理@解除静音@目标用户ID】
```

[KOOK 语音接口](https://developer.kookapp.cn/doc/http/voice) · [频道管理](https://developer.kookapp.cn/doc/http/channel) · [服务器静音与闭麦](https://developer.kookapp.cn/doc/http/guild)

### 频道历史与消息详情

| 命令 | 怎么填 |
| --- | --- |
| 【KOOK频道消息@最新或置顶@条数@频道ID】 | 选择 `最新` 或 `置顶`。条数可填 1 至 50，默认 50，频道 ID 默认当前频道。 |
| 【KOOK频道消息@之前或之后或附近@条数@频道ID@参考消息ID】 | 选择查询方向。参考消息 ID 必填，可从上次查询结果或【事件内容@平台消息ID】取得。 |
| 【KOOK频道消息@详情@平台消息ID】 | 平台消息 ID 必填，返回一条消息。 |

私聊历史使用【KOOK频道消息@最新@条数@目标用户ID@@私聊】。支持相同的时间方向，参考消息 ID 放第四项，最后一项填 `私聊`。私聊详情使用【KOOK频道消息@详情@平台消息ID@会话码@@私聊】，会话码可在私聊消息事件内省略。

```text
【KOOK频道消息@最新@10@目标用户ID@@私聊】
```

查询返回的消息对象有消息ID、正文、发送者ID、发送时间、附件链接、文件名。这里的消息 ID 是 KOOK 平台 ID，可用于继续翻页。缓存中的 RedReply 消息 ID 可继续通过【群历史消息】取得，用于【编辑消息】、【置顶消息】或【撤回】。

```text
【取长度@【KOOK频道消息@最新@10】】
```

[KOOK 频道历史](https://developer.kookapp.cn/doc/http/message) · [私聊历史](https://developer.kookapp.cn/doc/http/direct-message) · [频道事件](https://developer.kookapp.cn/doc/event/channel)

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
