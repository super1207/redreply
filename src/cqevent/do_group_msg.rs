use std::{cell::RefCell, collections::HashSet, rc::Rc};

use crate::{
    cqapi::*,
    mytool::{json_to_cq_str, read_json_str},
    redlang::RedLang,
    runtime, runtime_context,
    status::add_recv_group_msg,
};

use super::{get_script_info, is_key_match, set_normal_message_info};

pub fn msg_id_map_insert(
    self_id: String,
    user_id: String,
    group_id: String,
    message_id: String,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime_context()
        .session
        .insert_message_id(self_id, user_id, group_id, message_id)
}

fn do_redlang(
    root: &serde_json::Value,
    ban_pkgs: &HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg = json_to_cq_str(root)?;
    // 在这里处理输入流
    {
        let user_id = read_json_str(root, "user_id");
        let self_id = read_json_str(root, "self_id");
        let group_id = read_json_str(root, "group_id");
        let message_id = read_json_str(root, "message_id");
        runtime_context().session.send_group_input_streams(
            &self_id,
            &user_id,
            &group_id,
            &msg,
            &message_id,
        )?;
    }

    // 数据统计
    {
        let platform = read_json_str(root, "platform");
        let bot_id = read_json_str(root, "self_id");
        add_recv_group_msg(&platform, &bot_id)?;
    }

    let script_json = runtime_context().scripts.read_scripts();
    let mut is_set_msg_id_map = false;
    for i in 0..script_json
        .as_array()
        .ok_or("script.json文件不是数组格式")?
        .len()
    {
        let (keyword, cffs, code, ppfs, name, pkg_name) = get_script_info(&script_json[i])?;
        if ban_pkgs.contains(pkg_name) {
            continue;
        }
        let mut rl = RedLang::new();
        if cffs == "群聊触发" || cffs == "群、私聊触发" {
            set_normal_message_info(&mut rl, root)?;
            if !is_set_msg_id_map {
                is_set_msg_id_map = true;
                let user_id = rl.get_exmap("发送者ID");
                let group_id = rl.get_exmap("群ID");
                let message_id = rl.get_exmap("消息ID");
                let self_id = rl.get_exmap("机器人ID");
                msg_id_map_insert(
                    self_id.to_string(),
                    user_id.to_string(),
                    group_id.to_string(),
                    message_id.to_string(),
                )?;
            }
            {
                let sender = root.get("sender").ok_or("sender not exists")?;
                {
                    let role = read_json_str(sender, "role");
                    let role_str = match role.as_str() {
                        "owner" => "群主",
                        "admin" => "管理",
                        &_ => "群员",
                    };
                    rl.set_exmap("发送者权限", role_str);
                }
                if let Some(js_v) = sender.get("card") {
                    rl.set_exmap("发送者名片", js_v.as_str().unwrap_or(""));
                }
                if let Some(js_v) = sender.get("title") {
                    rl.set_exmap("发送者专属头衔", js_v.as_str().unwrap_or(""));
                }
                rl.set_exmap("当前消息", &msg);
            }
            if is_key_match(&mut rl, ppfs, keyword, &msg)? {
                let exmap = (*rl.exmap).borrow().clone();
                let code_t = code.to_owned();
                let pkg_name_t = pkg_name.to_owned();
                let script_name_t = name.to_owned();
                runtime().spawn_blocking(move || {
                    let mut rl = RedLang::new();
                    rl.exmap = Rc::new(RefCell::new(exmap));
                    rl.pkg_name = pkg_name_t.to_owned();
                    rl.script_name = script_name_t.to_owned();
                    if let Err(e) = super::do_script(&mut rl, &code_t, "normal", false) {
                        cq_add_log_w(
                            format!("err in do_group_msg:do_redlang:{}", e).as_str(),
                        )
                        .unwrap();
                    }
                });
            }
        }
    }
    Ok(())
}

// 处理群聊事件
pub fn do_group_msg(root: &serde_json::Value, ban_pkgs: &HashSet<String>) {
    if let Err(e) = do_redlang(root, ban_pkgs) {
        cq_add_log_w(format!("err in do_group_msg:do_redlang:{}", e).as_str()).unwrap();
    }
}
