fn cq_text_encode(data:&str) -> String {
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

pub fn read_json_str(root:&serde_json::Value,key:&str) -> String {
    if let Some(js_v) = root.get(key) {
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

fn cq_params_encode(data:&str) -> String {
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
