use std::collections::HashMap;
use std::sync::Mutex;

/// uid/群/被动消息映射的存活时间：长期未访问的条目在读取时清理，
/// 防止这三张表随时间无限增长（跨平台会话多的实例每天可累积数十万条）
const ENTRY_TTL_SECS: u64 = 24 * 60 * 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) struct OneBotStore {
    next_id: Mutex<i64>,
    red_id_map: Mutex<HashMap<i32, (String, u64)>>,
    red_uid_map: Mutex<HashMap<i64, (String, u64)>>,
    groups_map: Mutex<HashMap<i64, (i64, String, u64)>>,
    passive_id_map: Mutex<HashMap<i64, (String, u64)>>,
}

impl OneBotStore {
    pub(crate) fn new() -> Self {
        Self {
            next_id: Mutex::new(1207),
            red_id_map: Mutex::new(HashMap::new()),
            red_uid_map: Mutex::new(HashMap::new()),
            groups_map: Mutex::new(HashMap::new()),
            passive_id_map: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn red_id_to_ob(&self, red_id: &str, now: u64) -> i32 {
        let mut red_ids = self.red_id_map.lock().unwrap();
        red_ids.retain(|_, (_, timestamp)| *timestamp + 60 * 2 >= now);
        for (id, entry) in red_ids.iter_mut() {
            if entry.0 == red_id {
                entry.1 = now;
                return *id;
            }
        }

        let mut next_id = self.next_id.lock().unwrap();
        *next_id = (1103515245_i64 * *next_id + 12345_i64) % 2147483648_i64;
        let ob_id = *next_id as i32;
        red_ids.insert(ob_id, (red_id.to_owned(), now));
        ob_id
    }

    pub(crate) fn ob_id_to_red(&self, ob_id: i32, now: u64) -> Option<String> {
        let mut red_ids = self.red_id_map.lock().unwrap();
        red_ids.retain(|_, (_, timestamp)| *timestamp + 60 * 2 >= now);
        red_ids.get(&ob_id).map(|(value, _)| value.clone())
    }

    pub(crate) fn remember_uid(&self, ob_id: i64, red_id: String) {
        let now = now_secs();
        let mut map = self.red_uid_map.lock().unwrap();
        map.retain(|_, (_, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.insert(ob_id, (red_id, now));
    }

    pub(crate) fn red_uid(&self, ob_id: i64) -> Option<String> {
        let now = now_secs();
        let mut map = self.red_uid_map.lock().unwrap();
        map.retain(|_, (_, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.get(&ob_id).map(|(value, _)| value.clone())
    }

    pub(crate) fn remember_group(&self, group_id: i64, original_group_id: i64, groups_id: String) {
        let now = now_secs();
        let mut map = self.groups_map.lock().unwrap();
        map.retain(|_, (_, _, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.insert(group_id, (original_group_id, groups_id, now));
    }

    pub(crate) fn group(&self, group_id: i64) -> Option<(i64, String)> {
        let now = now_secs();
        let mut map = self.groups_map.lock().unwrap();
        map.retain(|_, (_, _, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.get(&group_id)
            .map(|(original, groups_id, _)| (*original, groups_id.clone()))
    }

    pub(crate) fn remember_passive_id(&self, id: i64, message_id: String) {
        let now = now_secs();
        let mut map = self.passive_id_map.lock().unwrap();
        map.retain(|_, (_, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.insert(id, (message_id, now));
    }

    pub(crate) fn passive_id(&self, id: i64) -> Option<String> {
        let now = now_secs();
        let mut map = self.passive_id_map.lock().unwrap();
        map.retain(|_, (_, timestamp)| *timestamp + ENTRY_TTL_SECS >= now);
        map.get(&id).map(|(value, _)| value.clone())
    }
}

impl Default for OneBotStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::OneBotStore;

    #[test]
    fn message_ids_round_trip_with_ttl_refresh() {
        let store = OneBotStore::new();
        let id = store.red_id_to_ob("red-id", 100);
        assert_eq!(store.ob_id_to_red(id, 101).as_deref(), Some("red-id"));
        assert_eq!(store.red_id_to_ob("red-id", 102), id);
    }

    #[test]
    fn uid_and_group_maps_round_trip() {
        let store = OneBotStore::new();
        store.remember_uid(42, "red-uid".to_owned());
        assert_eq!(store.red_uid(42).as_deref(), Some("red-uid"));
        store.remember_group(7, 8, "groups-id".to_owned());
        assert_eq!(store.group(7), Some((8, "groups-id".to_owned())));
        store.remember_passive_id(9, "msg-id".to_owned());
        assert_eq!(store.passive_id(9).as_deref(), Some("msg-id"));
    }
}
