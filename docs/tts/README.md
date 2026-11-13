# 文字转语音

在红色问答管理台的「插件」中启用 `redlang` 和 `tts`。原生 TTS 随框架提供。合成和查询在线人声需要红色问答所在机器能连接 Edge TTS 服务。要把音频作为聊天回复发送，还需要连接支持语音消息的适配器。

## 命令说明

### TTS

【TTS@<font color="red">文本</font>@<font color="red">音色</font>@<font color="red">语速</font>@<font color="red">音调</font>@<font color="red">音量</font>@<font color="red">总超时秒</font>@<font color="red">额外重试次数</font>】

返回 MP3 字节集（24 kHz、48 kbps、单声道）。将结果交给 `【语音】` 可生成语音消息，将结果交给 `【写文件】` 可保存为 MP3。

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| 文本 | 必填 | 传入普通文本，空文本或全是空白的文本会报错。 |
| 音色 | `YunyangNeural`（云扬） | 支持完整 ID、中文别名和可以唯一匹配的旧音色名。 |
| 语速 | `0` | 相对默认语速的变化百分比。范围 `-99` 到 `1000`，例如 `-15` 或 `+20%`。 |
| 音调 | `0` | 相对默认音高的变化量，单位 Hz。范围 `-1000` 到 `1000`，例如 `-5Hz`。 |
| 音量 | `0` | 相对默认音量的变化百分比。范围 `-100` 到 `100`，例如 `-10%`。 |
| 总超时秒 | `12` | `1` 到 `120` 的整数，单位为秒。查人声、连接、接收音频及重试共用这段时间。 |
| 额外重试次数 | `1` | `0` 到 `3` 的整数。填 `0` 时只尝试一次，填 `2` 时最多尝试三次。 |

原有的 `【TTS@文本@音色】` 可以继续使用。从音色开始，每个参数都可留空以使用默认值。只传前面的参数时，可以省略末尾的 `@`。只设置后面的参数时，应保留中间的空参数位置。

音色建议填写 `【TTS人声】` 返回的完整 `id`，例如 `zh-CN-XiaoxiaoNeural`。`晓晓` 是该音色的别名。旧音色名片段匹配到多个人声时，错误会提示候选 ID。完整 ID 和已知别名对应的音色暂不可用时会报错。

数值参数只接受整数，音调支持 `Hz` 或 `hz` 后缀。三个调整参数填 `0` 时均使用服务默认值。表中的范围是脚本可接受的输入，实际听感由在线服务决定。自定义 SSML 和情绪标签会按普通文本处理。

#### 例子

以默认音色回复语音。

```text
【语音@【TTS@你好，欢迎使用文字转语音。】】
```

使用云希，语速提高 20%，音调降低 5 Hz，音量保持默认。

```text
【语音@【TTS@这是一段参数试听。@zh-CN-YunxiNeural@+20%@-5Hz@0】】
```

只把总超时改为 20 秒，并关闭额外重试。

```text
【语音@【TTS@你好。@@@@@20@0】】
```

保存音频到脚本包目录下的 `tts-demo.mp3`。再次运行会覆盖该文件。

```text
【定义变量@音频@【TTS@这段语音将保存为文件。@晓晓】】
【写文件@【应用目录】tts-demo.mp3@【变量@音频】】
```

硬编码的朗读内容如果包含英文空格或 `@`，可用原始字符串保留字面内容。消息触发脚本可直接传入变量或 `【子关键词】`。转义细节见[转义规则](/detailref/?id=转义规则)。

```text
【语音@【TTS@【@Hello, RedReply! 联系方式是 demo@example.com。】@晓晓】】
```

### TTS人声

【TTS人声@<font color="red">语言前缀</font>@<font color="red">性别</font>@<font color="red">总超时秒</font>】

查询当前在线目录，返回按 `id` 排序的对象数组。没有符合筛选条件的人声时返回空数组。网络请求失败时报告错误。

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| 语言前缀 | 全部 | 留空或填 `all` 查询全部语言。可填写 `zh`、`zh-CN`、`zh-HK`、`wuu` 或 `en-US` 等语言代码前缀。匹配忽略大小写。 |
| 性别 | 全部 | 留空或填 `all` 查询全部性别。可填写 `Male`、`Female`、`男` 或 `女`，英文大小写不限。 |
| 总超时秒 | `12` | `1` 到 `120` 的整数，单位为秒。 |

| 返回字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | 文本 | 完整人声 ID，可作为 `【TTS】` 的音色参数。 |
| `name` | 文本 | 在线服务返回的人声名称。 |
| `locale` | 文本 | 语言及地区代码。 |
| `gender` | 文本 | 在线服务返回的性别。 |
| `aliases` | 数组 | 本插件收录的中文别名。没有别名时为空数组。 |

同一代理设置下可复用最近 5 分钟内获取的目录。在线目录会更新，筛选结果以实际返回为准。

#### 例子

以下三行可分别运行，依次输出美国英语男声、所有女声和全部在线人声的数量。第一行将查询总超时设为 20 秒。第二行留空语言参数。

```text
【取长度@【TTS人声@en-US@男@20】】
【取长度@【TTS人声@@女】】
【取长度@【TTS人声】】
```

将中文女声的 ID 和语言代码各排一行。数组循环中，`【参数@2】` 是当前人声对象。

```text
【定义变量@人声列表@【TTS人声@zh@女】】
【循环@【变量@人声列表】@【函数定义@
    【取元素@【参数@2】@id】【空格】【取元素@【参数@2】@locale】【换行】
】】
```

### TTS样本

【TTS样本】

无参数。返回 28 个静态参考样本对象组成的数组。读取列表无需访问网络，播放样本时由浏览器或适配器访问对应的音频地址。

| 返回字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | 文本 | 参考人声的完整 ID。 |
| `name` | 文本 | 样本中文名称。 |
| `category` | 文本 | `female` 普通话女声、`male` 普通话男声、`dialect` 方言、`hk` 香港粤语或 `tw` 台湾普通话。 |
| `gender` | 文本 | `Female` 或 `Male`。 |
| `sample_url` | 文本 | 参考 MP3 的地址。 |
| `source_url` | 文本 | 样本来源页面。 |

#### 例子

使用 `【取元素】` 读取第一条样本并发为语音。数组下标从 `0` 开始。

```text
【定义变量@样本@【取元素@【TTS样本】@0】】
【语音@【取元素@【变量@样本】@sample_url】】
```

## 音频试听

点击播放器试听。GitHub 文件预览可使用每行的「打开 MP3」。这里的音频来自[原始参考页](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/index.html)。在线合成时请从 `【TTS人声】` 获取当前可用的音色 ID，再用 `【TTS】` 生成自己的文本。

### 普通话女声

| 音色 | 人声 ID | 试听 |
| --- | --- | --- |
| 晓晓 | `zh-CN-XiaoxiaoNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoxiaoNeural.mp3" aria-label="晓晓试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoxiaoNeural.mp3) |
| 晓依 | `zh-CN-XiaoyiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoyiNeural.mp3" aria-label="晓依试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoyiNeural.mp3) |
| 晓涵 | `zh-CN-XiaohanNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaohanNeural.mp3" aria-label="晓涵试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaohanNeural.mp3) |
| 晓墨 | `zh-CN-XiaomoNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaomoNeural.mp3" aria-label="晓墨试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaomoNeural.mp3) |
| 晓睿 | `zh-CN-XiaoruiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoruiNeural.mp3" aria-label="晓睿试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoruiNeural.mp3) |
| 晓双 | `zh-CN-XiaoshuangNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoshuangNeural.mp3" aria-label="晓双试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaoshuangNeural.mp3) |
| 晓梦 | `zh-CN-XiaomengNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaomengNeural.mp3" aria-label="晓梦试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/XiaomengNeural.mp3) |

### 普通话男声

| 音色 | 人声 ID | 试听 |
| --- | --- | --- |
| 云希 | `zh-CN-YunxiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunxiNeural.mp3" aria-label="云希试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunxiNeural.mp3) |
| 云扬 | `zh-CN-YunyangNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunyangNeural.mp3" aria-label="云扬试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunyangNeural.mp3) |
| 云健 | `zh-CN-YunjianNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunjianNeural.mp3" aria-label="云健试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunjianNeural.mp3) |
| 云夏 | `zh-CN-YunxiaNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunxiaNeural.mp3" aria-label="云夏试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunxiaNeural.mp3) |
| 云泽 | `zh-CN-YunzeNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunzeNeural.mp3" aria-label="云泽试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunzeNeural.mp3) |
| 云枫 | `zh-CN-YunfengNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunfengNeural.mp3" aria-label="云枫试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/YunfengNeural.mp3) |

### 方言

| 音色 | 人声 ID | 试听 |
| --- | --- | --- |
| 云彪（东北） | `zh-CN-liaoning-YunbiaoNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/liaoning-Yunbiao.mp3" aria-label="云彪（东北）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/liaoning-Yunbiao.mp3) |
| 晓北（东北） | `zh-CN-liaoning-XiaobeiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/liaoning-Xiaobei.mp3" aria-label="晓北（东北）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/liaoning-Xiaobei.mp3) |
| 云登（河南） | `zh-CN-henan-YundengNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/henan-Yundeng.mp3" aria-label="云登（河南）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/henan-Yundeng.mp3) |
| 云翔（山东） | `zh-CN-shandong-YunxiangNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/shandong-Yunxiang.mp3" aria-label="云翔（山东）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/shandong-Yunxiang.mp3) |
| 云熙（四川） | `zh-CN-sichuan-YunxiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/sichuan-Yunxi.mp3" aria-label="云熙（四川）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/sichuan-Yunxi.mp3) |
| 云奇（广西） | `zh-CN-guangxi-YunqiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/guangxi-Yunqi.mp3" aria-label="云奇（广西）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/guangxi-Yunqi.mp3) |
| 晓妮（陕西） | `zh-CN-shaanxi-XiaoniNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/shaanxi-Xiaoni.mp3" aria-label="晓妮（陕西）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/shaanxi-Xiaoni.mp3) |
| 云哲（吴语） | `wuu-CN-YunzheNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/wuu-Yunzhe.mp3" aria-label="云哲（吴语）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/wuu-Yunzhe.mp3) |
| 晓彤（吴语） | `wuu-CN-XiaotongNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/wuu-Xiaotong.mp3" aria-label="晓彤（吴语）试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/wuu-Xiaotong.mp3) |

### 香港粤语

| 音色 | 人声 ID | 试听 |
| --- | --- | --- |
| 晓佳 | `zh-HK-HiuGaaiNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-HiuGaai.mp3" aria-label="晓佳试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-HiuGaai.mp3) |
| 晓蔓 | `zh-HK-HiuMaanNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-HiuMaan.mp3" aria-label="晓蔓试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-HiuMaan.mp3) |
| 云龙 | `zh-HK-WanLungNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-WanLung.mp3" aria-label="云龙试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/HK-WanLung.mp3) |

### 台湾普通话

| 音色 | 人声 ID | 试听 |
| --- | --- | --- |
| 晓晨 | `zh-TW-HsiaoChenNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-HsiaoChen.mp3" aria-label="晓晨试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-HsiaoChen.mp3) |
| 晓雨 | `zh-TW-HsiaoYuNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-HsiaoYu.mp3" aria-label="晓雨试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-HsiaoYu.mp3) |
| 云哲 | `zh-TW-YunJheNeural` | <audio controls preload="none" src="https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-YunJhe.mp3" aria-label="云哲试听"></audio> [打开 MP3](https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/TW-YunJhe.mp3) |

用晓晓实时合成一句试听，可检查当前环境中的合成和语音回复。

```text
【语音@【TTS@你好，这是一段实时生成的人声试听。@zh-CN-XiaoxiaoNeural】】
```

## 群聊和私聊例子

在脚本管理中新建脚本，触发方式选「群、私聊触发」，匹配方式选「前缀匹配」，关键词填 `朗读 `（最后一个字符是半角空格）。将以下内容写入脚本。

```text
【定义变量@朗读内容@【去除两边空白@【子关键词】】】
【判断@【变量@朗读内容】@@
    【语音@【TTS@【变量@朗读内容】@晓晓】】@
    请在朗读后输入文字。
】
```

用户发送 `朗读 你好，欢迎回来。` 后，脚本将前缀之后的文本交给 TTS 并回复语音。匹配到前缀且余下内容为空时，脚本回复文字提示。

## 代理与常见错误

需要代理时，在合成或查询前调用 `【设置代理】`。将地址替换为本机使用的代理地址。没有设置代理时，TTS 直连服务。

```text
【设置代理@http://127.0.0.1:10809】
【语音@【TTS@你好。@晓晓】】
```

| 现象 | 检查方式 |
| --- | --- |
| `TTS 插件未启用` | 在管理台启用 `tts`。 |
| `TTS 文本不能为空` | 检查变量或 `【子关键词】` 是否传入了实际文字。 |
| 人声不唯一或在线目录中找不到音色 | 先运行 `【TTS人声】`，再把返回的完整 `id` 填入脚本。 |
| 参数格式错误或超出范围 | 核对参数位置、整数格式和单位，跳过中间参数时保留空位。 |
| 人声查询或合成超时 | 检查运行红色问答的机器能否访问 Edge TTS。网络较慢时可在允许范围内调整总超时。 |
| 提示请求过多 | 合成和在线人声查询共用 4 个并发请求名额。可等待后重试或降低触发频率。 |
| 能生成 MP3，聊天中没有语音 | 检查脚本是否用 `【语音】` 输出音频、适配器是否支持语音消息。部分 OneBot 协议端需要 FFmpeg。 |
| 文档播放器或样本 URL 无法播放 | 用「打开 MP3」检查样本源是否可达。脚本播放链接时，还需检查适配器能否访问该地址。 |

参见[语法详细参考](/detailref/)和[框架与插件架构](/plugin-architecture.md)。
