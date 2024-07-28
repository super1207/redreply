use std::collections::{HashMap, HashSet};

use crate::mytool::{read_json_str, str_msg_to_arr};

fn gen_lcg_id() -> i32 {
    lazy_static!{
        static ref X:std::sync::Mutex<i64> = std::sync::Mutex::new(1207);
    }
    let mut lx = X.lock().unwrap();
    (*lx) = (1103515245i64 * (*lx) + 12345i64) % 2147483648i64;
    return *lx as i32;
}

lazy_static!{
    static ref OB_RED_ID_MAP:std::sync::Mutex<HashMap<i32,(String,u64)>> = std::sync::Mutex::new(HashMap::new());
    static ref OB_RED_UID_MAP:std::sync::Mutex<HashMap<i64,String>> = std::sync::Mutex::new(HashMap::new());
    static ref OB_GROUPS_MAP:std::sync::Mutex<HashMap<i64,(i64,String)>> = std::sync::Mutex::new(HashMap::new());
    static ref PASSIVE_ID_MAP:std::sync::Mutex<HashMap<i64,String>> = std::sync::Mutex::new(HashMap::new());
}

pub fn red_id_to_ob(red_id:&str) -> i32 {
    let curr_tm = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut lk = OB_RED_ID_MAP.lock().unwrap();
    let mut to_remove = vec![];
    for (k,(_v,tm)) in &*lk {
        if tm + 60*2 < curr_tm {
            to_remove.push(k.to_owned());
        }
    }
    for k in to_remove {
        (*lk).remove(&k);
    }
    for (k,(v,tm)) in &mut *lk {
        if red_id == v {
            (*tm) = curr_tm;
            return *k;
        }
    }
    let ob_id = gen_lcg_id();
    lk.insert(ob_id, (red_id.to_owned(),curr_tm));
    return ob_id;
}

fn change_msg_id_to_i32(root:&mut serde_json::Value){
    lazy_static! {
        static ref ID_SET:HashSet<String> = {
            let mut st = HashSet::new();
            st.insert("message_id".to_owned());
            st
        };
    }
    if root.is_object() {
        for (k,v) in root.as_object_mut().unwrap() {
            if ID_SET.contains(k) {
                if v.is_string() {
                    (*v) = serde_json::json!(red_id_to_ob(v.as_str().unwrap()));
                }
            }else if v.is_array() || v.is_object() {
                change_msg_id_to_i32(v);
            }
        }
    }else if root.is_array() {
        for v in root.as_array_mut().unwrap() {
            change_msg_id_to_i32(v);
        }
    }
}

fn mycrc64(data:&str) -> i64 {
    let ret = crc64::crc64(0, data.as_bytes()) as i64;
    ret.abs()
}

fn change_uid_to_i64(root:&mut serde_json::Value){
    lazy_static! {
        static ref ID_SET:HashSet<String> = {
            let mut st = HashSet::new();
            st.insert("target_id".to_owned());
            st.insert("user_id".to_owned());
            st.insert("group_id".to_owned());
            st.insert("self_id".to_owned());
            st.insert("message_id".to_owned());
            st.insert("operator_id".to_owned());
            st
        };
    }
    if root.is_object() {
        for (k,v) in root.as_object_mut().unwrap() {
            if ID_SET.contains(k) {
                if v.is_string() {
                    let v_str = v.as_str().unwrap();
                    if let Err(_) = v_str.parse::<i64>() {
                        let cksum = mycrc64(v.as_str().unwrap());
                        OB_RED_UID_MAP.lock().unwrap().insert(cksum,v.as_str().unwrap().to_owned());
                        (*v) = serde_json::to_value(cksum).unwrap();
                    }else{
                        (*v) = serde_json::to_value(v_str.parse::<i64>().unwrap()).unwrap();
                    }
                    
                }
            }else if v.is_array() || v.is_object() {
                change_uid_to_i64(v);
            }
        }
    }else if root.is_array() {
        for v in root.as_array_mut().unwrap() {
            change_uid_to_i64(v);
        }
    }
}



fn ob_id_to_red(ob_id:i32) -> Option<String> {
    let curr_tm = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut lk = OB_RED_ID_MAP.lock().unwrap();
    let mut to_remove = vec![];
    for (k,(_v,tm)) in &*lk {
        if tm + 60*2 < curr_tm {
            to_remove.push(k.to_owned());
        }
    }
    for k in to_remove {
        (*lk).remove(&k);
    }
    lk.get(&ob_id).map(|s| s.0.clone())
}


fn change_event_at_and_reply(root:&mut serde_json::Value) -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let msg_opt = root.get_mut("message");
    if msg_opt.is_none() {
        return Ok(());
    }
    let msg = msg_opt.unwrap();
    if msg.is_string() {
        let r = str_msg_to_arr(msg);
        if r.is_err() {
            return Err("str_msg_to_arr err".into());
        }
        (*msg) = r.unwrap();
    }
    for node in msg.as_array_mut().unwrap() {
        let tp = node.get_mut("type").ok_or("type not in node")?;
        if tp == "at" {
            let data = node.get_mut("data").ok_or("no data in at node")?;
            let qq = data.get_mut("qq").ok_or("no qq in data node")?;
            let qq_str = qq.as_str().ok_or("qq in node not str")?;
            if let Err(_) = qq_str.parse::<i64>() {
                let cksum = mycrc64(qq_str);
                OB_RED_UID_MAP.lock().unwrap().insert(cksum,qq_str.to_owned());
                (*qq) = serde_json::to_value(cksum.to_string()).unwrap();
            }else{
                (*qq) = serde_json::to_value(qq_str.parse::<i64>().unwrap().to_string()).unwrap();
            }
        }else if tp == "reply" {
            let data = node.get_mut("data").ok_or("no data in reply node")?;
            let id_str = read_json_str(data, "id");
            if id_str == "" {
                return Err("id in node not str".into());
            }
            let id = data.get_mut("id").ok_or("no id in data node")?;
            let ob_id = red_id_to_ob(&id_str);
            (*id) = serde_json::to_value(ob_id.to_string()).unwrap();
        }
    }
    Ok(())
}


fn change_obid_to_str(root:&mut serde_json::Value) -> Result<(),Box<dyn std::error::Error + Send + Sync>>{
    lazy_static! {
        static ref ID_SET:HashSet<String> = {
            let mut st = HashSet::new();
            st.insert("target_id".to_owned());
            st.insert("user_id".to_owned());
            st.insert("group_id".to_owned());
            st.insert("self_id".to_owned());
            st
        };
    }
    if root.is_object() {
        for (k,v) in root.as_object_mut().unwrap() {
            if ID_SET.contains(k) {
                if v.is_i64() {
                    let ob_uid = v.as_i64().unwrap();
                    let lk = OB_RED_UID_MAP.lock().unwrap();
                    if let Some(red_uid) = lk.get(&ob_uid) {
                        (*v) = serde_json::to_value(red_uid).unwrap();
                    }
                }else if v.is_string() {
                    let ob_uid = v.as_str().unwrap().parse::<i64>()?;
                    let lk = OB_RED_UID_MAP.lock().unwrap();
                    if let Some(red_uid) = lk.get(&ob_uid) {
                        (*v) = serde_json::to_value(red_uid).unwrap();
                    }   
                }
                
            }else if v.is_array() || v.is_object() {
                change_obid_to_str(v)?;
            }
        }
    }else if root.is_array() {
        for v in root.as_array_mut().unwrap() {
            change_obid_to_str(v)?;
        }
    }
    Ok(())
}

pub fn deal_event_groups(root:&mut serde_json::Value) -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let groups_id = read_json_str(root, "groups_id");
    if groups_id == "" {
        return Ok(());
    }
    root.as_object_mut().ok_or("event not object")?.remove("groups_id");
    let group_id_opt = root.get_mut("group_id");
    if group_id_opt.is_none() {
        return Ok(());
    }
    let group_id = group_id_opt.unwrap();
    let group_id_int = group_id.as_i64().ok_or("group_id not i64")?;
    let hash_key = format!("{groups_id}|{group_id_int}");
    let new_group_id = mycrc64(&hash_key);
    (*group_id) = serde_json::json!(new_group_id);
    OB_GROUPS_MAP.lock().unwrap().insert(new_group_id,(group_id_int,groups_id));
    Ok(())
}



fn change_params_at_and_reply(root:&mut serde_json::Value) -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let msg_opt = root.get_mut("message");
    if msg_opt.is_none() {
        return Ok(());
    }
    let msg = msg_opt.unwrap();
    if msg.is_string() {
        let r = str_msg_to_arr(msg);
        if r.is_err() {
            return Err("str_msg_to_arr err".into());
        }
        (*msg) = r.unwrap();
    }
    for node in msg.as_array_mut().unwrap() {
        let tp = node.get_mut("type").ok_or("type not in node")?;
        if tp == "at" {
            let data = node.get_mut("data").ok_or("no data in at node")?;
            let qq_str = read_json_str(data, "qq");
            let qq_int = qq_str.parse::<i64>()?;
            let lk = OB_RED_UID_MAP.lock().unwrap();
            let red_id_opt = lk.get(&qq_int);
            let qq = data.get_mut("qq").ok_or("no qq in at data node")?;
            if let Some(red_id) = red_id_opt {
                (*qq) = serde_json::json!(red_id);
            }else{
                (*qq) = serde_json::json!(qq_int.to_string());
            }
            
        }
        else if tp == "reply" {
            let data = node.get_mut("data").ok_or("no data in reply node")?;
            let id_str = read_json_str(data, "id");
            let id_int = id_str.parse::<i32>()?;
            let red_id = ob_id_to_red(id_int).ok_or("id in reply node not found")?;
            let id = data.get_mut("id").ok_or("no id in data reply node")?;
            (*id) = serde_json::json!(red_id);
        }
    }
    Ok(())
}



fn deal_request_groups(root:&mut serde_json::Value)  -> Result<(),Box<dyn std::error::Error + Send + Sync>>{
    let group_id = read_json_str(root, "group_id");
    if group_id == "" {
        return Ok(());
    }
    let group_id_int = group_id.parse::<i64>()?;
    let lk = OB_GROUPS_MAP.lock().unwrap();
    let g_gs_opt = lk.get(&group_id_int);
    if g_gs_opt.is_none() {
        return Ok(());
    }
    let (group_id,groups_id) = g_gs_opt.unwrap();
    (*root.get_mut("group_id").unwrap()) = serde_json::json!(group_id);
    root.as_object_mut().unwrap().insert("groups_id".to_owned(), serde_json::json!(groups_id));
    Ok(())
}

pub fn event_to_onebot(root:&serde_json::Value) -> Result<(serde_json::Value,String,String),Box<dyn std::error::Error + Send + Sync>>{
    let mut root = root.clone();
    let passive_id = read_json_str(&root, "message_id");
    
    change_msg_id_to_i32(&mut root); // 处理message_id
    change_uid_to_i64(&mut root); // 处理处理uid
    change_event_at_and_reply(&mut root)?; // 处理at和回复
    deal_event_groups(&mut root)?; // 处理groups
    // 处理platform和self_id
    let platform = read_json_str(&root, "platform");
    let self_id =  read_json_str(&root, "self_id");
    // 处理passive_id
    root.as_object_mut().unwrap().remove("platform");
    if passive_id != "" {
        let group_id = read_json_str(&root, "group_id");
        let user_id = read_json_str(&root, "user_id");
        let mut key = String::new();
        if group_id != "" {
            key = group_id;
        }else if user_id != "" {
            key = user_id;
        }
        if key != "" {
            let mut lk = PASSIVE_ID_MAP.lock().unwrap();
            lk.insert(key.parse::<>()?, passive_id);
        }
    }
    return Ok((root,platform,self_id));
}

fn change_params_msg_id(root:&mut serde_json::Value) -> Result<(),Box<dyn std::error::Error + Send + Sync>>{
    let ob_msg_id = read_json_str(root, "message_id");
    if ob_msg_id == "" {
        return Ok(());
    }
    let ob_msg_id_int = ob_msg_id.parse::<i32>()?;
    let red_msg_id = ob_id_to_red(ob_msg_id_int).ok_or("message_id not found")?;
    root.as_object_mut().unwrap().insert("message_id".to_owned(), serde_json::json!(red_msg_id));
    Ok(())
}

pub fn request_to_red(root:&serde_json::Value) -> Result<(serde_json::Value,String),Box<dyn std::error::Error + Send + Sync>>{
    let mut root = root.clone();
    let mut passive_id = "".to_string();
    let params_opt = root.get_mut("params");
    if params_opt.is_none(){
        return Ok((root,passive_id));
    }
    let mut params = params_opt.unwrap();

    // 处理 passive_id
    let user_id = read_json_str(&params, "user_id");
    let group_id = read_json_str(&params, "group_id");
    if group_id != "" { //group
        let lk = PASSIVE_ID_MAP.lock().unwrap();
        let group_id_int = group_id.parse::<i64>()?;
        if let Some(v) = lk.get(&group_id_int) {
            passive_id = v.to_owned();
        }
    }
    if user_id != "" && passive_id == "" {
        let lk = PASSIVE_ID_MAP.lock().unwrap();
        let user_id_int = user_id.parse::<i64>()?;
        if let Some(v) = lk.get(&user_id_int) {
            passive_id = v.to_owned();
        }
    }

    deal_request_groups(&mut params)?;
    change_obid_to_str(&mut params)?;
    change_params_at_and_reply(&mut params)?;
    change_params_msg_id(&mut params)?;
    Ok((root,passive_id))
}


pub fn red_ret_to_ob(root:serde_json::Value,echo:Option<serde_json::Value>) -> serde_json::Value {
    let mut root = root.clone();
    if !root.is_object() {
        return serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":root.to_string(),
            "echo":echo
        })
    }
    let data_opt = root.get_mut("data");
    if data_opt.is_some(){
        let mut data = data_opt.unwrap();
        change_msg_id_to_i32(&mut data); // 处理message_id
        change_uid_to_i64(&mut data); // 处理处理uid
    }
   
    if echo.is_some(){
        root.as_object_mut().unwrap().insert("echo".to_owned(), echo.unwrap());
    }
    root
}




