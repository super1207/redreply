# onebot网络接口(测试中，可能会不太稳定)

## 目的

红色问答公开onebot服务，可以将红色问答支持的任何协议转为onebot11接口服务，使得其它插件框架，如MiraiCQ，可以对接红色问答。

注：

红色问答目前直接支持的平台为：
onebot11、olivos、satori、qq频道私区域(qqguild_private)、qq频道(qqguild_public)/群(qqgroup_public)公域。

间接支持**几乎所有聊天平台**

## 连接

目前，仅支持正向WS。签权需要的access_token为红色问答的web密码，**本机使用不需要鉴权**。

websocket地址：`ws://localhost:[redport]:/onebot/[机器人平台]/[机器人账号]`

如：`ws://localhost:1207/onebot/ntqqv1/1875159423`

当只有一个平台一个账号时，可以简写为(不推荐)：

`ws://127.0.0.1:1207/onebot`

## 局限性

1：对于qq官方频道那种，需要设置message_id才能回复的，会自动寻找最近的message_id进行回复，容易回复错人。

2：在具备两级群组的平台，如频道，获取群列表可能会失败。

3：每个message_id仅具备5分钟的有效时间。