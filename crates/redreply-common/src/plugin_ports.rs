//! 插件端口类型：宿主与插件之间共享的服务接口。
//!
//! 插件 crate 实现这些 trait 并注册为服务；宿主（脚本命令等）按服务名取用，
//! 从而与具体插件实现解耦。

use std::sync::Arc;

/// TTS 服务端口。
pub trait TtsService: Send + Sync {
    fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String>;
}

/// TTS 服务句柄（以具体类型注册，内部持有动态实现）。
pub struct TtsServiceHandle(pub Arc<dyn TtsService>);

impl TtsServiceHandle {
    pub fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.0.synthesize(text, player, proxy)
    }
}

/// 浏览器服务端口。
pub trait BrowserBridgeService: Send + Sync {
    /// 执行浏览器请求（JSON 协议见 `redreply_browser` crate）。
    fn execute_json(&self, request: &str) -> std::result::Result<Vec<u8>, String>;
}

/// 浏览器服务句柄。
pub struct BrowserServiceHandle(pub Arc<dyn BrowserBridgeService>);

impl BrowserServiceHandle {
    pub fn execute_json(&self, request: &str) -> std::result::Result<Vec<u8>, String> {
        self.0.execute_json(request)
    }
}

/// MQTT 服务端口：跨实例事件发布与远程 API 调用。
pub trait MqttService: Send + Sync {
    /// 是否已连上 broker。
    fn connected(&self) -> bool;

    /// 把本实例事件发布到 MQTT（来自 MQTT 的事件应自行忽略）。
    fn publish_event(&self, event: &serde_json::Value) -> std::result::Result<(), String>;

    /// 调用远端实例 API，返回 OneBot 原始 JSON 字符串。
    fn call_remote(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        payload: serde_json::Value,
        remote_id: &str,
    ) -> std::result::Result<String, String>;
}

/// MQTT 服务句柄。
pub struct MqttServiceHandle(pub Arc<dyn MqttService>);

impl MqttServiceHandle {
    pub fn connected(&self) -> bool {
        self.0.connected()
    }

    pub fn publish_event(
        &self,
        event: &serde_json::Value,
    ) -> std::result::Result<(), String> {
        self.0.publish_event(event)
    }

    pub fn call_remote(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        payload: serde_json::Value,
        remote_id: &str,
    ) -> std::result::Result<String, String> {
        self.0.call_remote(platform, self_id, passive_id, payload, remote_id)
    }
}

/// PostgreSQL 服务端口：消息落库。
pub trait PgService: Send + Sync {
    /// 已启用并连接 PostgreSQL 时写入一条消息事件。
    fn insert_message(&self, root: &serde_json::Value) -> std::result::Result<(), String>;
}

/// PostgreSQL 服务句柄。
pub struct PgServiceHandle(pub Arc<dyn PgService>);

impl PgServiceHandle {
    pub fn insert_message(
        &self,
        root: &serde_json::Value,
    ) -> std::result::Result<(), String> {
        self.0.insert_message(root)
    }
}


