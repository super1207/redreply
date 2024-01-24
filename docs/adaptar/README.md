# 适配器说明

## onebot11

以正向WS连接[ONEBOT11](https://github.com/botuniverse/onebot-11)

见[OpenShamrock](https://github.com/whitechi73/OpenShamrock)(推荐)、[kookonebot](https://github.com/super1207/KookOneBot)、[go-cqhttp](https://github.com/Mrs4s/go-cqhttp)、[opqonebot](https://github.com/super1207/opqonebot)

## olivos

[OlivOS](https://github.com/OlivOS-Team/OlivOS) 平台的opk插件自动配置，测试中，进主页交流群了解更多信息...

## satori

可以连接[satorijs](https://github.com/satorijs) 或 [satoricq](https://github.com/super1207/satoricq)

## qq频道、群

可以对接[QQ官方平台](https://q.qq.com/)

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