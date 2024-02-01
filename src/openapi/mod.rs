use std::{collections::{HashMap, LinkedList}, sync::{Arc, Mutex}};

lazy_static! {
    static ref G_EVENT:std::sync::Mutex<HashMap<String,(i64,LinkedList<Arc<serde_json::Value>>)>> = Mutex::new(HashMap::new());
}

pub fn insert_event(json:&serde_json::Value) {
    let json_rc = Arc::new(json.to_owned());
    let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let mut lk = G_EVENT.lock().unwrap();
    // 移除超时的flag
    let mut to_remove_flags = vec![];
    for (flag,(tm,_data)) in &*lk{
        if now_time > tm + 2 * 60 { // 2分钟超时
            to_remove_flags.push(flag.to_owned());
        }
    }
    for flag in to_remove_flags {
        lk.remove(&flag);
    }
    // 插入事件
    for (_flag,(_tm,data)) in &mut *lk{
        data.push_back(json_rc.clone());
    }
}

pub fn get_event(flag:&str) -> serde_json::Value {
    let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let mut lk = G_EVENT.lock().unwrap();
    let mut to_ret = vec![];
    if lk.contains_key(flag) {
        let d = lk.get_mut(flag).unwrap();
        for it in &d.1 {
            to_ret.push((**it).clone());
        }
        d.1.clear();
        d.0 = now_time;
    }else {
        lk.insert(flag.to_owned(),(now_time,LinkedList::new()) );
        to_ret = vec![];
    }
    let to_ret = serde_json::json!({
        "retcode":0,
        "data":to_ret,
        "nonce":crate::redlang::get_random().unwrap().to_string()
    });
    to_ret
}