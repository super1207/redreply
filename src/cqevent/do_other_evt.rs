use crate::{cqapi::{cq_add_log_w, cq_add_log}, read_code, redlang::RedLang};

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

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let script_json = read_code()?;
    let evt_flag = get_evt_flag(root)?;
    cq_add_log(&format!("收到事件:`{}`",evt_flag.join(":"))).unwrap();
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,_ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "事件触发" {
            set_normal_evt_info(&mut rl, root)?;
            let key_vec = keyword.split(":").collect::<Vec<&str>>();
            for j in 0..key_vec.len() {
                if &key_vec.get(j).unwrap_or(&"").trim() != evt_flag.get(j).unwrap_or(&""){
                    // 匹配失败，继续匹配
                    continue;
                }
            }
            rl.pkg_name = pkg_name.to_owned();
            rl.script_name = name.to_owned();
            super::do_script(&mut rl,code,true)?;
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
