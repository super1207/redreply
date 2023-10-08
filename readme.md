# 强大的聊天自定义问答系统-红色问答
<div align=center>
	<a href="http://red.super1207.top" />官网：http://red.super1207.top</a>
</div>

<div align=center>
	<a href="https://imgse.com/i/pPkld9x"><img src="https://s1.ax1x.com/2023/08/04/pPkld9x.png" alt="OM7U3D.png" border="0" /></a>
</div>

## 文档

[语法简述](/doc/readme.md)

## 背景

受`铃心自定义`的启发，制作一个类似的自定义系统。 <br />

随着`酷Q`、`先驱`等机器人平台的停运，其上运行的`铃心自定义`也逐渐不再被其作者很好的维护。再加上各种跨平台的开源机器人平台的逐渐流行，一个全开源、跨平台的自定义问答系统被期待着。<br />

`红色问答`因此而出现。

## 主要功能

以自定义回复为核心，附带群管、监控、词库、语音、图片、调用接口、对接网站、黑白名单、定时任务、网页爬虫、简单编程、入群验证、发言限制、Web服务器、制作接口等功能。<br />

## 开始使用

1：登录一个`onebot11`的协议端，如[go-cqhttp](https://github.com/Mrs4s/go-cqhttp)、[KookOnebot](https://github.com/super1207/KookOneBot)并开启[ws正向连接](https://github.com/botuniverse/onebot-11/tree/master/communication)。<br />

2：下载项目release中的`redlang.exe`，并执行。<br />

3：在自动弹出的浏览器界面中，点击`连接ONEBOT`，然后添加第一步中的`ws正向连接`，如：<br />

`ws://127.0.0.1:8080?access_token=77156`

4：如果出现任何疑惑，可以到我们的`QQ`群中随意询问(没有疑惑也可以来玩。

<br /> QQ群：920220179(已满)、556515826

此外，欢迎来我的KOOK群玩！邀请链接：https://kook.top/3SEwQj


## 访问控制

在`config.json`中，若存在`web_password`这个字符串字段且不为空字符串，则访问webui时需要先输入密码登录，才能访问，输入此密码，将获得读写权限。

在`config.json`中，若存在`readonly_web_password`这个字符串字段且不为空字符串，则访问webui时需要先输入密码登录，才能访问，输入此密码，将获得只读权限。

如果要完全禁止他人访问，你必须同时设置这两个密码！！！


## 自行编译

注意，通常情况下，如果您不打算参与此项目的开发，就无需自行编译，请直接到release(或者github action)中去下载。<br />

1：安装好[rust编译环境](https://www.rust-lang.org/)。<br />

2：<br />
    在`windows`下，仅需要在项目目录下运行`cargo build`即可。<br />
    在`linux`下，编译过程参考github action


## 开源说明

[GNU Affero General Public License](https://en.wikipedia.org/wiki/GNU_Affero_General_Public_License)

特别注意：

1：分发、使用此软件或其代码请明确告知用户此软件的原始开源地址：https://github.com/super1207/redreply<br />

2：使用修改后的软件提供服务，或传播修改后的软件，请保持相同开源协议开源并明确指出修改内容，不得隐藏软件已经被修改的事实。<br />

3：此软件不做质量保证，若因此软件或其修改版本造成任何损失，概不负责。<br />

4：请合法使用。


## 其它重要事项

1：`红色问答`中很大一部分命令参考了`铃心自定义`，感谢铃心自定义的制作团队！<br />

2：`红色问答`的`红色`两字，并无政治上意义，也没有其它特殊内涵，仅仅是因为`super1207`在项目开启的初期喜欢红色。<br />

3：`红色问答`中大部分代码由`super1207`编写，但其语法和机制是很多人共同探讨出来的。<br />

4：`红色问答`并没有设计自己的图标，而是采用`近月少女的礼仪`中的人物`樱小路露娜`作为标志，`super1207`已经尽可能的降低了图片清晰度，若仍然认为有可能侵权的行为，请立刻与我联系。
