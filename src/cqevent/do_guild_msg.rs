use crate::{cqapi::*, redlang::RedLang, mytool::json_to_cq_str, read_code};

use super::{is_key_match, get_script_info};

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    let script_json = read_code()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "频道触发"{
            rl.set_exmap("内容", &msg)?;
            rl.set_exmap("当前消息",&msg)?;
            super::set_normal_message_info(&mut rl, root)?;
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                rl.pkg_name = pkg_name.to_owned();
                rl.script_name = name.to_owned();
                super::do_script(&mut rl,code,true)?;
            }    
        }
    }
    Ok(())
}

// 处理频道事件
pub fn do_guild_msg(root: &serde_json::Value) -> Result<i32, Box<dyn std::error::Error>> {
    if let Err(e) = do_redlang(&root) {
        cq_add_log_w(format!("err in do_guild_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
    }
    Ok(0)
}
