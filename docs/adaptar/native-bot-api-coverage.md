# KOOK 与 Telegram Bot API 逐项对照

快照日期为 2026 年 11 月 14 日（香港时间）。KOOK 按公开 v3 文档及公开 OAuth2 换令牌接口，Telegram 按官方 Bot API 10.4。基线使用 PR #4 的 `343e948`，逐项清单在接入后更新。脚本参数和示例见[使用说明](native-bot-api.md)。 [KOOK 官方目录](https://developer.kookapp.cn/doc/reference) · [Telegram Bot API 10.4](https://core.telegram.org/bots/api#november-9-2026)

| 状态 | 判断依据 |
| --- | --- |
| 已接入 | 有直接填值的 RedLang 命令或带对应结构的事件入口。 |
| 部分 | 有脚本入口，但缺主要模式、行为或专用字段。只通过通用通知拿到原生对象的事件也归为部分。 |
| 未接入 | 缺直接填值入口或独立事件入口。通用 API 调用不计为专用接入。 |
| 配置／运行时 | 由后台或适配器管理的开关、传输协议或内部调用。 |
| 平台限制 | 官方标为弃用、未开放，或属于另一种授权流程。 |

## 本轮对照出的优先缺口

| 平台 | 项目 | 变更前 | 本轮状态 | RedLang 入口 |
| --- | --- | --- | --- | --- |
| KOOK | 模板列表、创建、更新、删除 | 未接入 | 已接入 | 【KOOK模板管理】 |
| KOOK | 频道、私聊的模板发送与编辑 | 部分 | 部分 | 【输出流】接收【KOOK模板消息】，【编辑消息】支持模板类型，原生的临时目标和高级回复参数另计。 |
| KOOK | 入管道消息 | 未接入 | 已接入 | 【KOOK管道消息】发送文字、卡片或模板输入。 |
| KOOK | 私聊会话列表、详情、创建与删除 | 部分／未接入 | 已接入 | 【KOOK私聊会话】，可通过【事件内容@私聊会话码】读会话码。 |
| KOOK | 查询成员所在语音频道 | 未接入 | 已接入 | 【KOOK语音管理@用户所在频道】。 |
| KOOK | 黑名单变动事件 | 部分 | 已接入 | 【事件内容@黑名单变动】读取成员、操作人、原因与服务器。 |
| Telegram | 发送发票 | 未接入 | 部分 | 【输出流】接收【Telegram发票】，覆盖商品、价格、联系资料、物流、小费、图片等常见参数，仍有高级发送属性待接入。 |
| Telegram | 生成发票链接 | 未接入 | 已接入 | 【Telegram发票链接】支持普通链接与星币订阅链接。 |
| Telegram | 运费、预结算应答 | 未接入 | 已接入 | 【同意】与【拒绝】按当前查询类型应答。 |
| Telegram | 星币余额、流水、退款、取消或恢复续订 | 未接入 | 已接入 | 【Telegram星币】支持四项方法。 |
| Telegram | 发送付费媒体 | 未接入 | 部分 | 【输出流】接收【Telegram付费媒体】，支持常见图片、视频、说明与订单识别值。 |
| Telegram | 运费、预结算、支付成功、已退款、购买付费媒体事件 | 部分／未接入 | 已接入 | 【事件内容@支付信息】读取中文支付字段，支付成功与退款另有独立事件。 |
| Telegram | 位置、场所、联系人及其事件 | 部分／未接入 | 已接入常用功能 | 通过【输出流】发送。位置支持【编辑消息】更新和停止。对应事件提供中文字段。临时发送支持静态位置、场所和联系人。 |
| KOOK | 查询和更新亲密度 | 未接入 | 已接入 | 【KOOK亲密度】支持分数、文字和形象。 |
| KOOK | 服务器徽章 | 未接入 | 已接入 | 【KOOK服务器@徽章】提供三种样式和图片字节集。 |
| KOOK | 游戏列表、创建、更新、删除 | 未接入 | 已接入 | 【KOOK游戏】支持列表范围及分页。 |
| KOOK | 游戏和音乐动态 | 未接入 | 已接入 | 【KOOK动态】可分别开始和停止，官方公开接口提供写入操作。 |
| Telegram | 发送语音便笺 | 未接入 | 部分 | 【语音】的便笺模式支持本轮公共发送选项。实体数组和付费广播待接入。 |
| Telegram | 发送视频便笺 | 未接入 | 部分 | 【视频】的便笺模式支持本轮公共发送选项，付费广播待接入。 |
| Telegram | 发送和编辑动画 | 未接入／部分 | 部分 | 【动画】、【编辑消息】。发送支持本轮公共选项，实体数组和付费广播待接入。 |
| Telegram | 语音便笺、视频便笺、动画的新消息和编辑事件 | 部分 | 已接入 | 独立通知及【事件内容】中文选择项。 |
| Telegram | 静默发送和内容保护 | 部分 | 已接入 | 【输出流】、【发送私聊】、【Telegram文件】、【Telegram投票】、【转发消息】的发送选项。 |
| Telegram | 消息效果 | 未接入 | 已接入 | 发送选项的第三项填写效果 ID，按公开方法限制为私聊。 |
| Telegram | 发送、审核建议帖和对应事件 | 未接入 | 已接入 | 发送选项的第四项用于频道私信。【同意】、【拒绝】和【事件内容@建议帖】沿用同类命令。 |
| Telegram | 临时消息的发送、编辑、撤回和事件 | 未接入 | 已接入常用功能 | 【输出流】定向发送，【编辑消息】和【撤回】复用临时模式，提供独立事件。富文本和临时命令开关另列。 |
| KOOK | 频道音质、密码、容量、排序、分页及子频道 | 部分 | 已接入 | 【KOOK频道】和【群信息】接受位置参数。 |
| KOOK | 服务器分页、成员认证和时间排序 | 部分 | 已接入 | 【KOOK服务器@列表】和【群成员@列表】。 |
| KOOK | 频道临时发送与更新、消息标记与配额回复 | 部分 | 已接入 | 【输出流】、【发送私聊】和【编辑消息】接受位置参数。 |
| KOOK | 私聊会话码、历史页码及消息结构 | 部分 | 已接入常用功能 | 【发送私聊】和【KOOK频道消息】支持会话码及相关查询位置。 |
| Telegram | 富文本、富文本草稿及事件 | 未接入 | 已接入常用功能 | 【Telegram富文本】配合【输出流】和【编辑消息】，【Telegram富文本草稿】支持中止按钮。 |
| Telegram | 临时命令 | 部分 | 已接入 | 【Telegram命令菜单】支持设置临时开关及查询详情。 |
| Telegram | 高级引用 | 部分 | 已接入常用功能 | 发送支持片段、位置、跨会话、清单、投票、富文本引文及引用样式数组。事件提供中文引用字段。 |
| KOOK | 消息链接预览 | 部分 | 已接入 | 【KOOK频道消息】归一化预览字段，【事件内容@链接预览】随事件提供。 |
| KOOK | 服务器事件 | 部分 | 已接入 | 【事件内容@服务器事件】读取成员、资料、表情和封禁。 |
| Telegram | 富文本内容块 | 部分 | 已接入 | 【Telegram富文本】覆盖官方块类型，事件提供中文嵌套内容、表格、按钮和媒体字段。 |
| Telegram | 行内富文本结果 | 未接入 | 已接入 | 【Telegram应答查询】、【Telegram小程序应答】、【Telegram准备行内消息】，选择事件有中文字段。 |
| Telegram | 编辑媒体、单条转发和复制、批量复制 | 部分 | 已接入常用功能 | 【编辑消息】和【转发消息】补齐常用位置选项，复制可改说明、附键盘或引用。 |
| KOOK | 视频和音频 | 部分 | 已接入 | 普通事件和查询提供视频、音频和卡片媒体列表的中文字段。 |

下文每项标注基线状态。具体实现以本 PR 的位置参数说明为准。

## KOOK HTTP 接口

共 97 项。路径默认位于 `/api/v3/`，已去重各页面的交叉引用。

### 服务器、成员及服务器语音

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/guild)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `guild-boost/history` | 已接入 | 【KOOK服务器@助力历史】 |
| `guild-mute/create` | 已接入 | 【KOOK语音管理】闭麦、静音、解除与限制列表 |
| `guild-mute/delete` | 已接入 | 【KOOK语音管理】闭麦、静音、解除与限制列表 |
| `guild-mute/list` | 已接入 | 【KOOK语音管理】闭麦、静音、解除与限制列表 |
| `guild/kickout` | 已接入 | 【群管理】踢出、退出、昵称 |
| `guild/leave` | 已接入 | 【群管理】踢出、退出、昵称 |
| `guild/list` | 部分 | 【KOOK服务器@列表】支持页码和每页数量。官方列出通用排序语法，未列出此接口支持的排序字段 |
| `guild/nickname` | 已接入 | 【群管理】踢出、退出、昵称 |
| `guild/user-list` | 已接入 | 【群成员】支持成员、搜索、身份组、分页、手机认证、时间排序和频道筛选 |
| `guild/view` | 已接入 | 【KOOK服务器@详情】提供服务器资料、身份组列表和频道列表 |

### 频道与频道权限

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/channel)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `channel-role/create` | 已接入 | 【KOOK频道权限】查询、创建、设置、删除、同步 |
| `channel-role/delete` | 已接入 | 【KOOK频道权限】查询、创建、设置、删除、同步 |
| `channel-role/index` | 已接入 | 【KOOK频道权限】查询、创建、设置、删除、同步 |
| `channel-role/sync` | 已接入 | 【KOOK频道权限】查询、创建、设置、删除、同步 |
| `channel-role/update` | 已接入 | 【KOOK频道权限】查询、创建、设置、删除、同步 |
| `channel/create` | 已接入 | 【KOOK频道@创建】支持文字、语音、分组及语音音质 |
| `channel/delete` | 已接入 | 【KOOK频道@删除】 |
| `channel/kickout` | 已接入 | 【KOOK语音管理】移出、移动 |
| `channel/list` | 已接入 | 【KOOK频道@列表】支持页码、每页数量、类型和分组 |
| `channel/message` | 平台限制 | 已弃用，使用频道消息创建 |
| `channel/move-user` | 已接入 | 【KOOK语音管理】移出、移动 |
| `channel/update` | 已接入 | 【KOOK频道@编辑】支持名称、简介、慢速、分组、排序、容量、音质及密码 |
| `channel/user-list` | 已接入 | 【群成员@语音】 |
| `channel/view` | 已接入 | 【群信息】支持读取子频道 |

### 频道消息

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/message)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `message/add-reaction` | 已接入 | 【表情回应】 |
| `message/create` | 已接入 | 【输出流】支持普通消息、媒体、模板、临时目标、标记及配额回复 |
| `message/delete` | 已接入 | 【撤回】 |
| `message/delete-reaction` | 已接入 | 【表情回应】 |
| `message/list` | 部分 | 【KOOK频道消息】支持公开接口的消息查询与常用字段，返回中文链接预览及常用消息属性 |
| `message/pin` | 已接入 | 【置顶消息】 |
| `message/reaction-list` | 已接入 | 【KOOK回应列表】 |
| `message/send-pipemsg` | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| `message/unpin` | 已接入 | 【置顶消息】 |
| `message/update` | 已接入 | 【编辑消息】支持文本、卡片、模板、临时目标、引用修改和配额回复 |
| `message/view` | 部分 | 【KOOK频道消息】支持公开接口的消息查询与常用字段，返回中文链接预览及常用消息属性 |

### 频道用户

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/channel-user)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `channel-user/get-joined-channel` | 未接入 | 缺直接填值的命令 |

### 私聊会话

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/user-chat)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `user-chat/create` | 未接入 | 缺直接填值的命令 |
| `user-chat/delete` | 未接入 | 缺直接填值的命令 |
| `user-chat/list` | 部分 | 适配器 OneBot 好友列表使用该路径，缺会话管理的 RedLang 位置参数入口 |
| `user-chat/view` | 未接入 | 缺直接填值的命令 |

### 私聊消息

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/direct-message)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `direct-message/add-reaction` | 已接入 | 【表情回应】 |
| `direct-message/create` | 已接入 | 【输出流】和【发送私聊】支持普通消息、媒体、模板、私聊会话码、标记和配额回复 |
| `direct-message/delete` | 已接入 | 【撤回】 |
| `direct-message/delete-reaction` | 已接入 | 【表情回应】 |
| `direct-message/list` | 部分 | 【KOOK频道消息】支持私聊会话码、页码、引用和常用媒体属性，链接预览已映射为中文字段 |
| `direct-message/reaction-list` | 已接入 | 【KOOK回应列表】 |
| `direct-message/update` | 已接入 | 【编辑消息】支持文本、卡片、模板、引用修改和配额回复 |
| `direct-message/view` | 部分 | 【KOOK频道消息】支持公开接口的消息查询与常用字段，返回中文链接预览及常用消息属性 |

### 网关

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/gateway)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `gateway/index` | 配置／运行时 | 适配器建立 WebSocket 连接时调用 |

### 用户

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/user)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `user/get-online-status` | 未接入 | 缺直接填值的命令 |
| `user/me` | 配置／运行时 | 适配器登录和获取机器人身份时调用 |
| `user/offline` | 未接入 | 缺直接填值的命令 |
| `user/online` | 未接入 | 缺直接填值的命令 |
| `user/view` | 已接入 | 【取发送者信息】【取头像】【权限】【BOT权限】 |

### 语音连接

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/voice)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `voice/join` | 配置／运行时 | 【KOOK共享播放】【KOOK停止播放】管理播放连接，尚无手动连接生命周期命令 |
| `voice/keep-alive` | 配置／运行时 | 【KOOK共享播放】【KOOK停止播放】管理播放连接，尚无手动连接生命周期命令 |
| `voice/leave` | 配置／运行时 | 【KOOK共享播放】【KOOK停止播放】管理播放连接，尚无手动连接生命周期命令 |
| `voice/list` | 已接入 | 【KOOK语音管理@已加入】 |

### 媒体资源

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/asset)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `asset/create` | 已接入 | 【KOOK上传文件】 |

### 服务器身份组

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/guild-role)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `guild-role/create` | 已接入 | 【KOOK身份组】 |
| `guild-role/delete` | 已接入 | 【KOOK身份组】 |
| `guild-role/grant` | 已接入 | 【KOOK身份组】 |
| `guild-role/list` | 已接入 | 【KOOK身份组】 |
| `guild-role/revoke` | 已接入 | 【KOOK身份组】 |
| `guild-role/update` | 已接入 | 【KOOK身份组】 |

### 亲密度

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/intimacy)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `intimacy/index` | 已接入 | 【KOOK亲密度@查询】 |
| `intimacy/update` | 已接入 | 【KOOK亲密度@更新】和【KOOK亲密度@清空社交信息】 |

### 服务器表情

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/guild-emoji)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `guild-emoji/create` | 已接入 | 【KOOK服务器表情】 |
| `guild-emoji/delete` | 已接入 | 【KOOK服务器表情】 |
| `guild-emoji/list` | 已接入 | 【KOOK服务器表情】 |
| `guild-emoji/update` | 已接入 | 【KOOK服务器表情】 |

### 邀请

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/invite)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `invite/create` | 已接入 | 【邀请链接】 |
| `invite/delete` | 已接入 | 【邀请链接】 |
| `invite/invitees` | 已接入 | 【邀请链接】 |
| `invite/list` | 已接入 | 【邀请链接】 |

### 黑名单

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/blacklist)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `blacklist/create` | 已接入 | 【群管理】封禁、解封、黑名单 |
| `blacklist/delete` | 已接入 | 【群管理】封禁、解封、黑名单 |
| `blacklist/list` | 已接入 | 【群管理】封禁、解封、黑名单 |

### 徽章

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/badge)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `badge/guild` | 已接入 | 【KOOK服务器@徽章】返回图片字节集 |

### 游戏及音乐状态

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/game)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `game` | 已接入 | 【KOOK游戏@列表】 |
| `game/activity` | 已接入 | 【KOOK动态@游戏】和【KOOK动态@音乐】 |
| `game/create` | 已接入 | 【KOOK游戏@创建】 |
| `game/delete` | 已接入 | 【KOOK游戏@删除】 |
| `game/delete-activity` | 已接入 | 【KOOK动态@停止游戏】和【KOOK动态@停止音乐】 |
| `game/update` | 已接入 | 【KOOK游戏@更新】 |

### 帖子

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/thread)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `category/list` | 已接入 | 【KOOK帖子】 |
| `thread/create` | 已接入 | 【KOOK帖子】 |
| `thread/delete` | 已接入 | 【KOOK帖子】 |
| `thread/list` | 已接入 | 【KOOK帖子】 |
| `thread/post` | 已接入 | 【KOOK帖子】 |
| `thread/reply` | 已接入 | 【KOOK帖子】 |
| `thread/view` | 已接入 | 【KOOK帖子】 |

### OAuth2

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/oauth)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `/api/oauth2/token` | 平台限制 | OAuth2 用户授权换令牌 |

### 模板管理及管道

[KOOK 官方接口](https://developer.kookapp.cn/doc/http/template)

| 官方路径 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| `template/create` | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| `template/delete` | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| `template/list` | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| `template/update` | 未接入 | 缺直接填值的命令，本轮优先补齐 |

### 管道、授权与事件传输边界

| 项目 | 当前状态 | 说明 |
| --- | --- | --- |
| 入管道的创建与配置 | 配置／运行时 | 目前在开发者后台创建并取得入管道令牌。 |
| 出管道 | 平台限制 | KOOK 当前未开放。 |
| 模板审核 | 平台限制 | KOOK 文档标为待开发。 |
| 模板消息预览 | 配置／运行时 | 开发者后台及官方预览组件。 |
| OAuth2 授权跳转 | 平台限制 | 由用户同意，后续请求使用 Bearer，不属于已连接机器人的 Bot 令牌。 |
| WebSocket 连接、压缩、握手、事件、心跳、重连 | 配置／运行时 | KOOK 适配器使用 WebSocket。涉及恢复会话的细节不计入脚本方法。 |
| Webhook 地址、Challenge、响应、加密 | 未接入 | KOOK 适配器没有对等的 Webhook 收件入口。 |

[消息模板和管道](https://developer.kookapp.cn/doc/http/template) · [消息预览](https://developer.kookapp.cn/doc/message-preview) · [OAuth2](https://developer.kookapp.cn/doc/oauth2) · [WebSocket](https://developer.kookapp.cn/doc/websocket) · [Webhook](https://developer.kookapp.cn/doc/webhook)

## KOOK 事件

共 34 项命名系统事件和 8 种具有官方结构说明的普通消息。系统事件通过类型 255 的消息接收。

| 普通消息类型 | 当前状态 | RedLang 入口 |
| --- | --- | --- |
| 1 文字 | 已接入 | 私聊或频道消息 |
| 2 图片 | 已接入 | 消息内容含图片 |
| 3 视频 | 已接入 | 【事件内容@视频】、查询记录中的视频对象，包含链接、时长和尺寸 |
| 4 文件 | 已接入 | 普通消息以及可识别的群文件通知 |
| 8 音频 | 已接入 | 【事件内容@音频】、查询记录中的音频对象，卡片可读媒体列表 |
| 9 KMarkdown | 已接入 | 私聊或频道消息 |
| 10 卡片 | 已接入 | 私聊或频道消息，媒体组合可触发专用通知 |
| 12 道具 | 未接入 | 适配器按字符串读取正文，无法完整接收道具对象 |

[结构与音频类型](https://developer.kookapp.cn/doc/event/event-introduction) · [消息及道具类型](https://developer.kookapp.cn/doc/event/message)

### 频道

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/channel)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `added_reaction` | 已接入 | `notice:kook:added_reaction` |
| `deleted_reaction` | 已接入 | `notice:kook:deleted_reaction` |
| `updated_message` | 已接入 | `notice:kook:updated_message` |
| `deleted_message` | 已接入 | `notice:kook:deleted_message` |
| `added_channel` | 已接入 | `notice:kook:added_channel` |
| `updated_channel` | 已接入 | `notice:kook:updated_channel` |
| `deleted_channel` | 已接入 | `notice:kook:deleted_channel` |
| `pinned_message` | 已接入 | `notice:kook:pinned_message` |
| `unpinned_message` | 已接入 | `notice:kook:unpinned_message` |

### 私聊

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/direct-message)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `updated_private_message` | 已接入 | `notice:kook:updated_private_message` |
| `deleted_private_message` | 已接入 | `notice:kook:deleted_private_message` |
| `private_added_reaction` | 已接入 | `notice:kook:private_added_reaction` |
| `private_deleted_reaction` | 已接入 | `notice:kook:private_deleted_reaction` |

### 服务器成员

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/guild-member)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `joined_guild` | 已接入 | `notice:kook:joined_guild`，【事件内容@服务器事件】 |
| `exited_guild` | 已接入 | `notice:kook:exited_guild`，【事件内容@服务器事件】 |
| `updated_guild_member` | 已接入 | `notice:kook:updated_guild_member`，【事件内容@服务器事件】 |
| `guild_member_online` | 已接入 | `notice:kook:guild_member_online`，【事件内容@服务器事件】 |
| `guild_member_offline` | 已接入 | `notice:kook:guild_member_offline`，【事件内容@服务器事件】 |

### 身份组

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/guild-role)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `added_role` | 已接入 | `notice:kook:added_role` |
| `deleted_role` | 已接入 | `notice:kook:deleted_role` |
| `updated_role` | 已接入 | `notice:kook:updated_role` |

### 服务器

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/guild)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `updated_guild` | 已接入 | `notice:kook:updated_guild`，【事件内容@服务器事件】 |
| `deleted_guild` | 已接入 | `notice:kook:deleted_guild`，【事件内容@服务器事件】 |
| `added_block_list` | 已接入 | `notice:kook:added_block_list`，【事件内容@服务器事件】 |
| `deleted_block_list` | 已接入 | `notice:kook:deleted_block_list`，【事件内容@服务器事件】 |
| `added_emoji` | 已接入 | `notice:kook:added_emoji`，【事件内容@服务器事件】 |
| `removed_emoji` | 已接入 | `notice:kook:removed_emoji`，【事件内容@服务器事件】 |
| `updated_emoji` | 已接入 | `notice:kook:updated_emoji`，【事件内容@服务器事件】 |

### 用户

[KOOK 官方事件](https://developer.kookapp.cn/doc/event/user)

| 官方事件 | 当前状态 | 脚本触发 |
| --- | --- | --- |
| `joined_channel` | 已接入 | `notice:kook:joined_channel` |
| `exited_channel` | 已接入 | `notice:kook:exited_channel` |
| `user_updated` | 部分 | `notice:kook:user_updated`，通过原生事件读取字段 |
| `self_joined_guild` | 已接入 | `notice:kook:self_joined_guild`，【事件内容@服务器事件】 |
| `self_exited_guild` | 已接入 | `notice:kook:self_exited_guild`，【事件内容@服务器事件】 |
| `message_btn_click` | 已接入 | `notice:kook:message_btn_click` |

## Telegram HTTP 方法

共 193 项。以官方 HTML 方法标题为准，其中富文本的两项也纳入清单。

### 收取更新及 webhook

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`getUpdates`](https://core.telegram.org/bots/api#getupdates) | 配置／运行时 | 适配器长轮询，订阅类型由管理台配置 |
| [`setWebhook`](https://core.telegram.org/bots/api#setwebhook) | 未接入 | 当前为轮询适配器，缺 webhook 管理和收件入口 |
| [`deleteWebhook`](https://core.telegram.org/bots/api#deletewebhook) | 未接入 | 当前为轮询适配器，缺 webhook 管理和收件入口 |
| [`getWebhookInfo`](https://core.telegram.org/bots/api#getwebhookinfo) | 未接入 | 当前为轮询适配器，缺 webhook 管理和收件入口 |

### 通用方法

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`getMe`](https://core.telegram.org/bots/api#getme) | 配置／运行时 | 适配器登录时调用 |
| [`logOut`](https://core.telegram.org/bots/api#logout) | 未接入 | 缺直接填值的命令 |
| [`close`](https://core.telegram.org/bots/api#close) | 未接入 | 缺直接填值的命令 |
| [`sendMessage`](https://core.telegram.org/bots/api#sendmessage) | 部分 | 【输出流】支持静默、内容保护、私聊效果、建议帖、临时消息和高级引用。富文本另列，付费广播待接入 |
| [`forwardMessage`](https://core.telegram.org/bots/api#forwardmessage) | 已接入 | 【转发消息】支持视频起点、平台来源、话题和公开发送选项 |
| [`forwardMessages`](https://core.telegram.org/bots/api#forwardmessages) | 部分 | 【转发消息】支持常用转发、复制与话题，未覆盖所有附加选项 |
| [`copyMessage`](https://core.telegram.org/bots/api#copymessage) | 部分 | 【转发消息】支持视频起点、新说明、格式、说明位置、键盘和引用，独立说明实体数组与付费广播待接入 |
| [`copyMessages`](https://core.telegram.org/bots/api#copymessages) | 已接入 | 【转发消息】支持批量移除说明、平台来源、话题和发送选项 |
| [`sendPhoto`](https://core.telegram.org/bots/api#sendphoto) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`sendLivePhoto`](https://core.telegram.org/bots/api#sendlivephoto) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`sendAudio`](https://core.telegram.org/bots/api#sendaudio) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`sendDocument`](https://core.telegram.org/bots/api#senddocument) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`sendVideo`](https://core.telegram.org/bots/api#sendvideo) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`sendAnimation`](https://core.telegram.org/bots/api#sendanimation) | 部分 | 【动画】配合【输出流】，支持动画、说明、缩略图和防剧透，缺实体数组、付费广播等选项 |
| [`sendVoice`](https://core.telegram.org/bots/api#sendvoice) | 部分 | 【语音】便笺模式配合【输出流】，缺实体数组、付费广播等选项 |
| [`sendVideoNote`](https://core.telegram.org/bots/api#sendvideonote) | 部分 | 【视频】便笺模式配合【输出流】，缺付费广播等选项 |
| [`sendWidget`](https://core.telegram.org/bots/api#sendwidget) | 未接入 | 缺直接填值的命令 |
| [`sendSharedWidget`](https://core.telegram.org/bots/api#sendsharedwidget) | 未接入 | 缺直接填值的命令 |
| [`sendPaidMedia`](https://core.telegram.org/bots/api#sendpaidmedia) | 未接入 | 缺直接填值的命令 |
| [`sendMediaGroup`](https://core.telegram.org/bots/api#sendmediagroup) | 部分 | 【媒体组】配合【输出流】支持静默、内容保护、私聊效果和高级引用，付费广播待接入 |
| [`sendLocation`](https://core.telegram.org/bots/api#sendlocation) | 未接入 | 缺直接填值的命令 |
| [`sendVenue`](https://core.telegram.org/bots/api#sendvenue) | 未接入 | 缺直接填值的命令 |
| [`sendContact`](https://core.telegram.org/bots/api#sendcontact) | 未接入 | 缺直接填值的命令 |
| [`sendPoll`](https://core.telegram.org/bots/api#sendpoll) | 部分 | 【Telegram投票】支持普通与测验投票，缺媒体和多项高级设置 |
| [`sendChecklist`](https://core.telegram.org/bots/api#sendchecklist) | 部分 | 【Telegram任务清单】【输出流】支持任务、授权开关、静默、保护和消息效果 |
| [`sendDice`](https://core.telegram.org/bots/api#senddice) | 未接入 | 缺直接填值的命令 |
| [`sendMessageDraft`](https://core.telegram.org/bots/api#sendmessagedraft) | 未接入 | 缺直接填值的命令 |
| [`sendChatAction`](https://core.telegram.org/bots/api#sendchataction) | 已接入 | 【Telegram聊天动作】 |
| [`setMessageReaction`](https://core.telegram.org/bots/api#setmessagereaction) | 已接入 | 【表情回应】 |
| [`getUserProfilePhotos`](https://core.telegram.org/bots/api#getuserprofilephotos) | 已接入 | 【取头像@用户ID@文件ID列表】 |
| [`getUserProfileAudios`](https://core.telegram.org/bots/api#getuserprofileaudios) | 未接入 | 缺直接填值的命令 |
| [`setUserEmojiStatus`](https://core.telegram.org/bots/api#setuseremojistatus) | 未接入 | 缺直接填值的命令 |
| [`getFile`](https://core.telegram.org/bots/api#getfile) | 已接入 | 【Telegram下载文件】按文件 ID 下载，有平台文件大小限制 |
| [`banChatMember`](https://core.telegram.org/bots/api#banchatmember) | 部分 | 【群管理@封禁】支持基本封禁，缺定时封禁等选项 |
| [`unbanChatMember`](https://core.telegram.org/bots/api#unbanchatmember) | 已接入 | 【群管理@解封】针对已封禁成员 |
| [`restrictChatMember`](https://core.telegram.org/bots/api#restrictchatmember) | 已接入 | 【Telegram群权限】【禁言】 |
| [`promoteChatMember`](https://core.telegram.org/bots/api#promotechatmember) | 已接入 | 【Telegram群权限】【禁言】 |
| [`setChatAdministratorCustomTitle`](https://core.telegram.org/bots/api#setchatadministratorcustomtitle) | 未接入 | 缺直接填值的命令 |
| [`setChatMemberTag`](https://core.telegram.org/bots/api#setchatmembertag) | 未接入 | 缺直接填值的命令 |
| [`banChatSenderChat`](https://core.telegram.org/bots/api#banchatsenderchat) | 未接入 | 缺直接填值的命令 |
| [`unbanChatSenderChat`](https://core.telegram.org/bots/api#unbanchatsenderchat) | 未接入 | 缺直接填值的命令 |
| [`setChatPermissions`](https://core.telegram.org/bots/api#setchatpermissions) | 已接入 | 【Telegram群权限】【禁言】 |
| [`exportChatInviteLink`](https://core.telegram.org/bots/api#exportchatinvitelink) | 已接入 | 【邀请链接】 |
| [`createChatInviteLink`](https://core.telegram.org/bots/api#createchatinvitelink) | 已接入 | 【邀请链接】 |
| [`editChatInviteLink`](https://core.telegram.org/bots/api#editchatinvitelink) | 已接入 | 【邀请链接】 |
| [`createChatSubscriptionInviteLink`](https://core.telegram.org/bots/api#createchatsubscriptioninvitelink) | 已接入 | 【邀请链接】 |
| [`editChatSubscriptionInviteLink`](https://core.telegram.org/bots/api#editchatsubscriptioninvitelink) | 已接入 | 【邀请链接】 |
| [`revokeChatInviteLink`](https://core.telegram.org/bots/api#revokechatinvitelink) | 已接入 | 【邀请链接】 |
| [`approveChatJoinRequest`](https://core.telegram.org/bots/api#approvechatjoinrequest) | 已接入 | 【同意】【拒绝】 |
| [`declineChatJoinRequest`](https://core.telegram.org/bots/api#declinechatjoinrequest) | 已接入 | 【同意】【拒绝】 |
| [`answerChatJoinRequestQuery`](https://core.telegram.org/bots/api#answerchatjoinrequestquery) | 已接入 | 【同意】【拒绝】 |
| [`sendChatJoinRequestWebApp`](https://core.telegram.org/bots/api#sendchatjoinrequestwebapp) | 未接入 | 缺直接填值的命令 |
| [`setChatPhoto`](https://core.telegram.org/bots/api#setchatphoto) | 已接入 | 【Telegram群头像】 |
| [`deleteChatPhoto`](https://core.telegram.org/bots/api#deletechatphoto) | 已接入 | 【群管理】 |
| [`setChatTitle`](https://core.telegram.org/bots/api#setchattitle) | 已接入 | 【群管理】 |
| [`setChatDescription`](https://core.telegram.org/bots/api#setchatdescription) | 已接入 | 【群管理】 |
| [`pinChatMessage`](https://core.telegram.org/bots/api#pinchatmessage) | 已接入 | 【置顶消息】支持普通及商务消息 |
| [`unpinChatMessage`](https://core.telegram.org/bots/api#unpinchatmessage) | 已接入 | 【置顶消息】支持普通及商务消息 |
| [`unpinAllChatMessages`](https://core.telegram.org/bots/api#unpinallchatmessages) | 未接入 | 缺直接填值的命令 |
| [`leaveChat`](https://core.telegram.org/bots/api#leavechat) | 已接入 | 【群管理】 |
| [`getChat`](https://core.telegram.org/bots/api#getchat) | 已接入 | 【群信息】【群成员】【权限】与【BOT权限】 |
| [`getChatAdministrators`](https://core.telegram.org/bots/api#getchatadministrators) | 已接入 | 【群信息】【群成员】【权限】与【BOT权限】 |
| [`getChatMemberCount`](https://core.telegram.org/bots/api#getchatmembercount) | 已接入 | 【群信息】【群成员】【权限】与【BOT权限】 |
| [`getChatMember`](https://core.telegram.org/bots/api#getchatmember) | 已接入 | 【群信息】【群成员】【权限】与【BOT权限】 |
| [`getChatShareThemeBackdrops`](https://core.telegram.org/bots/api#getchatsharethemebackdrops) | 未接入 | 缺直接填值的命令 |
| [`getUserPersonalChatMessages`](https://core.telegram.org/bots/api#getuserpersonalchatmessages) | 未接入 | 缺直接填值的命令 |
| [`setChatStickerSet`](https://core.telegram.org/bots/api#setchatstickerset) | 未接入 | 缺直接填值的命令 |
| [`deleteChatStickerSet`](https://core.telegram.org/bots/api#deletechatstickerset) | 未接入 | 缺直接填值的命令 |
| [`getForumTopicIconStickers`](https://core.telegram.org/bots/api#getforumtopiciconstickers) | 已接入 | 【Telegram话题】 |
| [`createForumTopic`](https://core.telegram.org/bots/api#createforumtopic) | 已接入 | 【Telegram话题】 |
| [`editForumTopic`](https://core.telegram.org/bots/api#editforumtopic) | 已接入 | 【Telegram话题】 |
| [`closeForumTopic`](https://core.telegram.org/bots/api#closeforumtopic) | 已接入 | 【Telegram话题】 |
| [`reopenForumTopic`](https://core.telegram.org/bots/api#reopenforumtopic) | 已接入 | 【Telegram话题】 |
| [`deleteForumTopic`](https://core.telegram.org/bots/api#deleteforumtopic) | 已接入 | 【Telegram话题】 |
| [`unpinAllForumTopicMessages`](https://core.telegram.org/bots/api#unpinallforumtopicmessages) | 已接入 | 【Telegram话题】 |
| [`editGeneralForumTopic`](https://core.telegram.org/bots/api#editgeneralforumtopic) | 已接入 | 【Telegram话题】 |
| [`closeGeneralForumTopic`](https://core.telegram.org/bots/api#closegeneralforumtopic) | 已接入 | 【Telegram话题】 |
| [`reopenGeneralForumTopic`](https://core.telegram.org/bots/api#reopengeneralforumtopic) | 已接入 | 【Telegram话题】 |
| [`hideGeneralForumTopic`](https://core.telegram.org/bots/api#hidegeneralforumtopic) | 已接入 | 【Telegram话题】 |
| [`unhideGeneralForumTopic`](https://core.telegram.org/bots/api#unhidegeneralforumtopic) | 已接入 | 【Telegram话题】 |
| [`unpinAllGeneralForumTopicMessages`](https://core.telegram.org/bots/api#unpinallgeneralforumtopicmessages) | 已接入 | 【Telegram话题】 |
| [`answerCallbackQuery`](https://core.telegram.org/bots/api#answercallbackquery) | 部分 | 【Telegram应答按钮】支持提示和弹窗，缺回调跳转或缓存 |
| [`answerWidgetQuery`](https://core.telegram.org/bots/api#answerwidgetquery) | 未接入 | 缺直接填值的命令 |
| [`answerGuestQuery`](https://core.telegram.org/bots/api#answerguestquery) | 未接入 | 缺直接填值的命令 |
| [`getUserChatBoosts`](https://core.telegram.org/bots/api#getuserchatboosts) | 未接入 | 缺直接填值的命令 |
| [`getBusinessConnection`](https://core.telegram.org/bots/api#getbusinessconnection) | 已接入 | 【Telegram商务账户】 |
| [`getManagedBotToken`](https://core.telegram.org/bots/api#getmanagedbottoken) | 未接入 | 缺直接填值的命令 |
| [`replaceManagedBotToken`](https://core.telegram.org/bots/api#replacemanagedbottoken) | 未接入 | 缺直接填值的命令 |
| [`getManagedBotAccessSettings`](https://core.telegram.org/bots/api#getmanagedbotaccesssettings) | 未接入 | 缺直接填值的命令 |
| [`setManagedBotAccessSettings`](https://core.telegram.org/bots/api#setmanagedbotaccesssettings) | 未接入 | 缺直接填值的命令 |
| [`setMyCommands`](https://core.telegram.org/bots/api#setmycommands) | 已接入 | 【Telegram命令菜单】支持临时开关 |
| [`deleteMyCommands`](https://core.telegram.org/bots/api#deletemycommands) | 已接入 | 【Telegram命令菜单】 |
| [`getMyCommands`](https://core.telegram.org/bots/api#getmycommands) | 已接入 | 【Telegram命令菜单@查询详情】返回临时开关 |
| [`setMyName`](https://core.telegram.org/bots/api#setmyname) | 已接入 | 【Telegram机器人资料】 |
| [`getMyName`](https://core.telegram.org/bots/api#getmyname) | 已接入 | 【Telegram机器人资料】 |
| [`setMyDescription`](https://core.telegram.org/bots/api#setmydescription) | 已接入 | 【Telegram机器人资料】 |
| [`getMyDescription`](https://core.telegram.org/bots/api#getmydescription) | 已接入 | 【Telegram机器人资料】 |
| [`setMyShortDescription`](https://core.telegram.org/bots/api#setmyshortdescription) | 已接入 | 【Telegram机器人资料】 |
| [`getMyShortDescription`](https://core.telegram.org/bots/api#getmyshortdescription) | 已接入 | 【Telegram机器人资料】 |
| [`setMyProfilePhoto`](https://core.telegram.org/bots/api#setmyprofilephoto) | 未接入 | 缺直接填值的命令 |
| [`removeMyProfilePhoto`](https://core.telegram.org/bots/api#removemyprofilephoto) | 未接入 | 缺直接填值的命令 |
| [`setChatMenuButton`](https://core.telegram.org/bots/api#setchatmenubutton) | 已接入 | 【Telegram菜单按钮】 |
| [`getChatMenuButton`](https://core.telegram.org/bots/api#getchatmenubutton) | 已接入 | 【Telegram菜单按钮】 |
| [`setMyDefaultAdministratorRights`](https://core.telegram.org/bots/api#setmydefaultadministratorrights) | 未接入 | 缺直接填值的命令 |
| [`getMyDefaultAdministratorRights`](https://core.telegram.org/bots/api#getmydefaultadministratorrights) | 未接入 | 缺直接填值的命令 |
| [`getAvailableGifts`](https://core.telegram.org/bots/api#getavailablegifts) | 未接入 | 缺直接填值的命令 |
| [`sendGift`](https://core.telegram.org/bots/api#sendgift) | 未接入 | 缺直接填值的命令 |
| [`giftPremiumSubscription`](https://core.telegram.org/bots/api#giftpremiumsubscription) | 未接入 | 缺直接填值的命令 |
| [`verifyUser`](https://core.telegram.org/bots/api#verifyuser) | 未接入 | 缺直接填值的命令 |
| [`verifyChat`](https://core.telegram.org/bots/api#verifychat) | 未接入 | 缺直接填值的命令 |
| [`removeUserVerification`](https://core.telegram.org/bots/api#removeuserverification) | 未接入 | 缺直接填值的命令 |
| [`removeChatVerification`](https://core.telegram.org/bots/api#removechatverification) | 未接入 | 缺直接填值的命令 |
| [`readBusinessMessage`](https://core.telegram.org/bots/api#readbusinessmessage) | 已接入 | 【Telegram商务已读】 |
| [`deleteBusinessMessages`](https://core.telegram.org/bots/api#deletebusinessmessages) | 已接入 | 【撤回】 |
| [`setBusinessAccountName`](https://core.telegram.org/bots/api#setbusinessaccountname) | 已接入 | 【Telegram商务账户】 |
| [`setBusinessAccountUsername`](https://core.telegram.org/bots/api#setbusinessaccountusername) | 已接入 | 【Telegram商务账户】 |
| [`setBusinessAccountBio`](https://core.telegram.org/bots/api#setbusinessaccountbio) | 已接入 | 【Telegram商务账户】 |
| [`setBusinessAccountProfilePhoto`](https://core.telegram.org/bots/api#setbusinessaccountprofilephoto) | 已接入 | 【Telegram商务账户】 |
| [`removeBusinessAccountProfilePhoto`](https://core.telegram.org/bots/api#removebusinessaccountprofilephoto) | 已接入 | 【Telegram商务账户】 |
| [`setBusinessAccountGiftSettings`](https://core.telegram.org/bots/api#setbusinessaccountgiftsettings) | 已接入 | 【Telegram商务礼物】 |
| [`getBusinessAccountStarBalance`](https://core.telegram.org/bots/api#getbusinessaccountstarbalance) | 已接入 | 【Telegram商务礼物】 |
| [`transferBusinessAccountStars`](https://core.telegram.org/bots/api#transferbusinessaccountstars) | 已接入 | 【Telegram商务礼物】 |
| [`getBusinessAccountGifts`](https://core.telegram.org/bots/api#getbusinessaccountgifts) | 已接入 | 【Telegram商务礼物】 |
| [`getUserGifts`](https://core.telegram.org/bots/api#getusergifts) | 未接入 | 缺直接填值的命令 |
| [`getChatGifts`](https://core.telegram.org/bots/api#getchatgifts) | 未接入 | 缺直接填值的命令 |
| [`getUniqueGift`](https://core.telegram.org/bots/api#getuniquegift) | 未接入 | 缺直接填值的命令 |
| [`convertGiftToStars`](https://core.telegram.org/bots/api#convertgifttostars) | 已接入 | 【Telegram商务礼物】 |
| [`upgradeGift`](https://core.telegram.org/bots/api#upgradegift) | 已接入 | 【Telegram商务礼物】 |
| [`transferGift`](https://core.telegram.org/bots/api#transfergift) | 已接入 | 【Telegram商务礼物】 |
| [`deleteBusinessAccountGift`](https://core.telegram.org/bots/api#deletebusinessaccountgift) | 已接入 | 【Telegram商务礼物】 |
| [`postStory`](https://core.telegram.org/bots/api#poststory) | 部分 | 【Telegram商务动态】支持图片、视频与文字，缺可点区域等选项 |
| [`repostStory`](https://core.telegram.org/bots/api#repoststory) | 已接入 | 【Telegram商务动态】转发、删除 |
| [`editStory`](https://core.telegram.org/bots/api#editstory) | 部分 | 【Telegram商务动态】支持图片、视频与文字，缺可点区域等选项 |
| [`deleteStory`](https://core.telegram.org/bots/api#deletestory) | 已接入 | 【Telegram商务动态】转发、删除 |
| [`answerWebAppQuery`](https://core.telegram.org/bots/api#answerwebappquery) | 部分 | 【Telegram小程序应答】支持文章与富文本结果 |
| [`savePreparedInlineMessage`](https://core.telegram.org/bots/api#savepreparedinlinemessage) | 部分 | 【Telegram准备行内消息】支持文章、富文本及既有媒体结果类型，支持四类目标范围 |
| [`savePreparedKeyboardButton`](https://core.telegram.org/bots/api#savepreparedkeyboardbutton) | 未接入 | 缺直接填值的命令 |

### 编辑与删除消息

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`editMessageText`](https://core.telegram.org/bots/api#editmessagetext) | 部分 | 【编辑消息】支持常用内容及行内按钮，缺部分高级参数 |
| [`editMessageCaption`](https://core.telegram.org/bots/api#editmessagecaption) | 部分 | 【编辑消息】支持常用内容及行内按钮，缺部分高级参数 |
| [`editMessageMedia`](https://core.telegram.org/bots/api#editmessagemedia) | 部分 | 【编辑消息】按媒体类型直接填写时长、尺寸、缩略图、防剧透、视频起点及封面等，独立说明实体数组待接入 |
| [`editMessageLiveLocation`](https://core.telegram.org/bots/api#editmessagelivelocation) | 未接入 | 缺直接填值的命令 |
| [`stopMessageLiveLocation`](https://core.telegram.org/bots/api#stopmessagelivelocation) | 未接入 | 缺直接填值的命令 |
| [`editMessageChecklist`](https://core.telegram.org/bots/api#editmessagechecklist) | 部分 | 【编辑消息】支持常用内容及行内按钮，缺部分高级参数 |
| [`editMessageWidgetData`](https://core.telegram.org/bots/api#editmessagewidgetdata) | 未接入 | 缺直接填值的命令 |
| [`editSharedWidgetData`](https://core.telegram.org/bots/api#editsharedwidgetdata) | 未接入 | 缺直接填值的命令 |
| [`editMessageReplyMarkup`](https://core.telegram.org/bots/api#editmessagereplymarkup) | 部分 | 【编辑消息】支持常用内容及行内按钮，缺部分高级参数 |
| [`stopPoll`](https://core.telegram.org/bots/api#stoppoll) | 部分 | 【Telegram结束投票】关闭投票，未提供替换键盘选项 |
| [`editEphemeralMessageText`](https://core.telegram.org/bots/api#editephemeralmessagetext) | 部分 | 【编辑消息】支持临时文本、富文本及预览选项，实体数组待接入 |
| [`editEphemeralMessageMedia`](https://core.telegram.org/bots/api#editephemeralmessagemedia) | 已接入 | 【编辑消息】的临时媒体模式 |
| [`editEphemeralMessageCaption`](https://core.telegram.org/bots/api#editephemeralmessagecaption) | 部分 | 【编辑消息】的临时说明模式，实体与说明位置待接入 |
| [`editEphemeralMessageReplyMarkup`](https://core.telegram.org/bots/api#editephemeralmessagereplymarkup) | 已接入 | 【编辑消息】的临时键盘模式 |
| [`approveSuggestedPost`](https://core.telegram.org/bots/api#approvesuggestedpost) | 已接入 | 【同意】的建议帖模式 |
| [`declineSuggestedPost`](https://core.telegram.org/bots/api#declinesuggestedpost) | 已接入 | 【拒绝】的建议帖模式 |
| [`deleteMessage`](https://core.telegram.org/bots/api#deletemessage) | 已接入 | 【撤回】 |
| [`deleteMessages`](https://core.telegram.org/bots/api#deletemessages) | 已接入 | 【撤回】 |
| [`deleteEphemeralMessage`](https://core.telegram.org/bots/api#deleteephemeralmessage) | 已接入 | 【撤回】的临时模式 |
| [`deleteMessageReaction`](https://core.telegram.org/bots/api#deletemessagereaction) | 已接入 | 【表情回应】 |
| [`deleteAllMessageReactions`](https://core.telegram.org/bots/api#deleteallmessagereactions) | 未接入 | 缺直接填值的命令 |

### 贴纸

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`sendSticker`](https://core.telegram.org/bots/api#sendsticker) | 部分 | 【输出流】【媒体组】【贴纸】或【Telegram文件】覆盖常见发送，缺该方法的部分选填参数 |
| [`getStickerSet`](https://core.telegram.org/bots/api#getstickerset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`getCustomEmojiStickers`](https://core.telegram.org/bots/api#getcustomemojistickers) | 已接入 | 【Telegram自定义表情】 |
| [`uploadStickerFile`](https://core.telegram.org/bots/api#uploadstickerfile) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`createNewStickerSet`](https://core.telegram.org/bots/api#createnewstickerset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`addStickerToSet`](https://core.telegram.org/bots/api#addstickertoset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`setStickerPositionInSet`](https://core.telegram.org/bots/api#setstickerpositioninset) | 未接入 | 缺直接填值的命令 |
| [`deleteStickerFromSet`](https://core.telegram.org/bots/api#deletestickerfromset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`replaceStickerInSet`](https://core.telegram.org/bots/api#replacestickerinset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`setStickerEmojiList`](https://core.telegram.org/bots/api#setstickeremojilist) | 未接入 | 缺直接填值的命令 |
| [`setStickerKeywords`](https://core.telegram.org/bots/api#setstickerkeywords) | 未接入 | 缺直接填值的命令 |
| [`setStickerMaskPosition`](https://core.telegram.org/bots/api#setstickermaskposition) | 未接入 | 缺直接填值的命令 |
| [`setStickerSetTitle`](https://core.telegram.org/bots/api#setstickersettitle) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |
| [`setStickerSetThumbnail`](https://core.telegram.org/bots/api#setstickersetthumbnail) | 未接入 | 缺直接填值的命令 |
| [`setCustomEmojiStickerSetThumbnail`](https://core.telegram.org/bots/api#setcustomemojistickersetthumbnail) | 未接入 | 缺直接填值的命令 |
| [`deleteStickerSet`](https://core.telegram.org/bots/api#deletestickerset) | 已接入 | 【Telegram贴纸包】【Telegram上传贴纸】 |

### 富文本消息

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`sendRichMessage`](https://core.telegram.org/bots/api#sendrichmessage) | 部分 | 【Telegram富文本】支持 Markdown、HTML、官方块类型及资源表。付费广播和部分块内附加属性待接入 |
| [`sendRichMessageDraft`](https://core.telegram.org/bots/api#sendrichmessagedraft) | 已接入 | 【Telegram富文本草稿】支持重用草稿 ID、私聊话题、停止与保留 |

### 行内模式

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`answerInlineQuery`](https://core.telegram.org/bots/api#answerinlinequery) | 部分 | 【Telegram应答查询】支持富文本、文章、部分媒体行内结果和行内键盘，其他结果种类待接入 |

### 支付

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`sendInvoice`](https://core.telegram.org/bots/api#sendinvoice) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`createInvoiceLink`](https://core.telegram.org/bots/api#createinvoicelink) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`answerShippingQuery`](https://core.telegram.org/bots/api#answershippingquery) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`answerPreCheckoutQuery`](https://core.telegram.org/bots/api#answerprecheckoutquery) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`getMyStarBalance`](https://core.telegram.org/bots/api#getmystarbalance) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`getStarTransactions`](https://core.telegram.org/bots/api#getstartransactions) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`refundStarPayment`](https://core.telegram.org/bots/api#refundstarpayment) | 未接入 | 缺直接填值的命令，本轮优先补齐 |
| [`editUserStarSubscription`](https://core.telegram.org/bots/api#edituserstarsubscription) | 未接入 | 缺直接填值的命令，本轮优先补齐 |

### Telegram Passport

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`setPassportDataErrors`](https://core.telegram.org/bots/api#setpassportdataerrors) | 未接入 | 缺直接填值的命令 |

### 游戏

| 官方方法 | 当前状态 | 入口或缺口 |
| --- | --- | --- |
| [`sendGame`](https://core.telegram.org/bots/api#sendgame) | 未接入 | 缺直接填值的命令 |
| [`setGameScore`](https://core.telegram.org/bots/api#setgamescore) | 未接入 | 缺直接填值的命令 |
| [`getGameHighScores`](https://core.telegram.org/bots/api#getgamehighscores) | 未接入 | 缺直接填值的命令 |

## Telegram 事件

共 29 项官方 Update 变体。识别和去重用的 `update_id` 不作为事件计数。

| 官方更新 | 当前状态 | 脚本触发或缺口 |
| --- | --- | --- |
| `message` | 已接入 | 普通私聊或群消息，还可读【事件内容】 |
| `edited_message` | 已接入 | 编辑事件与消息 ID |
| `channel_post` | 已接入 | 频道事件与消息 ID |
| `edited_channel_post` | 已接入 | 频道编辑事件与消息 ID |
| `business_connection` | 已接入 | 【事件内容@商务连接】和商务连接 ID |
| `business_message` | 已接入 | 商务会话、消息、清单等事件字段 |
| `edited_business_message` | 已接入 | 商务会话、消息、清单等事件字段 |
| `deleted_business_messages` | 已接入 | 商务会话与两种删除消息 ID |
| `guest_message` | 部分 | `notice:telegram:guest_message`，通过原生事件访问字段 |
| `message_reaction` | 已接入 | 【事件内容@新回应】和【事件内容@旧回应】 |
| `message_reaction_count` | 部分 | `notice:telegram:message_reaction_count`，通过原生事件访问字段 |
| `inline_query` | 已接入 | 【事件内容@查询内容】【事件内容@查询偏移】 |
| `chosen_inline_result` | 已接入 | `notice:telegram:chosen_inline_result`，【事件内容@行内结果】和【事件内容@行内消息ID】 |
| `callback_query` | 已接入 | 【事件内容@按钮值】【Telegram应答按钮】 |
| `widget_query` | 部分 | `notice:telegram:widget_query`，通过原生事件访问字段 |
| `shared_widget_query` | 部分 | `notice:telegram:shared_widget_query`，通过原生事件访问字段 |
| `shipping_query` | 部分 | `notice:telegram:shipping_query`，通过原生事件访问字段，本轮优先补齐 |
| `pre_checkout_query` | 部分 | `notice:telegram:pre_checkout_query`，通过原生事件访问字段，本轮优先补齐 |
| `purchased_paid_media` | 部分 | `notice:telegram:purchased_paid_media`，通过原生事件访问字段，本轮优先补齐 |
| `poll` | 已接入 | 【事件内容@投票人数】 |
| `poll_answer` | 已接入 | 【事件内容@投票选项】 |
| `my_chat_member` | 部分 | `notice:telegram:my_chat_member`，通过原生事件访问字段 |
| `chat_member` | 部分 | `notice:telegram:chat_member`，通过原生事件访问字段 |
| `chat_join_request` | 已接入 | 【同意】【拒绝】 |
| `chat_boost` | 部分 | `notice:telegram:chat_boost`，通过原生事件访问字段 |
| `removed_chat_boost` | 部分 | `notice:telegram:removed_chat_boost`，通过原生事件访问字段 |
| `managed_bot` | 部分 | `notice:telegram:managed_bot`，通过原生事件访问字段 |
| `subscription` | 部分 | `notice:telegram:subscription`，通过原生事件访问字段 |
| `stopped_message_generation` | 已接入 | `notice:telegram:stopped_message_generation` 与草稿中文选择项 |

[官方 Update 对象](https://core.telegram.org/bots/api#update)

### 对应消息内容与服务事件

这些字段由消息类 Update 承载。本表逐项列出消息内容、服务事件及具有独立脚本语义的分组和转发标记。

| 官方消息字段 | 当前状态 | 脚本触发或缺口 |
| --- | --- | --- |
| `text` | 已接入 | 普通文字消息，可用【事件内容@消息文字】 |
| `forward_origin` | 已接入 | 转发通知及【事件内容@转发来源类型】 |
| `media_group_id` | 已接入 | 媒体组通知 |
| `suggested_post_info` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `rich_message` | 部分 | 独立接收及编辑通知，【事件内容@富文本】可读正文、方向和中文内容块，部分块内附加属性待接入 |
| `animation` | 已接入 | 动画与动画编辑通知，【事件内容@动画】 |
| `audio` | 部分 | 原生事件可读媒体字段，媒体组时另有媒体通知 |
| `document` | 已接入 | 【事件内容@文件ID】【事件内容@文件名】 |
| `live_photo` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `paid_media` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `photo` | 已接入 | 普通图片消息，媒体组时另有媒体通知 |
| `sticker` | 已接入 | 贴纸通知及【事件内容@贴纸文件ID】 |
| `story` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video_note` | 已接入 | 视频便笺与视频便笺编辑通知，【事件内容@视频便笺】 |
| `voice` | 已接入 | 语音便笺与语音便笺编辑通知，【事件内容@语音便笺】 |
| `widget` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `checklist` | 已接入 | 【事件内容@清单标题】【事件内容@清单任务】 |
| `contact` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `dice` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `game` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `poll` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `venue` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `location` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `new_chat_members` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `left_chat_member` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `chat_owner_left` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `chat_owner_changed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `new_chat_title` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `new_chat_photo` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `delete_chat_photo` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `group_chat_created` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `supergroup_chat_created` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `channel_chat_created` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `message_auto_delete_timer_changed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `migrate_to_chat_id` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `migrate_from_chat_id` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `pinned_message` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `invoice` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `successful_payment` | 未接入 | 缺支付成功专用事件，本轮优先补齐 |
| `refunded_payment` | 未接入 | 缺已退款专用事件，本轮优先补齐 |
| `users_shared` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `chat_shared` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `gift` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `unique_gift` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `gift_upgrade_sent` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `connected_website` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `write_access_allowed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `passport_data` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `proximity_alert_triggered` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `boost_added` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `chat_background_set` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `checklist_tasks_done` | 已接入 | 商务消息中可直接读完成和未完成任务 ID |
| `checklist_tasks_added` | 已接入 | 商务消息中可直接读新增任务 |
| `community_chat_added` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `community_chat_joined` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `community_chat_removed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `direct_message_price_changed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `forum_topic_created` | 已接入 | `notice:telegram:forum_topic_created` |
| `forum_topic_edited` | 已接入 | `notice:telegram:forum_topic_edited` |
| `forum_topic_closed` | 已接入 | `notice:telegram:forum_topic_closed` |
| `forum_topic_reopened` | 已接入 | `notice:telegram:forum_topic_reopened` |
| `general_forum_topic_hidden` | 已接入 | `notice:telegram:general_forum_topic_hidden` |
| `general_forum_topic_unhidden` | 已接入 | `notice:telegram:general_forum_topic_unhidden` |
| `giveaway_created` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `giveaway` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `giveaway_winners` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `giveaway_completed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `managed_bot_created` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `paid_message_price_changed` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `paid_messages_waived` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `poll_option_added` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `poll_option_deleted` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `suggested_post_approved` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `suggested_post_approval_failed` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `suggested_post_declined` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `suggested_post_paid` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `suggested_post_refunded` | 已接入 | 专用通知和【事件内容@建议帖】 |
| `ton_wallet_transfer` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video_chat_scheduled` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video_chat_started` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video_chat_ended` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `video_chat_participants_invited` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |
| `web_app_data` | 部分 | 随原生消息提供，缺专用事件或中文位置入口 |

[官方 Message 对象](https://core.telegram.org/bots/api#message) · [Telegram 支付更新](https://core.telegram.org/bots/payments-stars)

## 本轮之后的缺口

- KOOK 上线、下线接口适用于 Webhook，当前适配器使用 WebSocket。继续核对清单里标记为部分接入的频道、服务器与消息参数。
- Telegram 待补富文本块内剩余附加属性与更多行内结果种类、独立说明实体数组、付费广播，以及组件、贴纸集细项、游戏、受管机器人与 Passport。

## RedLang 参数检查

| 入口 | 检查结果 |
| --- | --- |
| 菜单、键盘、卡片、投票、邀请链接、查询和发送选项 | 常用流程已有位置参数和示例。检查脚本语法的测试会拒绝在用法代码块里使用通用 API 命令或【接口值】。 |
| 【编辑消息】、【转发消息】 | 常用流程直接填值，媒体选项、上传、行内目标、单条复制说明和批量去说明均有位置写法。既有对象兼容分支仍可处理未映射的低层参数。 |
| 【Telegram调用】、【KOOK调用】、【OB调用】、【接口值】 | 底层接口参考仍包含平台方法名、字段名、类型及上传字段，适用于直接调用平台 API。 |
| 【Telegram发票】、【Telegram发票链接】 | 服务商自定义资料为可选对象，键名由支付服务商定义。 |
| 【KOOK模板管理】和【KOOK模板消息】 | 推荐依次填数组值。已有自定义命名变量的模板仍可按模板定义传对象。 |
- 两端分别验证新命令与真实机器人权限、事件订阅和实际返回值。
