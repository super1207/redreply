use std::collections::{BTreeMap, HashMap};

use crate::{cqapi::cq_add_log_w, redlang::{add_fun, exfun::http_post, RedLang}, RT_PTR};
use base64::{Engine as _, engine::{self, general_purpose}, alphabet};
const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

pub fn init_ai_fun_map() {
    add_fun(vec!["GPT-创建单轮对话"],|self_t,params|{
        let uid = uuid::Uuid::new_v4().to_string();
        let base_url = self_t.get_param(params, 0)?;
        let key = self_t.get_param(params, 1)?;
        let model = self_t.get_param(params, 2)?;
        let to_ret = format!("{uid}base_url,{base_url}{uid}key,{key}{uid}model,{model}");
        self_t.set_gobalmap(&uid, &to_ret)?;
        return Ok(Some(uid));
    });
    add_fun(vec!["GPT-增加文本"],|self_t,params|{
        let uid = self_t.get_param(params, 0)?;
        let handle = self_t.get_gobalmap(&uid);
        let text = self_t.get_param(params, 1)?;
        let to_ret = format!("{handle}{uid}text,{text}");
        self_t.set_gobalmap(&uid, &to_ret)?;
        return Ok(Some("".to_owned()));
    });
    add_fun(vec!["GPT-增加图片"],|self_t,params|{
        let uid = self_t.get_param(params, 0)?;
        let handle = self_t.get_gobalmap(&uid);
        let mut image = self_t.get_param(params, 1)?;
        if self_t.get_type(&image)? == "字节集" {
            let bin = RedLang::parse_bin_raw(&image)?;
            let b64 = BASE64_CUSTOM_ENGINE.encode(bin);
            image = format!("data:image/png;base64,{b64}");
        }
        let to_ret = format!("{handle}{uid}image,{image}");
        self_t.set_gobalmap(&uid, &to_ret)?;
        return Ok(Some("".to_owned()));
    });
    add_fun(vec!["GPT-发送请求"],|self_t,params|{
        let mut gpt_params: HashMap<&str, &str> = HashMap::new();
        let uid = self_t.get_param(params, 0)?;
        let handle = self_t.get_gobalmap(&uid);
        let binding = handle.split(&uid).collect::<Vec<&str>>();
        let gpt_params_str = binding.get(1..).ok_or("参数错误")?;
        for gpt_param in gpt_params_str {
            let pos = gpt_param.find(",").ok_or("参数错误")?;
            let key = gpt_param.get(0..pos).ok_or("key err")?;
            let val = gpt_param.get(pos+1..).ok_or("val err")?;
            gpt_params.insert(key, val);
        }
        let base_url = gpt_params.get("base_url").ok_or("请设置base_url")?;
        let key = gpt_params.get("key").ok_or("请设置key")?;
        let mut content_arr = Vec::new();
        for it in gpt_params_str {
            let pos = it.find(",").ok_or("参数错误")?;
            let key = it.get(0..pos).ok_or("key err")?;
            let val = it.get(pos+1..).ok_or("val err")?;
            // cq_add_log_w(&format!("GPT-参数:{}={}",key,val)).unwrap();
            if key == "text" {
                content_arr.push(serde_json::json!({
                    "type":"text",
                    "text":val
                }));
            } else if key == "image" {
                content_arr.push(serde_json::json!({
                    "type":"image_url",
                    "image_url":{
                        "url":val
                    }
                }));
            }
        }
        let proxy = self_t.get_coremap("代理");
        let mut timeout_str = self_t.get_coremap("访问超时");
        if timeout_str == "" {
            timeout_str = "60000".to_owned();
        }
        let mut http_header = BTreeMap::new();
        let http_header_str = self_t.get_coremap("访问头");
        if http_header_str != "" {
            http_header = RedLang::parse_obj(&http_header_str)?;
            if !http_header.contains_key("User-Agent"){
                http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
        }else {
            http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
        }
        http_header.insert("Authorization".to_string(),format!("Bearer {}",key));
        http_header.insert("Content-Type".to_string(), "application/json".to_string());
        
        let data = serde_json::json!({
            "model": gpt_params.get("model").ok_or("请设置model")?,
            "messages": [{
                "role": "user",
                "content": content_arr
            }]
        }).to_string().into_bytes();

        let timeout = timeout_str.parse::<u64>()?;
        let content = RT_PTR.block_on(async { 
            let ret = tokio::select! {
                val_rst = http_post(base_url,data,&http_header,&proxy,"POST") => {
                    if let Ok(val) = val_rst {
                        val
                    } else {
                        cq_add_log_w(&format!("{:?}",val_rst.err().unwrap())).unwrap();
                        (vec![],"".to_owned())
                    }
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                    cq_add_log_w(&format!("POST访问:`{}`超时",base_url)).unwrap();
                    (vec![],"".to_owned())
                }
            };
            return ret;
        });
        let json:serde_json::Value = serde_json::from_slice(&content.0)?;
        if let Some(ret_content) = json["choices"][0]["message"]["content"].as_str() {
            self_t.set_gobalmap(&uid, &format!("{uid}ret_content,{ret_content}"))?;
            return Ok(Some("".to_owned()));
        } else {
             return Err(format!("GPT返回出错:{}",json.to_string()).into());
        }
    });
    add_fun(vec!["GPT-获取回复"],|self_t,params|{
        let uid = self_t.get_param(params, 0)?;
        let handle = self_t.get_gobalmap(&uid);
        let binding = handle.split(&uid).collect::<Vec<&str>>();
        let gpt_params_str = binding.get(1..).ok_or("参数错误")?;
        for gpt_param in gpt_params_str {
            let pos = gpt_param.find(",").ok_or("参数错误")?;
            let key = gpt_param.get(0..pos).ok_or("key err")?;
            let val = gpt_param.get(pos+1..).ok_or("val err")?;
            if key == "ret_content" {
                return Ok(Some(val.to_owned()));
            }
        }
        return Err("GPT获取回复出错，你可能还没有发送请求".into());
    });

    add_fun(vec!["GPT-删除指针"],|self_t,params|{
        let uid = self_t.get_param(params, 0)?;
        self_t.set_gobalmap(&uid, "")?;
        return Ok(Some("".to_owned()));
    });

}