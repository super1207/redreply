use crate::cqapi::{cq_call_api};
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
    if cmd == "发送者QQ" || cmd == "发送者ID" {
        let qq = self_t.get_exmap("发送者ID")?;
        return Ok(Some(qq.to_string()));
    }else if cmd == "当前群号" || cmd == "群号" || cmd == "群ID" {
        let group = self_t.get_exmap("群ID")?;
        return Ok(Some(group.to_string()));
    }else if cmd == "发送者昵称" {
        let nickname = self_t.get_exmap("发送者昵称")?;
        return Ok(Some(nickname.to_string()));
    }else if cmd == "机器人QQ" || cmd == "机器人ID" {
        let qq = self_t.get_exmap("机器人ID")?;
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
    else if cmd == "消息ID" {
        let msg_id = self_t.get_exmap("消息ID")?;
        return Ok(Some(msg_id.to_string()));
    }else if cmd == "撤回" {
        let mut msg_id = self_t.get_param(params, 0)?;
        if msg_id == ""{
            msg_id = self_t.get_exmap("消息ID")?.to_string();
        }
        if msg_id == "" {
            return Ok(Some("".to_string()));
        }
        let int32_msg_id = msg_id.parse::<i32>()?;
        let send_json = serde_json::json!({
            "action":"delete_msg",
            "params":{
                "message_id":int32_msg_id
            }
        });
        cq_call_api(&send_json.to_string())?;
    }else if cmd == "输出流" {
        let user_id_str = self_t.get_exmap("发送者ID")?.to_string();
        let group_id_str = self_t.get_exmap("群ID")?.to_string();
        let msg_type:&str;
        if group_id_str != "" {
            msg_type = "group";
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
            return Err(self_t.make_err(err));
        }
        let err = "输出流调用失败，获取message_id失败";
        let msg_id = ret_json.get("data").ok_or(err)?.get("message_id").ok_or(err)?.as_i64().ok_or(err)?;
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
    }else if cmd == "CQ码转义" {
        let cq_code = self_t.get_param(params, 0)?;
        return Ok(Some(cq_encode(&cq_code)));
    }
    else if cmd == "CQ转义" {
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
    }else if cmd == "OB调用" {
        self_t.get_param(params, 0)?;
        let content = self_t.get_param(params, 1)?;
        let call_ret = cq_call_api(&content)?;
        let js_v:serde_json::Value = serde_json::from_str(&call_ret)?;
        let ret = do_json_parse(&js_v, &self_t.type_uuid)?;
        return Ok(Some(ret));
    }else if cmd == "CQ码解析" {
        let data_str = self_t.get_param(params, 0)?;
        let pos1 = data_str.find(",").ok_or("CQ码解析失败")?;
        let tp = data_str.get(4..pos1).ok_or("CQ码解析失败")?;
        let mut sub_key_obj = String::new();
        sub_key_obj.push_str(&self_t.type_uuid);
        sub_key_obj.push('O');
        sub_key_obj.push('4');
        sub_key_obj.push(',');
        sub_key_obj.push_str("type");
        sub_key_obj.push_str(&tp.len().to_string());
        sub_key_obj.push(',');
        sub_key_obj.push_str(tp);
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
                sub_key_obj.push_str(&key.len().to_string());
                sub_key_obj.push(',');
                sub_key_obj.push_str(&key);
                sub_key_obj.push_str(&val.len().to_string());
                sub_key_obj.push(',');
                sub_key_obj.push_str(&val);
            }
        }
        return Ok(Some(sub_key_obj));
    }else if cmd == "CQ反转义" {
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
    }
    return Ok(None);
}