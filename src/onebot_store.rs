use std::collections::HashMap;
use std::sync::Mutex;

pub(crate) struct OneBotStore {
    next_id: Mutex<i64>,
    red_id_map: Mutex<HashMap<i32, (String, u64)>>,
    red_uid_map: Mutex<HashMap<i64, String>>,
    groups_map: Mutex<HashMap<i64, (i64, String)>>,
    passive_id_map: Mutex<HashMap<i64, String>>,
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
        self.red_uid_map.lock().unwrap().insert(ob_id, red_id);
    }

    pub(crate) fn red_uid(&self, ob_id: i64) -> Option<String> {
        self.red_uid_map.lock().unwrap().get(&ob_id).cloned()
    }

    pub(crate) fn remember_group(&self, group_id: i64, original_group_id: i64, groups_id: String) {
        self.groups_map
            .lock()
            .unwrap()
            .insert(group_id, (original_group_id, groups_id));
    }

    pub(crate) fn group(&self, group_id: i64) -> Option<(i64, String)> {
        self.groups_map.lock().unwrap().get(&group_id).cloned()
    }

    pub(crate) fn remember_passive_id(&self, id: i64, message_id: String) {
        self.passive_id_map.lock().unwrap().insert(id, message_id);
    }

    pub(crate) fn passive_id(&self, id: i64) -> Option<String> {
        self.passive_id_map.lock().unwrap().get(&id).cloned()
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
}
