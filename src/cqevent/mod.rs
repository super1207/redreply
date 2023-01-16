pub(crate) mod do_group_msg;
mod do_private_msg;
mod do_guild_msg;
mod do_other_evt;

use std::{rc::Rc, collections::HashMap, sync::Arc, cell::RefCell};

use crate::{redlang::RedLang, mytool::read_json_str, PAGING_UUID, CLEAR_UUID, add_running_script_num, dec_running_script_num, cqapi::cq_add_log_w};

// 处理1207号事件
pub fn do_1207_event(onebot_json_str: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let root:serde_json::Value = serde_json::from_str(onebot_json_str)?;
    if let Some(message_type) = root.get("message_type") {
        if message_type == "group" {
            do_group_msg::do_group_msg(&root)?;
        }else if message_type == "private"{
            do_private_msg::do_private_msg(&root)?;
        }else if message_type == "guild"{
            do_guild_msg::do_guild_msg(&root)?;
        }
    }
    do_other_evt::do_other_evt(&root)?;
    Ok(0)
}

pub fn do_paging(outstr:&str) -> Result<Vec<&str>, Box<dyn std::error::Error>> {
    let out = outstr.split(PAGING_UUID.as_str());
    let outvec = out.collect::<Vec<&str>>();
    return Ok(outvec);
}

pub fn get_msg_type(rl:& RedLang) -> &'static str {
    let user_id_str = rl.get_exmap("发送者ID").to_string();
    let group_id_str = rl.get_exmap("群ID").to_string();
    let guild_id_str = rl.get_exmap("频道ID").to_string();
    let channel_id_str = rl.get_exmap("子频道ID").to_string();
    let msg_type:&str;
    if group_id_str != "" {
        msg_type = "group";
    }else if channel_id_str != "" && guild_id_str != ""{
        msg_type = "channel";
    }else if user_id_str  != "" {
        msg_type = "private";
    }else{
        msg_type = "";
    }
    return msg_type;
}

pub fn do_script(rl:&mut RedLang,code:&str,deal_err:bool) -> Result<(), Box<dyn std::error::Error>>{
    if add_running_script_num() == false {
        return Ok(());
    }
    let _guard = scopeguard::guard((),|_| {
        dec_running_script_num();
    });

    let out_str_t_rst = rl.parse(code);
    if let Err(err) = out_str_t_rst {
        let err_str = format!("在脚本`{}`中发送错误:{}",rl.script_name,err);
        // 如果需要处理错误
        if deal_err == true {
            let err_str_t = err_str.clone();
            let exmap = (*rl.exmap).borrow().clone();
            let script_name = rl.script_name.clone();
            let pkg_name = rl.pkg_name.clone();
            let _foo = std::thread::spawn(move ||{
                
                fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str), Box<dyn std::error::Error>>{
                    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
                    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
                    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
                    return Ok((cffs,code));
                } 
                fn fun(err_str:String,exmap:HashMap<String, Arc<String>>,pkg_name:String,script_name:String) -> Result<i32, Box<dyn std::error::Error>> {
                    let script_json = crate::read_code()?;
                    let exmap_ptr = Rc::new(RefCell::new(exmap));
                    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
                        let (cffs,code) = get_script_info(&script_json[i])?;
                        if cffs == "脚本错误" {
                            let mut rl2 = crate::redlang::RedLang::new();
                            rl2.exmap = exmap_ptr.clone();
                            rl2.pkg_name = pkg_name.clone();
                            rl2.script_name = script_name.clone();
                            rl2.set_coremap("错误信息", &err_str)?;
                            if let Err(err) = crate::cqevent::do_script(&mut rl2,&code,false) {
                                cq_add_log_w(&format!("{}",err)).unwrap();
                            }
                        }      
                    }
                    Ok(0)
                }
                if let Err(e) = fun(err_str_t,exmap,pkg_name,script_name) {
                    crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                }
            });
        }
        return Err(RedLang::make_err(&err_str));
    }
    let out_str_t = out_str_t_rst.unwrap();
    // 处理清空指令
    let mut after_clear:&str = &out_str_t;
    if let Some(pos) = out_str_t.rfind(CLEAR_UUID.as_str()) {
        after_clear = out_str_t.get((pos + 36)..).unwrap();
    }
    let out_str_vec = do_paging(after_clear)?;
    for out_str in out_str_vec {
        crate::redlang::cqexfun::send_one_msg(rl, out_str)?;
    }
    Ok(())
}

fn set_normal_evt_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    rl.set_exmap("机器人ID", &read_json_str(root,"self_id"))?;
    rl.set_exmap("发送者ID", &read_json_str(root,"user_id"))?;
    rl.set_exmap("群ID", &read_json_str(root,"group_id"))?;
    rl.set_exmap("机器人名字", "露娜sama")?;
    rl.set_exmap("原始事件", &root.to_string())?;
    rl.set_exmap("频道ID", &read_json_str(root,"guild_id"))?;
    rl.set_exmap("子频道ID", &read_json_str(root,"channel_id"))?;
    rl.set_exmap("机器人频道ID", &read_json_str(root,"self_tiny_id"))?;
    Ok(())
}

fn set_normal_message_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    rl.set_exmap("消息ID", &read_json_str(root,"message_id"))?;
    {
        let sender = root.get("sender").ok_or("sender not exists")?;
        if let Some(js_v) = sender.get("nickname") {
            rl.set_exmap("发送者昵称", js_v.as_str().unwrap_or(""))?;
        }
    }
    set_normal_evt_info(rl,root)?;
    Ok(())
}


fn is_key_match(rl:&mut RedLang,ppfs:&str,keyword:&str,msg:&str) -> Result<bool, Box<dyn std::error::Error>>{
    let mut is_match = false;
    if ppfs == "完全匹配"{
        if keyword == msg {
            is_match = true;
        }
    }else if ppfs == "模糊匹配"{
        if let Some(_pos)  = msg.find(keyword) {
            is_match = true;
        }
    }else if ppfs == "前缀匹配"{
        if msg.starts_with(keyword){
            is_match = true;
            rl.set_exmap("子关键词", msg.get(keyword.len()..).ok_or("前缀匹配失败")?)?;
        }
    }else if ppfs == "正则匹配"{
        let re = fancy_regex::Regex::new(keyword)?;
        let mut sub_key_vec = String::new();
        sub_key_vec.push_str(&rl.type_uuid);
        sub_key_vec.push('A');
        for cap_iter in re.captures_iter(&msg) {
            let cap = cap_iter?;
            is_match = true;
            let len = cap.len();
            let mut temp_vec = String::new();
            temp_vec.push_str(&rl.type_uuid);
            temp_vec.push('A');
            for i in 0..len {
                let s = cap.get(i).ok_or("regex cap访问越界")?.as_str();
                temp_vec.push_str(&s.len().to_string());
                temp_vec.push(',');
                temp_vec.push_str(s);
            }
            sub_key_vec.push_str(&temp_vec.len().to_string());
            sub_key_vec.push(',');
            sub_key_vec.push_str(&temp_vec);
        }
        rl.set_exmap("子关键词", &sub_key_vec)?;
    }
    Ok(is_match)
}

fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
    let name = script_json.get("name").ok_or("脚本中无name")?.as_str().ok_or("脚本中name不是str")?;
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let keyword = node.get("关键词").ok_or("脚本中无关键词")?.as_str().ok_or("脚本中关键词不是str")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    let ppfs = node.get("匹配方式").ok_or("脚本中无匹配方式")?.as_str().ok_or("脚本中匹配方式不是str")?;
    
    return Ok((keyword,cffs,code,ppfs,name));
}