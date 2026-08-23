use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock, RwLock,
};

use crate::browserbridge::BrowserService;
use redreply_adapter::BotRegistry;

pub(crate) struct ServiceHub {
    runtime: OnceLock<Arc<tokio::runtime::Runtime>>,
    bot_registry: OnceLock<BotRegistry>,
    proxy_cache: tokio::sync::Mutex<(Option<String>, u64)>,
    pgsql_enabled: AtomicBool,
    pgsql_client: tokio::sync::RwLock<Option<Arc<tokio_postgres::Client>>>,
    mqtt_client: Mutex<Option<rumqttc::v5::Client>>,
    mqtt_client_id: Mutex<String>,
    mqtt_remote_clients: Mutex<Vec<String>>,
    mqtt_echo_map: Mutex<HashMap<String, std::sync::mpsc::Sender<serde_json::Value>>>,
    auto_close: Mutex<bool>,
    default_font: RwLock<Option<Vec<u8>>>,
    browser: BrowserService,
}

impl ServiceHub {
    pub(crate) fn new() -> Self {
        Self {
            runtime: OnceLock::new(),
            bot_registry: OnceLock::new(),
            proxy_cache: tokio::sync::Mutex::new((None, 0)),
            pgsql_enabled: AtomicBool::new(false),
            pgsql_client: tokio::sync::RwLock::new(None),
            mqtt_client: Mutex::new(None),
            mqtt_client_id: Mutex::new(String::new()),
            mqtt_remote_clients: Mutex::new(Vec::new()),
            mqtt_echo_map: Mutex::new(HashMap::new()),
            auto_close: Mutex::new(false),
            default_font: RwLock::new(None),
            browser: BrowserService::new(),
        }
    }

    pub(crate) fn runtime(&self) -> Arc<tokio::runtime::Runtime> {
        self.runtime
            .get_or_init(|| Arc::new(tokio::runtime::Runtime::new().unwrap()))
            .clone()
    }

    pub(crate) fn bot_registry(&self) -> &BotRegistry {
        self.bot_registry.get_or_init(BotRegistry::new)
    }

    pub(crate) fn browser(&self) -> &BrowserService {
        &self.browser
    }

    pub(crate) fn proxy_cache(&self) -> &tokio::sync::Mutex<(Option<String>, u64)> {
        &self.proxy_cache
    }

    pub(crate) fn pgsql_enabled(&self) -> bool {
        self.pgsql_enabled.load(Ordering::SeqCst)
    }

    pub(crate) fn set_pgsql_enabled(&self, enabled: bool) {
        self.pgsql_enabled.store(enabled, Ordering::SeqCst);
    }

    pub(crate) async fn pgsql_client(&self) -> Option<Arc<tokio_postgres::Client>> {
        self.pgsql_client.read().await.clone()
    }

    pub(crate) async fn set_pgsql_client(&self, client: Option<Arc<tokio_postgres::Client>>) {
        *self.pgsql_client.write().await = client;
    }

    pub(crate) fn mqtt_client(&self) -> Option<rumqttc::v5::Client> {
        self.mqtt_client.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_mqtt_client(&self, client: Option<rumqttc::v5::Client>) {
        *self.mqtt_client.lock().unwrap_or_else(|e| e.into_inner()) = client;
    }

    pub(crate) fn mqtt_client_id(&self) -> String {
        self.mqtt_client_id.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_mqtt_client_id(&self, client_id: String) {
        *self.mqtt_client_id.lock().unwrap_or_else(|e| e.into_inner()) = client_id;
    }

    pub(crate) fn set_mqtt_remote_clients(&self, clients: Vec<String>) {
        *self.mqtt_remote_clients.lock().unwrap_or_else(|e| e.into_inner()) = clients;
    }

    pub(crate) fn mqtt_remote_clients(&self) -> Vec<String> {
        self.mqtt_remote_clients.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn add_mqtt_echo(
        &self,
        echo: String,
        sender: std::sync::mpsc::Sender<serde_json::Value>,
    ) {
        self.mqtt_echo_map.lock().unwrap_or_else(|e| e.into_inner()).insert(echo, sender);
    }

    pub(crate) fn remove_mqtt_echo(&self, echo: &str) {
        self.mqtt_echo_map.lock().unwrap_or_else(|e| e.into_inner()).remove(echo);
    }

    pub(crate) fn send_mqtt_echo(
        &self,
        echo: &str,
        value: serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let sender = self.mqtt_echo_map.lock().unwrap_or_else(|e| e.into_inner()).get(echo).cloned();
        if let Some(sender) = sender {
            sender.send(value)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn set_auto_close(&self, auto_close: bool) {
        *self.auto_close.lock().unwrap_or_else(|e| e.into_inner()) = auto_close;
    }

    pub(crate) fn auto_close(&self) -> bool {
        *self.auto_close.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn default_font(&self) -> Option<Vec<u8>> {
        self.default_font.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_default_font(&self, font: Vec<u8>) {
        *self.default_font.write().unwrap_or_else(|e| e.into_inner()) = Some(font);
    }
}

impl Default for ServiceHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceHub;

    #[test]
    fn service_state_round_trips() {
        let services = ServiceHub::new();
        assert!(!services.auto_close());
        services.set_auto_close(true);
        assert!(services.auto_close());
        services.set_default_font(vec![1, 2, 3]);
        assert_eq!(services.default_font(), Some(vec![1, 2, 3]));
    }
}
