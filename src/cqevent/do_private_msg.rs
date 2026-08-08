use std::{cell::RefCell, collections::HashSet, rc::Rc};

use crate::{
    cqapi::*,
    mytool::{json_to_cq_str, read_json_str},
    redlang::RedLang,
    runtime, runtime_context,
    status::add_recv_private_msg,
};

use super::{get_script_info, is_key_match, set_normal_message_info};

fn do_redlang(
    root: &serde_json::Value,
    ban_pkgs: &HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg = json_to_cq_str(root)?;
    // 在这里处理输入流
    {
        let user_id = read_json_str(root, "user_id");
        let self_id = read_json_str(root, "self_id");
        // 仅匹配私聊输入流（group_id 为空），避免群内等待被私聊误唤醒
        runtime_context()
            .session
            .send_private_input_streams(&self_id, &user_id, &msg)?;
    }

    // 数据统计
    {
        let platform = read_json_str(root, "platform");
        let bot_id = read_json_str(root, "self_id");
        add_recv_private_msg(&platform, &bot_id)?;
    }

    let script_json = runtime_context().scripts.read_scripts();
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
        if cffs == "私聊触发" || cffs == "群、私聊触发" {
            set_normal_message_info(&mut rl, root)?;
            rl.set_exmap("当前消息", &msg);
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
                        cq_add_log_w(format!("err in do_private_msg:do_redlang:{}", e).as_str())
                            .unwrap();
                    }
                });
            }
        }
    }
    Ok(())
}

// 处理私聊事件
pub fn do_private_msg(root: &serde_json::Value, ban_pkgs: &HashSet<String>) {
    if let Err(e) = do_redlang(root, ban_pkgs) {
        cq_add_log_w(format!("err in do_private_msg:do_redlang:{}", e).as_str()).unwrap();
    }
}
