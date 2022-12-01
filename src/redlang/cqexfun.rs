use std::{fs, collections::HashMap, path::Path, env::current_exe};

use crate::{cqapi::{cq_call_api, cq_get_cookies, cq_get_app_directory}, mytool::read_json_str};
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

pub fn cqexfun(self_t:&mut RedLang,cmd: &str,params: &[String],) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if cmd.to_uppercase() == "发送者QQ" || cmd.to_uppercase() == "发送者ID" {
        let qq = self_t.get_exmap("发送者ID")?;
        return Ok(Some(qq.to_string()));
    }else if cmd == "当前群号" || cmd == "群号" || cmd.to_uppercase() == "群ID" {
        let group = self_t.get_exmap("群ID")?;
        return Ok(Some(group.to_string()));
    }else if cmd == "发送者昵称" {
        let nickname = self_t.get_exmap("发送者昵称")?;
        return Ok(Some(nickname.to_string()));
    }else if cmd.to_uppercase() == "机器人QQ" {
        let qq = self_t.get_exmap("机器人ID")?;
        return Ok(Some(qq.to_string()));
    }else if cmd.to_uppercase() == "机器人ID" {
        let qq:&str;
        if self_t.get_exmap("子频道ID")? != "" {
            qq = self_t.get_exmap("机器人频道ID")?;
        }else{
            qq = self_t.get_exmap("机器人ID")?;
        }
        return Ok(Some(qq.to_string()));
    }else if cmd == "机器人名字" {
        let name = self_t.get_exmap("机器人名字")?;
        return Ok(Some(name.to_string()));
    }else if cmd == "权限" || cmd == "发送者权限" {
        let role = self_t.get_exmap("发送者权限")?;
        return Ok(Some(role.to_string()));
    }else if cmd == "发送者名片" {
        let card = self_t.get_exmap("发送者名片")?;
        return Ok(Some(card.to_string()));
    }else if cmd == "发送者专属头衔" {
        let title = self_t.get_exmap("发送者专属头衔")?;
        return Ok(Some(title.to_string()));
    }
    else if cmd.to_uppercase() == "消息ID" {
        let msg_id = self_t.get_exmap("消息ID")?;
        return Ok(Some(msg_id.to_string()));
    }
    else if cmd.to_uppercase() == "当前频道ID" {
        let guild_id = self_t.get_exmap("频道ID")?;
        return Ok(Some(guild_id.to_string()));
    }
    else if cmd.to_uppercase() == "当前子频道ID" {
        let channel_id = self_t.get_exmap("子频道ID")?;
        return Ok(Some(channel_id.to_string()));
    }
    else if cmd == "图片" {
        let pic = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&pic)?;
        let mut ret:String = String::new();
        if tp == "字节集" {
            let bin = self_t.parse_bin(&pic)?;
            let b64_str = base64::encode(bin);
            ret = format!("[CQ:image,file=base64://{}]",b64_str);
        }else if tp == "文本" {
            if pic.starts_with("http://") || pic.starts_with("https://"){
                ret = format!("[CQ:image,file={}]",pic);
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
    }
    else if cmd == "撤回" {
        let mut msg_id = self_t.get_param(params, 0)?;
        if msg_id == ""{
            msg_id = self_t.get_exmap("消息ID")?.to_string();
        }
        if msg_id == "" {
            return Ok(Some("".to_string()));
        }
        if self_t.get_exmap("子频道ID")? != "" {
            let send_json = serde_json::json!({
                "action":"delete_msg",
                "params":{
                    "message_id":msg_id
                }
            });
            cq_call_api(&send_json.to_string())?;
        }else{
            let int32_msg_id = msg_id.parse::<i32>()?;
            let send_json = serde_json::json!({
                "action":"delete_msg",
                "params":{
                    "message_id":int32_msg_id
                }
            });
            cq_call_api(&send_json.to_string())?;
        }
        return Ok(Some("".to_string()));
    }else if cmd == "输出流" {
        let user_id_str = self_t.get_exmap("发送者ID")?.to_string();
        let group_id_str = self_t.get_exmap("群ID")?.to_string();
        let guild_id_str = self_t.get_exmap("频道ID")?.to_string();
        let channel_id_str = self_t.get_exmap("子频道ID")?.to_string();
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
        if msg_type == "" {
            return Ok(Some("".to_string()));
        }
        let send_json:serde_json::Value;
        if msg_type == "group" {
            send_json = serde_json::json!({
                "action":"send_group_msg",
                "params":{
                    "group_id":self_t.get_exmap("群ID")?.parse::<i64>()?,
                    "message":self_t.get_param(params, 0)?
                }
            });
        }else if msg_type == "channel" {
            send_json = serde_json::json!( {
                "action":"send_guild_channel_msg",
                "params":{
                    "guild_id": guild_id_str,
                    "channel_id": channel_id_str,
                    "message":self_t.get_param(params, 0)?
                }
            });
        }else if msg_type == "private" {
            send_json = serde_json::json!( {
                "action":"send_private_msg",
                "params":{
                    "user_id":self_t.get_exmap("发送者ID")?.parse::<i64>()?,
                    "message":self_t.get_param(params, 0)?
                }
            });
        }else{
            return Err(self_t.make_err(&("不支持的输出流:".to_string() + msg_type)));
        }
        let cq_ret = cq_call_api(send_json.to_string().as_str())?;
        let ret_json:serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = "输出流调用失败,retcode 不为0";
        if ret_json.get("retcode").ok_or(err)?.as_i64().ok_or(err)? != 0 {
            return Err(self_t.make_err(&format!("{}:{}",err,cq_ret)));
        }
        let err = "输出流调用失败，获取message_id失败";
        let msg_id = read_json_str(ret_json.get("data").ok_or(err)?,"message_id");
        return Ok(Some(msg_id.to_string()));
    }else if cmd == "艾特" {
        let mut user_id = self_t.get_param(params, 0)?;
        if user_id == ""{
            user_id = self_t.get_exmap("发送者ID")?.to_string();
        }
        if user_id == "" {
            return Ok(Some("".to_string()));
        }else{
            return Ok(Some(format!("[CQ:at,qq={}]",user_id)));
        }
    }else if cmd.to_uppercase() == "CQ码转义" {
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_encode(&cq_code)));
    }
    else if cmd.to_uppercase() == "CQ转义" {
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_encode_t(&cq_code)));
    }
    else if cmd == "子关键词" {
        let key = self_t.get_exmap("子关键词")?.to_string();
        return Ok(Some(key));
    }else if cmd == "事件内容" {
        let dat = self_t.get_exmap("事件内容")?;
        if dat == "" {
            let raw_data = self_t.get_exmap("原始事件")?;
            let raw_json = serde_json::from_str(raw_data)?;
            let redlang_str = do_json_parse(&raw_json,&self_t.type_uuid)?;
            self_t.set_exmap("事件内容", &redlang_str)?;
            return Ok(Some(redlang_str));
        }
        return Ok(Some(dat.to_string()));
    }else if cmd.to_uppercase() == "OB调用" {
        self_t.get_param(params, 0)?;
        let content = self_t.get_param(params, 1)?;
        let call_ret = cq_call_api(&content)?;
        let js_v:serde_json::Value = serde_json::from_str(&call_ret)?;
        let ret = do_json_parse(&js_v, &self_t.type_uuid)?;
        return Ok(Some(ret));
    }else if cmd.to_uppercase() == "CQ码解析" {
        let data_str = self_t.get_param(params, 0)?;
        let pos1 = data_str.find(",").ok_or("CQ码解析失败")?;
        let tp = data_str.get(4..pos1).ok_or("CQ码解析失败")?;
        let mut sub_key_obj:HashMap<String,String> = HashMap::new();
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
    }else if cmd.to_uppercase() == "CQ反转义" {
        let content = self_t.get_param(params, 0)?;
        let content = content.replace("&#91;", "[");
        let content = content.replace("&#93;", "]");
        let content = content.replace("&amp;", "&");
        return Ok(Some(content));
    }else if cmd == "定义常量" {
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        let mut mp = crate::G_CONST_MAP.write()?;
        mp.insert(k, v);
        return Ok(Some("".to_string()));
    }else if cmd == "常量" {
        let k = self_t.get_param(params, 0)?;
        let mp = crate::G_CONST_MAP.read()?;
        let defstr = String::new();
        let ret = mp.get(k.as_str()).unwrap_or(&defstr);
        return Ok(Some(ret.to_string()));
    }else if cmd.to_uppercase() == "进程ID" {
        let ret = cq_get_cookies("pid")?;
        return Ok(Some(ret.to_string()));
    }else if cmd.to_uppercase() == "CPU使用" {
        let ret = cq_get_cookies("cpu_usage")?;
        return Ok(Some(ret.to_string()));
    }else if cmd == "内存使用" {
        let ret = cq_get_cookies("mem_usage")?;
        return Ok(Some(ret.to_string()));
    }else if cmd == "读词库文件" {
        let path = self_t.get_param(params, 0)?;
        let path_t = path.clone();
        let file_dat = fs::read_to_string(path)?;
        let file_dat_without_r = file_dat.replace('\r', "");
        let words_list = file_dat_without_r.split("\n\n");
        let mut dict_obj:HashMap<String,String> = HashMap::new();
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
    }else if cmd == "应用目录" {
        let app_dir = cq_get_app_directory()?;
        return Ok(Some(app_dir));
    }
    return Ok(None);
}