use std::{collections::HashSet, rc::Rc, cell::RefCell};

use crate::{cqapi::*, mytool::{json_to_cq_str, read_json_str}, read_code_cache, redlang::RedLang, status::add_recv_private_msg, G_INPUTSTREAM_VEC, RT_PTR};

use super::{is_key_match, get_script_info, set_normal_message_info};

fn do_redlang(root: &serde_json::Value,ban_pkgs:&HashSet<String>) -> Result<(), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    // 在这里处理输入流
    {
        let user_id = read_json_str(root,"user_id");
        let self_id = read_json_str(root,"self_id");
        let vec_lk = G_INPUTSTREAM_VEC.read()?;
        let vec_len = vec_lk.len();
        for i in 0..vec_len {
            let st = vec_lk.get(i).unwrap();
            if st.stream_type == "输入流" {
                if self_id == st.self_id && user_id == st.user_id {
                    let k_arc = st.tx.clone().unwrap();
                    k_arc.lock().unwrap().send(msg.clone())?;
                }
            }
        }
    }

    // 数据统计
    {
        let platform = read_json_str(root,"platform");
        let bot_id = read_json_str(root,"self_id");
        add_recv_private_msg(&platform,&bot_id)?;
    }

    let script_json = read_code_cache()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        if ban_pkgs.contains(pkg_name) {
            continue;
        }
        let mut rl = RedLang::new();
        if cffs == "私聊触发" || cffs == "群、私聊触发"{
            set_normal_message_info(&mut rl, root)?;
            rl.set_exmap("当前消息",&msg)?;
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                let exmap = (*rl.exmap).borrow().clone();
                let code_t = code.to_owned();
                let pkg_name_t = pkg_name.to_owned();
                let script_name_t = name.to_owned();
                RT_PTR.spawn_blocking(move ||{
                    let mut rl = RedLang::new();
                    rl.exmap = Rc::new(RefCell::new(exmap));
                    rl.pkg_name = pkg_name_t.to_owned();
                    rl.script_name = script_name_t.to_owned();
                    if let Err(e) = super::do_script(&mut rl,&code_t,"normal",false) {
                        cq_add_log_w(format!("err in do_private_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
                    }
                });
            }
        }
    }
    Ok(())
}

// 处理私聊事件
pub fn do_private_msg(root: &serde_json::Value,ban_pkgs:&HashSet<String>) {
    if let Err(e) = do_redlang(&root,ban_pkgs) {
        cq_add_log_w(format!("err in do_private_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
    }
}
