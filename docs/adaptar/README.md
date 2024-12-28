# 适配器说明

## onebot11

以正向WS连接[ONEBOT11](https://github.com/botuniverse/onebot-11)

见[OpenShamrock](https://github.com/whitechi73/OpenShamrock)(推荐)、[go-cqhttp](https://github.com/Mrs4s/go-cqhttp)、[opqonebot](https://github.com/super1207/opqonebot)

如果是QQ平台，目前可以自定义音卡签名。在`config.json`同级目录，创建一个`adapter_onebot11_config.json`文件，内容为:

```json
{
    "music_card_sign":"https://oiapi.net/API/QQMusicJSONArk"
}
```
就可以自动使用独角兽的API来签名custom类型的音乐卡片了。

## olivos

[OlivOS](https://github.com/OlivOS-Team/OlivOS) 平台的opk插件自动配置，测试中，进主页交流群了解更多信息...

## satori

可以连接[satorijs](https://github.com/satorijs) 或 [satoricq](https://github.com/super1207/satoricq)

## qq频道、群
.
可以对接[QQ官方平台](https://q.qq.com/)

支持直接发送wav、flac、mp3、ogg(vorbis)格式的音频，无需配置ffmpeg。

支持QQ官方的markdown，可以这么发:`[CQ:qmarkdown,data=xxx]`。`xxx`是类似如下json
```json
{
    "markdown": {
        "content": "# 标题 \n## 简介很开心 \n内容[🔗腾讯](https://www.qq.com)"
    }
}
```
的base64编码。以上例子写做CQ码可以这么写：
`[CQ:qmarkdown,data=ewogICJtYXJrZG93biI6IHsKICAgICJjb250ZW50IjogIiMg5qCH6aKYIFxuIyMg566A5LuL5b6I5byA5b+DIFxu5YaF5a65W+2gve20l+iFvuiur10oaHR0cHM6Ly93d3cucXEuY29tKSIKICB9Cn0=]`

支持在`markdown`同级位置放入`keyboard`。以下是一个同时放markdown和keyboard的例子。
```
{
    "markdown": {
        "content": "# 标题 \n## 简介很开心 \n内容[🔗腾讯](https://www.qq.com)"
    },
    "keyboard": {
        "id": "123"
    }
}
```
以上例子写做CQ码可以这么写：
`[CQ:qmarkdown,data=ewogICAgIm1hcmtkb3duIjogewogICAgICAgICJjb250ZW50IjogIiMg5qCH6aKYIFxuIyMg566A5LuL5b6I5byA5b+DIFxu5YaF5a65W+2gve20l+iFvuiur10oaHR0cHM6Ly93d3cucXEuY29tKSIKICAgIH0sCiAgICAia2V5Ym9hcmQiOiB7CiAgICAgICAgImlkIjogIjEyMyIKICAgIH0KfQ==]`

更详细信息参考QQ的文档[markdown](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/type/markdown.html)
、[keyboard](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/trans/msg-btn.html)。

## 邮件

支持IMAP/SMTP邮件收发协议。支持接收纯文本的邮件，支持发送图文混合的邮件，触发方式`私聊触发`。

## KOOK

支持对接KOOK官方平台。

## telegram

支持对接telegram官方平台。目前只支持了基础的群组和私聊的图文收发，所以某些平台相关的命令暂时是不可用的，若有需要，请向我们反馈。