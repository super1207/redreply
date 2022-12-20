use crate::{cqapi::{cq_add_log_w, cq_call_api}, read_config, redlang::RedLang};

use super::{set_normal_evt_info, get_script_info};

fn get_evt_flag(root: &serde_json::Value) -> Result<Vec<&str>, Box<dyn std::error::Error>>{
    let post_type = root.get("post_type").ok_or("缺少post_type")?.as_str().unwrap_or("");
    let mut ret_vec = vec![post_type];
    match post_type {
        "message" => {
            ret_vec.push(root.get("message_type").ok_or("缺少message_type")?.as_str().unwrap_or(""));
        },
        "notice" => {
            ret_vec.push(root.get("notice_type").ok_or("缺少notice_type")?.as_str().unwrap_or(""));
        },
        "request" => {
            ret_vec.push(root.get("request_type").ok_or("缺少request_type")?.as_str().unwrap_or(""));
        },
        "meta_event" => {
            ret_vec.push(root.get("meta_event_type").ok_or("缺少meta_event_type")?.as_str().unwrap_or(""));
        },
        _ => {
            return None.ok_or("unkown post_type")?;
        }
    }
    ret_vec.push(match root.get("sub_type") {
        Some(v) => {
            v.as_str().unwrap_or("")
        },
        None => {
            ""
        }
    });
    Ok(ret_vec)
}

fn do_script(rl:&mut RedLang,code:&str) -> Result<(), Box<dyn std::error::Error>>{
    let user_id_str = rl.get_exmap("发送者ID")?.to_string();
    let group_id_str = rl.get_exmap("群ID")?.to_string();
    let guild_id_str = rl.get_exmap("频道ID")?.to_string();
    let channel_id_str = rl.get_exmap("子频道ID")?.to_string();
    let out_str_t = rl.parse(code)?;
    let out_str_vec = super::do_paging(&out_str_t)?;
    for out_str in out_str_vec {
        if group_id_str != "" {
            if out_str != "" {
                let send_json = serde_json::json!({
                    "action":"send_group_msg",
                    "params":{
                        "group_id": group_id_str.parse::<i32>()?,
                        "message":out_str
                    }
                });
                let ret_str = cq_call_api(&send_json.to_string())?;
                let ret_json:serde_json::Value = serde_json::from_str(&ret_str)?;
                let retcode = ret_json.get("retcode").ok_or("retcode not found")?.as_i64().ok_or("retcode not int")?;
                if retcode != 0 {
                    cq_add_log_w(&ret_str).unwrap();
                }else {
                    let data = ret_json.get("data").ok_or("data not found")?;
                    let message_id = data.get("message_id").ok_or("message_id not found")?.as_i64().ok_or("retcode not int")?;
                    let self_id = rl.get_exmap("机器人ID")?;
                    super::do_group_msg::msg_id_map_insert(self_id.to_string(),group_id_str.clone(),message_id.to_string())?;
                }
            }
        }else if channel_id_str != "" && guild_id_str != "" {
            if out_str != "" {
                let send_json = serde_json::json!({
                    "action":"send_guild_channel_msg",
                    "params":{
                        "guild_id": guild_id_str,
                        "channel_id": channel_id_str,
                        "message":out_str
                    }
                });
                cq_call_api(&send_json.to_string())?;
            }
        }else if user_id_str != "" {
            if out_str != "" {
                let send_json = serde_json::json! ({
                    "action":"send_private_msg",
                    "params":{
                        "user_id": user_id_str.parse::<i32>()?,
                        "message":out_str
                    }
                });
                cq_call_api(&send_json.to_string())?;
            }
        }
    }
    Ok(())
}

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let script_json = read_config()?;
    let evt_flag = get_evt_flag(root)?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,_ppfs) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "事件触发" {
            set_normal_evt_info(&mut rl, root)?;
            let key_vec = keyword.split(":").collect::<Vec<&str>>();
            for j in 0..key_vec.len() {
                if key_vec.get(j).unwrap_or(&"") != evt_flag.get(j).unwrap_or(&""){
                    return Ok(());
                }
            }
            do_script(&mut rl,code)?;
        }
    }
    Ok(())
}

// 处理其它事件
pub fn do_other_evt(root: &serde_json::Value) -> Result<i32, Box<dyn std::error::Error>> {
    if let Err(e) = do_redlang(&root) {
        cq_add_log_w(format!("err in do_other_evt:do_redlang:{}", e.to_string()).as_str()).unwrap();
    }
    Ok(0)
}
