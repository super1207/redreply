use crate::{cqapi::*, redlang::{RedLang}, mytool::json_to_cq_str, read_config};

use super::{is_key_match, get_script_info, set_normal_message_info};

fn do_script(rl:&mut RedLang,code:&str) -> Result<(), Box<dyn std::error::Error>>{
    let out_str_t = rl.parse(code)?;
    let out_str_vec = super::do_paging(&out_str_t)?;
    for out_str in out_str_vec {
        if out_str != "" {
            let send_json = serde_json::json! ({
                "action":"send_private_msg",
                "params":{
                    "user_id": rl.get_exmap("发送者ID")?.parse::<i32>()?,
                    "message":out_str
                }
            });
            cq_call_api(&send_json.to_string())?;
        }
    }
    Ok(())
}

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    let script_json = read_config()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "私聊触发" || cffs == "群、私聊触发"{
            rl.set_exmap("内容", &msg)?;
            set_normal_message_info(&mut rl, root)?;
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                do_script(&mut rl,code)?;
            }
        }
    }
    Ok(())
}

// 处理私聊事件
pub fn do_private_msg(root: &serde_json::Value) -> Result<i32, Box<dyn std::error::Error>> {
    if let Err(e) = do_redlang(&root) {
        cq_add_log_w(format!("err in do_private_msg:do_redlang:{}", e.to_string()).as_str()).unwrap();
    }
    Ok(0)
}
