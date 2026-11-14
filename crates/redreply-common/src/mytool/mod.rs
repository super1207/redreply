pub mod all_to_silk;
mod deal_flac;
mod deal_m4a;
mod deal_ogg;
mod deal_silk;
mod mp3_deal;
mod wav_deal;

use std::{collections::HashMap, str::FromStr, time::SystemTime};

use crate::proxy::proxy_cache;

use serde_json::Value;
use zhconv::{zhconv, Variant};

fn make_err(message: &str) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.to_string()).into()
}

pub fn cq_text_encode(data: &str) -> String {
    let mut ret_str: String = String::new();
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

pub fn str_msg_to_arr(
    js: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let cqstr;
    if let Some(val) = js.as_str() {
        cqstr = val.chars().collect::<Vec<char>>();
    } else {
        return Err(make_err("无法获得字符串消息"));
    }
    let mut text = "".to_owned();
    let mut type_ = "".to_owned();
    let mut val = "".to_owned();
    let mut key = "".to_owned();
    let mut jsonarr: Vec<serde_json::Value> = vec![];
    let mut cqcode: HashMap<String, serde_json::Value> = HashMap::new();
    let mut stat = 0;
    let mut i = 0usize;
    let mut code_start = 0usize;
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
                        code_start = i;
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
    if stat != 0 {
        // 未闭合的 CQ 码：把已消费的原文回退为普通文本，避免尾部内容静默丢失
        let tail: String = cqstr[code_start..].iter().collect();
        text.push_str(&tail);
    }
    if !text.is_empty() {
        let mut node: HashMap<String, serde_json::Value> = HashMap::new();
        node.insert("type".to_string(), serde_json::json!("text"));
        node.insert("data".to_string(), serde_json::json!({"text": text}));
        jsonarr.push(serde_json::json!(node));
    }
    let mut result = serde_json::Value::Array(jsonarr);
    cq_fix_image_file_in_arr(&mut result);
    Ok(result)
}

/// 遍历消息数组中的 image 节点：
/// 如果 file 指向本地文件且文件不存在，则把 url 字段（如果有）当作 file。
pub fn cq_fix_image_file_in_arr(arr: &mut serde_json::Value) {
    let Some(nodes) = arr.as_array_mut() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.get("type").and_then(|value| value.as_str()) != Some("image") {
            continue;
        }
        let Some(data) = node.get_mut("data").and_then(|value| value.as_object_mut()) else {
            continue;
        };
        let Some(file) = data
            .get("file")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if file.is_empty() {
            continue;
        }
        if file.starts_with("http")
            || file.starts_with("base64://")
            || file.starts_with("file://")
            || file.contains("://")
        {
            continue;
        }
        if std::path::Path::new(&file).is_file() {
            continue;
        }
        if let Some(url) = data.get("url").and_then(|value| value.as_str()) {
            if !url.is_empty() {
                data.insert("file".to_owned(), serde_json::json!(url));
            }
        }
    }
}

pub fn read_json_str(root: &serde_json::Value, key: &str) -> String {
    if let Some(js_v) = root.get(key) {
        if let Some(value) = js_v.as_u64() {
            return value.to_string();
        }
        if let Some(value) = js_v.as_i64() {
            return value.to_string();
        }
        if let Some(value) = js_v.as_str() {
            return value.to_string();
        }
    }
    String::new()
}

pub fn read_json_obj<'a>(root: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    root.get(key)
        .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
}

pub fn read_json_obj_or_null(root: &serde_json::Value, key: &str) -> serde_json::Value {
    read_json_obj(root, key)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

pub fn read_json_or_default<'a>(
    root: &'a serde_json::Value,
    key: &'a str,
    def_val: &'a serde_json::Value,
) -> &'a serde_json::Value {
    root.get(key).unwrap_or(def_val)
}

// 将字符串转化为简体
pub fn str_to_jt(s: &str) -> String {
    zhconv(s, Variant::ZhCN)
}

// 将字符串转化为繁体
pub fn str_to_ft(s: &str) -> String {
    zhconv(s, Variant::ZhHK)
}

pub fn cq_params_encode(data: &str) -> String {
    let mut ret_str: String = String::new();
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

fn json_as_str(json: &Value) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(number) = json.as_number() {
        return Ok(number.to_string());
    }
    let ret = json
        .as_str()
        .ok_or(format!("can't convert json:`{json:?}` to str"))?;
    Ok(ret.to_owned())
}

pub fn json_to_cq_str(js: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let msg_json = js.get("message").ok_or("json中缺少message字段")?;
    let mut ret: String = String::new();
    if let Some(text) = msg_json.as_str() {
        return Ok(text.to_owned());
    }
    for i in 0..msg_json.as_array().ok_or("message不是array")?.len() {
        let tp = msg_json[i]
            .get("type")
            .ok_or("消息中缺少type字段")?
            .as_str()
            .ok_or("type字段不是str")?;
        let nodes = &msg_json[i].get("data").ok_or("json中缺少data字段")?;
        if tp == "text" {
            let temp = nodes
                .get("text")
                .ok_or("消息中缺少text字段")?
                .as_str()
                .ok_or("消息中text字段不是str")?;
            ret.push_str(cq_text_encode(temp).as_str());
        } else {
            let mut cqcode = "[CQ:".to_owned() + tp + ",";
            if nodes.is_object() {
                for j in nodes.as_object().ok_or("msg nodes 不是object")? {
                    let k = j.0;
                    if let Ok(v) = json_as_str(j.1) {
                        cqcode.push_str(k);
                        cqcode.push('=');
                        cqcode.push_str(cq_params_encode(&v).as_str());
                        cqcode.push(',');
                    } else {
                        cqcode.push_str(k);
                        cqcode.push('=');
                        cqcode.push_str(cq_params_encode(&j.1.to_string()).as_str());
                        cqcode.push(',');
                    }
                }
            }
            let n = &cqcode[0..cqcode.len() - 1];
            let cqcode_out = n.to_owned() + "]";
            ret.push_str(cqcode_out.as_str());
        }
    }
    Ok(ret)
}

pub fn deal_path_str(path_str: &str) -> &str {
    path_str.strip_prefix(r"\\?\").unwrap_or(path_str)
}

pub fn path_to_os_str(path_str: &str) -> String {
    let new_str = path_str.replace("\\", std::path::MAIN_SEPARATOR.to_string().as_str());
    let ret_str = new_str.replace("/", std::path::MAIN_SEPARATOR.to_string().as_str());
    ret_str
}

pub async fn github_proxy() -> Option<String> {
    let urls_to_test = [
        "https://mirror.ghproxy.com/",
        "https://github.moeyy.xyz/",
        "https://github.moeyy.cn/",
        "",
    ];
    let (tx, mut rx) = tokio::sync::mpsc::channel(urls_to_test.len() + 1);
    for url in urls_to_test {
        let tx = tx.clone();
        tokio::spawn(async move {
            let Some(client) = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .no_proxy()
                .build()
                .ok()
            else {
                return;
            };
            let Ok(uri) = reqwest::Url::from_str(&(url.to_owned() +
				"https://raw.githubusercontent.com/super1207/redreply/master/res/version.txt")) else {
                return;
            };
            let Ok(req) = client.get(uri).build() else {
                return;
            };
            if let Ok(ret) = client.execute(req).await {
                if ret.status() == reqwest::StatusCode::OK {
                    let _err = tx.send(url).await;
                }
            };
        });
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let _err = tx.send("timeout").await;
    });
    let ret = rx.recv().await;
    if let Some(r) = ret {
        if r != "timeout" {
            return Some(r.to_owned());
        }
    }
    None
}

pub async fn get_github_proxy() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut proxy = proxy_cache().lock().await;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    if proxy.0.is_none() || proxy.1 + 60 < now {
        proxy.0 = github_proxy().await;
        proxy.1 = now;
    }
    proxy
        .0
        .clone()
        .ok_or_else(|| "cann't connect to github".into())
}
