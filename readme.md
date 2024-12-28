<div align=center>
	<h1> 红色问答 </h1>
</div>

<div align=center>
	<a href="res/favicon.ico"><img src="res/favicon.ico" alt="favicon.ico" border="0" /></a>
</div>

> 强大的聊天自定义问答系统

## 背景

受铃心自定义的启发，制作一个类似的自定义系统。 <br />

随着酷Q、先驱等机器人平台的停运，其上运行的铃心自定义也逐渐不再被其作者很好的维护。再加上各种跨平台的开源机器人平台的逐渐流行，一个 **全开源** 、**跨平台** 的自定义问答系统被期待着。<br />

红色问答因此而出现。

## 主要功能

以自定义回复为核心，附带群管、监控、词库、语音、图片、调用接口、对接网站、黑白名单、定时任务、网页爬虫、简单编程、入群验证、发言限制、Web服务器、制作接口等功能。<br />

## 展示

> 红色问答使用浏览器作为界面，并且开箱即用，您可以躺在床上用手机一只手愉快地编写插件。

![example1](https://github.com/user-attachments/assets/d11eafbe-70c6-4e37-b702-9ed135cefc8d)


![example2](https://github.com/user-attachments/assets/050046b3-5dd8-4255-bada-687e8f390fd2)

## 平台协议支持

> 红色问答支持众多聊天平台协议，有聊天框框的地方就可以有红色问答，您编写的插件无需任何改动即可在这些聊天平台上面运行。

[qq官方频道/群](https://q.qq.com/)：QQ官方协议接口，redreply内置支持

[kook/开黑啦](https://www.kookapp.cn/)：KOOK官方协议接口，redreply内置支持

[llonebot](https://github.com/LLOneBot/LLOneBot) ：社区支持的三方QQ协议,使你的NTQQ支持OneBot11协议进行QQ机器人开发

[chronocat](https://github.com/chrononeko/chronocat) ：社区支持的三方QQ协议,模块化的 Satori 框架(satori协议)

[NapCatQQ](https://github.com/NapNeko/NapCatQQ)：社区支持，基于NTQQ的无头Bot框架

[Lagrange.Core](https://github.com/LagrangeDev/Lagrange.Core)：An Implementation of NTQQ Protocol, with Pure C#, Derived from Konata.Core(onebot协议)

[koishi/satori](https://koishi.chat/)：Cross-platform chatbot framework made with love.支持telegram、discord、飞书、钉钉等

[olivos](https://github.com/OlivOS-Team/OlivOS)：OlivOS / Witness Union，一个强大的跨平台交互栈与机器人框架，支持ff14、hackchat、bilibili等

[onebot11](https://github.com/botuniverse/onebot-11)：其它onebot11实现

[邮件](https://baike.baidu.com/item/%E7%94%B5%E5%AD%90%E9%82%AE%E4%BB%B6%E5%8D%8F%E8%AE%AE/4105152) 支持使用imap和smtp协议来收发邮件，可以对接QQ邮箱，163邮箱等，提供邮件自动回复服务

[redreply](https://github.com/super1207/redreply)：是的，没看错，redreply可以导出onebot11接口，所以一个redreply可以连接另一个redreply，但是请不要自己连接自己（笑）

[telegram](https://telegram.org)：一款国外的著名通信软件


## 操作平台支持

红色问答为每个您熟悉的平台发布可执行文件，其中包括：windows、linux、android。

红色问答只有一个可执行文件，基本不需要安装其它依赖，你只需要双击一次即可完成部署工作。

对于某些不直接提供可执行文件的平台，也可以自己编译，比如freebsd。

> 是的，没有ios和mac

## 文档

> 红色问答有很多高级功能，如果需要，可以仔细阅读文档来了解。

[文档](https://super1207.github.io/redreply)


## 插件商店

红色问答维护一个插件商店，您可以在这里[发布您的插件](https://github.com/super1207/redreplyhub)，让更多的人享受到您的劳动成果。


## 开源说明

[GNU Affero General Public License](https://en.wikipedia.org/wiki/GNU_Affero_General_Public_License)

特别注意：

1：分发、使用此软件或其代码请明确告知用户此软件的原始开源地址：https://github.com/super1207/redreply<br />

2：使用修改后的软件提供服务，或传播修改后的软件，请保持相同开源协议开源并明确指出修改内容，不得隐藏软件已经被修改的事实。<br />

3：此软件不做质量保证，若因此软件或其修改版本造成任何损失，概不负责。<br />

4：请合法使用。

## 为什么要使用红色问答

1：红色问答是全开源的。开源，意味着可控制，你不用担心跑路，软件行为可控，也可用随意增删魔改其功能，并不需要额外授权，因为代码就在你手上，只需要遵守一个非常宽松的AGPLv3。

2：红色问答跨平台的。得益于rust优秀的跨平台能力，红色问答能在各种奇奇怪怪的环境下运行，比如机顶盒，游戏机，学习机。

3：红色问答是适配多种聊天平台的。通过精心设计的协议抽象，使你的大多数插件能达成"一次编写，处处运行"，如果你有一些rust编写能力的话，你甚至能自己适配新的平台。

4：红色问答没有魔法。rust是一门炫酷的编程语言，但是红色问答在编写时刻意压制了编程技巧、设计模式的使用(因为根本不会)，看红色问答的代码就像在看摆烂大学生的课后实践大作业(本来就是)，你不需要高超的软件技术就能理解其实现，虽然有点无聊。

5：红色问答是稳定的。红色问答几乎没有使用rust的unsafe特性，并且代码结构十分清晰，这意味着所有的崩溃、内存泄露、栈溢出、cpu占用，都是可以被调试器轻易发现的，这从根本上保证了软件的可持续维护性。

6：红色问答支持其它编程语言。红色问答有很强的python、lua支持，即使你完全不想学红色问答的语法，也不懂rust，你也能轻易编写红色问答的插件。

7：红色问答注重用户体验。红色问答使的所有功能、设计都不是凭空产生，而是来源于明确的需求，红色问答是一个活在现实中的项目。

8：红色问答是可爱的。有谁能拒绝可爱的luna sama呢？这是**最为重要**的一点，如果你承认这点，那么上面我的各种自吹自夸都不重要了，请立刻开始red start吧！


## 其它重要事项

1：红色问答中很大一部分命令参考了铃心自定义，感谢铃心自定义的作者！<br />

2：红色问答的红色两字，并无政治上意义，也没有其它特殊内涵，仅仅是因为super1207在项目开启的初期喜欢红色。<br />

3：红色问答中大部分代码由super1207编写，但其语法和机制是很多人共同探讨出来的。<br />

4：红色问答并没有设计自己的图标，而是采用近月少女的礼仪中的人物樱小路露娜作为标志，super1207已经尽可能的降低了图片清晰度，若仍然认为有可能侵权的行为，请立刻与我联系。

5：红色问答在编写过程中参考了许多博客、项目、书籍；也得到了学校老师，同门师兄师姐、不知名网友等各路人士的帮助，因个人隐私和篇幅等相关原因，我不能在这里一一列举。实际上，大多数项目都是站在巨人的肩膀上的，个人的努力只是其中很小一部分，比如你总得先给冯诺依曼磕一个。

5：有个交流群：920220179
