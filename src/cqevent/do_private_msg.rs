use crate::{cqapi::*, redlang::RedLang, mytool::{json_to_cq_str, read_json_str}, read_code, G_INPUTSTREAM_VEC};

use super::{is_key_match, get_script_info, set_normal_message_info};

fn do_redlang(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>>{
    let msg = json_to_cq_str(&root)?;
    // 在这里处理输入流
    {
        let user_id = read_json_str(root,"user_id");
        let vec_lk = G_INPUTSTREAM_VEC.read()?;
        let vec_len = vec_lk.len();
        for i in 0..vec_len {
            let st = vec_lk.get(i).unwrap();
            if st.stream_type == "输入流" {
                if user_id == st.user_id && st.group_id == "" && st.channel_id == "" {
                    let k_arc = st.tx.clone().unwrap();
                    k_arc.lock().unwrap().send(msg.clone())?;
                }
            }
        }
    }
    let script_json = read_code()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "私聊触发" || cffs == "群、私聊触发"{
            rl.set_exmap("内容", &msg)?;
            rl.set_exmap("当前消息",&msg)?;
            set_normal_message_info(&mut rl, root)?;
            if is_key_match(&mut rl,&ppfs,keyword,&msg)? {
                rl.script_name = name.to_owned();
                rl.pkg_name = pkg_name.to_owned();
                super::do_script(&mut rl,code,true)?;
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
