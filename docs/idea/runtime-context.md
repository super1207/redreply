# RuntimeContext 设计

## 目的

红色问答早期使用多个 `lazy_static` 保存进程状态。它可以工作，但状态的所有权分散在 `lib.rs`、HTTP、适配器、MQTT 和数据库模块中，导致测试需要依赖进程级状态，也容易让不同模块直接修改不属于自己的缓存。

本设计采用渐进式迁移：先引入 `RuntimeContext` 作为运行时状态的归属，再按职责分批迁移旧状态。迁移期间保留原有公共函数，避免一次修改所有调用点。

## 当前结构

```text
config_manager
└── 唯一 config.json 进程缓存

RuntimeContext
├── ScriptStore
│   ├── 脚本缓存和包名
│   ├── 包级常量和临时常量
│   ├── 自定义命令
│   └── 全局过滤器和初始化代码缓存
├── SyncPrimitives
│   ├── 包级脚本锁及条件变量
│   ├── 信号等待及条件变量
│   └── 文件锁及条件变量
├── ControlPlane
│   ├── 退出和包加载标记
│   ├── 运行脚本登记和等待
│   └── 脚本运行 ID、启动时间
├── SessionStore
│   ├── 消息 ID 映射和输入流
│   ├── 脚本输出关联
│   └── 历史日志缓冲
├── StatusStore
│   └── 统计增量缓存和刷新水位
├── SchedulerStore
│   ├── CRON 时间水位
│   └── 延迟触发队列
├── OneBotStore
│   ├── 消息 ID 映射
│   ├── 用户/群组 ID 映射
│   └── 被动消息映射
└── ServiceHub
    ├── Tokio runtime
    ├── 自动退出看门状态
    ├── 默认字体、代理和浏览器服务
    ├── BotRegistry、MQTT 和 PostgreSQL
    └── HTTP/Python/OneBot 会话
```

入口位于 `src/runtime_context.rs`：

- `RuntimeContext`：运行时上下文的根对象。
- `config_manager`：配置文件读写和唯一进程缓存。
- `runtime_context()`：获取进程级上下文。

当前上下文使用 `OnceLock<Arc<RuntimeContext>>` 保存。它仍然是进程级唯一实例，但状态的职责已经从业务模块顶层静态变量移动到明确的 Store 中。
`RuntimeContext`、Store 类型和上下文字段均为 crate 内部实现，不作为主 crate 的公共 API 暴露。

## Batch A 已迁移状态

### 配置读取

所有配置字段都由 `config_manager::read_config()` 读取。该函数缓存完整的
`serde_json::Value`，密码、禁用包、消息时间阈值和适配器 URL 不再各自维护一份字段级缓存。

### 禁用包

以下接口名称和行为保持不变：

- `get_ban_pkgs()`
- `set_ban_pkg()`
- `rename_ban_pkg()`

配置写入由 `config_manager` 内部的写锁串行化，避免多个请求同时读改写 `config.json`。
写入成功后同步更新唯一配置缓存，不改变包禁用的业务语义。

### 消息时间阈值

事件处理通过 `skip_msg_time()` 从中央配置缓存读取，初始化仍由 `init_global_var()` 完成。

### 适配器 URL 缓存

`botconn` 原有的连接变更标记和 URL 缓存已移除。

- `set_ws_urls()` 和 `set_disabled_ws_urls()` 写入配置后，会同步更新中央配置缓存。
- `get_ws_urls()` 的连接循环入口仍然保留。
- `disabled_ws_urls` 按原有索引规则过滤。

## Batch B 已迁移状态

`ScriptStore` 已接管以下脚本相关状态：

- 脚本缓存和包名集合。
- 包级常量、临时常量及过期清理。
- 自定义命令文本和命令函数。
- 全局过滤器、全局初始化代码缓存。

`script_manager.rs`、`redlang` 和 HTTP 插件安装流程通过 `RuntimeContext` 访问这些状态。
脚本读取、包名读取和脚本内存清理由 `script_manager` 和 `ScriptStore` 正式 API 提供，
不再通过 crate 根函数转发。
`ScriptStore` 不负责脚本生命周期控制，加载标记、运行脚本登记和退出等待仍沿用原有控制状态。

`SyncPrimitives` 已接管以下同步状态：

- 包级脚本锁和锁等待唤醒。
- 信号注册、发送、超时和退出取消。
- 文件锁和等待超时诊断。

锁的等待时长、唤醒方式和现有锁顺序保持不变。`SyncPrimitives` 只负责同步原语，
不负责包退出标记、运行脚本登记或脚本生命周期。

## Batch C 已迁移状态

`ControlPlane` 已接管退出标记、包退出/加载标记、运行脚本登记、脚本退出等待、
运行 ID 和启动时间。原有运行表锁顺序及脚本 scopeguard 行为保持不变。

`SessionStore` 已接管消息 ID 映射、输入流、脚本输出关联和历史日志缓冲。输入流
发送仍在释放 Store 读锁前完成，脚本输出和日志不会在持有 Store 锁时执行外部调用。

`ServiceHub` 已接管 Tokio runtime、自动退出状态和默认字体缓存。runtime 延迟创建，
保留原有按需初始化行为；Tokio worker 内的 `Handle::try_current` 防死锁逻辑保持不变。

协议标识和 Python bootstrap 已改为编译期 `const`，不再使用进程级可变字符串缓存。

## Batch D 已迁移状态

`StatusStore` 已接管统计增量缓存和刷新水位，`SchedulerStore` 已接管 CRON 水位和延迟
触发队列，`OneBotStore` 已接管 OneBot 协议 ID 映射。`ServiceHub` 还接管了 BotRegistry、
MQTT/PostgreSQL 客户端、GitHub 代理缓存和浏览器 DLL 服务。

适配器中的 echo、HTTP 缓存、审核等待和 Telegram 用户名映射已经改为连接实例状态，
不再使用模块级 `lazy_static`。

## 尚未迁移

当前仅保留两类进程根入口：`RuntimeContext` 的 `OnceLock`，以及适配器 Host 的
`OnceLock` 回调注册。它们分别是应用状态根和主 crate 到适配器的注入边界，不再新增同类
模块级缓存。

特别地，本次迁移不修改 `wait_for_quit()` 或现有脚本退出等待机制。

## 后续批次

### Batch E：拆分宿主编排和切换 Store API

Batch E 已完成第一阶段：

- `config_manager.rs`：配置文件读写、中央配置缓存、`StartupConfig` 解析和配置刷新入口。
- `script_manager.rs`：脚本加载、包保存/改名/删除、脚本备份和全局脚本代码。
- `lifecycle.rs`：脚本运行登记、退出等待和进程退出流程。
- `resource_manager.rs`：资源释放、Python bootstrap、版本、导航和文件锁 API。
- `lib.rs`：只保留类型、常量、初始化编排和对外兼容 facade。

事件、CRON、RedLang、HTTP、MQTT 和 PostgreSQL 模块已经优先直接依赖所属模块或
`RuntimeContext` Store。原来位于 crate 根的管理函数兼容入口已经删除，调用者应直接使用
`config_manager`、`script_manager`、`lifecycle` 和 `resource_manager` 模块；`lib.rs` 的
初始化编排也直接调用这些管理模块。

### 配置读取和缓存策略

所有 `config.json` 读取都经过 `config_manager::read_config()`。首次调用从磁盘读取，后续
调用返回同一份进程级缓存的副本。`init_global_var()` 在启动阶段校验配置；
`StartupConfig` 只是从中央配置值解析出的结构化视图，不再由 `RuntimeContext` 单独保存。

外部直接修改 `config.json` 不会自动改变运行中的配置。显式调用
`refresh_cached_config()` 会重新读取磁盘并在成功后替换完整缓存；读取或解析失败时保留
旧缓存。通过 `set_*` API 写入配置时，写盘和缓存更新在同一写锁保护下完成。

全局过滤器和全局初始化代码属于 `ScriptStore` 的 SQLite-backed 脚本状态，不属于
`config.json`；应通过 `set_gobal_filter_code()` / `set_gobal_init_code()` 更新，读取结果
在 `ScriptStore` 中缓存。

Batch E 的根级兼容入口清理已完成。后续新增代码不应再向 `lib.rs` 添加管理函数 facade；
需要扩展行为时，应在对应管理模块或 Store 上增加 API。

## 约束

1. 新增状态不应再放入 `lib.rs` 顶层 `lazy_static!`。
2. 业务模块应通过 `config_manager` API 访问配置，不应直接读写配置缓存的内部锁。
3. 缓存失效必须是显式动作，并在接口文档中说明。
4. 迁移只改变状态归属，不顺便修改事件匹配、脚本生命周期或退出流程。
5. 每个 Store 都应提供不依赖 HTTP、Bot 或真实数据库的单元测试入口。
