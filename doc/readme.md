# RedLang 语法简述


注意，目前项目正在快速迭代，所有规则都有可能会改变，并且不会有任何通知，如果有自己的想法或者需求，可以一起讨论:

作者qq：1875159423

qq群号：920220179(已满) 556515826

开源地址：https://github.com/super1207/redreply

开源协议：https://github.com/super1207/redreply

使用到的三方库：[Cargo.toml](https://github.com/super1207/redreply/blob/master/Cargo.toml)


## 目标


一个简单但强大的文本生成规则，由各种命令组成，**将会**支持读写文件，网络访问等一切和文本处理相关的事情。


## 代码一览


生成五个hello：


```
【循环@5@hello】
```


输出：


```
hellohellohellohellohello
```


当然，也可以很复杂，如：


```
【赋值变量@n@20】
递归计算斐波那契数列第【变量@n】项：
【定义变量@斐波那契数列函数@
    【函数定义@
        【判断@【计算@【参数@1】==1】@假@1【返回】】
        【判断@【计算@【参数@1】==2】@假@1【返回】】
        【计算@
            【调用函数@【变量@斐波那契数列函数】@【计算@【参数@1】-1】】
                            +
            【调用函数@【变量@斐波那契数列函数】@【计算@【参数@1】-2】】
        】
    】
】
【调用函数@【变量@斐波那契数列函数】@【变量@n】】
```


输出：


```
递归计算斐波那契数列第20项：6765
```


## 支持数据类型


文本、对象、数组、字节集、函数。文本是唯一可见(可输出)的数据类型。RedLang不直接支持数值、布尔、空等类型，主要原因是数值、布尔都是可见的，容易与文本混淆，而空类型容易与空文本混淆。


### 文本


正确的文本为UTF8格式的字符串。


### 对象


即键值对的组合，在有些编程语言中也叫做字典或者map。


### 数组


多个元素按次序组合的结构称为数组，有时也称为列表。


### 函数


函数被视作一种普通的类型，可以储存在变量中。函数本身也可以在定义时按值捕获外部变量(通过"闭包"指令)，如其它编程语言中的lambda一样。


### 字节集


二进制串


## 作用域规则


只有调用函数会产生新的作用域，如果没有被函数包裹，则位于全局作用域(不能跨越脚本，如果想要跨越脚本，请使用**定义常量**命令)。


## 转义规则


只有**字符串字面量**需要转义，转义符号为<font color="red">\\</font>。需要转义的字符有 <font color="red">@</font>、<font color="red">【</font>、<font color="red">】</font>、<font color="red">\\</font>。另外，空格和换行的字面量会被忽略，需要使用命令【空格】、【换行】代替。特别说明的是，空格也可以用<font color="red">\\</font>来转义。


## 命令格式


【命令名@参数1@参数2@....】

命令由命令名和参数组成，中间由@分割。

特殊说明：如果命令名后紧接着下一个命令，那么之间的@可以省略

如【命令名@【命令名...】...】可以等效为【命令名【命令名...】...】

**内置的命令的命令名**是大小写不敏感、繁体简体不敏感的，但是注意，命令的参数通常是大小写敏感的。


## 通用命令说明


### 换行


【换行】用来代替字面量的\\n

### 回车


【回车】用来代替字面量的\\r


### 空格


【空格】用来代替字面量的空格


### 隐藏


【隐藏@<font color="red">要隐藏的内容</font>】

用来隐藏命令输出，被隐藏的输出，可以通过【传递】命令在之后取出。


### 传递


【传递】

用来使用距离当前作用域最近的被【隐藏】命令隐藏的内容，若不存在，返回空文本。


### 屏蔽


【屏蔽@<font color="red">要屏蔽的内容</font>】

用来隐藏命令输出。

### 当前版本


【当前版本】

返回红色问答的当前版本。


### 定义变量


【定义变量@<font color="red">变量名</font>@<font color="red">变量值</font>】

用来在当前作用域定义变量，若当前作用域已经存在，则替换。


### 变量


【变量@<font color="red">变量名</font>】

用来使用距离当前作用域最近的变量，若变量不存在，返回空文本。


### 赋值变量


【赋值变量@<font color="red">变量名</font>@<font color="red">变量值</font>】

用来修改距离当前作用域最近的变量，若搜索完所有作用域都无此变量，则在当前定义域定义此变量。


### 判断


【判断@<font color="red">文本1</font>@<font color="red">文本2</font>@<font color="red">不同执行</font>@<font color="red">相同执行</font>】

其中<font color="red">相同执行</font>可以省略。

注意：此命令与【判等】命令完全相同，请优先使用判等，更加见名知义。

### 判等


【判等@<font color="red">文本1</font>@<font color="red">文本2</font>@<font color="red">不同执行</font>@<font color="red">相同执行</font>】

与【判断】相同。


### 判真


【判真@<font color="red">文本</font>@<font color="red">不为真执行</font>@<font color="red">为真执行</font>】

用于判断<font color="red">文本</font>是否为<font color="red">真</font>。


### 选择


【选择@<font color="red">数字</font>@<font color="red">内容1</font>@<font color="red">内容2</font>@...】

根据<font color="red">数字</font>来选择要执行的内容，从1开始数。如果越界，则返回空文本。如果<font color="red">数字</font>为空，则表示随机选择一个。


### 循环


【循环@<font color="red">循环次数</font>@<font color="red">循环语句</font>】

【循环@<font color="red">数组</font>@<font color="red">处理函数</font>】

【循环@<font color="red">对象</font>@<font color="red">处理函数</font>】

此命令有三种形式，第二种形式中：<font color="red">处理函数</font>是一个回调函数，它有两个参数，第一个参数表示数组下标，第二个参数表示数组元素。如`【循环@【数组@a@b@c】@【函数定义【参数@1】【参数@2】】】`将会输出`0a1b2c`。

第三种形式中：<font color="red">处理函数</font>是一个回调函数，它有两个参数，第一个参数表示对象的键，第二个参数表示对象的值。如`【循环@【对象@a@b@c@d】@【函数定义【参数@1】【参数@2】】】`将会输出`abcd`。


### 判循


【判循@<font color="red">循环条件</font>@<font color="red">循环语句</font>】

循环条件为<font color="red">真</font>，则循环。


### 判空


【判空@<font color="red">被判断内容</font>@<font color="red">为空替换</font>】

如果<font color="red">被判断内容</font>长度为0，则此变量表示的值为<font color="red">为空替换</font>，否则为<font color="red">被判断内容</font>


### 跳出


【跳出】

用来跳出当前循环，注意必须在循环体中使用，等效于其它语言中的break语句。


### 继续


【继续】用来继续下次循环，注意必须在循环体中使用，等效于其它语言中的continue语句。


### 函数定义


【函数定义@<font color="red">函数体</font>】

用来定义一个函数，可以将其存入变量中。

### 定义命令


【定义命令@<font color="red">命令名</font>@<font color="red">命令内容</font>】

用来定义一个命令，在红色问答重启之前，定义的命令在所有脚本中都是有效的。

命令不产生新的作用域，所以在命令中使用【返回】指令将直接跳出当前作用域。

命令不产生新的作用域，所以在命令中定义变量将在当前作用域定义变量。
**调用命令**时，命令名不参与解析，也不处理转义。所以，您在定义命令时，命令名中不要有需要转义的符号。

自定义的命令可以覆盖内置命令。


### 调用函数


【调用函数@<font color="red">函数内容</font>@<font color="red">参数1</font>@<font color="red">参数2</font>@......】

用来调用一个函数，函数内容通常是存在某个变量中的；参数个数没有限制，也可以没有参数；调用函数是形成新作用域的唯一办法。
特别说明的是，函数内容可以是常量的名字


### 参数


【参数@<font color="red">第几个参数</font>】

参数个数从1开始数，如【参数@1】代表第一个参数。

若参数越界，返回空文本。

### 参数个数


【参数个数】

返回参数的个数，此命令只能在函数或命令中使用。


### 返回


【返回】

跳出当前作用域。一般用于跳出函数，在函数之外也**可以**使用，表示跳出脚本。


### 计算


【计算@<font color="red">表达式</font>】

用于数值计算和逻辑计算。

支持的数值运算符：

^(指数计算)

\+ - * / %(取余数) //(整除)

支持的逻辑运算符： 

\==(等于) !=(不等于) > >= < <=

&&(与) ||(或) !(非)

逻辑运算表达式返回<font color="red">真</font>或<font color="red">假</font>。

| 符号 | 说明     | 优先级 |
| ---- | -------- | ------ |
| ^    | 指数运算 | 1      |
| !    | 取非     | 2      |
| -    | 取负     | 2      |
| %    | 取余数3  | 3      |
| //   | 整除     | 3      |
| /    | 除       | 3      |
| *    | 乘       | 3      |
| -    | 减       | 4      |
| +    | 加       | 4      |
| !=   | 不等     | 5      |
| <=   | 小于等于 | 5      |
| \>=  | 大于等于 | 5      |
| >    | 大于     | 5      |
| <    | 小于     | 5      |
| ==   | 等于     | 5      |
| &&   | 与       | 6      |
| \|\|   | 或       | 7      |

优先级数字越小越高。


### 数组


【数组@<font color="red">元素1</font>@<font color="red">元素2</font>@......】

用来构建一个数组，可以为空数组：【数组】


### 对象


【对象@<font color="red">key1</font>@<font color="red">value1</font>@<font color="red">key2</font>@<font color="red">value2</font>@......】

用来构建一个对象，可以为空对象：【对象】


### 取长度

【取长度@<font color="red">内容</font>】

对于数组，返回元素个数；对于对象，返回key的个数；对于文本，返回utf8字符个数，对于字节集，返回字节个数。

### 转文本

【转文本@<font color="red">内容</font>@<font color="red">字节集的编码</font>】

当<font color="red">内容</font>为字节集时，将转化为对应编码的文本。

当内容为对象、数组、文本时，将转化为对应的json格式文本。<font color="red">字节集的编码</font>支持UTF8、GBK，也可以省略，默认UTF8


### 增加元素

【增加元素@<font color="red">变量名</font>@<font color="red">元素</font>@<font color="red">元素</font>......】

变量支持对象，文本，数组，字节集。

若为对象，则需写成：

【增加元素@<font color="red">变量名</font>@<font color="red">key</font>@<font color="red">value</font>......】

### 替换元素

【替换元素@<font color="red">变量名</font>@<font color="red">下标</font>@<font color="red">值</font>】

变量支持对象，文本，数组，字节集。

注意：如果为文本，那么值必须为单个字符；如果为字节集，值应该为单个字节集；如果为对象，与【增加元素】效果一样，但仅支持一个键值对。

### 删除元素

【删除元素@<font color="red">变量名</font>@<font color="red">下标</font>】

变量支持对象，文本，数组，字节集。

注意：下标越界或不存在**不会**报错。

### 取元素

【取元素@<font color="red">内容</font>@<font color="red">下标</font>@<font color="red">下标</font>@......】

内容支持对象，文本，数组。

为对象时，下标为key

为数组时，下标从0开始数

为文本时，下标从0开始数，返回的是UTF8字符(文本)

当下标不存在(或越界)时，返回空文本


### 取变量元素

【取变量元素@<font color="red">变量名</font>@<font color="red">下标</font>】

变量支持对象，文本，数组，字节集。

为对象时，下标为key，当下标不存在(或越界)时，返回空文本

为数组时，下标从0开始数，当下标不存在(或越界)时，返回空文本

为文本时，下标从0开始数，返回的是UTF8字符(文本)，当下标不存在(或越界)时，返回空文本

为字节集时，下标从0开始数，返回的也是字节集，当下标不存在(或越界)时，返回空字节集


### 取对象key


【取对象key@<font color="red">对象</font>】

返回对象的key数组。


### 取类型


【取类型@<font color="red">内容</font>】

返回内容的类型：数组(A)，文本(T)，对象(O)，字节集(B)，函数(F)


### 取随机数


【取随机数@<font color="red">X</font>@<font color="red">Y</font>】

随机返回X、Y之间的整数，包括X、Y。

X，Y都必须为非负整数，且Y**不能小于**X。

对于32位版本，X、Y最大支持32位二进制位，对于64位版本，X、Y最大支持64位二进制位。


### 闭包


【闭包@<font color="red">语句</font>】

用于在函数定义的时候使用，闭包中的语句会在**函数定义**时执行，成为函数定义的一部分。


### 随机取


【随机取@<font color="red">数组</font>@<font color="red">为空替换</font>】

随机返回数组中的一个元素，若数组为空则此变量的值为<font color="red">为空替换</font>


### 取中间


【取中间@<font color="red">文本内容</font>@<font color="red">文本开始</font>@<font color="red">文本结束</font>】

返回一个数组。


### 截取


【截取@<font color="red">内容</font>@<font color="red">开始位置</font>@<font color="red">要截取的长度</font>】

暂时只能截取文本或数组

返回截取后的文本(或数组)。若长度越界则截取到文本(或数组)末尾，若开始位置越界则返回空文本(或空数组)，若开始位置为负数，则开始位置从内容末尾计算。


### 转大写

【转大写@<font color="red">文本</font>】
将文本转为大写表示。


### 转小写

【转小写@<font color="red">文本</font>】
将文本转为小写表示。


### 访问


【访问@<font color="red">网址</font>】

GET访问网页，返回字节集，失败返回空字节集。


### POST访问


【POST访问@<font color="red">网址</font>@<font color="red">访问体</font>】

POST访问网页，访问体必须是字节集或文本，返回字节集，失败返回空字节集。


### 设置访问头


【设置访问头@<font color="red">key</font>@<font color="red">value</font>】

例子：


```
【设置访问头@User-Agent@Mozilla/5.0\ (Windows\ NT\ 6.1;\ Win64;\ x64)\ AppleWebKit/537.36\ (KHTML,\ like\ Gecko)\ Chrome/89.0.4389.72\ Safari/537.36】
```


在使用<font color="red">访问</font>、<font color="red">POST访问</font>命令之前使用。


### 设置代理


【设置代理@<font color="red">value</font>】

例子：


```
【设置代理@http://127.0.0.1:10809】
```


在使用<font color="red">访问</font>、<font color="red">POST访问</font>命令之前使用。


### 设置访问超时


【设置访问超时@<font color="red">超时时间</font>】

在使用<font color="red">访问</font>、<font color="red">POST访问</font>命令之前使用，时间单位为毫秒，注意，最小超时时间为1000毫秒。



### 编码


【编码@<font color="red">要编码的内容</font>】

对url进行编码，如：


```
https://image.baidu.com/search/index?tn=baiduimage&word=【编码@樱小路露娜】
```


### 解码


【解码@<font color="red">要解码的文本</font>】

对url进行解码，返回解码后的文本。

### Json解析 


【Json解析@<font color="red">Json内容</font>@<font color="red">jsonpath</font>】

返回RedLang对应的对象，若解析失败，则返回空文本。

jsonpath的规则参见[jsonpath-rust](https://github.com/besok/jsonpath-rust)，可以省略。

注意，json中的数值，将会转化成文本；json中的布尔型，将会转化成<font color="red">真</font>或<font color="red">假</font>；json中的null，将会转化成空文本。

若<font color="red">Json内容</font>为字节集，则会自动先按utf-8转换成文本，再进行Json解析。


### 读文件 


【读文件@<font color="red">文件路径</font>】

返回文件内容(字节集)。若文件不存在，返回空字节集。

### 写文件 


【写文件@<font color="red">文件路径</font>@<font color="red">字节集</font>】

创建文件，并写文件。若路径不存在，会自动创建路径。若文件存在，则会先清空文件，再写文件。

### 追加文件 


【追加文件@<font color="red">文件路径</font>@<font color="red">字节集</font>】

在文件后面追加内容。若路径不存在，会自动创建路径。若文件不存在，则会先创建文件。

### 目录分隔符 


【目录分隔符】

返回路径分隔符，windows下为\，linux下为/


### 读目录 


【读目录@<font color="red">路径</font>】

返回一个数组，数组中包含目录下的文件和目录(末尾有分隔符)。
返回的文件和目录均为绝对路径。


### 读目录文件


【读目录文件@<font color="red">路径</font>】

返回一个数组，数组中包含目录下的文件。
返回的文件为绝对路径。


### 创建目录 


【创建目录@<font color="red">路径</font>】

创建目录，可以一次性创建多级目录。


### 删除目录


【删除目录@<font color="red">路径</font>】

删除目录。无论成功与否，都返回空文本。


### 删除文件


【删除文件@<font color="red">路径</font>】

删除目录。无论成功与否，都返回空文本。


### 文件信息


【文件信息@<font color="red">路径</font>】

返回文件(或目录)的元数据。示例返回:
```
{"修改时间":"1676121266","创建时间":"1675318094","大小":"12176384","符号链接":"假","类型":"文件","访问时间":"1676121283"}
```
返回的是一个RedLang对象。时间为10位unix时间戳，大小的单位为字节，类型为`文件`或`目录`，若文件不存在，则返回空对象。


### 分割 


【分割@<font color="red">要分割的文本</font>@<font color="red">分割符号</font>】

返回文本数组。

### 去除开始空白


【去除开始空白@<font color="red">文本</font>】


### 去除结尾空白


【去除结尾空白@<font color="red">文本</font>】

### 去除两边空白


【去除两边空白@<font color="red">文本</font>】

### 数字转字符

【数字转字符@<font color="red">数字</font>】

将1到127(包括1和127)之间的数字转为ascii字符。例如【数字转字符@64】将返回A


### 判含 


【判含@<font color="red">被判断文本</font>@<font color="red">被包含文本</font>@<font color="red">不包含返回</font>@<font color="red">包含返回</font>】

【判含@<font color="red">被判断数组</font>@<font color="red">被包含文本</font>】

此命令有两种结构。

第一种用于判断一段文本中是否包含另一段文本。

第二种用于从数组中找出包含某文本的元素集合，返回的是一个数组。


### 正则判含 


【正则判含@<font color="red">被判断文本</font>@<font color="red">正则表达式</font>@<font color="red">不包含返回</font>@<font color="red">包含返回</font>】

【正则判含@<font color="red">被判断数组</font>@<font color="red">正则表达式</font>】

此命令有两种结构。

第一种用于判断正则表达式是否匹配被判断文本。

第二种用于从数组中找出可以被正则表达式匹配的元素集合，返回的是一个数组。


### 正则


【正则@<font color="red">文本</font>@<font color="red">正则表达式</font>】

返回正则匹配结果(一个二维数组)


### 文本替换


【文本替换@<font color="red">文本</font>@<font color="red">旧文本</font>@<font color="red">新文本</font>】

返回替换结果

### 文本查找


【文本查找@<font color="red">文本</font>@<font color="red">子文本</font>@<font color="red">开始位置</font>】

返回子文本在文本中开始的位置，若没有找到，则返回-1。若开始位置省略，则默认从0开始找。例子：`【文本查找@0123456789@5@4】`返回`5`，`【文本查找@0123456789@5@6】`返回`-1`。


### 定义常量


【定义常量@<font color="red">常量名</font>@<font color="red">常量内容</font>】

定义一个常量，常量在所有脚本中可见


### 常量


【常量@<font color="red">常量名</font>】

【常量@<font color="red">包名</font>@<font color="red">常量名</font>】

读取一个常量，若常量不存在，返回空文本。
此命令有两种形式，第一种用于获取当前包的常量，第二种用于获取其它包的常量。


### 定义持久常量


【定义持久常量@<font color="red">常量名</font>@<font color="red">常量内容</font>】

定义一个持久常量，持久常量在同包的所有脚本中可见，且重启后不会丢失。


### 持久常量


【持久常量@<font color="red">常量名</font>@<font color="red">默认值</font>】

读取一个持久常量，若持久常量不存在，返回默认值，默认值可以省略，默认为空文本。


### 转字节集


【转字节集@<font color="red">文本</font>@<font color="red">字节集编码</font>】

将文本转为字节集，<font color="red">字节集编码</font>支持UTF-8、GBK，可以省略，默认UTF-8

注意，只有文本才能转字节集


### BASE64编码


【BASE64编码@<font color="red">字节集</font>】

将字节集转为base64编码的文本

注意，只有字节集才能进行BASE64编码


### BASE64解码


【BASE64解码@<font color="red">base64文本</font>】

将base64编码的文本转为字节集

注意，只有base64编码的文本才能进行BASE64解码


### 延时


【延时@<font color="red">毫秒数</font>】

如【延时@<font color="red">1000</font>】表示延时1秒

### 打印日志


【打印日志@<font color="red">文本</font>】

打印debug日志到控制台。


### 序号

【序号@<font color="red">key</font>】

【序号@<font color="red">key</font>@<font color="red">value</font>】

此命令有两种形式：

第一种形式，返回序号<font color="red">key</font>的当前值(默认从0开始)，并将序号<font color="red">key</font>的值+1。

第二种形式，设置序号<font color="red">key</font>的值为<font color="red">value</font>，返回空文本。


### 时间戳


【时间戳】

返回10位unix时间戳


【13位时间戳】

返回13位时间戳


### 时间戳转文本


【时间戳转文本@时间戳】

参数为10位unix时间戳，返回本地时间的文本表示(年-月-日-时-分-秒)，如<font color="red">2022-09-01-13-55-56</font>


### 文本转时间戳


【文本转时间戳@文本】

参数为当地时间的时间文本(年-月-日-时-分-秒)，如<font color="red">2022-09-01-13-55-56</font>，返回10位unix时间戳。(10位时间戳不一定只有10位，五千年后10位unix时间戳是12位)


### 运行脚本


【运行脚本@<font color="red">脚本内容</font>@<font color="red">参数1</font>@<font color="red">参数2</font>...】

在一个新的环境中运行RedLang脚本，返回脚本执行结果，可以带参数

QQ相关的命令依赖的数据，会被共享；而普通变量、序号等信息不会保留。


### 后台运行脚本


【后台运行脚本@<font color="red">脚本内容</font>@<font color="red">参数1</font>@<font color="red">参数2</font>...】

在一个新的环境中运行RedLang脚本，立刻返回空文本，不会阻塞当前脚本，可以带参数

QQ相关的命令依赖的数据，会被复制；而普通变量、序号等信息不会保留。

### MD5编码


【MD5编码@<font color="red">字节集</font>】

将字节集转为md5编码的文本，全小写

注意，只有字节集才能进行md5编码。


### RCNB编码


【RCNB编码@<font color="red">字节集</font>】

将字节集转为[rcnb](https://github.com/rcnbapp)编码的文本。

注意，只有字节集才能进行rcnb编码。


### 进程ID


【进程ID】

返回当前进程的进程ID


### CPU使用(暂不可用)


【CPU使用】

返回当前进程的CPU占用百分比


### 内存使用(暂不可用)


【内存使用】

返回当前进程使用的内存（专用工作集），单位为MB


### 运行目录


【运行目录】

返回主进程对应的可执行文件所在目录，末尾有分隔符


### 图片信息


【图片信息@<font color="red">图片字节集</font>】

返回一个表示图片信息的RedLang对象，例如`{"宽":"640","格式":"png","高":"320"}`
支持的格式有:png、jpg、gif、webp、bmp


### 透视变换


【透视变换@<font color="red">图片字节集</font>@<font color="red">目标点</font>@<font color="red">原点</font>】

例子：`【透视变换@【变量@img】@【数组@0@0@330@0@330@330@0@330】@【数组@0@0@640@0@640@640@0@640】】`

上面例子将640x640的图片转为330x330的图片。每个数组里面的元素分别为左上，右上，右下，左下。

其中，<font color="red">原点</font>可以省略，默认为原图片的4个顶点。如：`【透视变换@【变量@img】@【数组@0@0@330@0@330@330@0@330】】`，效果一样。


### 图片叠加


【图片叠加@<font color="red">大图片字节集</font>@<font color="red">小图片字节集</font>@<font color="red">x</font>@<font color="red">y</font>】

将两张图片叠加起来，大图片放上面，小图片放下面，x，y为小图片的放置位置。


### 图片上叠加


【图片上叠加@<font color="red">大图片字节集</font>@<font color="red">小图片字节集</font>@<font color="red">x</font>@<font color="red">y</font>】

将两张图片叠加起来，大图片放下面，小图片放上面，x，y为小图片的放置位置。两张图片按透明度混合。


### 图片覆盖


【图片覆盖@<font color="red">大图片字节集</font>@<font color="red">小图片字节集</font>@<font color="red">x</font>@<font color="red">y</font>】

将两张图片覆盖，大图片放下面，小图片放上面，x，y为小图片的放置位置。和【图片上叠加】的区别是：图片覆盖不会把两张图片按透明度混合，而是会先直接清空大图片相应区域的数据，再叠加上小图片。


### 图片遮罩


【图片覆盖@<font color="red">大图片字节集</font>@<font color="red">小图片字节集</font>@<font color="red">x</font>@<font color="red">y</font>】

以小图片为遮罩层，对大图片的指定位置进行遮罩。规则：大图片透明度 = 最大透明度 - 小图片透明度


### 水平翻转


【水平翻转@<font color="red">图片字节集</font>】

将图片水平翻转


### 垂直翻转


【垂直翻转@<font color="red">图片字节集</font>】

将图片垂直翻转


### 图片旋转


【图片旋转@<font color="red">图片字节集</font>@<font color="red">旋转角度</font>】

将图片顺时针旋转指定角度


### 图片嵌字


【图片嵌字@<font color="red">图片字节集</font>@<font color="red">要嵌入的字</font>@<font color="red">x坐标</font>@<font color="red">y坐标</font>@<font color="red">字体大小</font>@<font color="red">rgba颜色数组</font>@<font color="red">字体名称</font>@<font color="red">字间距</font>@<font color="red">行间距</font>】

字间距和行间距默认为0。<font color="red">字体名称</font>可以是字节集。注意在非windows系统下，字体名称必须是字节集。

例如：
```
【定义变量@img1@【访问【取元素【取图片】@0】】】
【图片【图片嵌字@【变量@img1】@舔舔？@0@0@30@【数组@255@0@0@255】@宋体】】
```

### 创建图片


【创建图片@<font color="red">宽</font>@<font color="red">高</font>@<font color="red">rgba颜色数组</font>】

用于创建一张空白图片。

### 文字转图片

【文字转图片@<font color="red">图片的宽</font>@<font color="red">文字</font>@<font color="red">字体大小</font>@<font color="red">rgba颜色数组</font>@<font color="red">字体名称</font>@<font color="red">字间距</font>@<font color="red">行间距</font>】

字间距和行间距默认为0。<font color="red">字体名称</font>可以是字节集。注意在非windows系统下，字体名称必须是字节集。


### 图片大小调整


【图片大小调整@<font color="red">图片字节集</font>@<font color="red">调整后的宽度</font>@<font color="red">调整后的高度</font>】

调整图片大小


### 图片模糊


【图片模糊@<font color="red">图片字节集</font>@<font color="red">sigma</font>】

图片模糊使用高斯模糊，<font color="red">sigma</font>为高斯函数中的sigma参数，必须大于0（否则原样输出），越大越模糊。


### GIF合成


【GIF合成@<font color="red">图片字节集数组</font>@<font color="red">延时</font>】

合成gif，延时的单位为毫秒，用于确定gif的播放速度。


### GIF分解


【GIF分解@<font color="red">gif图片字节集</font>】

将gif分解为png图片字节集数组。


### 图片变圆


【图片变圆@<font color="red">图片字节集</font>】

将图片变成圆形，通常用于头像处理。


### 图片变灰


【图片变灰@<font color="red">图片字节集</font>】

将图片变成灰色，公式：Gray = (Red * 0.3 + Green * 0.589 + Blue * 0.11)


### 应用目录


【应用目录】

返回红色问答的应用目录，应用目录就是包的script.json所在目录，末尾有分隔符。


### 网页截图(仅windows可用)


【网页截图@<font color="red">网址</font>@<font color="red">CSS选择器</font>】

用无界面的浏览器打开网址，进行截图，返回图片字节集。若访问失败，返回空字节集。

CSS选择器可以选择截图的元素，省略CSS选择器表示截图整个网页

注意：使用此命令前，请自行安装[Chrome浏览器](https://www.google.cn/chrome/index.html)

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)


### 截屏(仅windows可用)

【截屏】

返回第一个显示器的屏幕截图。若没有显示器，则返回空字节集

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)


### 命令行

【命令行@<font color="red">文本</font>】

运行一个命令，返回命令输出。windows上，用cmd运行；linux上用sh运行。


### 启动

【启动@<font color="red">文本</font>】

启动一个软件。可以是文件（使用默认软件打开）、网页（默认浏览器打开）、软件。此命令返回空文本，且不会阻塞。


### 清空


【清空】

清空脚本之前的输出。此命令可以清除分页。


### 错误信息


【错误信息】

用于在<font color="red">脚本错误</font>事件中获取错误信息。


### 运行SQL


【运行SQL@<font color="red">文件名</font>@<font color="red">sql语句</font>@<font color="red">参数数组</font>】

使用sqlite3引擎来执行sql语句。其中，<font color="red">参数数组</font>可以省略。命令返回的是一个二维数组。

例子：`【转文本【运行sql@aaa.db@SELECT\ sqlite_version()】】`,返回`[["3.39.2"]]`。


### 压缩

【压缩@<font color="red">目录</font>@<font color="red">压缩文件</font>】

将<font color="red">目录</font>里面的文件添加到<font color="red">压缩文件</font>。压缩文件目前只支持7z。

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)


### 解压

【解压@<font color="red">压缩文件</font>@<font color="red">目录</font>】

将<font color="red">压缩文件</font>里面的文件解压到<font color="red">目录</font>。压缩文件目前只支持7z。

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)

### 去重

【去重@<font color="red">数组</font>】

使数组中不含重复元素。


### 打乱

【打乱@<font color="red">数组</font>】

打乱数组。


### 合并

【合并@<font color="red">数组</font>@<font color="red">中间符号</font>】

将中间符号插入数组中，返回文本。如`【合并【数组@1@3@6@3@4@4@5】@。】`，将会输出`1。3。6。3。4。4。5`。


### 系统代理(暂时仅windows可用)

【系统代理】

获取系统代理，若系统代理没有开启，则返回空文本。

### IE代理(仅windows可用)

【IE代理】

获取IE代理,若没有设置IE代理，则返回空文本。

系统代理和IE代理的区别是，IE代理可以在代理软件没有开启的时候获得。


### 骰

【骰@<font color="red">onedice参数</font>】

例子：`【骰@3d6】`

解释：`3d6`表示投出3颗6面骰子，下限是 3，上限是 3*6 即(3~18)。

onedice参数的写法参考[onedice](https://github.com/OlivOS-Team/onedice)。

**注意**：此命令**不支持**onedice中提到的[初等代数运算](https://github.com/OlivOS-Team/onedice/blob/main/inputdescribe.md#%E5%88%9D%E7%AD%89%E4%BB%A3%E6%95%B0)

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)


### 加锁

【加锁@<font color="red">锁名</font>】

若锁已经存在，则等待锁消失，然后再创建锁;若已存在的锁为当前脚本创建的，则什么也不做。

脚本结束时，当前脚本创建的锁名会自动删除。


### 解锁

【解锁@<font color="red">锁名</font>】

删除当前脚本创建的锁名。


### 补位

【补位@<font color="red">数字</font>@<font color="red">格式</font>】

将数字按格式进行补0或者确定精度。例如`【补位@12.07@000.0】`将会输出`012.1`；`【补位@12.07@0.000】`将会输出`12.070`。

### 运行WASM

【运行WASM@<font color="red">WASM字节集</font>@<font color="red">调用函数名</font>@<font color="red">宿主redlang接口函数名</font>@<font color="red">内存分配函数名</font>】

略。

**注意**：这是一个实验性质的api，用法很有可能会在之后的版本中发生变化(可能删除)


## QQ、频道相关命令说明


### 发送者QQ


【发送者QQ】


### 发送者ID


【发送者ID】

频道相关消息、事件中，为发送者的频道ID，其它地方等同于【发送者QQ】。


### 当前群号


【当前群号】

只能在群聊中使用


### 当前频道ID


【当前频道ID】

只能在频道中使用


### 当前子频道ID


【当前子频道ID】

只能在频道中使用


### 发送者昵称


【发送者昵称】


### 机器人QQ


【机器人QQ】


### 机器人ID


【机器人ID】

频道相关消息、事件中，为机器人的频道ID，其它地方等同于【机器人QQ】。


### 机器人名字


【机器人名字】

返回机器人名字。


### 发送者权限


【发送者权限】

只能在群聊中使用，返回<font color="red">群主</font>、<font color="red">管理</font>、<font color="red">群员</font>


### 发送者名片


【发送者名片】

只能在群聊中使用


### 发送者专属头衔


【发送者专属头衔】

只能在群聊中使用


### BOT权限


【BOT权限】

只能在群聊中使用，返回<font color="red">群主</font>、<font color="red">管理</font>、<font color="red">群员</font>


### 消息ID

【消息ID】

【消息ID@<font color="red">目标QQ</font>】

这个命令有两种形式：

第一种返回当前消息的消息ID

第二种返回目标QQ在当前群聊的历史消息ID数组（因内存条很贵，所以最多返回20条数据）。

### 获取消息

【获取消息@<font color="red">消息ID</font>】

根据消息ID获取消息，返回文本格式的消息，此命令需要onebot协议端支持。


### 撤回


【撤回@<font color="red">消息ID</font>或<font color="red">消息ID数组</font>】

若省略参数，则表示撤回当前消息，此命令需要onebot协议端支持。

### 禁言


【禁言@<font color="red">时间(秒)</font>】

禁言当前发消息的人，0表示解除禁言。


### 输出流


【输出流@<font color="red">内容</font>】

发送一条消息，然后返回消息ID，注意，输出流不支持【分页】。


### 艾特


【艾特】

at发送者，如果要at其它人，可以这么写：【艾特@<font color="red">其它人的ID</font>】

### 分页


【分页】

将一条消息分成两条消息发送。


### CQ码解析


【CQ码解析@<font color="red">CQ码文本</font>】

返回一个RedLang对象。类似这样:<font color="red">{"type":"at","qq":"1875159423"}</font>


### CQ反转义


【CQ反转义@<font color="red">内容</font>】

返回反转义后的文本。


### CQ码转义


【CQ码转义@<font color="red">内容</font>】

CQ码**内部**中的字符需要CQ码转义


### CQ转义


【CQ转义@<font color="red">内容</font>】

CQ码**外部**的字符需要CQ转义，以上三个命令的作用可以参考：[onebot字符格式消息转义规则](https://github.com/botuniverse/onebot-11/blob/master/message/string.md#%E8%BD%AC%E4%B9%89)


### 图片


【图片@<font color="red">文本或字节集</font>@<font color="red">不使用缓存</font>】

支持http/https链接，绝对地址，字节集。

当为http/https连接时，如果第二个参数为`假`，则不使用缓存，否则使用缓存。


### 语音


【语音@<font color="red">文本或字节集</font>@<font color="red">不使用缓存</font>】

支持http/https链接，绝对地址，字节集

当为http/https连接时，如果第二个参数为`假`，则不使用缓存，否则使用缓存。

注意，可能需要给onebot协议端安装ffmpeg，才能正常使用此功能。


### 子关键词


【子关键词】

<font color="red">模糊匹配</font>和<font color="red">完全匹配</font>没有子关键词

<font color="red">前缀匹配</font>的子关键词是关键词中的非前缀部分

<font color="red">正则匹配</font>的子关键词是一个二维数组，表示各个捕获


### 当前消息


【当前消息】

用于在群聊，私聊，频道消息事件中获取当前消息文本。


### 设置来源


【设置来源@<font color="red">键</font>@<font color="red">值</font>】

红色问答中脚本的执行输出，会自动根据来源发送到指定群、频道、用户。

支持的键包括：<font color="red">机器人ID</font>、<font color="red">机器人频道ID</font>、<font color="red">频道ID</font>、<font color="red">子频道ID</font>、<font color="red">群ID</font>、<font color="red">发送者ID</font>。

受此命令影响的命令有：文档待补充.jpg


### 事件内容


【事件内容】

onebot事件json对应的RedLang对象。


### 取艾特


【取艾特】

取出消息事件中被艾特的人，返回一个数组。


### 取图片


【取图片】

取出消息事件中的图片url数组。


### 输入流


【输入流@<font color="red">超时</font>】

返回当前群、当前发送者的下一条消息，可以在私聊和频道中使用。

超时参数的单位是毫秒，可以省略，默认15000。

若超时，返回空文本。

注意：输入流容易导致逻辑混乱，不推荐使用。




### 群输入流


【群输入流@<font color="red">超时</font>】

返回当前群的下一条消息和其发送者(一个RedLang对象)，可以在频道中使用。

返回示例:`{"发送者ID":"1875159423","消息":"你好，世界"}`

超时参数的单位是毫秒，可以省略，默认15000。

若超时，返回空对象。

注意：输入流容易导致逻辑混乱，不推荐使用。


### OB调用


【OB调用@<font color="red">onebot要求的json文本</font>】

此命令用于发送原始onebot数据，以调用框架不支持，以及尚未支持的功能。

此命令返回api调用返回的RedLang对象。若调用失败或超时，返回空文本。


### 读词库文件


【读词库文件@<font color="red">词库路径</font>】

词库兼容铃心自定义的词库，但是文件编码需要为utf-8，文件格式如下：


```
114
这是号码百事通的电话
514
1+1+4==6

早
早上好
你看看现在几点了
```

返回一个<font color="red">RedLang对象</font>，对象的键是关键词，对象的值是关键词对应的回答数组，类似如下形式:


```
{
  "114":["这是号码百事通的电话","514","1+1+4==6"],
  "早":["早上好","你看看现在几点了"],
}
```


下面是一个使用词库的例子:


```
【定义常量@我的词库@【读词库文件@【应用目录】词库.txt】】
【定义常量@正则匹配词库函数@
  【函数定义@
    【定义变量@keys@【取对象Key【常量@我的词库】】】
    【定义变量@keys_len@【取长度【变量@keys】】】
    【定义变量@i@0】
    【循环【变量@keys_len】@
      【隐藏【正则@【参数@1】@【取元素@【变量@keys】@【变量@i】】】】
        【判断@【计算@【取长度【传递】】==0】@真@
        【隐藏【取元素@【常量@我的词库】@【取元素@【变量@keys】@【变量@i】】】】
        【运行脚本【随机取@【传递】】】
        【返回】
     】
     【赋值变量@i@【计算@【变量@i】+1】】
    】
  】
】
【调用函数@正则匹配词库函数@【子关键词】】
```


实际使用的时候，建议把读词库文件和函数定义放到初始化事件中，更合理些。


## 事件关键词


如果触发类型为<font color="red">事件触发</font>，那么关键词应该为<font color="red">事件关键词</font>。

事件关键词由事件类型组成:

如戳一戳事件的关键词为<font color="red">notice:notify:poke</font>

群消息撤回事件的关键词为<font color="red">notice:group_recall</font>

支持的事件关键词可以参考[onebot文档](https://github.com/botuniverse/onebot-11)中有关事件的描述。

如果想要发送消息，请先使用命令【设置来源】

## CRON表达式

如果触发类型为<font color="red">CRON定时器</font>，那么关键词应该为<font color="red">CRON表达式</font>。

您可以在此处查看cron表达式的写法：[cron_百度百科](https://baike.baidu.com/item/cron/10952601)

在定时器事件中，如果想要发送消息，请先使用命令【设置来源】


## 框架初始化事件


如果触发类型是<font color="red">框架初始化</font>，那么，脚本内容会在框架启动或点击保存按钮的时候执行一次。

此时可能还未连接onebot实现端，不一定能正常调用bot的各个接口。一般用于定义一些常量。

## 脚本错误事件


如果触发类型是<font color="red">脚本错误</font>，那么，脚本内容会在脚本发生错误时触发，可以通过命令【错误信息】来获取发生的错误。

QQ相关的信息，会复制到错误事件中；在错误事件中再次发生错误，不会再次触发错误事件。

## 包管理说明(未完全完成)

注意：创建或删除包后，需要重启才能生效。

### 默认包

`默认包`目录位于`【运行目录】plus_dir/default_pkg_dir/`。

红色问答始终存在唯一的默认包。

### 附加包

`附加包`目录位于`【运行目录】plus_dir/pkg_dir/xxxx/`。红色问答可以有多个附加包，`xxxx为包名`。

要手动增加一个附加包，在`【运行目录】plus_dir/pkg_dir/`目录下建一个文件夹即可，注意，文件夹的名字**不可以**为`默认包`。

### 包资源

不同的包中：自定义命令相互隔离，**暂时**还不能互相访问。包的【应用目录】，为各包的脚本文件`script.json`所在目录。

### 包描述文件(暂未实现，无视即可)

在各包的`script.json`所在的目录下，有一个名为`app.json`的包描述文件，里面包含包的基本信息:版本号、作者、描述、依赖。