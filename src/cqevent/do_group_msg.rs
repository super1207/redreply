use std::collections::VecDeque;

use crate::{cqapi::*, redlang::RedLang, mytool::{json_to_cq_str, read_json_str}, read_code_cache, G_INPUTSTREAM_VEC};

use super::{is_key_match, get_script_info, set_normal_message_info};

pub fn msg_id_map_insert(self_id:String,user_id:String,group_id:String,message_id:String) ->Result<(), Box<dyn std::error::Error>> {
    let flag = self_id + &user_id + &group_id;
    let mut mp = crate::G_MSG_ID_MAP.write()?;
    if mp.contains_key(&flag) {
        let v = mp.get_mut(&flag).unwrap();
        v.push_front(message_id.to_string());
        if v.len() > 20 {
            v.pop_back();
        }
    }else{
        let v = VecDeque::new();
        mp.insert(flag, v);
    }
    Ok(())
}

fn do_redlang(root: &serde_json::Value) -> Result< (), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    // 在这里处理输入流
    {
        let user_id = read_json_str(root,"user_id");
        let self_id = read_json_str(root,"self_id");
        let group_id = read_json_str(root,"group_id");
        let vec_lk = G_INPUTSTREAM_VEC.read()?;
        let vec_len = vec_lk.len();
        for i in 0..vec_len {
            let st = vec_lk.get(i).unwrap();
            if st.stream_type == "输入流" {
                if self_id == st.self_id && user_id == st.user_id && group_id ==st.group_id {
                    let k_arc = st.tx.clone().unwrap();
                    k_arc.lock().unwrap().send(msg.clone())?;
                }
            }else{
                if self_id == st.self_id && group_id ==st.group_id {
                    let k_arc = st.tx.clone().unwrap();
                    let to_send = serde_json::json!({
                        "发送者ID":user_id,
                        "消息":msg
                    });
                    k_arc.lock().unwrap().send(to_send.to_string())?;
                }
            }
        }
    }
    let script_json = read_code_cache()?;
    let mut is_set_msg_id_map = false;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "群聊触发" || cffs == "群、私聊触发"{
            set_normal_message_info(&mut rl, root)?;
            if is_set_msg_id_map == false {
                is_set_msg_id_map = true;
                let user_id = rl.get_exmap("发送者ID");
                let group_id = rl.get_exmap("群ID");
                let message_id = rl.get_exmap("消息ID");
                let self_id = rl.get_exmap("机器人ID");
                msg_id_map_insert(self_id.to_string(),user_id.to_string(),group_id.to_string(),message_id.to_string())?;
            }
            {
                let sender = root.get("sender").ok_or("sender not exists")?;
                {
                    let role = read_json_str(sender, "role");
                    let role_str = match role.as_str() {
                        "owner" => "群主",
                        "admin" => "管理",
                        &_ => "群员"
                    };
                    rl.set_exmap("发送者权限", role_str)?;
                }
                if let Some(js_v) = sender.get("card") {
                    rl.set_exmap("发送者名片", js_v.as_str().unwrap_or(""))?;
                }
                if let Some(js_v) = sender.get("title") {
                    rl.set_exmap("发送者专属头衔", js_v.as_str().unwrap_or(""))?;
                }
                rl.set_exmap("当前消息",&msg)?;
            }
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                rl.pkg_name = pkg_name.to_owned();
                rl.script_name = name.to_owned();
                if let Err(e) = super::do_script(&mut rl,code) {
                    cq_add_log_w(format!("err in do_group_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
                }
            }    
        }
    }
    Ok(())
}

// 处理群聊事件
pub fn do_group_msg(root: &serde_json::Value) -> Result<i32, Box<dyn std::error::Error>> {
 
    if let Err(e) = do_redlang(&root) {
        cq_add_log_w(format!("err in do_group_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
    }
    Ok(0)
}
