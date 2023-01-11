use std::{fs, collections::BTreeMap, path::Path, env::current_exe, vec};

use crate::{cqapi::{cq_call_api, cq_get_app_directory2}, mytool::read_json_str, PAGING_UUID, redlang::{get_const_val, set_const_val}, CLEAR_UUID};
use serde_json;
use super::{RedLang, exfun::do_json_parse};

fn cq_encode(cq_code:&str) -> String {
    let mut ret_str = String::new();
    for ch in cq_code.chars() {
        let s:String;
        ret_str.push_str(match ch {
            '&' => "&amp;",
            '[' => "&#91;",
            ']' => "&#93;",
            ',' => "&#44;",
            ch_t => {
                s = ch_t.to_string();
                &s
            }
        });
    }
    return ret_str;
}
fn cq_encode_t(cq_code:&str) -> String {
    let mut ret_str = String::new();
    for ch in cq_code.chars() {
        let s:String;
        ret_str.push_str(match ch {
            '&' => "&amp;",
            '[' => "&#91;",
            ']' => "&#93;",
            ch_t => {
                s = ch_t.to_string();
                &s
            }
        });
    }
    return ret_str;
}

pub fn send_one_msg(rl:& RedLang,msg:&str) -> Result<String, Box<dyn std::error::Error>> {
    if msg == "" {
        return Ok("".to_string());
    }
    let group_id_str = rl.get_exmap("群ID").to_string();
    let guild_id_str = rl.get_exmap("频道ID").to_string();
    let channel_id_str = rl.get_exmap("子频道ID").to_string();
    let msg_type:&'static str = crate::cqevent::get_msg_type(&rl);
    // 没有设置输出流类型，所以不输出
    if msg_type == "" {
        return Ok("".to_string());
    }
    let send_json:serde_json::Value;
    if msg_type == "group" {
        send_json = serde_json::json!({
            "action":"send_group_msg",
            "params":{
                "group_id":rl.get_exmap("群ID").parse::<i64>()?,
                "message":msg
            }
        });
    }else if msg_type == "channel" {
        send_json = serde_json::json!( {
            "action":"send_guild_channel_msg",
            "params":{
                "guild_id": guild_id_str,
                "channel_id": channel_id_str,
                "message":msg
            }
        });
    }else if msg_type == "private" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":rl.get_exmap("发送者ID").parse::<i64>()?,
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
    if msg_type == "group" {
        let self_id = rl.get_exmap("机器人ID");
        crate::cqevent::do_group_msg::msg_id_map_insert(self_id.to_string(),group_id_str,msg_id.clone())?;
    }
    return Ok(msg_id);
}

pub fn init_cq_ex_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string();
            if w.contains_key(&k) {
                let err_opt:Option<String> = None;
                err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
            }
            w.insert(k, fun);
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
        let name = self_t.get_exmap("机器人名字");
        return Ok(Some(name.to_string()));
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
            let flag = qq + &group_id.to_string();
            ret = match mp.get(&flag) {
                Some(v) => self_t.build_arr(v.to_vec()),
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
    add_fun(vec!["图片"],|self_t,_params|{
        let pic = self_t.get_param(_params, 0)?;
        let tp = self_t.get_type(&pic)?;
        let mut ret:String = String::new();
        if tp == "字节集" {
            let bin = RedLang::parse_bin(&pic)?;
            let b64_str = base64::encode(bin);
            ret = format!("[CQ:image,file=base64://{}]",b64_str);
        }else if tp == "文本" {
            if pic.starts_with("http://") || pic.starts_with("https://"){
                ret = format!("[CQ:image,file={}]",cq_encode(&pic));
            }else{
                if pic.len() > 2 && pic.get(1..2).ok_or("")? == ":" {
                    let path = Path::new(&pic);
                    let bin = std::fs::read(path)?;
                    let b64_str = base64::encode(bin);
                    ret = format!("[CQ:image,file=base64://{}]",b64_str);
                }else{
                    let path_str = format!("{}\\data\\image\\{}",current_exe()?.parent().ok_or("无法获取当前exe目录")?.to_string_lossy(),&pic);
                    let path = Path::new(&path_str);
                    let bin = std::fs::read(path)?;
                    let b64_str = base64::encode(bin);
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
            let b64_str = base64::encode(bin);
            ret = format!("[CQ:record,file=base64://{}]",b64_str);
        }else if tp == "文本" {
            if pic.starts_with("http://") || pic.starts_with("https://"){
                ret = format!("[CQ:record,file={}]",cq_encode(&pic));
            }else{
                if pic.len() > 2 && pic.get(1..2).ok_or("")? == ":" {
                    let path = Path::new(&pic);
                    let bin = std::fs::read(path)?;
                    let b64_str = base64::encode(bin);
                    ret = format!("[CQ:record,file=base64://{}]",b64_str);
                }else{
                    let path_str = format!("{}\\data\\record\\{}",current_exe()?.parent().ok_or("无法获取当前exe目录")?.to_string_lossy(),&pic);
                    let path = Path::new(&path_str);
                    let bin = std::fs::read(path)?;
                    let b64_str = base64::encode(bin);
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
        return Ok(Some(cq_encode(&cq_code)));
    });
    add_fun(vec!["CQ转义"],|self_t,params|{
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_encode_t(&cq_code)));
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
        if let Some(cap) = re.captures(&data_str)? {
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
        let k = self_t.get_param(params, 0)?;
        return Ok(Some(get_const_val(&self_t.pkg_name, &k)?.to_owned()));
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
            let mut arr_val:Vec<String> = vec![];
            for word in  word_list_t{
                arr_val.push(word.to_string());
            }
            let arr_str = self_t.build_arr(arr_val);
            dict_obj.insert(key.to_owned(), arr_str);
        }
        return Ok(Some(self_t.build_obj(dict_obj)));
    });
    add_fun(vec!["应用目录"],|_self_t,_params|{
        let app_dir = cq_get_app_directory2()?;
        return Ok(Some(app_dir));
    });
    add_fun(vec!["取艾特"],|self_t,_params|{
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json:serde_json::Value = serde_json::from_str(&*raw_data)?;
        let err = "获取message失败";
        let message = raw_json.get("message").ok_or(err)?.as_array().ok_or(err)?;
        let mut ret_vec:Vec<String> = vec![];
        for it in message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "at" {
                let qq = it.get("data").ok_or(err)?.get("qq").ok_or(err)?.as_str().ok_or(err)?;
                ret_vec.push(qq.to_string());
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
        let mut ret_vec:Vec<String> = vec![];
        for it in message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "image" {
                let url = it.get("data").ok_or(err)?.get("url").ok_or(err)?.as_str().ok_or(err)?;
                ret_vec.push(url.to_string());
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
}