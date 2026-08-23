use std::{sync::Arc, time::Duration};

use crate::{cqapi::cq_add_log_w, runtime, runtime_context};
pub use redreply_adapter::BotConnectTrait;
use redreply_adapter::{
    email::EmailConnect,
    host::{ensure_host, AdapterHost},
    kook::KookConnect,
    onebot11::OneBot11Connect,
    onebot115::OneBot115Connect,
    qqguild_private::QQGuildPrivateConnect,
    qqguild_public::QQGuildPublicConnect,
    satoriv1::Satoriv1Connect,
    telegram::TeleTramConnect,
    yunhuv1::Yunhuv1Connect,
    AdapterResult,
};

struct RedReplyAdapterHost;

impl AdapterHost for RedReplyAdapterHost {
    fn log(&self, msg: &str) {
        let _ = crate::cqapi::cq_add_log(msg);
    }

    fn warn(&self, msg: &str) {
        let _ = crate::cqapi::cq_add_log_w(msg);
    }

    fn dispatch_event(&self, event_json: &str) -> AdapterResult<()> {
        crate::cqevent::do_1207_event(event_json)
            .map(|_| ())
            .map_err(|err| err.to_string().into())
    }

    fn app_dir(&self) -> AdapterResult<String> {
        crate::cqapi::cq_get_app_directory1().map_err(|err| err.to_string().into())
    }

    fn all_to_silk(&self, input: &[u8]) -> AdapterResult<Vec<u8>> {
        crate::mytool::all_to_silk::all_to_silk(input).map_err(|err| err.to_string().into())
    }
}

fn ensure_adapter_host() {
    ensure_host(|| Arc::new(RedReplyAdapterHost));
}

fn get_ws_urls() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    crate::config_manager::ws_urls()
}

/// 连接失败退避状态：连续快速失败时指数拉长重试间隔，
/// 防止对不可达/拒绝服务的远端形成固定 5 秒一轮的重连风暴（日志洪水+接口配额消耗）
#[derive(Clone)]
struct RetryState {
    fail_count: u32,
    next_attempt: std::time::Instant,
}

static RETRY_STATE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, RetryState>>,
> = std::sync::OnceLock::new();

fn retry_state() -> &'static std::sync::Mutex<std::collections::HashMap<String, RetryState>> {
    RETRY_STATE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn can_attempt_connect_now(url: &str) -> bool {
    let map = match retry_state().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match map.get(url) {
        Some(state) => state.next_attempt <= std::time::Instant::now(),
        None => true,
    }
}

fn record_connect_failure(url: &str) {
    const BASE_RETRY_DELAY_SECS: u64 = 5;
    const MAX_RETRY_DELAY_SECS: u64 = 60;
    let mut map = match retry_state().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let state = map.entry(url.to_owned()).or_insert(RetryState {
        fail_count: 0,
        next_attempt: std::time::Instant::now(),
    });
    // 5s → 10s → 20s → 40s → 封顶 60s
    let delay = (BASE_RETRY_DELAY_SECS << state.fail_count.min(4)).min(MAX_RETRY_DELAY_SECS);
    state.fail_count = state.fail_count.saturating_add(1);
    state.next_attempt = std::time::Instant::now() + Duration::from_secs(delay);
}

fn record_connect_success(url: &str) {
    let mut map = match retry_state().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    map.remove(url);
}

pub async fn call_api(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    json: &mut serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    ensure_adapter_host();
    if let Some(ret) = runtime_context()
        .services
        .bot_registry()
        .call_api(platform, self_id, passive_id, json)
        .await?
    {
        return Ok(ret);
    }
    cq_add_log_w(&format!(
        "no such bot:platform:`{platform}`,self_id:`{self_id}`"
    ))
    .unwrap();
    Ok(serde_json::json!(""))
}

pub fn do_conn_event() -> Result<i32, Box<dyn std::error::Error>> {
    ensure_adapter_host();
    std::thread::spawn(move || {
        loop {
            let config_urls = match get_ws_urls() {
                Ok(urls) => urls,
                Err(err) => {
                    let _ = cq_add_log_w(&format!("读取连接配置失败，将重试: {}", err));
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
            };

            runtime().block_on(async move {
                // 删除所有不在列表中的url和死去的bot
                {
                    let earse_urls = runtime_context()
                        .services
                        .bot_registry()
                        .removable_urls(&config_urls)
                        .await;
                    // 移除这些bot
                    for url in &earse_urls {
                        runtime_context()
                            .services
                            .bot_registry()
                            .disconnect_and_remove(url)
                            .await;
                    }
                    // 有bot移除，等1秒再进行连接
                    if !earse_urls.is_empty() {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                // 清理已从配置中移除的 URL 的退避状态，防止表无限增长
                {
                    let url_set: std::collections::HashSet<&String> = config_urls.iter().collect();
                    let mut map = match retry_state().lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    map.retain(|url, _| url_set.contains(url));
                }
                // 连接未在bot_map中的url（try_begin_connect 保证同一 URL 不会并发重复连接）
                for url in &config_urls {
                    // 处于退避冷却期的 URL 本轮跳过（不领取 connecting 标记）
                    if !can_attempt_connect_now(url) {
                        continue;
                    }
                    if !runtime_context()
                        .services
                        .bot_registry()
                        .try_begin_connect(url)
                        .await
                    {
                        continue;
                    }
                    let url_t = url.clone();
                    runtime().spawn(async move {
                        // 无论连接成功、失败还是 panic，任务退出时都必须清除
                        // connecting 标记，否则该 URL 将永远失去重连机会
                        let _connect_guard = scopeguard::guard(url_t.clone(), |url| {
                            runtime().spawn(async move {
                                runtime_context()
                                    .services
                                    .bot_registry()
                                    .end_connect(&url)
                                    .await;
                            });
                        });
                        let connect_ok = async {
                            if url_t.starts_with("ws://") || url_t.starts_with("wss://") {
                                let mut bot = OneBot11Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到onebot失败:{},{}", url_t, err))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("ovo://") {
                                let mut bot = OneBot115Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到ovo失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("satori://") {
                                let mut bot = Satoriv1Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到satori失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("qqguild_private://") {
                                let mut bot = QQGuildPrivateConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!(
                                        "连接到qqguild_private失败:{url_t},{err:?}"
                                    ))
                                    .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("qqguild_public://") {
                                let mut bot = QQGuildPublicConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!(
                                        "连接到qqguild_public失败:{url_t},{err:?}"
                                    ))
                                    .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("kook://") {
                                let mut bot = KookConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到kook失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("email://") {
                                let mut bot = EmailConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到email失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("telegram://") {
                                let mut bot = TeleTramConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到telegram失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else if url_t.starts_with("yunhu://") {
                                let mut bot = Yunhuv1Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到yunhu失败:{url_t},{err:?}"))
                                        .unwrap();
                                    false
                                } else {
                                    runtime_context()
                                        .services
                                        .bot_registry()
                                        .insert(url_t.clone(), bot)
                                        .await;
                                    true
                                }
                            } else {
                                false
                            }
                        }
                        .await;
                        if !connect_ok {
                            record_connect_failure(&url_t);
                            runtime_context()
                                .services
                                .bot_registry()
                                .end_connect(&url_t)
                                .await;
                        } else {
                            record_connect_success(&url_t);
                        }
                    });
                }
            });

            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    Ok(0)
}
