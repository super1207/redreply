use std::collections::HashMap;

use serde_json::Value;

pub mod all_to_silk {
    use crate::AdapterResult;

    pub fn all_to_silk(input: &[u8]) -> AdapterResult<Vec<u8>> {
        crate::host::all_to_silk(input)
    }
}

pub fn cq_text_encode(data: &str) -> String {
    let mut ret_str = String::new();
    for ch in data.chars() {
        if ch == '&' {
            ret_str += "&amp;";
        } else if ch == '[' {
            ret_str += "&#91;";
        } else if ch == ']' {
            ret_str += "&#93;";
        } else {
            ret_str.push(ch);
        }
    }
    ret_str
}

pub fn cq_params_encode(data: &str) -> String {
    let mut ret_str = String::new();
    for ch in data.chars() {
        if ch == '&' {
            ret_str += "&amp;";
        } else if ch == '[' {
            ret_str += "&#91;";
        } else if ch == ']' {
            ret_str += "&#93;";
        } else if ch == ',' {
            ret_str += "&#44;";
        } else {
            ret_str.push(ch);
        }
    }
    ret_str
}

pub fn str_msg_to_arr(js: &serde_json::Value) -> crate::AdapterResult<serde_json::Value> {
    let cqstr = if let Some(val) = js.as_str() {
        val.chars().collect::<Vec<char>>()
    } else {
        return Err("无法获得字符串消息".into());
    };

    let mut text = String::new();
    let mut type_ = String::new();
    let mut val = String::new();
    let mut key = String::new();
    let mut jsonarr: Vec<serde_json::Value> = vec![];
    let mut cqcode: HashMap<String, serde_json::Value> = HashMap::new();
    let mut stat = 0;
    let mut i = 0usize;

    while i < cqstr.len() {
        let cur_ch = cqstr[i];
        if stat == 0 {
            if cur_ch == '[' {
                if i + 4 <= cqstr.len() {
                    let t = &cqstr[i..i + 4];
                    if t.starts_with(&['[', 'C', 'Q', ':']) {
                        if !text.is_empty() {
                            let mut node: HashMap<String, serde_json::Value> = HashMap::new();
                            node.insert("type".to_string(), serde_json::json!("text"));
                            node.insert("data".to_string(), serde_json::json!({"text": text}));
                            jsonarr.push(serde_json::json!(node));
                            text.clear();
                        }
                        stat = 1;
                        i += 3;
                    } else {
                        text.push(cqstr[i]);
                    }
                } else {
                    text.push(cqstr[i]);
                }
            } else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i + 5];
                    if t.starts_with(&['&', '#', '9', '1', ';']) {
                        text.push('[');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '9', '3', ';']) {
                        text.push(']');
                        i += 4;
                    } else if t.starts_with(&['&', 'a', 'm', 'p', ';']) {
                        text.push('&');
                        i += 4;
                    } else {
                        text.push(cqstr[i]);
                    }
                } else {
                    text.push(cqstr[i]);
                }
            } else {
                text.push(cqstr[i]);
            }
        } else if stat == 1 {
            if cur_ch == ',' {
                stat = 2;
            } else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i + 5];
                    if t.starts_with(&['&', '#', '9', '1', ';']) {
                        type_.push('[');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '9', '3', ';']) {
                        type_.push(']');
                        i += 4;
                    } else if t.starts_with(&['&', 'a', 'm', 'p', ';']) {
                        type_.push('&');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '4', '4', ';']) {
                        type_.push(',');
                        i += 4;
                    } else {
                        type_.push(cqstr[i]);
                    }
                } else {
                    type_.push(cqstr[i]);
                }
            } else {
                type_.push(cqstr[i]);
            }
        } else if stat == 2 {
            if cur_ch == '=' {
                stat = 3;
            } else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i + 5];
                    if t.starts_with(&['&', '#', '9', '1', ';']) {
                        key.push('[');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '9', '3', ';']) {
                        key.push(']');
                        i += 4;
                    } else if t.starts_with(&['&', 'a', 'm', 'p', ';']) {
                        key.push('&');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '4', '4', ';']) {
                        key.push(',');
                        i += 4;
                    } else {
                        key.push(cqstr[i]);
                    }
                } else {
                    key.push(cqstr[i]);
                }
            } else {
                key.push(cqstr[i]);
            }
        } else if stat == 3 {
            if cur_ch == ']' {
                let mut node: HashMap<String, serde_json::Value> = HashMap::new();
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
            } else if cur_ch == ',' {
                cqcode.insert(key.clone(), serde_json::json!(val));
                key.clear();
                val.clear();
                stat = 2;
            } else if cur_ch == '&' {
                if i + 5 <= cqstr.len() {
                    let t = &cqstr[i..i + 5];
                    if t.starts_with(&['&', '#', '9', '1', ';']) {
                        val.push('[');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '9', '3', ';']) {
                        val.push(']');
                        i += 4;
                    } else if t.starts_with(&['&', 'a', 'm', 'p', ';']) {
                        val.push('&');
                        i += 4;
                    } else if t.starts_with(&['&', '#', '4', '4', ';']) {
                        val.push(',');
                        i += 4;
                    } else {
                        val.push(cqstr[i]);
                    }
                } else {
                    val.push(cqstr[i]);
                }
            } else {
                val.push(cqstr[i]);
            }
        }
        i += 1;
    }

    if !text.is_empty() {
        let mut node: HashMap<String, serde_json::Value> = HashMap::new();
        node.insert("type".to_string(), serde_json::json!("text"));
        node.insert("data".to_string(), serde_json::json!({"text": text}));
        jsonarr.push(serde_json::json!(node));
    }

    Ok(serde_json::Value::Array(jsonarr))
}

pub fn read_json_str(root: &serde_json::Value, key: &str) -> String {
    if let Some(js_v) = root.get(key) {
        if js_v.is_u64() {
            return js_v.as_u64().unwrap().to_string();
        }
        if js_v.is_i64() {
            return js_v.as_i64().unwrap().to_string();
        }
        if js_v.is_string() {
            return js_v.as_str().unwrap().to_string();
        }
    }
    String::new()
}

pub fn read_json_obj<'a>(
    root: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(js_v) = root.get(key) {
        if js_v.is_object() && !js_v.as_object().unwrap().is_empty() {
            return Some(js_v);
        }
    }
    None
}

pub fn read_json_obj_or_null(root: &serde_json::Value, key: &str) -> serde_json::Value {
    if let Some(js_v) = root.get(key) {
        if js_v.is_object() && !js_v.as_object().unwrap().is_empty() {
            return js_v.clone();
        }
    }
    serde_json::json!({})
}

pub fn read_json_or_default<'a>(
    root: &'a serde_json::Value,
    key: &'a str,
    def_val: &'a serde_json::Value,
) -> &'a serde_json::Value {
    root.get(key).unwrap_or(def_val)
}

fn json_as_str(json: &Value) -> crate::AdapterResult<String> {
    if json.is_number() {
        return Ok(json.as_number().unwrap().to_string());
    }
    let ret = json
        .as_str()
        .ok_or(format!("can't convert json:`{json:?}` to str"))?;
    Ok(ret.to_owned())
}

pub fn json_to_cq_str(js: &serde_json::Value) -> crate::AdapterResult<String> {
    let msg_json = js.get("message").ok_or("json中缺少message字段")?;
    let mut ret = String::new();
    if msg_json.is_string() {
        return Ok(msg_json.as_str().unwrap().to_owned());
    }
    for item in msg_json.as_array().ok_or("message不是array")? {
        let tp = item
            .get("type")
            .ok_or("消息中缺少type字段")?
            .as_str()
            .ok_or("type字段不是str")?;
        let nodes = item.get("data").ok_or("json中缺少data字段")?;
        if tp == "text" {
            let temp = nodes
                .get("text")
                .ok_or("消息中缺少text字段")?
                .as_str()
                .ok_or("消息中text字段不是str")?;
            ret.push_str(cq_text_encode(temp).as_str());
        } else {
            let mut cqcode = String::from("[CQ:".to_owned() + tp + ",");
            if nodes.is_object() {
                for (k, v) in nodes.as_object().ok_or("msg nodes 不是object")? {
                    cqcode.push_str(k);
                    cqcode.push('=');
                    if let Ok(v) = json_as_str(v) {
                        cqcode.push_str(cq_params_encode(&v).as_str());
                    } else {
                        cqcode.push_str(cq_params_encode(&v.to_string()).as_str());
                    }
                    cqcode.push(',');
                }
            }
            let n = &cqcode[0..cqcode.len() - 1];
            let cqcode_out = n.to_owned() + "]";
            ret.push_str(cqcode_out.as_str());
        }
    }
    Ok(ret)
}
