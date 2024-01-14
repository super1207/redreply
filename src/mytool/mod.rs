use std::collections::HashMap;

use crate::redlang::RedLang;

use zhconv::{zhconv, Variant};

pub fn cq_text_encode(data:&str) -> String {
    let mut ret_str:String = String::new();
    for ch in data.chars() {
        if ch == '&' {
            ret_str += "&amp;";
        }
        else if ch == '[' {
            ret_str += "&#91;";
        }
        else if ch == ']' {
            ret_str += "&#93;";
        }
        else{
            ret_str.push(ch);
        }
    }
    return ret_str;
}

pub fn str_msg_to_arr(js:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let cqstr;
    if let Some(val) = js.as_str() {
        cqstr = val.chars().collect::<Vec<char>>();
    } else {
        return Err(RedLang::make_err("无法获得字符串消息"));
    }
    let mut text = "".to_owned();
    let mut type_ = "".to_owned();
    let mut val = "".to_owned();
    let mut key = "".to_owned();
    let mut jsonarr:Vec<serde_json::Value> = vec![];
    let mut cqcode:HashMap<String,serde_json::Value> = HashMap::new();
    let mut stat = 0;
    let mut i = 0usize;
    while i < cqstr.len() {
        let cur_ch = cqstr[i];
        if stat == 0 {
            if cur_ch == '[' {
                if i + 4 <= cqstr.len() {
                    let t = &cqstr[i..i+4];
                    if t.starts_with(&['[','C','Q',':']) {
                        if text.len() != 0 {
                            let mut node:HashMap<String, serde_json::Value> = HashMap::new();
                            node.insert("type".to_string(), serde_json::json!("text"));
                            node.insert("data".to_string(), serde_json::json!({"text": text}));
                            jsonarr.push(serde_json::json!(node));
                            text.clear();
                        }
                        stat = 1;
                        i += 3;
                    }else {
                        text.push(cqstr[i]);
                    }
                }else{
                    text.push(cqstr[i]);
                }
            }else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i+5];
                    if t.starts_with(&['&','#','9','1',';']) {
                        text.push('[');
                        i += 4;
                    }else if t.starts_with(&['&','#','9','3',';']) {
                        text.push(']');
                        i += 4;
                    }else if t.starts_with(&['&','a','m','p',';']) {
                        text.push('&');
                        i += 4;
                    }else {
                        text.push(cqstr[i]);
                    }
                }else{
                    text.push(cqstr[i]);
                }
            }else{
                text.push(cqstr[i]);
            }
        }else if stat == 1 {
            if cur_ch == ',' {
                stat = 2;
            }else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i+5];
                    if t.starts_with(&['&','#','9','1',';']) {
                        type_.push('[');
                        i += 4;
                    }else if t.starts_with(&['&','#','9','3',';']) {
                        type_.push(']');
                        i += 4;
                    }else if t.starts_with(&['&','a','m','p',';']) {
                        type_.push('&');
                        i += 4;
                    }else if t.starts_with(&['&','#','4','4',';']) {
                        type_.push(',');
                        i += 4;
                    }else {
                        type_.push(cqstr[i]);
                    }
                }else{
                    type_.push(cqstr[i]);
                }
            }else {
                type_.push(cqstr[i]);
            }
        }else if stat == 2 {
            if cur_ch == '=' {
                stat = 3;
            }else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i+5];
                    if t.starts_with(&['&','#','9','1',';']) {
                        key.push('[');
                        i += 4;
                    }else if t.starts_with(&['&','#','9','3',';']) {
                        key.push(']');
                        i += 4;
                    }else if t.starts_with(&['&','a','m','p',';']) {
                        key.push('&');
                        i += 4;
                    }else if t.starts_with(&['&','#','4','4',';']) {
                        key.push(',');
                        i += 4;
                    }else {
                        key.push(cqstr[i]);
                    }
                }else{
                    key.push(cqstr[i]);
                }
            }else {
                key .push(cqstr[i]);
            }
        }else if stat == 3 {
            if cur_ch == ']'{
                let mut node:HashMap<String, serde_json::Value> = HashMap::new();
                cqcode.insert(key.clone(), serde_json::json!(val));
                node.insert("type".to_string(), serde_json::json!(type_));
                node.insert("data".to_string(), serde_json::json!(cqcode));
                jsonarr.push(serde_json::json!(node));
                key.clear();
                val.clear();
                text.clear();
                type_.clear();
                cqcode.clear();
                stat = 0;
            }else if cur_ch == ',' {
                cqcode.insert(key.clone(), serde_json::json!(val));
                key.clear();
                val.clear();
                stat = 2;
            }else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i+5];
                    if t.starts_with(&['&','#','9','1',';']) {
                        val.push('[');
                        i += 4;
                    }else if t.starts_with(&['&','#','9','3',';']) {
                        val.push(']');
                        i += 4;
                    }else if t.starts_with(&['&','a','m','p',';']) {
                        val.push('&');
                        i += 4;
                    }else if t.starts_with(&['&','#','4','4',';']) {
                        val.push(',');
                        i += 4;
                    }else {
                        val.push(cqstr[i]);
                    }
                }else{
                    val.push(cqstr[i]);
                }
            }else {
                val.push(cqstr[i]);
            }
        }
         i += 1;
    }
    if text.len() != 0 {
        let mut node:HashMap<String, serde_json::Value> = HashMap::new();
        node.insert("type".to_string(), serde_json::json!("text"));
        node.insert("data".to_string(), serde_json::json!({"text": text}));
        jsonarr.push(serde_json::json!(node));
    }
    Ok(serde_json::Value::Array(jsonarr))
}

pub fn read_json_str(root:&serde_json::Value,key:&str) -> String {
    if let Some(js_v) = root.get(key) {
        if js_v.is_u64() {
            return js_v.as_u64().unwrap().to_string();
        }
        if js_v.is_i64() {
            return js_v.as_i64().unwrap().to_string();
        }else if js_v.is_string() {
            return js_v.as_str().unwrap().to_string();
        }else{
            return "".to_string();
        }
    }else{
        return "".to_string();
    }
}

pub fn read_json_obj<'a>(root:&'a serde_json::Value,key:&str) -> Option<&'a serde_json::Value> {
    if let Some(js_v) = root.get(key) {
        if js_v.is_object() {
            if js_v.as_object().unwrap().len() != 0 {
                return Some(js_v);
            }
        }
    }
    return None;
}

pub fn read_json_obj_or_null(root:&serde_json::Value,key:&str) -> serde_json::Value {
    if let Some(js_v) = root.get(key) {
        if js_v.is_object() {
            if js_v.as_object().unwrap().len() != 0 {
                return js_v.clone();
            }
        }
    }
    return serde_json::json!({});
}


// 将字符串转化为简体
pub fn str_to_jt(s:&str) -> String {
    return zhconv(s, Variant::ZhHans);
}

// 将字符串转化为繁体
pub fn str_to_ft(s:&str) -> String {
    return zhconv(s, Variant::ZhHant);
}

pub fn cq_params_encode(data:&str) -> String {
    let mut ret_str:String = String::new();
    for ch in data.chars() {
        if ch == '&' {
            ret_str += "&amp;";
        }
        else if ch == '[' {
            ret_str += "&#91;";
        }
        else if ch == ']' {
            ret_str += "&#93;";
        }
        else if ch == ',' {
            ret_str += "&#44;";
        }
        else{
            ret_str.push(ch);
        }
    }
    return ret_str;
}

pub fn json_to_cq_str(js: & serde_json::Value) ->Result<String, Box<dyn std::error::Error>> {
    let msg_json = js.get("message").ok_or("json中缺少message字段")?;
    let mut ret:String = String::new();
    if msg_json.is_string() {
        return Ok(msg_json.as_str().unwrap().to_owned());
    }
    for i in 0..msg_json.as_array().ok_or("message不是array")?.len() {
        let tp = msg_json[i].get("type").ok_or("消息中缺少type字段")?.as_str().ok_or("type字段不是str")?;
        let nodes = &msg_json[i].get("data").ok_or("json中缺少data字段")?;
        if tp == "text" {
            let temp = nodes.get("text").ok_or("消息中缺少text字段")?.as_str().ok_or("消息中text字段不是str")?;
            ret.push_str(cq_text_encode(temp).as_str());
        }else{
            let mut cqcode = String::from("[CQ:".to_owned() + tp + ",");
            if nodes.is_object() {
                for j in nodes.as_object().ok_or("msg nodes 不是object")? {
                    let k = j.0;
                    let v = j.1.as_str().ok_or("j.1.as_str() err")?;
                    cqcode.push_str(k);
                    cqcode.push('=');
                    cqcode.push_str(cq_params_encode(v).as_str());
                    cqcode.push(',');    
                }
            }
            let n= &cqcode[0..cqcode.len()-1];
            let cqcode_out = n.to_owned() + "]";
            ret.push_str(cqcode_out.as_str());
        }
    }
    return  Ok(ret);
}


pub fn deal_path_str(path_str:&str) -> &str {
    if path_str.starts_with("\\\\?\\") {
        return &path_str[4..];
    }else{
        return path_str;
    }
}


