pub(crate) mod do_group_msg;
mod do_private_msg;
mod do_other_evt;
mod do_group_inc;

use std::{rc::Rc, collections::{HashMap, HashSet}, sync::Arc, cell::RefCell};

use crate::{add_running_script_num, cqapi::cq_add_log_w, dec_running_script_num, get_gobal_filter_code, httpserver::send_onebot_event, mqttclient::publish_mqtt_event, mytool::read_json_str, read_code_cache, redlang::RedLang, G_SKIP_MSG_TIME, PAGING_UUID, REDLANG_UUID, RT_PTR};

// 处理1207号事件
pub fn do_1207_event(onebot_json_str: &str) -> Result<i32, Box<dyn std::error::Error>> {
    if onebot_json_str.contains(&*crate::REDLANG_UUID) {
        cq_add_log_w(&format!("输入出现内部字符，放弃处理本条消息：`{}`",onebot_json_str)).unwrap();
        return Ok(0)
    }

    let mut root:serde_json::Value = serde_json::from_str(onebot_json_str)?;

    // 将消息转化成数组形式
    if let Some(msg) = root.get("message") {
        if msg.is_string() {
            let arrmsg = crate::mytool::str_msg_to_arr(&msg)?;
            root["message"] = arrmsg;
        }
    }

    // 强行补一个message_id，以符合规范
    if root.get("message_id").is_none() && root.is_object() {
        root["message_id"] = serde_json::to_value(uuid::Uuid::new_v4().to_string())?;
    }

    // 不处理10分钟之前的消息，made by tongyi ai
    if let Some(time) = root.get("time") {
        if let Some(time) = time.as_i64() {
            if time < (chrono::Local::now().timestamp() - *G_SKIP_MSG_TIME.read().unwrap()) {
                // 打印日志
                cq_add_log_w(&format!("10分钟前的消息，放弃处理本条消息：`{}`",onebot_json_str)).unwrap();
                return Ok(0);
            }
        }
    }

    // 全局过滤器
    {
        let code:String = get_gobal_filter_code()?;
        if code != "" {
            let mut rl = RedLang::new();
            set_normal_evt_info(&mut rl, &root)?;
            rl.pkg_name = "".to_owned();
            rl.script_name = "全局过滤器".to_owned();
            let ret = rl.parse(&code)?;
            if ret == "真" {
                return Ok(0);
            }
        }
    }

    // 处理onebot11 server
    let root_t = root.clone();
    RT_PTR.spawn(async {
        send_onebot_event(root_t).await;
    });

    // 发布mqtt事件
    if let Err(err) = publish_mqtt_event(&root) {
        cq_add_log_w(&format!("mqtt publish error:{}", err)).unwrap();
    }
    
    // 预处理脚本
    let script_json = read_code_cache()?;
    let mut ban_pkgs = HashSet::new();
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (_keyword,cffs,code,_ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        if cffs == "内容过滤" {
            let mut rl = RedLang::new();
            set_normal_evt_info(&mut rl, &root)?;
            rl.pkg_name = pkg_name.to_owned();
            rl.script_name = name.to_owned();
            let ret = rl.parse(code)?;
            if ret == "真" {
                ban_pkgs.insert(pkg_name.to_owned());
            }
        }
    }

    if let Some(message_type) = root.get("message_type") {
        if message_type == "group" {
            do_group_msg::do_group_msg(&root,&ban_pkgs);
        }else if message_type == "private"{
            do_private_msg::do_private_msg(&root,&ban_pkgs);
        }
    }

    if let Some(notice_type) = root.get("notice_type") {
        if notice_type == "group_increase" {
            do_group_inc::do_group_inc(&root,&ban_pkgs);
        }
    }
    do_other_evt::do_other_evt(&root,&ban_pkgs);
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
    let message_type = rl.get_exmap("消息类型").to_string();
    let msg_type:&str;
    if user_id_str == "" && group_id_str == "" { // 不能发送消息
        msg_type = "";
    } else if user_id_str != "" && group_id_str != "" && message_type == "private"{ // 发送时消息
        msg_type = "private_temp";
    } else if group_id_str != "" { // 发送群消息
        msg_type = "group";
    } else if user_id_str != "" { // 发送私聊消息
        msg_type = "private"; 
    } else {
        msg_type = ""; // 不能发送消息
    }
    return msg_type;
}


fn do_run_code_and_ret_check(rl:&mut RedLang,code:&str,can_ret_raw:bool)-> Result<String, Box<dyn std::error::Error>> {
    let ret = rl.parse(code)?;

    // 检查是否包含类型标记
    if !can_ret_raw && ret.contains(&*REDLANG_UUID) {
        return Err(RedLang::make_err("尝试输出非文本类型"));
    }
    return Ok(ret.to_owned());
}

pub fn do_script(rl:&mut RedLang,code:&str,script_type:&str,can_ret_raw:bool) -> Result<String, Box<dyn std::error::Error>>{
    // 增加脚本运行计数
    if add_running_script_num(&rl.pkg_name,&rl.script_name,script_type) == false {
        return Ok("".to_owned());
    }
    let pkg_name = rl.pkg_name.clone();
    let script_name = rl.script_name.clone();
    let _guard = scopeguard::guard((),|_| {
        dec_running_script_num(&pkg_name,&script_name);
    });

    // 执行脚本
    let out_str_t_rst = do_run_code_and_ret_check(rl,code,can_ret_raw);

    // 处理脚本执行错误
    if let Err(err) = out_str_t_rst {
        let err_str = format!("在包`{}`脚本`{}`中发送错误:{}",rl.pkg_name, rl.script_name,err);
        // 如果需要处理错误
        if rl.can_wrong == true {
            let err_str_t = err_str.clone();
            let exmap = (*rl.exmap).borrow().clone();
            let script_name = rl.script_name.clone();
            let pkg_name = rl.pkg_name.clone();
            let _foo = std::thread::spawn(move ||{
                
                fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
                    let pkg_name_opt = script_json.get("pkg_name");
                    let mut pkg_name = "";
                    if let Some(val) = pkg_name_opt {
                        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
                    }
                    // let name = script_json.get("name").ok_or("脚本中无name")?.as_str().ok_or("脚本中name不是str")?;
                    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
                    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
                    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
                    return Ok((cffs,code,pkg_name));
                } 
                fn fun(err_str:String,exmap:HashMap<String, Arc<String>>,pkg_name:String,script_name:String) -> Result<i32, Box<dyn std::error::Error>> {
                    let script_json = crate::read_code_cache()?;
                    let exmap_ptr = Rc::new(RefCell::new(exmap));
                    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
                        let (cffs,code,pkg_name_t) = get_script_info(&script_json[i])?;
                        if cffs == "脚本错误" {
                            if pkg_name_t != pkg_name {
                                continue;
                            }
                            let mut rl2 = crate::redlang::RedLang::new();
                            rl2.exmap = exmap_ptr.clone();
                            rl2.pkg_name = pkg_name.clone();
                            rl2.script_name = script_name.clone();
                            rl2.set_coremap("错误信息", &err_str)?;
                            rl2.can_wrong = false;
                            if let Err(err) = crate::cqevent::do_script(&mut rl2,&code,"normal",false) {
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

    // 处理分页指令
    let out_str_vec = do_paging(&out_str_t)?;

    // 发送到协议端
    for out_str in out_str_vec {
        crate::redlang::cqexfun::send_one_msg(rl, out_str)?;
    }
    return Ok(out_str_t);
}

fn set_normal_evt_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    rl.set_exmap("机器人ID", &read_json_str(root,"self_id"))?;
    rl.set_exmap("发送者ID", &read_json_str(root,"user_id"))?;
    rl.set_exmap("群ID", &read_json_str(root,"group_id"))?;
    rl.set_exmap("群组ID", &read_json_str(root,"groups_id"))?;
    rl.set_exmap("原始事件", &root.to_string())?;
    rl.set_exmap("机器人平台", &read_json_str(root,"platform"))?;
    rl.set_exmap("消息ID", &read_json_str(root,"message_id"))?;
    rl.set_exmap("消息类型", &read_json_str(root,"message_type"))?;
    rl.set_exmap("远程MQTT客户端ID", &read_json_str(root,"mqtt_client_id"))?;
    if let Some(sender) = root.get("sender") {
        if let Some(js_v) = sender.get("nickname") {
            rl.set_exmap("发送者昵称", js_v.as_str().unwrap_or(""))?;
        }
    }
    Ok(())
}

fn set_normal_message_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    set_normal_evt_info(rl,root)?;
    Ok(())
}


pub fn is_key_match(rl:&mut RedLang,ppfs:&str,keyword:&str,msg:&str) -> Result<bool, Box<dyn std::error::Error>>{
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
                if let Some(s) = cap.get(i) {
                    temp_vec.push_str(&s.as_str().len().to_string());
                    temp_vec.push(',');
                    temp_vec.push_str(s.as_str());
                }
            }
            sub_key_vec.push_str(&temp_vec.len().to_string());
            sub_key_vec.push(',');
            sub_key_vec.push_str(&temp_vec);
        }
        rl.set_exmap("子关键词", &sub_key_vec)?;
    }
    Ok(is_match)
}

fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json.get("name").ok_or("脚本中无name")?.as_str().ok_or("脚本中name不是str")?;
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let keyword = node.get("关键词").ok_or("脚本中无关键词")?.as_str().ok_or("脚本中关键词不是str")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    let ppfs = node.get("匹配方式").ok_or("脚本中无匹配方式")?.as_str().ok_or("脚本中匹配方式不是str")?;
    
    return Ok((keyword,cffs,code,ppfs,name,pkg_name));
}