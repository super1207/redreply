use std::{fs, collections::BTreeMap, path::{Path, PathBuf}, env::current_exe, vec, str::FromStr, sync::Arc, thread, time::SystemTime};

use crate::{cqapi::{cq_call_api, cq_get_app_directory2, cq_get_app_directory1}, mytool::{read_json_str, cq_params_encode, cq_text_encode}, PAGING_UUID, redlang::{get_const_val, set_const_val}, CLEAR_UUID, G_INPUTSTREAM_VEC,G_SCRIPT_RELATE_MSG, ScriptRelatMsg};
use serde_json;
use super::{RedLang, exfun::do_json_parse};
use base64::{Engine as _, engine::{self, general_purpose}, alphabet};
const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

pub fn get_app_dir(pkg_name:&str) -> Result<String, Box<dyn std::error::Error>> {
    let app_dir;
        if pkg_name == "" {
            app_dir = cq_get_app_directory2()?;
        }else{
            let plus_dir_str = cq_get_app_directory1()?;
            let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
            app_dir = pkg_dir.join(pkg_name).to_str().ok_or("获得应用目录失败")?.to_owned() + &std::path::MAIN_SEPARATOR.to_string();
        }
    return Ok(app_dir)
}


fn get_sub_id(rl:& RedLang,msg_type:&str) -> String {
    let sub_id;
    if msg_type == "group" {
        sub_id = rl.get_exmap("群ID").to_string();
    } else if msg_type == "channel" {
        sub_id = rl.get_exmap("子频道ID").to_string();
    } else {
        sub_id = "".to_owned();
    }
    sub_id
}

pub fn send_one_msg(rl:& RedLang,msg:&str) -> Result<String, Box<dyn std::error::Error>> {
    if msg == "" {
        return Ok("".to_string());
    }
    let msg_type:&'static str = crate::cqevent::get_msg_type(&rl);
    let sub_id = get_sub_id(rl,msg_type);
    let guild_id_str = rl.get_exmap("频道ID").to_string();
    
    // 没有设置输出流类型，所以不输出
    if msg_type == "" {
        return Ok("".to_string());
    }
    let send_json:serde_json::Value;
    if msg_type == "group" {
        send_json = serde_json::json!({
            "action":"send_group_msg",
            "params":{
                "group_id":sub_id.parse::<u64>()?,
                "message":msg
            }
        });
    }else if msg_type == "channel" {
        send_json = serde_json::json!( {
            "action":"send_guild_channel_msg",
            "params":{
                "guild_id": guild_id_str,
                "channel_id": sub_id,
                "message":msg
            }
        });
    }else if msg_type == "private" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":rl.get_exmap("发送者ID").parse::<u64>()?,
                "message":msg
            }
        });
    }else{
        return Err(RedLang::make_err(&("不支持的输出流:".to_string() + msg_type)));
    }
    let self_id = rl.get_exmap("机器人ID");
    let cq_ret = cq_call_api(&*self_id,send_json.to_string().as_str())?;
    let ret_json:serde_json::Value = serde_json::from_str(&cq_ret)?;
    let err = "输出流调用失败,retcode 不为0";
    if ret_json.get("retcode").ok_or(err)?.as_i64().ok_or(err)? != 0 {
        return Err(RedLang::make_err(&format!("{}:{}",err,cq_ret)));
    }
    let err = "输出流调用失败，获取message_id失败";
    let msg_id = read_json_str(ret_json.get("data").ok_or(err)?,"message_id");
    {
        let mut lk = G_SCRIPT_RELATE_MSG.write()?;
        let src_msg_id = rl.get_exmap("消息ID");
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        if *src_msg_id != "" {
            let key = format!("{}|{}|{}",rl.pkg_name,self_id,src_msg_id);
            let val_opt = lk.get_mut(&key);
            if val_opt.is_none() {
                
                let vc = ScriptRelatMsg {
                    self_id: (*self_id).clone(),
                    msg_id_vec: vec![msg_id.clone()],
                    create_time:tm
                };
                lk.insert(key, vc);
            }else   
            {
                let vc = val_opt.unwrap();
                vc.msg_id_vec.push(msg_id.clone());
            }
        }
        let mut del_msg_vec = vec![];
        for it in &*lk {
            if tm - it.1.create_time > 300 {
                del_msg_vec.push(it.0.to_owned());
            }
        }
        for it in del_msg_vec {
            lk.remove(&it);
        }
    }
    if msg_type == "group" {
        let self_id = rl.get_exmap("机器人ID");
        crate::cqevent::do_group_msg::msg_id_map_insert(self_id.to_string(),self_id.to_string(),sub_id,msg_id.clone())?;
    }
    return Ok(msg_id);
}

pub fn init_cq_ex_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string();
            let k_t = crate::mytool::cmd_to_jt(&k);
            if k == k_t {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
            }else {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
                if w.contains_key(&k_t) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k_t)).unwrap();
                }
                w.insert(k_t, fun);
            }
        }
    }
    add_fun(vec!["发送者ID","发送者QQ"],|self_t,_params|{
        let qq = self_t.get_exmap("发送者ID");
        return Ok(Some(qq.to_string()));
    });
    add_fun(vec!["当前群号","群号","群ID"],|self_t,_params|{
        let group = self_t.get_exmap("群ID");
        return Ok(Some(group.to_string()));
    });
    add_fun(vec!["发送者昵称"],|self_t,_params|{
        let nickname = self_t.get_exmap("发送者昵称");
        return Ok(Some(nickname.to_string()));
    });
    add_fun(vec!["机器人QQ"],|self_t,_params|{
        let qq = self_t.get_exmap("机器人ID");
        return Ok(Some(qq.to_string()));
    });
    add_fun(vec!["机器人ID"],|self_t,_params|{
        let qq:String;
        if *self_t.get_exmap("子频道ID") != "" {
            qq = self_t.get_exmap("机器人频道ID").to_string();
        }else{
            qq = self_t.get_exmap("机器人ID").to_string();
        }
        return Ok(Some(qq));
    });
    add_fun(vec!["机器人名字"],|self_t,_params|{
        let send_json = serde_json::json!({
            "action":"get_login_info",
            "params":{}
        });
        let self_id = self_t.get_exmap("机器人ID");
        let cq_ret = cq_call_api(&self_id,&send_json.to_string())?;
        let ret_json:serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = "获机器人名字失败";
        let bot_name = ret_json.get("data").ok_or(err)?.get("nickname").ok_or(err)?.as_str().ok_or(err)?;
        return Ok(Some(bot_name.to_string()));
    });
    add_fun(vec!["权限","发送者权限"],|self_t,_params|{
        let role = self_t.get_exmap("发送者权限");
        return Ok(Some(role.to_string()));
    });
    add_fun(vec!["发送者名片"],|self_t,_params|{
        let card = self_t.get_exmap("发送者名片");
        return Ok(Some(card.to_string()));
    });
    add_fun(vec!["发送者专属头衔"],|self_t,_params|{
        let title = self_t.get_exmap("发送者专属头衔");
        return Ok(Some(title.to_string()));
    });
    add_fun(vec!["消息ID"],|self_t,params|{
        let qq = self_t.get_param(params, 0)?;
        let ret:String;
        if qq == "" {
            let msg_id = self_t.get_exmap("消息ID");
            ret = msg_id.to_string();
        }else {
            let mp = crate::G_MSG_ID_MAP.read()?;
            let group_id = self_t.get_exmap("群ID").parse::<i32>()?;
            let self_id = self_t.get_exmap("机器人ID");
            let flag = self_id.to_string() + &qq + &group_id.to_string();
            ret = match mp.get(&flag) {  
                Some(v) => {
                    let mut vv:Vec<&str> = vec![];
                    for it in v {
                        vv.push(it);
                    }
                    self_t.build_arr(vv)
                },
                None => self_t.build_arr(vec![])
            };
        }
        return Ok(Some(ret));
    });
    add_fun(vec!["当前频道ID"],|self_t,_params|{
        let guild_id = self_t.get_exmap("频道ID");
        return Ok(Some(guild_id.to_string()));
    });
    add_fun(vec!["当前子频道ID"],|self_t,_params|{
        let channel_id = self_t.get_exmap("子频道ID");
        return Ok(Some(channel_id.to_string()));
    });
    add_fun(vec!["图片"],|self_t,params|{
        let pic = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&pic)?;
        let mut ret:String = String::new();
        if tp == "字节集" {
            let bin = RedLang::parse_bin(&pic)?;
            let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
            ret = format!("[CQ:image,file=base64://{}]",b64_str);
        }else if tp == "文本" {
            if pic.starts_with("http://") || pic.starts_with("https://"){
                let not_use_cache = self_t.get_param(params, 1)?;
                if  not_use_cache == "假" {
                    ret = format!("[CQ:image,file={},cache=0]",cq_params_encode(&pic));
                }else {
                    ret = format!("[CQ:image,file={}]",cq_params_encode(&pic));
                }
            }else{
                if pic.len() > 2 && pic.get(1..2).ok_or("")? == ":" {
                    let path = Path::new(&pic);
                    let bin = std::fs::read(path)?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                    ret = format!("[CQ:image,file=base64://{}]",b64_str);
                }else{
                    let path_str = format!("{}\\data\\image\\{}",current_exe()?.parent().ok_or("无法获取当前exe目录")?.to_string_lossy(),&pic);
                    let path = Path::new(&path_str);
                    let bin = std::fs::read(path)?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                    ret = format!("[CQ:image,file=base64://{}]",b64_str);
                }
            }
        }
        return Ok(Some(ret));
    });
    add_fun(vec!["语音"],|self_t,params|{
        let pic = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&pic)?;
        let mut ret:String = String::new();
        if tp == "字节集" {
            let bin = RedLang::parse_bin(&pic)?;
            let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
            ret = format!("[CQ:record,file=base64://{}]",b64_str);
        }else if tp == "文本" {
            if pic.starts_with("http://") || pic.starts_with("https://"){
                let not_use_cache = self_t.get_param(params, 1)?;
                if  not_use_cache == "假" {
                    ret = format!("[CQ:record,file={},cache=0]",cq_params_encode(&pic));
                }else {
                    ret = format!("[CQ:record,file={}]",cq_params_encode(&pic));
                }
            }else{
                if pic.len() > 2 && pic.get(1..2).ok_or("")? == ":" {
                    let path = Path::new(&pic);
                    let bin = std::fs::read(path)?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                    ret = format!("[CQ:record,file=base64://{}]",b64_str);
                }else{
                    let path_str = format!("{}\\data\\record\\{}",current_exe()?.parent().ok_or("无法获取当前exe目录")?.to_string_lossy(),&pic);
                    let path = Path::new(&path_str);
                    let bin = std::fs::read(path)?;
                    
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                    ret = format!("[CQ:record,file=base64://{}]",b64_str);
                }
            }
        }
        return Ok(Some(ret));
    });
    add_fun(vec!["撤回"],|self_t,params|{
        let mut msg_id_str = self_t.get_param(params, 0)?;
        if msg_id_str == "" {
            msg_id_str = self_t.get_exmap("消息ID").to_string();
        }
        let tp = self_t.get_type(&msg_id_str)?;
        let msg_id_vec:Vec<&str> = match tp.as_str() {
            "文本" => vec![&msg_id_str],
            "数组" => RedLang::parse_arr(&msg_id_str)?,
            _ => vec![]
        };
        for it in msg_id_vec {
            if *self_t.get_exmap("子频道ID") != "" {
                let send_json = serde_json::json!({
                    "action":"delete_msg",
                    "params":{
                        "message_id":it
                    }
                });
                let self_id = self_t.get_exmap("机器人ID");
                cq_call_api(&self_id,&send_json.to_string())?;
            }else{
                let int32_msg_id = it.parse::<i32>()?;
                let send_json = serde_json::json!({
                    "action":"delete_msg",
                    "params":{
                        "message_id":int32_msg_id
                    }
                });
                let self_id = self_t.get_exmap("机器人ID");
                cq_call_api(&self_id,&send_json.to_string())?;
            }
        }  
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["禁言"],|self_t,params|{
        let ban_time = self_t.get_param(params, 0)?;
        let user_id_str = self_t.get_exmap("发送者ID").to_string();
        let group_id_str = self_t.get_exmap("群ID").to_string();
        let send_json = serde_json::json!({
            "action":"set_group_ban",
            "params":{
                "group_id": group_id_str,
                "user_id": user_id_str,
                "duration":ban_time.parse::<usize>()?
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        cq_call_api(&self_id,&send_json.to_string())?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["输出流"],|self_t,params|{
        let msg = self_t.get_param(params, 0)?;
        let msg_id = send_one_msg(&self_t,&msg)?;
        return Ok(Some(msg_id));
    });
    add_fun(vec!["艾特"],|self_t,params|{
        let mut user_id = self_t.get_param(params, 0)?;
        if user_id == ""{
            user_id = self_t.get_exmap("发送者ID").to_string();
        }
        if user_id == "" {
            return Ok(Some("".to_string()));
        }else{
            return Ok(Some(format!("[CQ:at,qq={}]",user_id)));
        }
    });
    add_fun(vec!["CQ码转义"],|self_t,params|{
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_params_encode(&cq_code)));
    });
    add_fun(vec!["CQ转义"],|self_t,params|{
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_text_encode(&cq_code)));
    });
    add_fun(vec!["子关键词"],|self_t,_params|{
        let key = self_t.get_exmap("子关键词").to_string();
        return Ok(Some(key));
    });
    add_fun(vec!["事件内容"],|self_t,_params|{
        let dat = self_t.get_exmap("事件内容");
        if *dat == "" {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json = serde_json::from_str(&*raw_data)?;
            let redlang_str = do_json_parse(&raw_json,&self_t.type_uuid)?;
            self_t.set_exmap("事件内容", &redlang_str)?;
            return Ok(Some(redlang_str));
        }
        return Ok(Some(dat.to_string()));
    });
    add_fun(vec!["OB调用"],|self_t,params|{
        let content = self_t.get_param(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let call_ret = cq_call_api(&*self_id,&content)?;
        let js_v:serde_json::Value = serde_json::from_str(&call_ret)?;
        let ret = do_json_parse(&js_v, &self_t.type_uuid)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["CQ码解析"],|self_t,params|{
        let data_str = self_t.get_param(params, 0)?;
        let pos1 = data_str.find(",").ok_or("CQ码解析失败")?;
        let tp = data_str.get(4..pos1).ok_or("CQ码解析失败")?;
        let mut sub_key_obj:BTreeMap<String,String> = BTreeMap::new();
        sub_key_obj.insert("type".to_string(), tp.to_string());
        let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])")?;

        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            if len == 3 {
                let key = &cap[1];
                let val = &cap[2];
                let key = key.replace("&#91;", "[");
                let key = key.replace("&#93;", "]");
                let key = key.replace("&#44;", ",");
                let key = key.replace("&amp;", "&");
                let val = val.replace("&#91;", "[");
                let val = val.replace("&#93;", "]");
                let val = val.replace("&#44;", ",");
                let val = val.replace("&amp;", "&");
                sub_key_obj.insert(key, val);
            }
        }
        return Ok(Some(self_t.build_obj(sub_key_obj)));
    });
    add_fun(vec!["CQ反转义"],|self_t,params|{
        let content = self_t.get_param(params, 0)?;
        let content = content.replace("&#91;", "[");
        let content = content.replace("&#93;", "]");
        let content = content.replace("&amp;", "&");
        return Ok(Some(content));
    });
    add_fun(vec!["定义常量"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        set_const_val(&self_t.pkg_name, &k, v)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["常量"],|self_t,params|{
        let params_len = params.len();
        if params_len == 1 { // 取当前包的常量
            let k = self_t.get_param(params, 0)?;
            return Ok(Some(get_const_val(&self_t.pkg_name, &k)?.to_owned()));
        }else{ // 取其它包的常量
            let pkg_name = self_t.get_param(params, 0)?;
            let k = self_t.get_param(params, 1)?;
            return Ok(Some(get_const_val(&pkg_name, &k)?.to_owned()));
        }
    });
    add_fun(vec!["进程ID"],|_self_t,_params|{
        return Ok(Some(std::process::id().to_string()));
    });
    add_fun(vec!["CPU使用"],|_self_t,_params|{
        //let ret = cq_get_cookies("cpu_usage")?;
        return Ok(Some("0".to_string()));
    });
    add_fun(vec!["内存使用"],|_self_t,_params|{
        //let ret = cq_get_cookies("mem_usage")?;
        return Ok(Some("0".to_string()));
    });
    add_fun(vec!["读词库文件"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path_t = path.clone();
        let file_dat = fs::read_to_string(path)?;
        let file_dat_without_r = file_dat.replace('\r', "");
        let words_list = file_dat_without_r.split("\n\n");
        let mut dict_obj:BTreeMap<String,String> = BTreeMap::new();
        let err = format!("词库文件格式错误:`{}`", &path_t);
        for words in words_list {
            let word_list = words.split('\n').collect::<Vec<&str>>();
            let key:&str = word_list.get(0).ok_or(err.clone())?;
            let word_list_t = word_list.get(1..).ok_or(err.clone())?;
            let mut arr_val:Vec<&str> = vec![];
            for word in  word_list_t{
                arr_val.push(word);
            }
            let arr_str = self_t.build_arr(arr_val);
            dict_obj.insert(key.to_owned(), arr_str);
        }
        return Ok(Some(self_t.build_obj(dict_obj)));
    });
    add_fun(vec!["应用目录"],|self_t,_params|{
        let app_dir;
        app_dir = get_app_dir(&self_t.pkg_name)?;
        return Ok(Some(app_dir));
    });
    add_fun(vec!["取艾特"],|self_t,_params|{
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json:serde_json::Value = serde_json::from_str(&*raw_data)?;
        let err = "获取message失败";
        let message = raw_json.get("message").ok_or(err)?.as_array().ok_or(err)?;
        let mut ret_vec:Vec<&str> = vec![];
        for it in message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "at" {
                let qq = it.get("data").ok_or(err)?.get("qq").ok_or(err)?.as_str().ok_or(err)?;
                ret_vec.push(qq);
            }
        }
        let ret = self_t.build_arr(ret_vec);
        return Ok(Some(ret));
    });
    add_fun(vec!["取图片"],|self_t,_params|{
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json:serde_json::Value = serde_json::from_str(&*raw_data)?;
        let err = "获取message失败";
        let message = raw_json.get("message").ok_or(err)?.as_array().ok_or(err)?;
        let mut ret_vec:Vec<&str> = vec![];
        for it in message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "image" {
                let url = it.get("data").ok_or(err)?.get("url").ok_or(err)?.as_str().ok_or(err)?;
                ret_vec.push(url);
            }
        }
        let ret = self_t.build_arr(ret_vec);
        return Ok(Some(ret));
    });
    add_fun(vec!["分页"],|_self_t,_params|{
        return Ok(Some(PAGING_UUID.to_string()));
    });
    add_fun(vec!["设置来源"],|self_t,params|{
        let key = self_t.get_param(params, 0)?;
        if ["机器人ID","机器人频道ID","频道ID","子频道ID","群ID","发送者ID"].contains(&key.as_str()){
            if key.contains("频道") {
                self_t.set_exmap("群ID","")?;
            }else if key.contains("群") {
                self_t.set_exmap("机器人频道ID","")?;
                self_t.set_exmap("频道ID","")?;
                self_t.set_exmap("子频道ID","")?;
            }
            let val = self_t.get_param(params, 1)?;
            self_t.set_exmap(&key, &val)?;
        }else {
            let val = self_t.get_param(params, 1)?;
            self_t.set_exmap(&key, &val)?;
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["清空"],|_self_t,_params|{
        return Ok(Some(CLEAR_UUID.to_string()));
    });
    add_fun(vec!["获取消息"],|self_t,params|{
        let msg_id = self_t.get_param(params, 0)?;
        let send_json = serde_json::json!({
            "action":"get_msg",
            "params":{
                "message_id":msg_id.parse::<i32>()?
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let cq_ret = cq_call_api(&self_id,&send_json.to_string())?;
        let ret_json:serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取消息失败:{ret_json}");
        let raw_message = crate::mytool::json_to_cq_str(ret_json.get("data").ok_or(err)?)?;
        return Ok(Some(raw_message));
    });
    add_fun(vec!["当前消息"],|self_t,_params|{
        let msg = self_t.get_exmap("当前消息");
        return Ok(Some(msg.to_string()));
    });
    add_fun(vec!["输入流"],|self_t,params|{
        let tm = self_t.get_param(params, 0)?;
        let d = std::time::Duration::from_millis(tm.parse::<u64>().unwrap_or(15000));
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let guild_id = self_t.get_exmap("频道ID");
        let channel_id = self_t.get_exmap("子频道ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (std::sync::mpsc::Sender<String>, std::sync::mpsc::Receiver<String>) = std::sync::mpsc::channel();
        let ip = crate::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            guild_id: guild_id.to_string(),
            channel_id: channel_id.to_string(),
            echo: echo.clone(),
            stream_type:"输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        {
            let mut lk_vec = G_INPUTSTREAM_VEC.write()?;
            lk_vec.push(ip);
        }
        let _guard = scopeguard::guard(echo, |echo| {
            let mut lk_vec = G_INPUTSTREAM_VEC.write().unwrap();
            let mut pos = 0usize;
            let mut isfind = false;
            for i in 0..lk_vec.len() {
                if lk_vec[i].echo == echo {
                    pos = i;
                    isfind = true;
                    break;
                }
            }
            if isfind {
                lk_vec.remove(pos);
            }
        });
        
        let rv = rx.recv_timeout(d);
        let mut ret_str = String::new();
        if let Ok(msg) = rv {
            ret_str = msg;
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["群输入流"],|self_t,params|{
        let tm = self_t.get_param(params, 0)?;
        let d = std::time::Duration::from_millis(tm.parse::<u64>().unwrap_or(15000));
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let guild_id = self_t.get_exmap("频道ID");
        let channel_id = self_t.get_exmap("子频道ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (std::sync::mpsc::Sender<String>, std::sync::mpsc::Receiver<String>) = std::sync::mpsc::channel();
        let ip = crate::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            guild_id: guild_id.to_string(),
            channel_id: channel_id.to_string(),
            echo: echo.clone(),
            stream_type:"群输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        {
            let mut lk_vec = G_INPUTSTREAM_VEC.write()?;
            lk_vec.push(ip);
        }
        let _guard = scopeguard::guard(echo, |echo| {
            let mut lk_vec = G_INPUTSTREAM_VEC.write().unwrap();
            let mut pos = 0usize;
            let mut isfind = false;
            for i in 0..lk_vec.len() {
                if lk_vec[i].echo == echo {
                    pos = i;
                    isfind = true;
                    break;
                }
            }
            if isfind {
                lk_vec.remove(pos);
            }
        });
        
        let rv = rx.recv_timeout(d);
        let mut ret_str = self_t.build_obj(BTreeMap::new());
        if let Ok(msg) = rv {
            let js:serde_json::Value = serde_json::from_str(&msg).unwrap();
            let js_obj = js.as_object().unwrap();
            let mut mp:BTreeMap::<String,String> = BTreeMap::new();
            mp.insert("发送者ID".to_string(), js_obj["发送者ID"].as_str().unwrap().to_owned());
            mp.insert("消息".to_string(), js_obj["消息"].as_str().unwrap().to_owned());
            ret_str = self_t.build_obj(mp);
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["BOT权限"],|self_t,_params|{
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("机器人ID");
        let send_json = serde_json::json!({
            "action":"get_group_member_info",
            "params":{
                "group_id":group_id.to_string(),
                "user_id":user_id.to_string()
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let cq_ret = cq_call_api(&self_id,&send_json.to_string())?;
        let ret_json:serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取BOT权限失败:{ret_json}");
        let dat_json = ret_json.get("data").ok_or(err)?;
        let role = read_json_str(dat_json,"role");
        let role_ret;
        if role == "admin" {
            role_ret = "管理"
        }else if role == "owner" {
            role_ret = "群主"
        }else if role == "member" {
            role_ret = "群员"
        }else {
            return Err(RedLang::make_err("获取BOT权限失败:返回的json中无role字段"));
        }
        return Ok(Some(role_ret.to_string()));
    });
    add_fun(vec!["伪造OB事件"],|self_t,params|{
        let ob_event = self_t.get_param(params, 0)?;
        thread::spawn(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&ob_event) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
        return Ok(Some("".to_owned()));
    });
    add_fun(vec!["同意"],|self_t,params|{
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json:serde_json::Value = serde_json::from_str(&*raw_data)?;
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let remark = self_t.get_param(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add"  || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":true,
                    "reason":"".to_owned()
                }
            });
            
            cq_call_api(&self_id,&send_json.to_string())?;
        }else if request_type == "friend"{
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":true,
                    "remark":remark
                }
            });
            cq_call_api(&self_id,&send_json.to_string())?;
        }
        return Ok(Some("".to_owned()));
    });
    add_fun(vec!["拒绝"],|self_t,params|{
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json:serde_json::Value = serde_json::from_str(&*raw_data)?;
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let reason = self_t.get_param(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add"  || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":false,
                    "reason":reason
                }
            });
            cq_call_api(&self_id,&send_json.to_string())?;
        }else if request_type == "friend"{
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":false,
                    "remark":"".to_owned()
                }
            });
            cq_call_api(&self_id,&send_json.to_string())?;
        }
        return Ok(Some("".to_owned()));
    });
    add_fun(vec!["脚本输出"],|self_t,params|{
        let src_msg_id = self_t.get_param(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let key = format!("{}|{}|{}",self_t.pkg_name,self_id,src_msg_id);
        let lk = G_SCRIPT_RELATE_MSG.read()?;
        let val_opt = lk.get(&key);
        if val_opt.is_none() {
            return Ok(Some(self_t.build_arr(vec![])));
        }else{
            let val = val_opt.unwrap();
            let msg_id_vec = &val.msg_id_vec;
            let ret_vec = msg_id_vec.iter().map(AsRef::as_ref).collect();
            return Ok(Some(self_t.build_arr(ret_vec)));
        }
    });
    add_fun(vec!["积分-增加"],|self_t,params|{
        let key0 = self_t.get_exmap("子频道ID");
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}{}",key0,key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let add_score = self_t.get_param(params, 0)?.parse::<i64>()?;

        // 创建表
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let conn = rusqlite::Connection::open(sql_file)?;
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;
        
        // 查询积分
        let ret_rst:Result<i64,rusqlite::Error> = conn.query_row("SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?", [group_id.clone(),user_id.clone()], |row| row.get(0));
        let mut ret_num:i64;
        if let Ok(ret) =  ret_rst {
            ret_num = ret;
        }else {
            ret_num = 0;
        }

        // 积分变动
        ret_num += add_score;
        if ret_num < 0 {
            ret_num = 0;
        }

        // 积分设置
        conn.execute("REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)", [group_id,user_id,ret_num.to_string()])?;
        
        return Ok(Some("".to_string()));
    });

    add_fun(vec!["积分-设置"],|self_t,params|{
        let key0 = self_t.get_exmap("子频道ID");
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}{}",key0,key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let set_score = self_t.get_param(params, 0)?.parse::<u32>()?;

        // 创建表
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let conn = rusqlite::Connection::open(sql_file)?;
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;
        
        // 积分设置
        conn.execute("REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)", [group_id,user_id,set_score.to_string()])?;
        
        return Ok(Some("".to_string()));
    });


    add_fun(vec!["积分"],|self_t,_params|{
        let key0 = self_t.get_exmap("子频道ID");
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}{}",key0,key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        // 查询积分
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let conn = rusqlite::Connection::open(sql_file)?;
        let ret_rst:Result<i64,rusqlite::Error> = conn.query_row("SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?", [group_id.clone(),user_id.clone()], |row| row.get(0));
        let ret_num:i64;
        if let Ok(ret) =  ret_rst {
            ret_num = ret;
        }else {
            ret_num = 0;
        }

        return Ok(Some(ret_num.to_string()));
    });

    add_fun(vec!["积分-排行"],|self_t,params|{
        let key0 = self_t.get_exmap("子频道ID");
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}{}",key0,key1);

        let limit = self_t.get_param(params, 0)?;
        let limit_num;
        if limit == "" {
            limit_num = 10;
        }else{
            limit_num = limit.parse::<i32>()?;
        }

        // 查询积分
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let conn = rusqlite::Connection::open(sql_file)?;
        let mut stmt = conn.prepare("SELECT USER_ID,VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? ORDER BY VALUE DESC LIMIT ?")?;
        let mut rows = stmt.query(rusqlite::params_from_iter([group_id,limit_num.to_string()]))?;
        let mut vec:Vec<String> = vec![];
        while let Some(row) = rows.next()? {
            let mut v:Vec<String> = vec![];
            for i in 0..2 {
                let k = row.get_ref_unwrap(i);
                let dat = match k.data_type(){
                    rusqlite::types::Type::Null => "".to_string(),
                    rusqlite::types::Type::Integer => k.as_i64().unwrap().to_string(),
                    rusqlite::types::Type::Real => k.as_f64().unwrap().to_string(),
                    rusqlite::types::Type::Text => k.as_str().unwrap().to_owned(),
                    rusqlite::types::Type::Blob => self_t.build_bin(k.as_blob().unwrap().to_vec())
                };
                v.push(dat);
            }
            vec.push(self_t.build_arr(v.iter().map(AsRef::as_ref).collect()));
        }
        return Ok(Some(self_t.build_arr(vec.iter().map(AsRef::as_ref).collect())));
    });
}