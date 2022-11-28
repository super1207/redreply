use crate::{cqapi::*, redlang::{RedLang}, mytool::json_to_cq_str, read_config};

use super::{is_key_match, get_script_info};

fn do_script(rl:&mut RedLang,code:&str) -> Result<(), Box<dyn std::error::Error>>{
    let out_str = rl.parse(code)?;
    if out_str != "" {
        let send_json = serde_json::json!({
            "action":"send_guild_channel_msg",
            "params":{
                "guild_id": rl.get_exmap("频道ID")?,
                "channel_id": rl.get_exmap("子频道ID")?,
                "message":out_str
            }
        });
        cq_call_api(&send_json.to_string())?;
    }
    Ok(())
}

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    let script_json = read_config()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "频道触发"{
            rl.set_exmap("内容", &msg)?;
            super::set_normal_message_info(&mut rl, root)?;
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                do_script(&mut rl,code)?;
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
