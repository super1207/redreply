//! 机器人 API 调用出口：统一走框架适配器管理器（`AdapterManager`）。
//!
//! 旧版的连接管理（`do_conn_event` + `BotRegistry`）已由框架适配器特殊层接管。

/// 兼容旧路径：OneBot 风格适配器接口。
pub use redreply_adapter::BotConnectTrait;

/// 调用平台 API。
///
/// - 返回 `Ok(值)`：适配器返回值（无可用连接时为字符串空值，与旧版语义一致）；
/// - 返回 `Err`：框架未启动等框架级错误。
pub async fn call_api(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    json: &mut serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let Some(framework) = crate::framework() else {
        return Err("框架尚未启动".into());
    };
    match framework
        .adapters()
        .call_api(platform, self_id, passive_id, json)
        .await
    {
        Ok(Some(ret)) => Ok(ret),
        Ok(None) => {
            let _ = crate::cqapi::cq_add_log_w(&format!(
                "no such bot:platform:`{platform}`,self_id:`{self_id}`"
            ));
            Ok(serde_json::json!(""))
        }
        Err(error) => Err(error.to_string().into()),
    }
}
