use std::{cell::RefCell, collections::{BTreeMap, HashMap}, fs, io::Read, path::{Path, PathBuf}, str::FromStr, time::{Duration, SystemTime}, vec};
use chrono::TimeZone;
use encoding::Encoding;
use flate2::{read::{GzDecoder, ZlibDecoder}, write::{GzEncoder, ZlibEncoder}, Compression};
use ini::Ini;
use jsonpath_rust::JsonPath;
use md5::{Md5, Digest};
use mlua::MultiValue;
use rusttype::Scale;
use base64::{Engine as _, engine::{self, general_purpose}, alphabet};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
use time::UtcOffset;
use super::RedLang;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use std::io::Write;
use crate::{add_file_lock, cq_add_log_w, cqapi::{cq_add_log, get_tmp_dir}, cronevent::{OneTimeRunStruct, G_ONE_TIME_RUN}, del_file_lock, get_python_cmd_name, pkg_can_run, pyserver::call_py_block, redlang::get_random, G_DEFAULF_FONT, RT_PTR};

use image::{AnimationDecoder, EncodableLayout, GenericImageView, ImageBuffer, ImageFormat, Rgba};
use imageproc::geometric_transformations::{Projection, warp_with, rotate_about_center};
use std::io::Cursor;
use imageproc::geometric_transformations::Interpolation;
use std::sync::Arc;
use ab_glyph::{FontRef, PxScale};

const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);


pub fn get_raw_data(self_t:&mut RedLang,input:String) -> Result<String, Box<dyn std::error::Error>>{
    let raw_data;
    let tp = self_t.get_type(&input)?;
    if tp == "文本" {
        raw_data = input;
    }else if tp == "字节集" {
        let temp = RedLang::parse_bin(&self_t.bin_pool, &input)?;
        raw_data = self_t.build_bin(temp);
    } else if tp == "数组" {
        let mut retarr = vec![];
        let arr = RedLang::parse_arr(&input)?;
        for it in arr {
            retarr.push(get_raw_data(self_t, it.to_owned())?);
        }
        let bind = retarr.iter().map(|x|x.as_ref()).collect();
        raw_data = self_t.build_arr(bind);
    } else if tp == "对象" {
        let mut retobj = BTreeMap::new();
        let obj = RedLang::parse_obj(&input)?;
        for (k,v) in obj {
            retobj.insert(get_raw_data(self_t, k)?, get_raw_data(self_t, v)?);
        }
        raw_data = self_t.build_obj(retobj);
    }
    else {
        return Err(RedLang::make_err(&format!("不支持的参数类型:{tp}")));
    }
    Ok(raw_data)
}

pub async fn http_post(url:&str,data:Vec<u8>,headers:&BTreeMap<String, String>,proxy_str:&str,method:&str) -> Result<(Vec<u8>,String), Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let uri = reqwest::Url::from_str(url)?;
    if proxy_str == "" {
        if uri.scheme() == "http" {
            client = reqwest::Client::builder().no_proxy().http1_title_case_headers().build()?;
        } else {
            client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().http1_title_case_headers().build()?;
        }
    }else {
        if uri.scheme() == "http" {
            let proxy = reqwest::Proxy::http(proxy_str)?;
            client = reqwest::Client::builder().proxy(proxy).http1_title_case_headers().build()?;
        }else{
            let proxy = reqwest::Proxy::https(proxy_str)?;
            client = reqwest::Client::builder().danger_accept_invalid_certs(true).http1_title_case_headers().proxy(proxy).build()?;
        }
    }
    let mut req;
    if method == "POST" {
        req = client.post(uri).body(reqwest::Body::from(data)).build()?;
    }else if method == "GET" {
        req = client.get(uri).body(reqwest::Body::from(data)).build()?;
    }else if method == "PUT" {
        req = client.put(uri).body(reqwest::Body::from(data)).build()?;
    }else if method == "PATCH" {
        req = client.patch(uri).body(reqwest::Body::from(data)).build()?;
    }else if method == "DELETE" {
        req = client.delete(uri).body(reqwest::Body::from(data)).build()?;
    }else if method == "HEAD" {
        req = client.head(uri).body(reqwest::Body::from(data)).build()?;
    }else {
        return Err(format!("不支持的访问方法:{method}").into());
    }
    for (key,val) in headers {
        req.headers_mut().append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
    }
    let retbin;
    let ret = client.execute(req).await?;
    let header_map_obj = ret.headers().iter().map(|(key,val)|{
        (key.as_str().to_string(),val.to_str().unwrap_or_default().to_string())
    }).collect::<BTreeMap<String,String>>();
    let header_map =RedLang::build_obj_with_uid(&crate::REDLANG_UUID, header_map_obj);
    retbin = ret.bytes().await?.to_vec();
    return Ok((retbin,header_map));
}

pub fn init_ex_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string().to_uppercase();
            let k_t = crate::mytool::str_to_ft(&k);
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


    add_fun(vec!["返回头","取返回头"],|self_t,params|{
        let ret_headers = self_t.get_coremap("返回头")?.to_string();
        if ret_headers == "" {
            return Ok(Some("".to_string())); 
        }
        let key = self_t.get_param(params, 0)?;
        if key == "" {
            return Ok(Some(ret_headers.to_string()));
        } 
        else {
            let obj = RedLang::parse_obj(&ret_headers)?;
            let defstr = String::new();
            let val = obj.get(&key).unwrap_or(&defstr);
            return Ok(Some(val.to_owned()));
        }
    });
    add_fun(vec!["访问"],|self_t,params|{
        fn access(self_t:&mut RedLang,url:&str) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let proxy = self_t.get_coremap("代理")?;
            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = BTreeMap::new();
            let http_header_str = self_t.get_coremap("访问头")?;
            if http_header_str != "" {
                http_header = RedLang::parse_obj(&http_header_str)?;
                if !http_header.contains_key("User-Agent"){
                    http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
                }
            }else {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let timeout = timeout_str.parse::<u64>()?;
            let content = RT_PTR.block_on(async { 
                let ret = tokio::select! {
                    val_rst = http_post(url,Vec::new(),&http_header,&proxy,"GET") => {
                        if let Ok(val) = val_rst {
                            val
                        } else {
                            cq_add_log_w(&format!("{:?}",val_rst.err().unwrap())).unwrap();
                            (vec![],String::new())
                        }
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                        cq_add_log_w(&format!("GET访问:`{}`超时",url)).unwrap();
                        (vec![],String::new())
                    }
                };
                return ret;
            });
            self_t.set_coremap("返回头",&content.1)?;
            Ok(Some(self_t.build_bin(content.0)))
        }
        let url = self_t.get_param(params, 0)?;
        self_t.set_coremap("返回头","")?;
        match access(self_t,&url) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                cq_add_log_w(&format!("{:?}",err)).unwrap();
                Ok(Some(self_t.build_bin(vec![])))
            },
        }
    });
    fn do_http(method:&str,self_t:&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>> {
        fn access(method:&str,self_t:&mut RedLang,url:&str,data_t:&str) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let tp = self_t.get_type(&data_t)?;
            let data:Vec<u8>;
            if tp == "字节集" {
                data = RedLang::parse_bin(&mut self_t.bin_pool,&data_t)?;
            }else if tp == "文本" {
                data = data_t.as_bytes().to_vec();
            }else {
                return Err(RedLang::make_err(&("不支持的访问体类型:".to_owned()+&tp)));
            }

            let proxy = self_t.get_coremap("代理")?;
            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = BTreeMap::new();
            let http_header_str = self_t.get_coremap("访问头")?;
            if http_header_str != "" {
                http_header = RedLang::parse_obj(&http_header_str)?;
                if !http_header.contains_key("User-Agent"){
                    http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
                }
            }else {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let timeout = timeout_str.parse::<u64>()?;

            let content = RT_PTR.block_on(async { 
                let ret = tokio::select! {
                    val_rst = http_post(url,data,&http_header,&proxy,method) => {
                        if let Ok(val) = val_rst {
                            val
                        } else {
                            cq_add_log_w(&format!("{:?}",val_rst.err().unwrap())).unwrap();
                            (vec![],String::new())
                        }
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                        cq_add_log_w(&format!("{}访问:`{}`超时",method,url)).unwrap();
                        (vec![],String::new())
                    }
                };
                return ret;
            });
            self_t.set_coremap("返回头",&content.1)?;
            Ok(Some(self_t.build_bin(content.0)))
        }
        let url = self_t.get_param(params, 0)?;
        let data_t = self_t.get_param(params, 1)?;
        self_t.set_coremap("返回头","")?;
        match access(method,self_t,&url,&data_t) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                cq_add_log_w(&format!("{:?}",err)).unwrap();
                Ok(Some(self_t.build_bin(vec![])))
            },
        }
    }
    add_fun(vec!["POST访问"],|self_t,params|{
        do_http("POST", self_t, params)
    });
    add_fun(vec!["GET访问"],|self_t,params|{
        do_http("POST", self_t, params)
    });
    add_fun(vec!["PUT访问"],|self_t,params|{
        do_http("POST", self_t, params)
    });
    add_fun(vec!["PATCH访问"],|self_t,params|{
        do_http("POST", self_t, params)
    });
    add_fun(vec!["DELETE访问"],|self_t,params|{
        do_http("POST", self_t, params)
    });
    add_fun(vec!["设置访问头"],|self_t,params|{
        let http_header = self_t.get_coremap("访问头")?.to_string();
        let mut http_header_map:BTreeMap<String, String> = BTreeMap::new();
        if http_header != "" {
            for (k,v) in RedLang::parse_obj(&http_header)?{
                http_header_map.insert(k, v.to_string());
            }
        }
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        http_header_map.insert(k, v);
        self_t.set_coremap("访问头", &self_t.build_obj(http_header_map))?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["设置代理"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        self_t.set_coremap("代理", &k)?;
        return Ok(Some("".to_string()));
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["系统代理"],|_self_t,_params|{
        if cfg!(target_os = "windows") {
            const HKEY_CURRENT_USER: winreg::HKEY = 0x80000001u32 as usize as winreg::HKEY;
            let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
            let internet_setting: winreg::RegKey = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
            // ensure the proxy is enable, if the value doesn't exist, an error will returned.
            let proxy_enable: u32 = internet_setting.get_value("ProxyEnable")?;
            let proxy_server: String = internet_setting.get_value("ProxyServer")?;
            let mut proxy = "".to_string();
            if proxy_enable == 1 {
                if !(proxy.starts_with("http://") || proxy.starts_with("https://")) {
                    proxy = format!("http://{}",proxy_server);
                }else{
                    proxy = proxy_server;
                }
            }
            return Ok(Some(proxy));
        } else {
            return Ok(Some("".to_string()));
        }
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["IE代理"],|_self_t,_params|{
        if cfg!(target_os = "windows") {
            const HKEY_CURRENT_USER: winreg::HKEY = 0x80000001u32 as usize as winreg::HKEY;
            let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
            let internet_setting: winreg::RegKey = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
            let proxy_server_rst = internet_setting.get_value("ProxyServer");
            let mut proxy = "".to_string();
            if let Ok(proxy_server) = proxy_server_rst {
                if !(proxy.starts_with("http://") || proxy.starts_with("https://")) {
                    proxy = format!("http://{}",proxy_server);
                }else{
                    proxy = proxy_server;
                }
            }
            return Ok(Some(proxy));
        } else {
            return Ok(Some("".to_string()));
        }
    });

    add_fun(vec!["设置访问超时"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        k.parse::<u64>()?;
        self_t.set_coremap("访问超时", &k)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["编码"],|self_t,params|{
        let urlcode = self_t.get_param(params, 0)?;
        let encoded:String = url::form_urlencoded::byte_serialize(urlcode.as_bytes()).collect();
        return Ok(Some(encoded));
    });
    add_fun(vec!["解码"],|self_t,params|{
        let urlcode = self_t.get_param(params, 0)?;
        let decoded: String = url::form_urlencoded::parse(urlcode.as_bytes())
            .map(|(key, val)| [key, val].concat())
            .collect();
        return Ok(Some(decoded));
    });
    add_fun(vec!["随机取"],|self_t,params|{
        let arr_data = self_t.get_param(params, 0)?;
        let arr = RedLang::parse_arr(&arr_data)?;
        if arr.len() == 0 {
            return Ok(Some(self_t.get_param(params, 1)?));
        }
        let index = self_t.parse(&format!("【取随机数@0@{}】",arr.len() - 1))?.parse::<usize>()?;
        let ret = arr.get(index).ok_or("数组下标越界")?;
        return Ok(Some(ret.to_string()))
    });
    add_fun(vec!["取中间"],|self_t,params|{
        let s = self_t.get_param(params, 0)?;
        let sub_begin = self_t.get_param(params, 1)?;
        let sub_end = self_t.get_param(params, 2)?;
        let ret_vec = get_mid(&s, &sub_begin, &sub_end)?;
        let mut ret_str:Vec<&str> = vec![];
        for it in ret_vec {
            ret_str.push(it);
        }
        return Ok(Some(self_t.build_arr(ret_str)))
    });
    add_fun(vec!["截取"],|self_t,params|{
        let content = self_t.get_param(params, 0)?;
        let begin = self_t.get_param(params, 1)?;
        let len = self_t.get_param(params, 2)?;
        let tp = self_t.get_type(&content)?;
        let ret:String;
        if tp == "文本" {
            let chs = content.chars().collect::<Vec<char>>();
            let begen_pos;
            if begin.starts_with("-") {
                let pos_rev = begin.get(1..).unwrap().parse::<usize>()?;
                if pos_rev > chs.len() {
                    return Ok(Some("".to_string()));
                }else {
                    begen_pos = chs.len() - pos_rev;
                }
            }else {
                begen_pos = begin.parse::<usize>()?;
            }
            let sub_len:usize;
            if len == "" {
                if chs.len() < begen_pos {
                    sub_len = 0;
                } else {
                    sub_len = chs.len() - begen_pos;
                }
            }else{
                sub_len = len.parse::<usize>()?;
            }
            let mut end_pos = begen_pos+sub_len;
            if end_pos > chs.len() {
                end_pos = chs.len();
            }
            ret = match chs.get(begen_pos..end_pos) {
                Some(value) => value.iter().collect::<String>(),
                None => "".to_string()
            };
        }else if tp == "数组" {
            let arr = RedLang::parse_arr(&content)?;
            let begen_pos;
            if begin.starts_with("-") {
                let pos_rev = begin.get(1..).unwrap().parse::<usize>()?;
                if pos_rev > arr.len() {
                    return Ok(Some(self_t.build_arr(vec![])));
                }else {
                    begen_pos = arr.len() - pos_rev;
                }
            }else {
                begen_pos = begin.parse::<usize>()?;
            }
            let sub_len:usize;
            if len == "" {
                if arr.len() < begen_pos {
                    sub_len = 0;
                } else {
                    sub_len = arr.len() - begen_pos;
                }
            }else{
                sub_len = len.parse::<usize>()?;
            }
            let mut end_pos = begen_pos+sub_len;
            if end_pos > arr.len() {
                end_pos = arr.len();
            }
            ret = match arr.get(begen_pos..end_pos) {
                Some(value) => {
                    let mut array:Vec<&str> = vec![];
                    for it in value {
                        array.push(it);
                    }
                    self_t.build_arr(array)
                },
                None => self_t.build_arr(vec![])
            };
        }
        else{
            return Err(RedLang::make_err("截取命令目前仅支持文本或数组"));
        }
        
        return Ok(Some(ret))
    });
    add_fun(vec!["JSON解析"],|self_t,params|{

        fn red_to_serde_value(self_t: &RedLang, red_str: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
            let tp = self_t.get_type(red_str)?;
            match tp.as_str() {
                "文本" => {
                    if let Ok(num) = red_str.parse::<i64>() {
                        Ok(serde_json::Value::Number(num.into()))
                    } else if let Ok(num) = red_str.parse::<f64>() {
                        Ok(serde_json::json!(num))
                    } else if red_str == "真" {
                        Ok(serde_json::Value::Bool(true))
                    } else if red_str == "假" {
                        Ok(serde_json::Value::Bool(false))
                    } else {
                        Ok(serde_json::Value::String(red_str.to_string()))
                    }
                }
                "数组" => {
                    let arr = RedLang::parse_arr(red_str)?;
                    let mut json_arr = Vec::new();
                    for item in arr {
                        json_arr.push(red_to_serde_value(self_t, item)?);
                    }
                    Ok(serde_json::Value::Array(json_arr))
                }
                "对象" => {
                    let obj = RedLang::parse_obj(red_str)?;
                    let mut json_map = serde_json::Map::new();
                    for (key, value) in obj {
                        json_map.insert(key, red_to_serde_value(self_t, &value)?);
                    }
                    Ok(serde_json::Value::Object(json_map))
                }
                "字节集" => Err(RedLang::make_err("字节集不能直接转换为JSON值")),
                _ => Err(RedLang::make_err(&format!("不支持的类型向JSON转换: {}", tp))),
            }
        }

        let json_obj_str = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&json_obj_str)?;

        let json_data: serde_json::Value;

        if tp == "字节集" {
            let u8_vec = RedLang::parse_bin(&mut self_t.bin_pool, &json_obj_str)?;
            let json_str = String::from_utf8(u8_vec)?;
            json_data = serde_json::from_str(&json_str).map_err(|e| format!("字节集内容JSON解析失败: {}", e))?;
        } else if tp == "对象" {
            json_data = red_to_serde_value(self_t, &json_obj_str)?;
        } else {
            let json_str = &json_obj_str;
            json_data = serde_json::from_str(&json_str).map_err(|e| format!("文本内容JSON解析失败: {}", e))?;
        }

        let jsonpath = self_t.get_param(params, 1)?;
        let json_parse_out;
        if jsonpath != "" {
            if let Ok(v) = json_data.query(&jsonpath) {
                json_parse_out = do_json_parse(&serde_json::json!(v), &self_t.type_uuid)?;
            } else {
                cq_add_log_w("jsonpath解析失败").unwrap();
                return Ok(Some("".to_string()));
            }
        } else {
            json_parse_out = do_json_parse(&json_data, &self_t.type_uuid)?;
        }
        return Ok(Some(json_parse_out));
    });
    add_fun(vec!["读文件"],|self_t,params|{
        let file_path = self_t.get_param(params, 0)?;
        let file_path = crate::mytool::path_to_os_str(&file_path);

        let path = Path::new(&file_path);
        if !path.exists() {
            return Ok(Some(self_t.build_bin(vec![])));
        }
        add_file_lock(&file_path);
        let _guard = scopeguard::guard(file_path.clone(), |file_path| {
            del_file_lock(&file_path);
        });
        let content = std::fs::read(path)?;
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["运行目录"],|_self_t,_params|{
        let exe_dir = std::env::current_exe()?;
        let exe_path = exe_dir.parent().ok_or("无法获得运行目录")?;
        let mut exe_path_str = exe_path.to_string_lossy().to_string();
        if !exe_path_str.ends_with(std::path::MAIN_SEPARATOR)
        {
            exe_path_str.push(std::path::MAIN_SEPARATOR);
        }
        return Ok(Some(crate::mytool::deal_path_str(&exe_path_str).to_string()));
    });
    add_fun(vec!["分割"],|self_t,params|{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let mut split_ret:Vec<&str> = data_str.split(&sub_str).collect();
        if sub_str == "" {
            split_ret = split_ret.get(1..split_ret.len()-1).ok_or("无法获得分割结果")?.to_vec();
        }
        let mut ret_str = format!("{}A",self_t.type_uuid);
        for it in split_ret {
            ret_str.push_str(&it.len().to_string());
            ret_str.push(',');
            ret_str.push_str(it);
        }
        return Ok(Some(ret_str));
    });
    add_fun(vec!["判含"],|self_t,params|{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&data_str)?;
        if tp == "文本" {
            if !data_str.contains(&sub_str){
                return Ok(Some(self_t.get_param(params, 2)?));
            }else{
                return Ok(Some(self_t.get_param(params, 3)?));
            }
        }else if tp == "数组" {
            let mut ret_str = format!("{}A",self_t.type_uuid);
            for it in RedLang::parse_arr(&data_str)? {
                if it.contains(&sub_str){
                    ret_str.push_str(&it.len().to_string());
                    ret_str.push(',');
                    ret_str.push_str(it);
                }
            }
            return Ok(Some(ret_str)); 
        }else{
            return Err(RedLang::make_err(&("对应类型不能使用判含:".to_owned()+&tp)));
        }
    });
    add_fun(vec!["正则判含"],|self_t,params|{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&data_str)?;
        if tp == "文本" {
            let re = fancy_regex::Regex::new(&sub_str)?;
            let mut is_have = false;
            for _cap_iter in re.captures_iter(&data_str) {
                is_have = true;
                break;
            }
            if is_have == false {
                return Ok(Some(self_t.get_param(params, 2)?));
            }else {
                return Ok(Some(self_t.get_param(params, 3)?));
            }
        }else if tp == "数组" {
            let mut ret_str = format!("{}A",self_t.type_uuid);
            for it in RedLang::parse_arr(&data_str)? {
                let re = fancy_regex::Regex::new(&sub_str)?;
                let mut is_have = false;
                for _cap_iter in re.captures_iter(&it) {
                    is_have = true;
                    break;
                }
                if is_have {
                    ret_str.push_str(&it.len().to_string());
                    ret_str.push(',');
                    ret_str.push_str(it);
                }
            }
            return Ok(Some(ret_str)); 
        }else{
            return Err(RedLang::make_err(&("对应类型不能使用正则判含:".to_owned()+&tp)));
        }
    });
    add_fun(vec!["正则"],|self_t,params|{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let re = fancy_regex::Regex::new(&sub_str)?;
        let mut sub_key_vec:Vec<String> = vec![];
        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            let mut temp_vec:Vec<String> = vec![];
            for i in 0..len {
                if let Some(s) = cap.get(i) {
                    temp_vec.push(s.as_str().to_owned());
                }
            }
            sub_key_vec.push(self_t.build_arr(temp_vec.iter().map(AsRef::as_ref).collect()));
        }
        return Ok(Some(self_t.build_arr(sub_key_vec.iter().map(AsRef::as_ref).collect())));
    });
    add_fun(vec!["转字节集"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&text)?;
        if tp != "文本" {
            return Err(RedLang::make_err(&("转字节集不支持的类型:".to_owned()+&tp)));
        }
        let code_t = self_t.get_param(params, 1)?;
        let code = code_t.to_lowercase();
        let str_vec:Vec<u8>;
        if code == "" || code == "utf8" || code == "utf-8" {
            str_vec = text.as_bytes().to_vec();
        }else if code == "gbk" {
            str_vec = encoding::Encoding::encode(encoding::all::GBK, &text, encoding::EncoderTrap::Ignore)?;
        }else{
            return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
        }
        return Ok(Some(self_t.build_bin(str_vec)));
    });
    add_fun(vec!["BASE64编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&mut self_t.bin_pool,&text)?;
        let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
        return Ok(Some(b64_str));
    });
    add_fun(vec!["BASE64解码"],|self_t,params|{
        let b64_str = self_t.get_param(params, 0)?;
        let content = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::general_purpose::PAD), b64_str)?;
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["GZIP解码"],|self_t,params|{
        let gzip_str = self_t.get_param(params, 0)?;
        let gzip_bin = RedLang::parse_bin(&mut self_t.bin_pool,&gzip_str)?;
        let mut d = GzDecoder::new(gzip_bin.as_slice());
        let mut buf = vec![];
        d.read_to_end(&mut buf)?;
        return Ok(Some(self_t.build_bin(buf)));
    });
    add_fun(vec!["GZIP编码"],|self_t,params|{
        let gzip_str = self_t.get_param(params, 0)?;
        let gzip_bin = RedLang::parse_bin(&mut self_t.bin_pool,&gzip_str)?;
        let mut e = GzEncoder::new(vec![],Compression::default());
        e.write_all(&gzip_bin)?;
        let buf = e.finish()?;
        return Ok(Some(self_t.build_bin(buf)));
    });
    add_fun(vec!["ZLIB解码"],|self_t,params|{
        let zlib_str = self_t.get_param(params, 0)?;
        let zlib_bin = RedLang::parse_bin(&mut self_t.bin_pool,&zlib_str)?;
        let mut d = ZlibDecoder::new(zlib_bin.as_slice());
        let mut buf = vec![];
        d.read_to_end(&mut buf)?;
        return Ok(Some(self_t.build_bin(buf)));
    });
    add_fun(vec!["ZLIB编码"],|self_t,params|{
        let zlib_str = self_t.get_param(params, 0)?;
        let zlib_bin = RedLang::parse_bin(&mut self_t.bin_pool,&zlib_str)?;
        let mut e = ZlibEncoder::new(vec![],Compression::default());
        e.write_all(&zlib_bin)?;
        let buf = e.finish()?;
        return Ok(Some(self_t.build_bin(buf)));
    });
    add_fun(vec!["延时"],|self_t,params|{
        let mut mill = self_t.get_param(params, 0)?.parse::<u64>()?;
        while mill > 1000 {
            let time_struct = core::time::Duration::from_secs(1);
            std::thread::sleep(time_struct);
            mill -= 1000;
            if pkg_can_run(&self_t.pkg_name,"延时") == false {
                return Err("延时终止，因用户要求退出".into());
            }
        }
        if pkg_can_run(&self_t.pkg_name,"延时") == false {
            return Err("延时终止，因用户要求退出".into());
        }
        let time_struct = core::time::Duration::from_millis(mill);
        std::thread::sleep(time_struct);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["序号"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        if v != "" {
            // 说明是设置序号
            self_t.xuhao.insert(k.to_owned(), v.parse::<usize>()?);
            return Ok(Some("".to_string()));
        }else {
            // 说明是取序号
            let ret:usize;
            if self_t.xuhao.contains_key(&k) {
                let x = self_t.xuhao.get_mut(&k).unwrap();
                ret = *x;
                *x += 1;
            }else {
                self_t.xuhao.insert(k.to_owned(), 1);
                ret = 0;
            }
            return Ok(Some(ret.to_string()));
        }
    });
    add_fun(vec!["时间戳","10位时间戳"],|_self_t,_params|{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_secs().to_string()));
    });
    add_fun(vec!["13位时间戳"],|_self_t,_params|{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_millis().to_string()));
    });
    add_fun(vec!["时间戳转文本"],|self_t,params|{
        let numstr = self_t.get_param(params, 0)?;
        if numstr.len() > 10 {
            return Ok(Some("".to_string()));
        }
        let num = numstr.parse::<i64>()?;
        let datetime_rst = chrono::prelude::Local.timestamp_opt(num, 0);
        if let chrono::LocalResult::Single(datetime) = datetime_rst {
            let newdate = datetime.format("%Y-%m-%d-%H-%M-%S");
            return Ok(Some(format!("{}",newdate)));
        }
        return Ok(Some("".to_string()));
    });

    add_fun(vec!["文本转时间戳"],|self_t,params|{
        let time_str = self_t.get_param(params, 0)?;
        const FORMAT: &str = "%Y-%m-%d-%H-%M-%S";
        let utc_offset;
        if let Ok(v) = UtcOffset::current_local_offset() {
            utc_offset = v;
        } else {
            utc_offset = UtcOffset::from_hms(8,0,0).unwrap();
        }
        let tm = chrono::NaiveDateTime::parse_from_str(&time_str, FORMAT)?.and_utc().timestamp() - utc_offset.whole_seconds() as i64;
        return Ok(Some(tm.to_string()));
    });

    add_fun(vec!["MD5编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&mut self_t.bin_pool,&text)?;
        let mut hasher = Md5::new();
        hasher.update(bin);
        let result = hasher.finalize();
        let mut content = String::new();
        for ch in result {
            content.push_str(&format!("{:02x}",ch));
        }
        return Ok(Some(content));
    });
    add_fun(vec!["RCNB编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&mut self_t.bin_pool,&text)?;
        let content = rcnb_rs::encode(bin);
        return Ok(Some(content));
    });
    add_fun(vec!["图片信息","图像信息"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let (img_fmt,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text)?;
        let mut mp = BTreeMap::new();
        mp.insert("宽".to_string(), img.width().to_string());
        mp.insert("高".to_string(), img.height().to_string());
        let img_fmt_str = match img_fmt {
            image::ImageFormat::Png => "png",
            image::ImageFormat::Jpeg => "jpg",
            image::ImageFormat::Gif => "gif",
            image::ImageFormat::WebP => "webp",
            image::ImageFormat::Bmp => "bmp",
            image::ImageFormat::Ico => "",
            _ => ""
        };
        if img_fmt_str == "" {
            return Err(RedLang::make_err("不能识别的图片格式"));
        }else {
            mp.insert("格式".to_string(), img_fmt_str.to_string());
        }
        let retobj = self_t.build_obj(mp);
        return Ok(Some(retobj));
    });

    add_fun(vec!["透视变换"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let (_img_fmt,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let dst_t = RedLang::parse_arr(&text2)?;
        let img_width_str = img.width().to_string();
        let img_height_str = img.height().to_string();
        let src_t:Vec<&str>;
        if text3 == "" {
            src_t = vec!["0","0",&img_width_str,"0",&img_width_str,&img_height_str,"0",&img_width_str];
        }else{
            src_t = RedLang::parse_arr(&text3)?;
        }
        if dst_t.len() != 8 || src_t.len() != 8 {
            return Err(RedLang::make_err("透视变换参数错误1"));
        }
        fn cv(v:Vec<&str>) -> Result<[(f32,f32);4], Box<dyn std::error::Error>> {
            let v_ret = [
                (v[0].parse::<f32>()?,v[1].parse::<f32>()?),
                (v[2].parse::<f32>()?,v[3].parse::<f32>()?),
                (v[4].parse::<f32>()?,v[5].parse::<f32>()?),
                (v[6].parse::<f32>()?,v[7].parse::<f32>()?)
            ];
            return Ok(v_ret);
        }
        let dst = cv(dst_t)?;
        let src = cv(src_t)?;
        let p = Projection::from_control_points(src, dst).ok_or("Could not compute projection matrix")?.invert();
        let mut img2 = warp_with(
            &img,
            |x, y| p * (x, y),
            Interpolation::Bilinear,
            Rgba([0,0,0,0]),
        );
        fn m_min(v:Vec<f32>) -> f32 {
            if v.len() == 0 {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i < m {
                    m = i;
                }
            }
            m
        }
        fn m_max(v:Vec<f32>) -> f32 {
            if v.len() == 0 {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i > m {
                    m = i;
                }
            }
            m
        }
        let x_min = m_min(vec![dst[0].0,dst[1].0,dst[2].0,dst[3].0]);
        let x_max = m_max(vec![dst[0].0,dst[1].0,dst[2].0,dst[3].0]);
        let y_min = m_min(vec![dst[0].1,dst[1].1,dst[2].1,dst[3].1]);
        let y_max = m_max(vec![dst[0].1,dst[1].1,dst[2].1,dst[3].1]);
        let img_out = image::imageops::crop(&mut img2,x_min as u32,y_min as u32,(x_max - x_min) as u32,(y_max - y_min) as u32);
        let mm = img_out.to_image();
        let ret = self_t.build_img_bin((ImageFormat::Png, mm));
        return Ok(Some(ret));
    });
    add_fun(vec!["图片叠加","图像叠加"],|self_t,params|{
        fn img_paste(img1:ImageBuffer<Rgba<u8>, Vec<u8>>,img2:ImageBuffer<Rgba<u8>, Vec<u8>>,x:i64,y:i64) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn std::error::Error>>{
            let w = img1.width();
            let h = img1.height();
            let mut img:ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
            image::imageops::overlay(&mut img, &img2, x, y);
            image::imageops::overlay(&mut img, &img1, 0, 0);
            Ok(img)
        }
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let (_img_fmt,img_vec_big) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let (_img_fmt,img_vec_sub) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        let img_out = img_paste(img_vec_big,img_vec_sub,x,y)?;
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片横拼接","图片拼接"],|self_t,params|{
        let img_text1 = self_t.get_param(params, 0)?;
        let img_text2 = self_t.get_param(params, 1)?;
        let (_,img1) = RedLang::parse_img_bin(&mut self_t.bin_pool,&img_text1)?;
        let (_,img2) = RedLang::parse_img_bin(&mut self_t.bin_pool,&img_text2)?;
        let mut back_img = ImageBuffer::new(img1.width() + img2.width(), std::cmp::max(img1.height(),img2.height()));
        image::imageops::overlay(&mut back_img, &img1, 0, 0);
        image::imageops::overlay(&mut back_img, &img2, img1.width() as i64, 0);
        let ret = self_t.build_img_bin((ImageFormat::Png, back_img));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片竖拼接"],|self_t,params|{
        let img_text1 = self_t.get_param(params, 0)?;
        let img_text2 = self_t.get_param(params, 1)?;
        let (_,img1) = RedLang::parse_img_bin(&mut self_t.bin_pool,&img_text1)?;
        let (_,img2) = RedLang::parse_img_bin(&mut self_t.bin_pool,&img_text2)?;
        let mut back_img = ImageBuffer::new(std::cmp::max(img1.width(),img2.width()),img1.height() + img2.height());
        image::imageops::overlay(&mut back_img, &img1, 0, 0);
        image::imageops::overlay(&mut back_img, &img2, 0, img1.height() as i64);
        let ret = self_t.build_img_bin((ImageFormat::Png, back_img));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片截取","图像截取"],|self_t,params|{
        let img_text1 = self_t.get_param(params, 0)?;
        let (_,img1) = RedLang::parse_img_bin(&mut self_t.bin_pool,&img_text1)?;
        let x = self_t.get_param(params, 1)?.parse::<u32>()?;
        let y = self_t.get_param(params, 2)?.parse::<u32>()?;
        let mut width = self_t.get_param(params,3)?.parse::<u32>()?;
        let mut height = self_t.get_param(params, 4)?.parse::<u32>()?;
        if x >= img1.width() || y >= img1.height()  {
            return Err("图片截取原点超出图片范围".into());
        }
        if x + width > img1.width(){
            width = img1.width() - x;
        }
        if y + height > img1.height(){
            height = img1.height() - y;
        }
        let img2 = img1.view(x, y, width, height);
        let ret = self_t.build_img_bin((ImageFormat::Png, img2.to_image()));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片覆盖","图像覆盖"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let (_,mut img_big) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let (_,img_sub) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        for i in 0..img_sub.width() {
            for j in 0..img_sub.height() {
                let ii = x + i as i64;
                let jj = y + j as i64;
                if ii >= 0 && (ii as u32) < img_big.width() && jj >= 0 && (jj as u32) < img_big.height() {
                    let pix = img_big.get_pixel_mut_checked(ii as u32, jj as u32).ok_or("image out of bound")?;
                    let pix_sub = img_sub.get_pixel_checked(i, j).ok_or("image out of bound")?;
                    pix.0[0] = pix_sub.0[0];
                    pix.0[1] = pix_sub.0[1];
                    pix.0[2] = pix_sub.0[2];
                    pix.0[3] = pix_sub.0[3];
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img_big));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片模糊","图像模糊"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let sigma = self_t.get_param(params, 1)?.parse::<f32>()?;
        let img_out;
        if sigma <= 0. {
            img_out = img;
        }else {
            
            img_out = imageproc::filter::gaussian_blur_f32(&img,sigma);
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片锐化","图像锐化"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let sigma = self_t.get_param(params, 1)?.parse::<f32>()?;
        let threshold_text = self_t.get_param(params, 2)?;
        let threshold;
        if threshold_text == "" {
            threshold = 0;
        }else{
            threshold = threshold_text.parse::<i32>()?;
        }
        let img_out;
        img_out = image::imageops::unsharpen(&img,sigma,threshold);
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片上叠加","图像上叠加"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let (_img_fmt,mut img_big) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let (_img_fmt,img_sub) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        image::imageops::overlay(&mut img_big, &img_sub, x, y);
        let ret = self_t.build_img_bin((ImageFormat::Png, img_big));
        return Ok(Some(ret));
    });
    add_fun(vec!["GIF合成"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let delay = text2.parse::<u64>()?;
        let img_arr_str = RedLang::parse_arr(&text1)?;
        let mut frame_vec:Vec<image::Frame> = vec![];
        for it in img_arr_str {
            let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,it)?;
            let fm = image::Frame::from_parts(img, 0, 0, image::Delay::from_saturating_duration(Duration::from_millis(delay)));
            frame_vec.push(fm);
        }
        let mut v:Vec<u8> = vec![];
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut v);        
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
            encoder.encode_frames(frame_vec)?;
        }
        let ret = self_t.build_bin(v);
        return Ok(Some(ret));
    });

    add_fun(vec!["GIF分解"],|self_t,params|{
        let gif_str = self_t.get_param(params, 0)?;
        let gif_bin = RedLang::parse_bin(&mut self_t.bin_pool,&gif_str)?;
        let gif = image::codecs::gif::GifDecoder::new(Cursor::new(gif_bin))?;
        let gif_frames = gif.into_frames();
        let mut ret_vec:Vec<String> = vec![];
        for frame_rst in gif_frames {
            let frame = frame_rst?;
            let img_buf = frame.into_buffer();
            ret_vec.push(self_t.build_img_bin((ImageFormat::Png, img_buf)));
        }
        let ret = self_t.build_arr(ret_vec.iter().map(AsRef::as_ref).collect());
        return Ok(Some(ret));
    });

    add_fun(vec!["WEBP分解"],|self_t,params|{
        let webp_str = self_t.get_param(params, 0)?;
        let webp_bin = RedLang::parse_bin(&mut self_t.bin_pool,&webp_str)?;
        let webp = webp::AnimDecoder::new(&webp_bin).decode()?;
        let webp_frames = webp.get_frames(0..webp.len()).ok_or("解析webp失败")?;
        let mut ret_vec:Vec<String> = vec![];
        for frame in webp_frames {
            let mut bytes: Vec<u8> = Vec::new();
            match frame.get_layout() {
                webp::PixelLayout::Rgb => {
                    let img_buf: ImageBuffer<image::Rgb<u8>, Vec<_>> = ImageBuffer::from_vec(frame.width(),frame.height(),frame.get_image().to_vec()).ok_or("解析webp失败")?;
                    img_buf.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
                    ret_vec.push(self_t.build_bin(bytes));
                },
                webp::PixelLayout::Rgba => {
                    let img_buf: ImageBuffer<image::Rgba<u8>, Vec<_>> = ImageBuffer::from_vec(frame.width(),frame.height(),frame.get_image().to_vec()).ok_or("解析webp失败")?;
                    ret_vec.push(self_t.build_img_bin((ImageFormat::Png, img_buf)))
                }
            }
        }
        let ret = self_t.build_arr(ret_vec.iter().map(AsRef::as_ref).collect());
        return Ok(Some(ret));
    });

    add_fun(vec!["WEBP合成"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let delay = text2.parse::<i32>()?;
        let img_arr_str = RedLang::parse_arr(&text1)?;
        let mut frame_vec:Vec<webp::AnimFrame> = vec![];
        let mut img_buf_vec = vec![];
        for it in img_arr_str {
            let (_,img_buf) = RedLang::parse_img_bin(&mut self_t.bin_pool,it)?;
            img_buf_vec.push(img_buf);
            
        }
        for it in &img_buf_vec {
            let fm = webp::AnimFrame::from_rgba(&it,it.width() ,it.height() , delay);
            frame_vec.push(fm);
        }
        let width;
        let height;
        if frame_vec.len() != 0 {
            width = frame_vec[0].width();
            height = frame_vec[0].height();
        }else {
            return Err(RedLang::make_err("0张图片不能进行WEBP合成"));
        }
        let mut config = webp::WebPConfig::new().unwrap();
        config.lossless = 1;
        config.quality = 100f32;
        let mut wp = webp::AnimEncoder::new(width, height, &config);
        for frame in frame_vec {
            wp.add_frame(frame);
        }
        let binding = wp.encode();
        let v = binding.as_bytes();
        let ret = self_t.build_bin(v.to_vec());
        return Ok(Some(ret));
    });

    add_fun(vec!["图片变圆","图像变圆"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,mut img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let width = img.width();
        let height = img.height();
        let r:u32;
        if width < height {
            r = width / 2;
        }else{
            r = height / 2;
        }
        for x in 0..width {
            for y in 0..height {
                if x.wrapping_sub(r).wrapping_mul(x.wrapping_sub(r)) + y.wrapping_sub(r).wrapping_mul(y.wrapping_sub(r)) > r.wrapping_mul(r){
                    let pix = img.get_pixel_mut_checked(x, y).ok_or("image out of bound")?;
                    pix.0[0] = 0;  //r
                    pix.0[1] = 0;  //g
                    pix.0[2] = 0;  //b
                    pix.0[3] = 0;  //a
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });

    add_fun(vec!["圆角"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let left_top_t = self_t.get_param(params, 1)?.parse::<u32>()?;
        let right_top_t = self_t.get_param(params, 2)?.parse::<u32>()?;
        let right_bottom_t = self_t.get_param(params, 3)?.parse::<u32>()?;
        let left_bottom_t = self_t.get_param(params, 4)?.parse::<u32>()?;
        let (_,mut img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let width = img.width();
        let height = img.height();

        for x in 0..left_top_t {
            for y in 0..left_top_t {
                let r = left_top_t;
                let ranoud = (x - r)*(x - r) + (y - r)*(y - r);
                if ranoud > r * r {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0;  //r
                        pix.0[1] = 0;  //g
                        pix.0[2] = 0;  //b
                        pix.0[3] = 0;  //a
                    }
                }
            }
        }

        for x in (width - right_top_t)..width {
            for y in 0..right_top_t {
                let r = right_top_t;
                let ranoud = (x - (width - r))*(x - (width - r)) + (y - r)*(y - r);
                if ranoud > r * r {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0;  //r
                        pix.0[1] = 0;  //g
                        pix.0[2] = 0;  //b
                        pix.0[3] = 0;  //a
                    }
                }
            }
        }


        for x in (width - right_bottom_t)..width {
            for y in (height - right_bottom_t)..height {
                let r = right_bottom_t;
                let ranoud = (x - (width - r))*(x - (width - r)) + (y - (height - r))*(y - (height - r));
                if  ranoud > r * r {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0;  //r
                        pix.0[1] = 0;  //g
                        pix.0[2] = 0;  //b
                        pix.0[3] = 0;  //a
                    }
                }
            }
        }

        for x in 0..left_bottom_t {
            for y in (height - left_bottom_t)..height {
                let r = left_bottom_t;
                let ranoud = (x - r)*(x - r) + (y - (height - r))*(y - (height - r));
                if ranoud > r * r {
                    if let Some(pix) = img.get_pixel_mut_checked(x, y) {
                        pix.0[0] = 0;  //r
                        pix.0[1] = 0;  //g
                        pix.0[2] = 0;  //b
                        pix.0[3] = 0;  //a
                    }
                }
            }
        }

        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片遮罩","图像遮罩"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let (_img_fmt,mut img_big) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let (_img_fmt,img_sub) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        for i in 0..img_sub.width() {
            for j in 0..img_sub.height() {
                let ii = x + i as i64;
                let jj = y + j as i64;
                if ii >= 0 && (ii as u32) < img_big.width() && jj >= 0 && (jj as u32) < img_big.height() {
                    let pix = img_big.get_pixel_mut_checked(ii as u32, jj as u32).ok_or("image out of bound")?;
                    let pix_sub = img_sub.get_pixel_checked(i, j).ok_or("image out of bound")?;
                    pix.0[3] = 255 - pix_sub.0[3];
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img_big));
        return Ok(Some(ret));
    });

    add_fun(vec!["图片变灰","图像变灰"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,mut img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let width = img.width();
        let height = img.height();
        for x in 0..width {
            for y in 0..height {
                let pix = img.get_pixel_mut_checked(x, y).ok_or("image out of bound")?;
                let red = pix.0[0] as f32  * 0.3;
                let green = pix.0[1] as f32  * 0.589;
                let blue = pix.0[2] as f32  * 0.11;
                let color = (red + green + blue) as u8;
                pix.0[0] = color;
                pix.0[1] = color;
                pix.0[2] = color;
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });
    fn get_char_size(font:&rusttype::Font,scale:Scale,ch:char) -> (i32, i32) {
        let v_metrics = font.v_metrics(scale);
        let text = ch.to_string();
        let mut width = 0;
        let mut height = 0;
        for g in font.layout(&text, scale, rusttype::point(0.0, v_metrics.ascent)) {
            let h_metrics = font.glyph(g.id()).scaled(scale).h_metrics();
            width = (h_metrics.advance_width + 0.5) as i32;
            if let Some(bb) = g.pixel_bounding_box() {
                height = bb.max.y;
            }
        }
        return (width,height);
    }
    add_fun(vec!["竖文字转图片","竖文字转图像"],|self_t,params|{
        let image_height = self_t.get_param(params, 0)?.parse::<u32>()?;
        let text = self_t.get_param(params, 1)?;
        let text_size = self_t.get_param(params, 2)?.parse::<f32>()?;
        let text_color_text = self_t.get_param(params, 3)?;
        let text_color = RedLang::parse_arr(&text_color_text)?;
        let mut color = Rgba::<u8>([0,0,0,255]);
        color.0[0] = text_color.get(0).unwrap_or(&"0").parse::<u8>()?;
        color.0[1] = text_color.get(1).unwrap_or(&"0").parse::<u8>()?;
        color.0[2] = text_color.get(2).unwrap_or(&"0").parse::<u8>()?;
        color.0[3] = text_color.get(3).unwrap_or(&"255").parse::<u8>()?;
        let scale =  Scale::uniform(text_size);
       
        let font_text = self_t.get_param(params, 4)?;
        let font_type = self_t.get_type(&font_text)?;
        let font_dat;
        if font_type == "字节集" {
            font_dat = RedLang::parse_bin(&mut self_t.bin_pool,&font_text)?;
        }else {
            return Err(RedLang::make_err("字体参数必须是字节集类型"));
        }
        let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
        let font_t = FontRef::try_from_slice(&font_dat)?;
        let font_sep_text = self_t.get_param(params, 5)?;
        let line_sep_text = self_t.get_param(params, 6)?;
        let font_sep;
        if font_sep_text != "" {
            font_sep = font_sep_text.parse::<usize>()?;
        } else {
            font_sep = 0usize;
        }
        let line_sep;
        if line_sep_text != "" {
            line_sep = line_sep_text.parse::<usize>()?;
        } else {
            line_sep = 0usize;
        }
        let image_width;
        {
            let mut max_x = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = 0;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width,height) = get_char_size(&font, scale, ch);
                if width > max_x {
                    max_x = width;
                }
                if ch == ' '{
                    if cur_y + ((text_size / 2. + 0.5) as i32)  < image_height as i32{
                        cur_y += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_x += max_x + line_sep as i32;
                        cur_y = 0;
                        cur_y += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_y = 0;
                    cur_x += max_x + line_sep as i32;
                }else { 
                    if cur_y + height  < image_height as i32 {
                        cur_y += height + font_sep as i32;
                    } else {
                        cur_x += max_x + line_sep as i32;
                        cur_y = 0;
                        cur_y += height + font_sep as i32;
                    }
                }
            }
            cur_x += max_x + line_sep as i32;
            image_width = cur_x;
        }
        let mut img = ImageBuffer::new(image_width as u32, image_height);
        {
            let mut max_x = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = image_width;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width,height) = get_char_size(&font, scale, ch);
                if width > max_x {
                    max_x = width;
                }
                if ch == ' '{
                    if cur_y + ((text_size / 2. + 0.5) as i32)  < img.height() as i32{
                        cur_y += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_x -= max_x + line_sep as i32;
                        cur_y = 0;
                        cur_y += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_y = 0;
                    cur_x -= max_x + line_sep as i32;
                }else { 
                    if cur_y + height  < img.height() as i32{
                        imageproc::drawing::draw_text_mut(&mut img,color,cur_x - width,cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_y += height + font_sep as i32;
                    } else {
                        cur_x -= max_x + line_sep as i32;
                        cur_y = 0;
                        imageproc::drawing::draw_text_mut(&mut img,color,cur_x - width,cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_y += height + font_sep as i32;
                    }
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });
    add_fun(vec!["文字转图片","文字转图像"],|self_t,params|{
        let image_width = self_t.get_param(params, 0)?.parse::<u32>()?;
        let text = self_t.get_param(params, 1)?;
        let text_size = self_t.get_param(params, 2)?.parse::<f32>()?;
        let text_color_text = self_t.get_param(params, 3)?;
        let text_color = RedLang::parse_arr(&text_color_text)?;
        let mut color = Rgba::<u8>([0,0,0,255]);
        color.0[0] = text_color.get(0).unwrap_or(&"0").parse::<u8>()?;
        color.0[1] = text_color.get(1).unwrap_or(&"0").parse::<u8>()?;
        color.0[2] = text_color.get(2).unwrap_or(&"0").parse::<u8>()?;
        color.0[3] = text_color.get(3).unwrap_or(&"255").parse::<u8>()?;
        let scale =  Scale::uniform(text_size);
       
        let font_text = self_t.get_param(params, 4)?;
        let font_type = self_t.get_type(&font_text)?;
        let font_dat;
        if font_type == "字节集" {
            font_dat = RedLang::parse_bin(&mut self_t.bin_pool,&font_text)?;
        }else {
            return Err(RedLang::make_err("字体参数必须是字节集类型"));
        }
        let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
        let font_t = FontRef::try_from_slice(&font_dat)?;
        let font_sep_text = self_t.get_param(params, 5)?;
        let line_sep_text = self_t.get_param(params, 6)?;
        let font_sep;
        if font_sep_text != "" {
            font_sep = font_sep_text.parse::<usize>()?;
        } else {
            font_sep = 0usize;
        }
        let line_sep;
        if line_sep_text != "" {
            line_sep = line_sep_text.parse::<usize>()?;
        } else {
            line_sep = 0usize;
        }
        let image_height;
        {
            let mut max_y = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = 0;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width,height) = get_char_size(&font, scale, ch);
                if height > max_y {
                    max_y = height;
                }
                if ch == ' '{
                    if cur_x + ((text_size / 2. + 0.5) as i32)  < image_width as i32{
                        cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        cur_x += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_x = 0;
                    cur_y += max_y + line_sep as i32;
                }else { 
                    if cur_x + width  < image_width as i32 {
                        cur_x += width + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        cur_x += width + font_sep as i32;
                    }
                }
            }
            cur_y += max_y + line_sep as i32;
            image_height = cur_y;
        }
        let mut img = ImageBuffer::new(image_width, image_height as u32);
        {
            let mut max_y = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = 0;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width,height) = get_char_size(&font, scale, ch);
                if height > max_y {
                    max_y = height;
                }
                if ch == ' '{
                    if cur_x + ((text_size / 2. + 0.5) as i32)  < img.width() as i32{
                        cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        cur_x += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_x = 0;
                    cur_y += max_y + line_sep as i32;
                }else { 
                    if cur_x + width  < img.width() as i32{
                        imageproc::drawing::draw_text_mut(&mut img,color,cur_x,cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_x += width + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        imageproc::drawing::draw_text_mut(&mut img,color,cur_x,cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_x += width + font_sep as i32;
                    }
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });
    add_fun(vec!["图片嵌字","图像嵌字"],|self_t,params|{
        let image_text = self_t.get_param(params, 0)?;
        let (_,mut img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&image_text)?;
        let text = self_t.get_param(params, 1)?;
        let text_x = self_t.get_param(params, 2)?.parse::<i32>()?;
        let text_y = self_t.get_param(params, 3)?.parse::<i32>()?;
        let text_size = self_t.get_param(params, 4)?.parse::<f32>()?;
        let text_color_text = self_t.get_param(params, 5)?;
        let text_color = RedLang::parse_arr(&text_color_text)?;
        let mut color = Rgba::<u8>([0,0,0,255]);
        color.0[0] = text_color.get(0).unwrap_or(&"0").parse::<u8>()?;
        color.0[1] = text_color.get(1).unwrap_or(&"0").parse::<u8>()?;
        color.0[2] = text_color.get(2).unwrap_or(&"0").parse::<u8>()?;
        color.0[3] = text_color.get(3).unwrap_or(&"255").parse::<u8>()?;
        let scale = Scale {
            x: text_size,
            y: text_size,
        };
        let font_text = self_t.get_param(params, 6)?;
        let font_type = self_t.get_type(&font_text)?;
        let font_dat;
        if font_type == "字节集" {
            font_dat = RedLang::parse_bin(&mut self_t.bin_pool,&font_text)?;
        }else{
            return Err(RedLang::make_err("字体参数必须是字节集类型"));
        }
        let font = rusttype::Font::try_from_bytes(&font_dat).ok_or("无法获得字体2")?;
        let font_t = FontRef::try_from_slice(&font_dat)?;
        let font_sep_text = self_t.get_param(params, 7)?;
        let line_sep_text = self_t.get_param(params, 8)?;
        let font_sep;
        if font_sep_text != "" {
            font_sep = font_sep_text.parse::<usize>()?;
        } else {
            font_sep = 0usize;
        }
        let line_sep;
        if line_sep_text != "" {
            line_sep = line_sep_text.parse::<usize>()?;
        } else {
            line_sep = 0usize;
        }
        {
            let mut max_y = 0;
            let text_chars = text.chars().collect::<Vec<char>>();
            let mut cur_x = 0;
            let mut cur_y = 0;
            for ch in text_chars {
                let (width,height) = get_char_size(&font, scale, ch);
                if height > max_y {
                    max_y = height;
                }
                if ch == ' '{
                    if text_x + cur_x + ((text_size / 2. + 0.5) as i32)  < img.width() as i32{
                        cur_x += ((text_size / 2. + 0.5) as i32) + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        cur_x += (text_size as i32) + font_sep as i32;
                    }
                } else if ch == '\n' {
                    cur_x = 0;
                    cur_y += max_y + line_sep as i32;
                }else { 
                    if text_x + cur_x + width  < img.width() as i32{
                        imageproc::drawing::draw_text_mut(&mut img,color,text_x + cur_x,text_y + cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_x += width + font_sep as i32;
                    } else {
                        cur_y += max_y + line_sep as i32;
                        cur_x = 0;
                        imageproc::drawing::draw_text_mut(&mut img,color,text_x + cur_x,text_y + cur_y,PxScale{x:scale.x,y:scale.y},&font_t,&ch.to_string());
                        cur_x += width + font_sep as i32;
                    }
                }
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });
    add_fun(vec!["创建图片","创建图像"],|self_t,params|{
        let image_width = self_t.get_param(params, 0)?.parse::<u32>()?;
        let image_height = self_t.get_param(params, 1)?.parse::<u32>()?;
        let text_color_text = self_t.get_param(params, 2)?;
        let text_color = RedLang::parse_arr(&text_color_text)?;
        let mut color = Rgba::<u8>([0,0,0,0]);
        color.0[0] = text_color.get(0).unwrap_or(&"0").parse::<u8>()?;
        color.0[1] = text_color.get(1).unwrap_or(&"0").parse::<u8>()?;
        color.0[2] = text_color.get(2).unwrap_or(&"0").parse::<u8>()?;
        color.0[3] = text_color.get(3).unwrap_or(&"255").parse::<u8>()?;
        let mut img:ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(image_width, image_height as u32);
        for x in 0..image_width {
            for y in 0..image_height {
                let pix = img.get_pixel_mut_checked(x, y).ok_or("image out of bound")?;
                pix.0 = color.0;
            }
        }
        let ret = self_t.build_img_bin((ImageFormat::Png, img));
        return Ok(Some(ret));
    });
    add_fun(vec!["水平翻转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let img_out = image::imageops::flip_horizontal(&img);
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });
    add_fun(vec!["垂直翻转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let img_out = image::imageops::flip_vertical(&img);
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });
    add_fun(vec!["图像旋转","图片旋转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
        let img_out = rotate_about_center(&img,theta,Interpolation::Bilinear,Rgba([0,0,0,0]));
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });
    add_fun(vec!["完整图像旋转","完整图片旋转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let (_,img_sub) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
        let img_width = img_sub.width();
        let img_height = img_sub.height();
        let (sin_val,cos_val) = theta.sin_cos();
        let vec1 = (img_width as f32 / 2.0,img_height as f32 / 2.0);
        let vec2 = (img_width as f32 / 2.0,-(img_height as f32 / 2.0));
        let vec1_t = (vec1.0 * cos_val - vec1.1 * sin_val,vec1.0 * sin_val + vec1.1 * cos_val);
        let vec2_t = (vec2.0 * cos_val - vec2.1 * sin_val,vec2.0 * sin_val + vec2.1 * cos_val);
        let max_width = std::cmp::max(vec1_t.0.abs() as i64,vec2_t.0.abs() as i64) as u32;
        let max_height = std::cmp::max(vec1_t.1.abs() as i64,vec2_t.1.abs() as i64) as u32;
        let max_wh = ((img_width * img_width + img_height * img_height) as f64).sqrt() as u32;
        let mut img_big:ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(max_wh, max_wh);
        image::imageops::overlay(&mut img_big, &img_sub, ((max_wh - img_width) / 2).into() , ((max_wh - img_height) / 2).into());
        let mut img_out_t = rotate_about_center(&img_big,theta,Interpolation::Bilinear,Rgba([0,0,0,0]));
        let img_out = image::imageops::crop(&mut img_out_t,((max_wh - max_width * 2) / 2).into(),((max_wh - max_height * 2) / 2).into(),max_width * 2,max_height * 2).to_image();
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });
    add_fun(vec!["图像大小调整","图片大小调整"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let mut width = self_t.get_param(params, 1)?.parse::<u32>()?;
        let mut height = self_t.get_param(params, 2)?.parse::<u32>()?;
        let (_,img) = RedLang::parse_img_bin(&mut self_t.bin_pool,&text1)?;
        if width == 0 && height != 0 {
            let k = (height as f64) / (img.height() as f64);
            width = ((img.width() as f64) * k).round() as u32;
        }else if width != 0 && height == 0 {
            let k = (width as f64) / (img.width() as f64);
            height = ((img.height() as f64) * k).round() as u32;
        }else if width == 0 && height == 0 {
            return Err(RedLang::make_err("目标图片高和宽均为0，无法调整大小"));
        }
        let img_out = image::imageops::resize(&img, width, height, image::imageops::FilterType::Nearest);
        let ret = self_t.build_img_bin((ImageFormat::Png, img_out));
        return Ok(Some(ret));
    });
    add_fun(vec!["转大写"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        return Ok(Some(text1.to_uppercase()));
    });
    add_fun(vec!["转小写"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        return Ok(Some(text1.to_lowercase()));
    });
    add_fun(vec!["打印日志"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        cq_add_log(&text).unwrap();
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["读目录"],|self_t,params|{
        let dir_name = self_t.get_param(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec:Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            let file_name = path.to_str().ok_or("获取目录文件异常")?;
            if path.is_dir() {
                ret_vec.push(format!("{}{}",file_name,std::path::MAIN_SEPARATOR));
            }else{
                ret_vec.push(file_name.to_owned());
            }
            
        }
        let ret = self_t.build_arr(ret_vec.iter().map(AsRef::as_ref).collect());
        return Ok(Some(ret));
    });
    add_fun(vec!["读目录文件"],|self_t,params|{
        let dir_name = self_t.get_param(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec:Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            if path.is_file() {
                let file_name = path.to_str().ok_or("获取目录文件异常")?.to_owned();
                ret_vec.push(file_name);
            }
        }
        let ret = self_t.build_arr(ret_vec.iter().map(AsRef::as_ref).collect());
        return Ok(Some(ret));
    });
    add_fun(vec!["目录分隔符"],|_self_t,_params|{
        return Ok(Some(std::path::MAIN_SEPARATOR.to_string()));
    });
    add_fun(vec!["去除开始空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim_start().to_string()));
    });
    add_fun(vec!["去除结尾空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim_end().to_string()));
    });
    add_fun(vec!["去除两边空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim().to_string()));
    });
    add_fun(vec!["数字转字符"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let num = text.parse::<u8>()?;
        if num > 127 || num < 1 {
            return Err(RedLang::make_err("在数字转字符中发生越界"));
        }
        return Ok(Some((num as char).to_string()));
    });
    add_fun(vec!["创建目录"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        fs::create_dir_all(path)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["读配置"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);
        let section = self_t.get_param(params, 1)?;
        let key = self_t.get_param(params, 2)?;

        let tp = self_t.get_type(&path)?;
        let conf;
        if tp == "字节集" {
            let ini_bin = RedLang::parse_bin(&mut self_t.bin_pool,&path)?;
            conf = Ini::load_from_str(&String::from_utf8(ini_bin)?)?;
        } else {
            if Path::new(&path).exists() {
                add_file_lock(&path);
                let _guard = scopeguard::guard(path.clone(), |path| {
                    del_file_lock(&path);
                });
                conf = Ini::load_from_file(path)?;
            } else {
                return Ok(Some("".to_string()));
            }
        }
        let section_t;
        if section == "" {
            let section_opt = conf.section(None::<String>);
            if section_opt.is_none() {
                return Ok(Some("".to_string()));
            }
            section_t = section_opt.unwrap();
        }else {
            let section_opt = conf.section(Some(section));
            if section_opt.is_none() {
                return Ok(Some("".to_string()));
            }
            section_t = section_opt.unwrap();
        }
        let val_opt = section_t.get(key);
        if val_opt.is_none() {
            return Ok(Some("".to_string()));
        }
        return Ok(Some(val_opt.unwrap().to_owned()));
        
    });
    add_fun(vec!["读配置节"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);

        let tp = self_t.get_type(&path)?;
        let conf;
        if tp == "字节集" {
            let ini_bin = RedLang::parse_bin(&mut self_t.bin_pool,&path)?;
            conf = Ini::load_from_str(&String::from_utf8(ini_bin)?)?;
        } else {
            if Path::new(&path).exists() {
                add_file_lock(&path);
                let _guard = scopeguard::guard(path.clone(), |path| {
                    del_file_lock(&path);
                });
                conf = Ini::load_from_file(path)?;
            } else {
                return Ok(Some(self_t.build_arr(vec![])));
            }
        }
        let mut to_ret_vec:Vec<&str> = vec![];
        for (sec, _prop) in &conf {
            if sec.is_some() {
                to_ret_vec.push(sec.unwrap());
            }
        }
        return Ok(Some(self_t.build_arr(to_ret_vec)));
    });

    add_fun(vec!["读配置键"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);

        let section_name = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&path)?;
        let conf;
        if tp == "字节集" {
            let ini_bin = RedLang::parse_bin(&mut self_t.bin_pool,&path)?;
            conf = Ini::load_from_str(&String::from_utf8(ini_bin)?)?;
        } else {
            if Path::new(&path).exists() {
                add_file_lock(&path);
                let _guard = scopeguard::guard(path.clone(), |path| {
                    del_file_lock(&path);
                });
                conf = Ini::load_from_file(path)?;
            } else {
                return Ok(Some(self_t.build_arr(vec![])));
            }
        }
        let mut to_ret_map = BTreeMap::new();
        if let Some(section) = conf.section(Some(section_name)) {
            for (key,val) in section.into_iter() {
                to_ret_map.insert(key.to_owned(), val.to_owned());
            }
        }else{
            return Ok(Some(self_t.build_obj(BTreeMap::new())));
        }
        return Ok(Some(self_t.build_obj(to_ret_map)));
    });
    
    add_fun(vec!["写配置"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);
        let section = self_t.get_param(params, 1)?;
        let key = self_t.get_param(params, 2)?;
        let value = self_t.get_param(params, 3)?;
        let parent_path = Path::new(&path).parent().ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        if Path::new(&path).exists() {
            let mut conf = Ini::load_from_file(&path)?;
            if section == "" {
                conf.with_section(None::<String>).set(key, value);
            }else{
                conf.with_section(Some(section)).set(key, value);
            }
            conf.write_to_file(path)?;
        }else {
            let mut conf = Ini::new();
            if section == "" {
                conf.with_section(None::<String>).set(key, value);
            }else{
                conf.with_section(Some(section)).set(key, value);
            }
            conf.write_to_file(path)?;
        }
        return Ok(Some("".to_string()));
    });
    fn text_to_bin_data(text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }
    add_fun(vec!["写文件"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);
        let bin_data_text = self_t.get_param(params, 1)?;
        let bin_data;
        if self_t.get_type(&bin_data_text)? == "文本" {
            bin_data = text_to_bin_data(&bin_data_text);
        } else {
            bin_data = RedLang::parse_bin(&mut self_t.bin_pool,&bin_data_text)?;
        }
        let parent_path = Path::new(&path).parent().ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        let mut f = fs::File::create(path)?;
        std::io::Write::write_all(&mut f, &bin_data)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["追加文件"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let path = crate::mytool::path_to_os_str(&path);
        let bin_data_text = self_t.get_param(params, 1)?;
        let bin_data;
        if self_t.get_type(&bin_data_text)? == "文本" {
            bin_data = text_to_bin_data(&bin_data_text);
        } else {
            bin_data = RedLang::parse_bin(&mut self_t.bin_pool,&bin_data_text)?;
        }
        let parent_path = Path::new(&path).parent().ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        let mut f;
        if Path::new(&path).exists() {
            f = fs::OpenOptions::new().append(true).open(path)?
        }else {
            f = fs::File::create(path)?;
        }
        std::io::Write::write_all(&mut f, &bin_data)?;
        return Ok(Some("".to_string()));
    });
    
    add_fun(vec!["设置浏览器宽高"],|self_t,params|{
        let k1 = self_t.get_param(params, 0)?;
        let k2 = self_t.get_param(params, 1)?;
        // 校验
        k1.parse::<u32>()?; 
        k2.parse::<u32>()?; 
        self_t.set_coremap("浏览器宽", &k1)?;
        self_t.set_coremap("浏览器高", &k2)?;
        return Ok(Some("".to_string()));
    });


    add_fun(vec!["网页截图调试"],|self_t,params|{
        let k1: String = self_t.get_param(params, 0)?;
        self_t.set_coremap("网页截图调试", &k1)?;
        return Ok(Some("".to_string()));
    });

    
    add_fun(vec!["网页截图"],|self_t,params|{
        fn access(self_t:&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let mut path = self_t.get_param(params, 0)?;
            if !cfg!(target_os = "windows") {
                if path.starts_with("/") {
                    path = format!("file://{}", &path);
                }
            }
            let sec = self_t.get_param(params, 1)?;
            let mut arg_vec:Vec<&std::ffi::OsStr> = vec![];
            let proxy_str = self_t.get_coremap("代理")?;
            let proxy:std::ffi::OsString;
            if proxy_str != "" {
                proxy = std::ffi::OsString::from("--proxy-server=".to_owned() + &proxy_str);
                arg_vec.push(&proxy);
            }

            let width = self_t.get_coremap("浏览器宽")?.parse::<u32>().unwrap_or(1920);
            let height = self_t.get_coremap("浏览器高")?.parse::<u32>().unwrap_or(1080);

            let headless;
            if self_t.get_coremap("网页截图调试")? == "真" {
                headless = false;
            } else{
                headless = true;
            }

            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "30000".to_owned();
            }

            let options = headless_chrome::LaunchOptions::default_builder()
                .window_size(Some((width, height)))
                .headless(headless)
                .devtools(false)
                .sandbox(false)
                .idle_browser_timeout(Duration::from_millis(timeout_str.parse()?))
                .args(arg_vec)
                .build()?;
            let browser = headless_chrome::Browser::new(options)?;
            let tab = browser.new_tab()?;
            {
                let http_header_str = self_t.get_coremap("访问头")?;
                if http_header_str != "" {
                    let http_header = RedLang::parse_obj(&http_header_str)?;
                    let mut headers = HashMap::new();
                    for (k,v) in http_header.iter() {
                            headers.insert(k.as_str(), v.as_str());
                    }
                    tab.set_extra_http_headers(headers)?;
                }
            }
            tab.navigate_to(&path)?.wait_until_navigated()?;
            let el_html= tab.wait_for_element("html")?;
            let body_height = el_html.get_box_model()?.height;
            let body_width = el_html.get_box_model()?.width;
            
            tab.set_bounds(headless_chrome::types::Bounds::Normal { left: Some(0), top: Some(0), width:Some(body_width), height: Some(body_height + 200f64) })?;
            let mut el = el_html;
            if sec != ""{
                el = tab.wait_for_element(&sec)?;
            }
            let png_data = tab.capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                Some(el.get_box_model()?.content_viewport()),
                true)?;
            return Ok(Some(self_t.build_bin(png_data)));
        }
        match access(self_t,params) {
            Ok(ret) => return Ok(ret),
            Err(err) => {
                cq_add_log_w(&format!("网页截图失败：`{:?}`",err)).unwrap();
                return Ok(Some(self_t.build_bin(vec![])));
            }
        }
    });

    add_fun(vec!["网页获取"],|self_t,params|{
        fn access(self_t:&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let path = self_t.get_param(params, 0)?;
            let sec = self_t.get_param(params, 1)?;
            let mut arg_vec:Vec<&std::ffi::OsStr> = vec![];
            let proxy_str = self_t.get_coremap("代理")?;
            let proxy:std::ffi::OsString;
            if proxy_str != "" {
                proxy = std::ffi::OsString::from("--proxy-server=".to_owned() + &proxy_str);
                arg_vec.push(&proxy);
            }

            let width = self_t.get_coremap("浏览器宽")?.parse::<u32>().unwrap_or(1920);
            let height = self_t.get_coremap("浏览器高")?.parse::<u32>().unwrap_or(1080);

            let headless;
            if self_t.get_coremap("网页截图调试")? == "真" {
                headless = false;
            } else{
                headless = true;
            }

            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "30000".to_owned();
            }

            let options = headless_chrome::LaunchOptions::default_builder()
                .window_size(Some((width, height)))
                .headless(headless)
                .sandbox(false)
                .devtools(false)
                .idle_browser_timeout(Duration::from_millis(timeout_str.parse()?))
                .args(arg_vec)
                .build()?;
            let browser = headless_chrome::Browser::new(options)?;
            let tab = browser.new_tab()?;
            {
                let http_header_str = self_t.get_coremap("访问头")?;
                if http_header_str != "" {
                    let http_header = RedLang::parse_obj(&http_header_str)?;
                    let mut headers = HashMap::new();
                    for (k,v) in http_header.iter() {
                            headers.insert(k.as_str(), v.as_str());
                    }
                    tab.set_extra_http_headers(headers)?;
                }
            }
            tab.navigate_to(&path)?.wait_until_navigated()?;
            let el_html= tab.wait_for_element("html")?;
            let mut el = el_html;
            if sec != ""{
                el = tab.wait_for_element(&sec)?;
            }
            
            let inner_text = el.get_content()?;
            return Ok(Some(inner_text));
        }
        match access(self_t,params) {
            Ok(ret) => return Ok(ret),
            Err(err) => {
                cq_add_log_w(&format!("网页获取失败：`{:?}`",err)).unwrap();
                return Ok(Some("".to_owned()));
            }
        }
    });


    add_fun(vec!["网页获取文本"],|self_t,params|{
        fn access(self_t:&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>> {
            let path = self_t.get_param(params, 0)?;
            let sec = self_t.get_param(params, 1)?;
            let mut arg_vec:Vec<&std::ffi::OsStr> = vec![];
            let proxy_str = self_t.get_coremap("代理")?;
            let proxy:std::ffi::OsString;
            if proxy_str != "" {
                proxy = std::ffi::OsString::from("--proxy-server=".to_owned() + &proxy_str);
                arg_vec.push(&proxy);
            }

            let width = self_t.get_coremap("浏览器宽")?.parse::<u32>().unwrap_or(1920);
            let height = self_t.get_coremap("浏览器高")?.parse::<u32>().unwrap_or(1080);

            let headless;
            if self_t.get_coremap("网页截图调试")? == "真" {
                headless = false;
            } else{
                headless = true;
            }

            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "30000".to_owned();
            }

            let options = headless_chrome::LaunchOptions::default_builder()
                .window_size(Some((width, height)))
                .headless(headless)
                .devtools(false)
                .sandbox(false)
                .idle_browser_timeout(Duration::from_millis(timeout_str.parse()?))
                .args(arg_vec)
                .build()?;
            let browser = headless_chrome::Browser::new(options)?;
            let tab = browser.new_tab()?;
            {
                let http_header_str = self_t.get_coremap("访问头")?;
                if http_header_str != "" {
                    let http_header = RedLang::parse_obj(&http_header_str)?;
                    let mut headers = HashMap::new();
                    for (k,v) in http_header.iter() {
                            headers.insert(k.as_str(), v.as_str());
                    }
                    tab.set_extra_http_headers(headers)?;
                }
            }
            tab.navigate_to(&path)?.wait_until_navigated()?;
            let el_html= tab.wait_for_element("html")?;
            let mut el = el_html;
            if sec != ""{
                el = tab.wait_for_element(&sec)?;
            }
            
            let inner_text = el.get_inner_text()?;
            return Ok(Some(inner_text));
        }
        match access(self_t,params) {
            Ok(ret) => return Ok(ret),
            Err(err) => {
                cq_add_log_w(&format!("网页获取文本失败：`{:?}`",err)).unwrap();
                return Ok(Some("".to_owned()));
            }
        }
    });
    
    add_fun(vec!["命令行"],|self_t,params|{
        let cmd_str = self_t.get_param(params, 0)?;
        let currdir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        let output = if cfg!(target_os = "windows") {
            // cmd 的解析规则too复杂，我不想看了 https://learn.microsoft.com/zh-cn/windows-server/administration/windows-commands/cmd
            let tmp_dir_rst = get_tmp_dir();
            if tmp_dir_rst.is_err() {
                return Err(format!("get_tmp_dir err:{:?}",tmp_dir_rst.err().unwrap()).into());
            }
            let tmp_dir = tmp_dir_rst.unwrap();
            let tmp_file = uuid::Uuid::new_v4().to_string() + ".cmd";
            let tmp_file_path = PathBuf::from_str(&tmp_dir)?.join(tmp_file);
            let mut gbk_cmd:Vec<u8> = vec![];
            gbk_cmd.append(&mut "@echo off\r\n".as_bytes().to_vec());
            gbk_cmd.append(&mut encoding::Encoding::encode(encoding::all::GBK, &cmd_str, encoding::EncoderTrap::Ignore)?);
            fs::write(&tmp_file_path, gbk_cmd)?;
            let _guard = scopeguard::guard(tmp_file_path.clone(), |tmp_file_path| {
                let _foo = fs::remove_file(tmp_file_path);
            });

            let mut command = std::process::Command::new("cmd");

            #[cfg(windows)]
            let out = command.creation_flags(0x08000000).current_dir(currdir).arg("/c").arg(tmp_file_path).output()?;

            #[cfg(not(windows))]
            let out = command.current_dir(currdir).arg("/c").arg(tmp_file_path).output()?;

            out
        } else {
            std::process::Command::new("sh").current_dir(currdir).arg("-c").arg(cmd_str).output()?
        };
        let mut output_str = 
        if cfg!(target_os = "windows") {
            let code_t = self_t.get_param(params, 1)?;
            let code = code_t.to_lowercase();
            if code == "utf8" || code == "utf-8" {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else if code == "gbk" || code == "" {
                encoding::all::GBK.decode(&output.stdout, encoding::DecoderTrap::Ignore)?
            } else {
                return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
            }
        }else {
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        let out_err = if cfg!(target_os = "windows") {
            encoding::all::GBK.decode(&output.stderr, encoding::DecoderTrap::Ignore)?
        }else {
            String::from_utf8_lossy(&output.stderr).to_string()
        };
        output_str.push_str(&out_err);
        return Ok(Some(output_str));
    });
    add_fun(vec!["启动"],|self_t,params|{
        let cmd_str = self_t.get_param(params, 0)?;
        opener::open(cmd_str)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["文本查找"],|self_t,params|{ 
        let text = self_t.get_param(params, 0)?;
        let sub = self_t.get_param(params, 1)?;
        let pos_str= self_t.get_param(params, 2)?;
        let pos;
        if pos_str == "" {
            pos = 0;
        }else {
            pos = pos_str.parse::<usize>()?;
        }
        let text_chs = text.chars().collect::<Vec<char>>();
        if pos >= text_chs.len() {
            return Ok(Some("-1".to_string()));
        }
        let text_str = text_chs.get(pos..).unwrap().iter().collect::<String>();
        if let Some(pos1) = text_str.find(&sub) {
            let t = text_str.get(0..pos1).unwrap();
            let pos2 = t.chars().collect::<Vec<char>>().len();
            return Ok(Some((pos + pos2).to_string()));
        }
        return Ok(Some("-1".to_string()));
    });

    add_fun(vec!["数组查找"],|self_t,params|{ 
        let arr_str = self_t.get_param(params, 0)?;
        let sub = self_t.get_param(params, 1)?;
        let pos_str= self_t.get_param(params, 2)?;
        let mut pos;
        if pos_str == "" {
            pos = 0;
        }else {
            pos = pos_str.parse::<usize>()?;
        }
        let arr = RedLang::parse_arr(&arr_str)?;
        while pos < arr.len() {
            if arr[pos] == sub {
                return Ok(Some(pos.to_string()));
            }
            pos += 1;
        }
        return Ok(Some("-1".to_string()));
    });

    add_fun(vec!["错误信息"],|self_t,_params|{
        return Ok(Some(self_t.get_coremap("错误信息")?.to_owned()));
    });
    
    add_fun(vec!["运行SQL"],|self_t,params|{
        let sql_file = self_t.get_param(params, 0)?;
        let sql_file = crate::mytool::path_to_os_str(&sql_file);
        let sql = self_t.get_param(params, 1)?;
        let sql_params_str = self_t.get_param(params, 2)?;
                let sql_params;
        if sql_params_str == "" {
            sql_params = vec![];
        }else{
            sql_params = RedLang::parse_arr(&sql_params_str)?;
        }

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        
        fn add_regexp_function(db: &rusqlite::Connection) -> rusqlite::Result<()> {
            use rusqlite::functions::FunctionFlags;
            use rusqlite::{Error, Result};
            type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
            db.create_scalar_function(
                "regexp",
                2,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |ctx| {
                    let regexp: Arc<fancy_regex::Regex> = ctx.get_or_create_aux(0, |vr| -> Result<_, BoxError> {
                        Ok(fancy_regex::Regex::new(vr.as_str()?)?)
                    })?;
                    let is_match = {
                        let text = ctx
                            .get_raw(1)
                            .as_str()
                            .map_err(|e| Error::UserFunctionError(e.into()))?;
                        regexp.is_match(text).map_err(|e| Error::UserFunctionError(e.into()))?
                    };
                    Ok(is_match)
                },
            )
        }


        fn add_red_function(self_t:&mut RedLang,db: &rusqlite::Connection) -> rusqlite::Result<()> {
            use rusqlite::functions::FunctionFlags;
            use rusqlite::Error;
            
            let exmap = (*self_t.exmap).borrow().clone();
            let pkg_name = self_t.pkg_name.clone();
            let script_name = self_t.script_name.clone();
            let can_wrong = self_t.can_wrong;

            db.create_scalar_function(
                "red",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |ctx| {
                    let code = ctx.get::<String>(0)?;
                    
                    let mut rl = RedLang::new();
                    rl.exmap = std::rc::Rc::new(std::cell::RefCell::new(exmap.clone()));
                    rl.pkg_name = pkg_name.clone();
                    rl.script_name = script_name.clone();
                    rl.can_wrong = can_wrong;
                    
                    let red_out_rst = rl.parse(&code);
                    match red_out_rst {
                        Ok(red_out) => Ok(red_out),
                        Err(e) => Err(Error::UserFunctionError(e.to_string().into())),
                    }
                },
            )
        }

        let conn = rusqlite::Connection::open(sql_file)?;
        add_regexp_function(&conn)?;
        add_red_function(self_t,&conn)?;
        let mut stmt = conn.prepare(&sql)?;
        let count = stmt.column_count();
        let mut vec:Vec<String> = vec![];
        let mut rows = stmt.query(rusqlite::params_from_iter(sql_params.iter()))?;
        while let Some(row) = rows.next()? {
            let mut v:Vec<String> = vec![];
            for i in 0..count {
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
        let changes = conn.changes().to_string();
        self_t.set_coremap("SQL修改数", &changes)?;
        return Ok(Some(self_t.build_arr(vec.iter().map(AsRef::as_ref).collect())));
    });
    add_fun(vec!["SQL修改数"],|self_t,_params|{
        let k = self_t.get_coremap("SQL修改数")?;
        if k == "" {
            return Ok(Some("0".to_owned()));
        }
        return Ok(Some(k));
    });
    add_fun(vec!["定义持久常量"],|self_t,params|{
        
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);
        
        let key = self_t.get_param(params, 0)?;
        let mut value = self_t.get_param(params, 1)?;

        if value.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            value = get_raw_data(self_t, value)?;
        }

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });
        
        let conn = rusqlite::Connection::open(sql_file)?;
        conn.execute("CREATE TABLE IF NOT EXISTS CONST_TABLE (KEY TEXT PRIMARY KEY,VALUE TEXT);", [])?;
        conn.execute("REPLACE INTO CONST_TABLE (KEY,VALUE) VALUES (?,?)", [key,value])?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["持久常量"],|self_t,params|{
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);

        let key = self_t.get_param(params, 0)?;
        
        
        let ret_rst:Result<String,rusqlite::Error>;

        {
            add_file_lock(&sql_file);
            let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
                del_file_lock(&sql_file);
            });
            let conn = rusqlite::Connection::open(sql_file)?;
            ret_rst = conn.query_row("SELECT VALUE FROM CONST_TABLE WHERE KEY = ?", [key], |row| row.get(0));
        }
        
        let ret_str;
        if let Ok(ret) =  ret_rst {
            ret_str = ret;
        }else {
            ret_str =  self_t.get_param(params, 1)?;
        }
        return Ok(Some(ret_str));
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["截屏"],|self_t,params|{
        let num_str = self_t.get_param(params, 0)?;
        let mut num = 0usize;
        if num_str != "" {
            num = num_str.parse::<usize>()?;
        }
        let monitors = xcap::Monitor::all()?;
        if num + 1 > monitors.len() {
            return Ok(Some(self_t.build_bin(vec![])));
        }
        let image = monitors[num].capture_image()?;
        let mut bytes: Vec<u8> = Vec::new();
        image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
        return Ok(Some(self_t.build_bin(bytes)));
    });
    
    add_fun(vec!["文件信息"],|self_t,params|{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        let mut ret_obj:BTreeMap<String, String> = BTreeMap::new();
        if !path.exists() {
            return Ok(Some(self_t.build_obj(BTreeMap::new())));
        }
        let file_info = fs::metadata(path)?;
        ret_obj.insert("大小".to_string(), file_info.len().to_string());
        if let Ok(val) = file_info.created() {
            ret_obj.insert("创建时间".to_string(), val.duration_since(std::time::UNIX_EPOCH)?.as_secs().to_string());
        }
        if let Ok(val) = file_info.modified() {
            ret_obj.insert("修改时间".to_string(), val.duration_since(std::time::UNIX_EPOCH)?.as_secs().to_string());
        }
        if let Ok(val) = file_info.accessed() {
            ret_obj.insert("访问时间".to_string(), val.duration_since(std::time::UNIX_EPOCH)?.as_secs().to_string());
        }
        if file_info.is_dir() {
            ret_obj.insert("类型".to_string(), "目录".to_string());
        }else {
            ret_obj.insert("类型".to_string(), "文件".to_string());
        }
        if file_info.is_symlink() {
            ret_obj.insert("符号链接".to_string(), "真".to_string());
        }else {
            ret_obj.insert("符号链接".to_string(), "假".to_string());
        }
        return Ok(Some(self_t.build_obj(ret_obj)));
    });

    add_fun(vec!["判存"],|self_t,params|{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        let ret_str;
        if !path.exists() {
            ret_str = self_t.get_param(params, 1)?;
        } else {
            ret_str = file_path;
        }
        return Ok(Some(ret_str));
    });

    add_fun(vec!["删除目录"],|self_t,params|{
        let dir_path = self_t.get_param(params, 0)?;
        let path = Path::new(&dir_path);
        let _foo = fs::remove_dir_all(path);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["删除文件"],|self_t,params|{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        let _foo = fs::remove_file(path)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["压缩"],|self_t,params|{
        let src_path = self_t.get_param(params, 0)?;
        let remote_path = self_t.get_param(params, 1)?;
        sevenz_rust::compress_to_path(src_path, remote_path)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["解压"],|self_t,params|{
        let src_path = self_t.get_param(params, 0)?;
        let remote_path = self_t.get_param(params, 1)?;
        sevenz_rust::decompress_file(src_path, remote_path)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["去重"],|self_t,params|{
        let arr_text = self_t.get_param(params, 0)?;
        let arr = RedLang::parse_arr(&arr_text)?;
        let mut arr_out:Vec<String> = vec![];
        for txt in arr {
            let txt_t = txt.to_owned();
            if !arr_out.contains(&txt_t) {
                arr_out.push(txt.to_owned());
            }
        }
        let arr_out_t = arr_out.iter().map(|x|x.as_str()).collect();
        return Ok(Some(self_t.build_arr(arr_out_t)));
    });
    add_fun(vec!["打乱"],|self_t,params|{
        let arr_text = self_t.get_param(params, 0)?;
        let arr = RedLang::parse_arr(&arr_text)?;
        let mut arr_out = vec![];
        for it in arr {
            arr_out.push(it);
        }
        for i in 0..arr_out.len() {
            let rand_i = get_random()? % arr_out.len();
            if rand_i != i {
                let k = arr_out[i];
                arr_out[i] = arr_out[rand_i];
                arr_out[rand_i] = k;
            }
        }
        return Ok(Some(self_t.build_arr(arr_out)));
    });
    add_fun(vec!["合并"],|self_t,params|{
        let arr_text = self_t.get_param(params, 0)?;
        let arr = RedLang::parse_arr(&arr_text)?;
        let txt = self_t.get_param(params, 1)?;
        let mut str_out = String::new();
        for i in 0..arr.len() {
            let tp = self_t.get_type(arr[i])?;
            if tp != "文本" {
                return Err(RedLang::make_err("只能对元素类型为文本的数组进行合并"));
            }
            str_out.push_str(arr[i]);
            if i != arr.len() - 1 {
                str_out.push_str(&txt);
            }
        }
        return Ok(Some(str_out));
    });

    
    add_fun(vec!["骰"],|self_t,params|{
        fn tou(input:&str) -> Result <String, Box<dyn std::error::Error>> {
            // 分割tokens
            fn get_token(input: &str) -> Vec<String> {
                let mut ret_vec:Vec<String> = vec![];
                let mut tmp = String::new();
                for ch in input.chars() {
                    if ch.is_ascii_digit() {
                        tmp.push(ch);
                    } else if ch.is_ascii_lowercase() {
                        if tmp != "" {
                            ret_vec.push(tmp.clone());
                            tmp.clear();
                            ret_vec.push(ch.to_string());
                        }
                    }
                }
                if tmp != "" {
                    ret_vec.push(tmp);
                }
                return ret_vec;
            }
        
            // 取参数
            fn get_params(tokens:&Vec<String>) -> HashMap<String,(Option<i64>,Option<i64>)> {
                let mut ret_map = HashMap::new();
                fn get_left(index:usize,tokens:&Vec<String>) -> Option<i64> {
                    if index == 0 {
                        return None;
                    } else {
                        if let Ok(num) = tokens[index - 1].parse::<i64>() {
                            return Some(num);
                        }else {
                            return None;
                        }
                    }
                }
                fn get_right(index:usize,tokens:&Vec<String>) -> Option<i64> {
                    if index == tokens.len() - 1 {
                        return None;
                    } else {
                        if let Ok(num) = tokens[index + 1].parse::<i64>() {
                            return Some(num);
                        }else {
                            return None;
                        }
                    }
                }
                for index in 0..tokens.len() {
                    let it = &tokens[index];
                    if it == "d" || it == "k" || it == "q" || it == "p" || it == "b" || it == "m" || it == "a" || it == "c" || it == "f"  {
                        ret_map.insert(it.to_owned(), (get_left(index,tokens),get_right(index,&tokens)));
                    }
                }
                return ret_map;
            }
            let tokens = get_token(input);
            let mp = get_params(&tokens);
        
            fn get_tou(m:i64,des:&mut String) -> i64 {
                let ret = get_random().unwrap() % m as usize + 1;
                des.push_str(&format!("掷出面数为{}的骰子:{}\n",m,ret));
                return ret as i64;
            }
        
            fn get_two_tou(des:&mut String) -> i64 {
                let ret1 = get_random().unwrap() % 10 as usize + 1;
                let ret2 = get_random().unwrap() % 10 as usize + 1;
                des.push_str(&format!("掷出两个面数为10的骰子:{:?}\n",vec![ret1,ret2]));
                let mut ret = (ret1 - 1) * 10 + ret2 - 1;
                if ret == 0 {
                    ret = 100;
                }
                des.push_str(&format!("组成一个面数位100的骰子:{}\n",ret));
                return ret as i64;
            }
            let mut des = String::new();
            if  mp.contains_key("f") { // FATE掷骰池
                des += "检测到c参数,所以为`FATE掷骰池`\n";
                let f = mp["f"].0.unwrap_or(4);
                let mut tou_vec:Vec<i64> = vec![];
                for _i in 0..f {
                    tou_vec.push(((get_random().unwrap() % 3 as usize) as i64) - 1);
                }
                des.push_str(&format!("掷出{f}个三面骰:{tou_vec:?}\n"));
                let mut sum:i64 = 0;
                for it in tou_vec {
                    sum += it;
                }
                des.push_str(&format!("{}个骰子的总和为:{}",f,sum));
            } else if mp.contains_key("d") { // 只可能是普通多面骰
                des += "检测到d参数,所以为`普通多面掷骰`\n";
                if mp.contains_key("a") { // 应该转化为无限加骰池
                    des += "检测到a参数,转化为`无限加骰池`\n";
                    let a = mp["d"].0.unwrap_or(1);
                    let bb = mp["d"].1.ok_or("err1")? + 1;
                    let b = bb - 1;
                    let e = mp["a"].1.ok_or("err2")?;
                    let zhjg = format!("{a}a{bb}k{e}m{b}");
                    des.push_str(&format!("转化结果:{zhjg}\n"));
                    des += &tou(&zhjg)?;
                } else if mp.contains_key("k") { // 选取最大
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut tou_vec:Vec<i64> = vec![];
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:",i+1));
                        tou_vec.push(get_tou(ms,&mut des));
                    }
                    tou_vec.sort();
                    tou_vec.reverse();
                    des.push_str(&format!("从大到小排序后为:{:?}\n",tou_vec));
                    let k = mp["k"].1.unwrap_or(ts);
                    let tou_vec2 = tou_vec.get(0..k as usize).ok_or("err3")?;
                    des.push_str(&format!("选取最大的{}个骰子:{:?}\n",k,tou_vec2));
                    let mut sum = 0;
                    for it in tou_vec2 {
                        sum += it;
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}",k,sum));
                } else if mp.contains_key("q") { // 选取最小
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut tou_vec:Vec<i64> = vec![];
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:",i+1));
                        tou_vec.push(get_tou(ms,&mut des));
                    }
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n",tou_vec));
                    let k = mp["q"].1.unwrap_or(ts);
                    let tou_vec2 = tou_vec.get(0..k as usize).ok_or("err4")?;
                    des.push_str(&format!("选取最小的{}个骰子:{:?}\n",k,tou_vec2));
                    let mut sum = 0;
                    for it in tou_vec2 {
                        sum += it;
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}",k,sum));
                } else if mp.contains_key("p") { // 追加惩罚
                    let p_num = mp["p"].1.unwrap_or(0);
                    let mut ret = get_two_tou(&mut des);
                    let mut p_vec:Vec<i64> = vec![];
                    for _i in 0..p_num {
                        p_vec.push((get_random().unwrap() % 10 as usize + 1) as i64)
                    }
                    des.push_str(&format!("掷出{p_num}个惩罚骰:{p_vec:?}\n"));
                    p_vec.sort();
                    p_vec.reverse();
                    des.push_str(&format!("从大到小排序后为:{:?}\n",p_vec));
                    let sw = ret / 10;
                    if p_vec.get(0).ok_or("err5")? > &sw {
                        ret = ret % 10 + p_vec[0] * 10;
                    }
                    des.push_str(&format!("惩罚后的结果是:{ret}"));
                } else if mp.contains_key("b") { // 追加奖励 
                    let p_num = mp["b"].1.unwrap_or(0);
                    let mut ret = get_two_tou(&mut des);
                    let mut p_vec:Vec<i64> = vec![];
                    for _i in 0..p_num {
                        p_vec.push((get_random().unwrap() % 10 as usize + 1) as i64)
                    }
                    des.push_str(&format!("掷出{p_num}个奖励骰:{p_vec:?}\n"));
                    p_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n",p_vec));
                    let sw = ret / 10;
                    if p_vec.get(0).ok_or("err6")? < &sw {
                        ret = ret % 10 + p_vec[0] * 10;
                    }
                    des.push_str(&format!("奖励后的结果是:{ret}"));
                } else { // 普通掷骰
                    let ts = mp["d"].0.unwrap_or(1);
                    let ms = mp["d"].1.unwrap_or(100);
                    let mut sum:i64 = 0;
                    for i in 0..ts {
                        des.push_str(&format!("第{}次:",i+1));
                        sum += get_tou(ms,&mut des);
                    }
                    des.push_str(&format!("{}个骰子的总和为:{}",ts,sum));
                }
            } else if mp.contains_key("p") { // 惩罚
                des += "检测到p参数,所以为`惩罚骰`\n";
                let p_num = mp["p"].1.unwrap_or(0);
                let mut ret = get_two_tou(&mut des);
                let mut p_vec:Vec<i64> = vec![];
                for _i in 0..p_num {
                    p_vec.push((get_random().unwrap() % 10 as usize + 1) as i64)
                }
                des.push_str(&format!("掷出{p_num}个惩罚骰:{p_vec:?}\n"));
                p_vec.sort();
                p_vec.reverse();
                des.push_str(&format!("从大到小排序后为:{:?}\n",p_vec));
                let sw = ret / 10;
                if p_vec.get(0).ok_or("err7")? > &sw {
                    ret = ret % 10 + p_vec[0] * 10;
                }
                des.push_str(&format!("惩罚后的结果是:{ret}"));
            } else if mp.contains_key("b") { // 奖励 
                des += "检测到p参数,所以为`奖励骰`\n";
                let p_num = mp["b"].1.unwrap_or(0);
                let mut ret = get_two_tou(&mut des);
                let mut p_vec:Vec<i64> = vec![];
                for _i in 0..p_num {
                    p_vec.push((get_random().unwrap() % 10 as usize + 1) as i64)
                }
                des.push_str(&format!("掷出{p_num}个奖励骰:{p_vec:?}\n"));
                p_vec.sort();
                des.push_str(&format!("从小到大排序后为:{:?}\n",p_vec));
                let sw = ret / 10;
                if p_vec.get(0).ok_or("err8")? < &sw {
                    ret = ret % 10 + p_vec[0] * 10;
                }
                des.push_str(&format!("奖励后的结果是:{ret}"));
            } else if  mp.contains_key("a") { // 无限加骰池
                des += "检测到a参数,所以为`无限加骰池`\n";
                let mut a1 = mp["a"].0.unwrap_or(1);
                let a2 = mp["a"].1.ok_or("err9")?;
                let mut m = 10;
                if mp.contains_key("m") {
                    m = mp["m"].1.unwrap();
                }
                let mut cgx = 8;
                if mp.contains_key("k") {
                    cgx = mp["k"].1.unwrap();
                }
                let mut fxcgx = m;
                if mp.contains_key("q") {
                    fxcgx = mp["q"].1.unwrap();
                }
                let mut ls: i32 = 1;
                let mut ret = 0;
                loop {
                    let mut tou_vec:Vec<i64> = vec![];
                    for _i in 0..a1 {
                        tou_vec.push((get_random().unwrap() % m as usize + 1) as i64);
                    }
                    des.push_str(&format!("第{ls}轮掷骰:{tou_vec:?}\n"));
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n",tou_vec));
                    a1 = 0;
                    for it in &tou_vec {
                        if it >= &a2 {
                            a1 += 1;
                        }
                    }
                    des.push_str(&format!("下一轮掷骰数目:{a1}\n"));
                    for it in tou_vec {
                        if it >= cgx && it <= fxcgx {
                            ret += 1;
                        }
                    }
                    des.push_str(&format!("累计成功骰数:{ret}"));
                    ls += 1;
                    if a1 != 0 {
                        des.push_str(&format!("\n"));
                    }else {
                        break;
                    }
                }
            }else if  mp.contains_key("c") { // 双重十字加骰池
                des += "检测到c参数,所以为`双重十字加骰池`\n";
                let mut a = mp["c"].0.unwrap();
                let b = mp["c"].1.unwrap();
                let mut m = 10;
                if mp.contains_key("m") {
                    m = mp["m"].1.unwrap();
                }
                let mut ls = 1;
                let mut ret = 0;
                loop {
                    let mut tou_vec:Vec<i64> = vec![];
                    for _i in 0..a {
                        tou_vec.push((get_random().unwrap() % m as usize + 1) as i64);
                    }
                    des.push_str(&format!("第{ls}轮掷骰:{tou_vec:?}\n"));
                    tou_vec.sort();
                    des.push_str(&format!("从小到大排序后为:{:?}\n",tou_vec));
                    a = 0;
                    for it in &tou_vec {
                        if it >= &b {
                            a += 1;
                        }
                    }
                    des.push_str(&format!("下一轮掷骰数目:{a}\n"));
                    if a != 0 {
                        ret += m;
                    }else {
                                                ret += tou_vec.get(tou_vec.len() - 1).ok_or("err10")?;
                    }
                    des.push_str(&format!("累计成功骰数:{ret}\n"));
                    if a == 0 {
                        break;
                    }else {
                        des.push_str(&format!("\n"));
                    }
                    ls += 1;
                }
            }
            return Ok(des);
        }
        let text = self_t.get_param(params, 0)?;
        let ret = tou(&text)?;
        return Ok(Some(ret));
    });
    add_fun(vec!["补位"],|self_t,params|{
        let mut num_f64 = self_t.get_param(params, 0)?.parse::<f64>()?;
        let is_f;
        if num_f64 < 0.0 {
            num_f64 = -num_f64;
            is_f = true;
        }else {
            is_f = false;
        }
        let mut num = num_f64.to_string();
        let num_format: String = self_t.get_param(params, 1)?;
        // 得到整数部分和小数部分的位数
        let mut num_format_left_count;
        let mut num_format_right_count;
        if let Some(pos) = num_format.find('.') {
            num_format_left_count = pos;
            num_format_right_count = num_format.len() - pos - 1;
        }else {
            num_format_left_count = num_format.len();
            num_format_right_count = 0;
        }
        // 得到整数部分和小数部分的位数
        let mut num_left_count;
        let mut num_right_count;
        if let Some(pos) = num.find('.') {
            num_left_count = pos;
            num_right_count = num.len() - pos - 1;
        }else {
            num_left_count = num.len();
            num_right_count = 0;
        }
        // 处理四舍五入
        if num_right_count > num_format_right_count {
            let ch = num.as_bytes()[num_left_count + 1 + num_format_right_count];
            if ch - 48 >= 5 {
                num = (num_f64 +  1_f64 / 10_f64.powf(num_format_right_count as f64)).to_string();
            }
            if let Some(pos) = num_format.find('.') {
                num_format_left_count = pos;
                num_format_right_count = num_format.len() - pos - 1;
            }else {
                num_format_left_count = num_format.len();
                num_format_right_count = 0;
            }
            // 得到整数部分和小数部分的位数
            if let Some(pos) = num.find('.') {
                num_left_count = pos;
                num_right_count = num.len() - pos - 1;
            }else {
                num_left_count = num.len();
                num_right_count = 0;
            }
        }
        let mut out_str_num = String::new();
        // 处理负号
        if is_f {
            out_str_num.push('-');
        }
        // 处理整数部分
        if num_left_count  < num_format_left_count {
            let num0 = num_format_left_count - num_left_count;
            for _i in 0..num0{
                out_str_num.push('0');
            }
        }
        out_str_num.push_str(&num[0..num_left_count]);
        // 处理小数点
        if num_format_right_count != 0 {
            out_str_num.push('.');
        }
        // 处理小数部分
        if num_right_count <= num_format_right_count {
            let num0 = num_format_right_count - num_right_count;
            if num_right_count != 0 {
                out_str_num.push_str(&num[num_left_count + 1..]);
            }
            for _i in 0..num0{
                out_str_num.push('0');
            }
        } else {
            out_str_num.push_str(&num[num_left_count + 1..num_left_count + num_format_right_count + 1]);
        }
        return Ok(Some(out_str_num));
    });
    add_fun(vec!["排序"],|self_t,params|{
        let arr_str = self_t.get_param(params, 0)?;
        let mut arr = RedLang::parse_arr(&arr_str)?;

        if arr.len() == 0 {
            return Ok(Some(self_t.build_arr(vec![])));
        } 

        if params.len() > 1 {
            let func = params.get(1).ok_or("函数获取失败")?.to_string();
            for i in 0..arr.len() - 1 {
                for j in i + 1 ..arr.len()
                {
                    let ret_str = self_t.call_fun(&[func.clone(),arr[i].to_owned(),arr[j].to_owned()],true)?;
                    if ret_str != "真" {
                        (arr[i],arr[j]) = (arr[j],arr[i]);
                    }
                }
            }
        } else {
            for i in 0..arr.len() - 1 {
                for j in i + 1 ..arr.len()
                {
                    let f1 = arr[i].parse::<f64>()?;
                    let f2 = arr[j].parse::<f64>()?;
                    if f1 > f2 {
                        (arr[i],arr[j]) = (arr[j],arr[i]);
                    }
                }
            }
        }
        let ret = self_t.build_arr(arr);
        return Ok(Some(ret));
    });
    add_fun(vec!["翻转"],|self_t,params|{
        let to_rev = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&to_rev)?;
        let ret;
        if tp == "数组" {
            let mut arr = RedLang::parse_arr(&to_rev)?;
            arr.reverse();
            ret = self_t.build_arr(arr);
        } else if tp == "文本" {
            let mut s = to_rev.chars().collect::<Vec<char>>();
            s.reverse();
            ret = String::from_iter(s);
        } else if tp == "字节集" {
            let mut bin = RedLang::parse_bin(&mut self_t.bin_pool,&to_rev)?;
            bin.reverse();
            ret = self_t.build_bin(bin);
        } else {
            return Err(RedLang::make_err(&format!("不支持的翻转类型:{tp}")));
        }
        return Ok(Some(ret));
    });
    add_fun(vec!["上传文件"],|self_t,params|{
        fn access(self_t:&mut RedLang,filename:&str,url:&str,file_data:&mut Vec<u8>) -> Result<String, Box<dyn std::error::Error>> {

            let proxy = self_t.get_coremap("代理")?;
            let mut timeout_str = self_t.get_coremap("访问超时")?;
            if timeout_str == "" {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = BTreeMap::new();
            let http_header_str = self_t.get_coremap("访问头")?;
            if http_header_str != "" {
                http_header = RedLang::parse_obj(&http_header_str)?;
                if !http_header.contains_key("User-Agent"){
                    http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
                }
            }else {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let bound = uuid::Uuid::new_v4().to_string();

            {
                let ct = "multipart/form-data; boundary=".to_owned() + &bound;
                http_header.insert("Content-Type".to_string(),ct); 
            }

            let mut data:Vec<u8> = vec![];
            data.append(&mut "--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            data.append(&mut "\r\nContent-Disposition: form-data; name=\"step\"\r\n\r\n0\r\n".as_bytes().to_owned());
            data.append(&mut "--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            let fname:String = url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
            data.append(&mut format!("\r\nContent-Disposition: form-data; name=\"file\";filename=\"{fname}\"\r\n\r\n").as_bytes().to_owned());
            data.append(file_data);
            data.append(&mut "\r\n--".as_bytes().to_owned());
            data.append(&mut bound.as_bytes().to_owned());
            data.append(&mut "--\r\n".as_bytes().to_owned());
            
            let timeout = timeout_str.parse::<u64>()?;
            let content = RT_PTR.block_on(async { 
                let ret = tokio::select! {
                    val_rst = http_post(url,data,&http_header,&proxy,"POST") => {
                        if let Ok(val) = val_rst {
                            val
                        } else {
                            cq_add_log_w(&format!("{:?}",val_rst.err().unwrap())).unwrap();
                            (vec![],"".to_owned())
                        }
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                        cq_add_log_w(&format!("POST访问:`{}`超时",url)).unwrap();
                        (vec![],"".to_owned())
                    }
                };
                return ret;
            });
            let json:serde_json::Value = serde_json::from_slice(&content.0)?;
            let url = json["data"].as_str().ok_or(format!("can't upload file:{json:?}", ))?;
            Ok(url.to_owned())
        }
        let bin_text = self_t.get_param(params, 0)?;
        let filename = self_t.get_param(params, 1)?;
        let mut bin = RedLang::parse_bin(&mut self_t.bin_pool,&bin_text)?;
        let url = "https://uhsea.com/Frontend/upload";
        match access(self_t,&filename,&url,&mut bin) {
            Ok(ret) => Ok(Some(ret)),
            Err(err) => {
                cq_add_log_w(&format!("{:?}",err)).unwrap();
                Ok(Some("".to_string()))
            },
        }
    });

    add_fun(vec!["运行PY"],|self_t,params|{
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint



def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode('utf-8')
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode('utf-8')).decode('utf-8')
    red_print(en)
"#;
        let code1 = self_t.get_param(params, 0)?;
        let mut input = self_t.get_param(params, 1)?;
        if input.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            input = get_raw_data(self_t,input)?;
        }
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;

        let tmp_dir = get_tmp_dir().map_err(|x|{format!("get_tmp_dir err:{:?}",x)})?;
        let python_id = crate::get_local_python_uid()?;

        // 末尾有分隔符
        let python_dir = format!("{}pymain_{}_{}{}",tmp_dir,self_t.pkg_name,python_id,std::path::MAIN_SEPARATOR.to_string());

        fs::create_dir_all(&python_dir)?;

        let python_env_is_create;
        if Path::new(&python_dir).join("redpymainok").is_file(){
            python_env_is_create = true;
        }else{
            python_env_is_create = false;
        }

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;


        if !python_env_is_create {
            
    
            #[cfg(windows)]
            let foo = std::process::Command::new(get_python_cmd_name().unwrap()).creation_flags(0x08000000).current_dir(python_dir.clone()).arg("-m").arg("venv").arg("pymain").status();
            
            #[cfg(not(windows))]
            let foo = std::process::Command::new(get_python_cmd_name().unwrap()).current_dir(python_dir.clone()).arg("-m").arg("venv").arg("pymain").status();
    
            if foo.is_err() {
                return Err(RedLang::make_err(&format!("python环境创建失败:{:?}",foo.err())));
            } else {
                let f = foo.unwrap();
                let is_ok = f.clone().success();
                if !is_ok {
                    return Err(format!("python环境创建异常:{:?}",f).into());
                } else {
                    let mut f = fs::File::create(Path::new(&python_dir).join("redpymainok"))?;
                    std::io::Write::write_all(&mut f, &[])?;
                }
            }
        }

        let curr_env = std::env::var("PATH").unwrap_or_default();

        let new_env = if cfg!(target_os = "windows") {
            format!("{}pymain/Scripts;{}",python_dir,curr_env)
        } else {
            format!("{}pymain/bin:{}",python_dir,curr_env)
        };
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = crate::G_RED_PY_DECODE.to_owned();

        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap()).creation_flags(0x08000000)
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .env("PATH", new_env)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;


        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap())
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .env("PATH", new_env)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;

        let s = p.stdin.take();
        if s.is_none() {
            p.kill()?;
        }else {
            s.unwrap().write_all(input_b64.as_bytes())?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if err != "" {
            cq_add_log_w(&format!("python中的警告或错误:{}",err)).unwrap();
        }
        let content_rst = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::general_purpose::PAD), &out);
        if content_rst.is_err() {
            return Err(RedLang::make_err(&out));
        }
        Ok(Some(String::from_utf8(content_rst.unwrap())?))
    });
        add_fun(vec!["运行本地PY"],|self_t,params|{
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode()
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode()).decode()
    red_print(en)
"#;
    
        let code1 = self_t.get_param(params, 0)?;
        let mut input = self_t.get_param(params, 1)?;
        if input.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            input = get_raw_data(self_t,input)?;
        }
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = crate::G_RED_PY_DECODE.to_owned();


        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap())
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;


        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap()).creation_flags(0x08000000)
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;

        let s = p.stdin.take();
        if s.is_none() {
            p.kill()?;
        }else {
            s.unwrap().write_all(input_b64.as_bytes())?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if err != "" {
            cq_add_log_w(&format!("python中的警告或错误:{}",err)).unwrap();
        }
        let content_rst = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::general_purpose::PAD), &out);
        if content_rst.is_err() {
            return Err(RedLang::make_err(&out));
        }
        Ok(Some(String::from_utf8(content_rst.unwrap())?))
    });
        add_fun(vec!["运行特殊PY"],|self_t,params|{
        let code = r#"
import os
import sysconfig
import sys

def myprint(*args,**kwargs):
    pass

red_print = sys.stdout.write

sys.stdout.write = myprint

def red_in():
    import base64
    inn = input()
    sw = base64.b64decode(inn).decode()
    return __red_py_decode(sw)

def red_out(sw):
    import base64
    en = base64.b64encode(__to_red_type(sw).encode()).decode()
    red_print(en)
"#;
    
        let mut to_add_path = self_t.get_param(params, 0)?;
        let code1 = self_t.get_param(params, 1)?;
        let mut input = self_t.get_param(params, 2)?;
        if input.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            input = get_raw_data(self_t,input)?;
        }
        let input_b64 = BASE64_CUSTOM_ENGINE.encode(input);
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let pip_in = std::process::Stdio::piped();

        let red_py_decode = crate::G_RED_PY_DECODE.to_owned();


        let curr_env = std::env::var("PATH").unwrap_or_default();
        let new_env;
        if to_add_path != "" {
            new_env = if cfg!(target_os = "windows") {
                if !to_add_path.ends_with(";") {
                    to_add_path.push(';');
                }
                format!("{}{}",to_add_path,curr_env)
            } else {
                if !to_add_path.ends_with(":") {
                    to_add_path.push(':');
                }
                format!("{}{}",to_add_path,curr_env)
            };
        } else {
            new_env = curr_env;
        }


        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        #[cfg(not(windows))]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap())
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .env("PATH", new_env)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;


        #[cfg(windows)]
        let mut p = std::process::Command::new(get_python_cmd_name().unwrap()).creation_flags(0x08000000)
        .stdin(pip_in)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(app_dir)
        .env("PATH", new_env)
        .arg("-c")
        .arg(format!("{red_py_decode}{code}{code1}"))
        .spawn()?;

        let s = p.stdin.take();
        if s.is_none() {
            p.kill()?;
        }else {
            s.unwrap().write_all(input_b64.as_bytes())?;
        }
        let output = p.wait_with_output()?;
        let out = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        if err != "" {
            cq_add_log_w(&format!("python中的警告或错误:{}",err)).unwrap();
        }
        let content_rst = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::general_purpose::PAD), &out);
        if content_rst.is_err() {
            return Err(RedLang::make_err(&out));
        }
        Ok(Some(String::from_utf8(content_rst.unwrap())?))
    });
    add_fun(vec!["默认字体"],|self_t,_params|{
        let mut ft_lk = G_DEFAULF_FONT.write().unwrap();
        if *ft_lk != "" {
            return Ok(Some(ft_lk.clone()));
        }
        let proxy = self_t.get_coremap("代理")?;
        let mut timeout_str = self_t.get_coremap("访问超时")?;
        if timeout_str == "" {
            timeout_str = "60000".to_owned();
        }
        let mut http_header = BTreeMap::new();
        let http_header_str = self_t.get_coremap("访问头")?;
        if http_header_str != "" {
            http_header = RedLang::parse_obj(&http_header_str)?;
            if !http_header.contains_key("User-Agent"){
                http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
        }else {
            http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
        }
        let url = "https://fonts.gstatic.com/s/notosanssc/v36/k3kCo84MPvpLmixcA63oeAL7Iqp5IZJF9bmaG9_FnYxNbPzS5HE.ttf";
        let timeout = timeout_str.parse::<u64>()?;
        let content = RT_PTR.block_on(async { 
            let ret = tokio::select! {
                val_rst = http_post(url,vec![],&http_header,&proxy,"GET") => {
                    if let Ok(val) = val_rst {
                        val
                    } else {
                        cq_add_log_w(&format!("{:?}",val_rst.err().unwrap())).unwrap();
                        (vec![],String::new())
                    }
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                    cq_add_log_w(&format!("GET访问:`{}`超时",url)).unwrap();
                    (vec![],String::new())
                }
            };
            return ret;
        });
        if content.0.len() == 10560380 {
            let ft = self_t.build_bin(content.0);
            (*ft_lk) = ft.clone();
            return Ok(Some(ft));
        }else {
            cq_add_log_w(&format!("默认字体下载失败！")).unwrap();
            return Ok(Some(self_t.build_bin(vec![])));
        }
    });
    add_fun(vec!["快速运行PY"],|self_t,params|{
        let code = self_t.get_param(params, 0)?;
        let mut input = self_t.get_param(params, 1)?;
        if input.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            input = get_raw_data(self_t,input)?;
        }
        let ret = call_py_block(&code,&input);
        Ok(Some(ret))
    });
    add_fun(vec!["区间选择"],|self_t,params|{
        let obj_text = self_t.get_param(params, 0)?;
        let select_num = self_t.get_param(params, 1)?.parse::<f64>()? + 0.0000001f64;
        let obj = RedLang::parse_obj(&obj_text)?;
        for (k,v) in &obj {
            let k_t = k.split('~').collect::<Vec<&str>>();
            let num1_str = k_t.get(0).ok_or("num1 not exist")?;
            let num2_str = k_t.get(1).ok_or("num2 not a exist")?;
            let mut ok1 = false;
            let mut ok2 = false;
            if *num1_str == "" || select_num > num1_str.parse::<f64>()? {
                ok1 = true;
            }
            if *num2_str == "" || select_num < num2_str.parse::<f64>()? {
                ok2 = true;
            }
            if ok1 && ok2 {
                return Ok(Some(v.to_owned()));
            }
        }
        Ok(Some("".to_string()))
    });

    add_fun(vec!["内存使用"],|_self_t,_params|{
        let s = sysinfo::System::new_all();
        if let Some(process) = sysinfo::System::process(&s, sysinfo::Pid::from(std::process::id() as usize)) {
            let num = sysinfo::Process::memory(process) as f32 / (1024 * 1024) as f32;
            return Ok(Some(num.to_string()))
        }
        return Ok(Some("".to_string()));
    });

    add_fun(vec!["正则替换"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let re = self_t.get_param(params, 1)?;
        let out_text = self_t.get_param(params, 2)?;
        let re_obj = fancy_regex::Regex::new(&re)?;
        let out = re_obj.replace_all(&text, out_text).to_string();
        Ok(Some(out))
    });

    add_fun(vec!["CPU使用"],|_self_t,_params|{
        let mut s = sysinfo::System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        s.refresh_processes_specifics(ProcessesToUpdate::All,true,ProcessRefreshKind::everything());
        let pid = sysinfo::Pid::from(std::process::id() as usize);
        let process = sysinfo::System::process(&s, pid).unwrap();
        return Ok(Some((sysinfo::Process::cpu_usage(process) /  sysinfo::System::cpus(&s).len() as f32).to_string()));
    });

    add_fun(vec!["系统信息"],|self_t,_params|{
        let mut s = sysinfo::System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        s.refresh_all();
        let mut mp = BTreeMap::new();


        let mut cpulist = vec![];
        for cpu in s.cpus() {
            let mut info = BTreeMap::new();
            info.insert("CpuUsage".to_owned(), cpu.cpu_usage().to_string());
            info.insert("Frequency".to_owned(), cpu.frequency().to_string());
            info.insert("Brand".to_owned(), cpu.brand().to_owned());
            info.insert("Name".to_owned(), cpu.name().to_owned());
            info.insert("VendorID".to_owned(), cpu.vendor_id().to_owned());
            let info_str = self_t.build_obj(info);
            cpulist.push(info_str);
        }
        mp.insert("CpuList".to_owned(),self_t.build_arr(cpulist.iter().map(|x|x.as_str()).collect()));

        mp.insert("SystemName".to_owned(), sysinfo::System::long_os_version().unwrap_or_default());
        mp.insert("SystemKernelVersion".to_owned(), sysinfo::System::kernel_version().unwrap_or_default());
        mp.insert("SystemOSVersion".to_owned(), sysinfo::System::os_version().unwrap_or_default());
        mp.insert("SystemHostName".to_owned(), sysinfo::System::host_name().unwrap_or_default());

        mp.insert("TotalMemory".to_owned(), s.total_memory().to_string());
        mp.insert("UsedMemory".to_owned(), s.used_memory().to_string());
        mp.insert("TotalSwap".to_owned(), s.total_swap().to_string());
        mp.insert("UsedSwap".to_owned(), s.used_swap().to_string());

        mp.insert("CPUCount".to_owned(), s.cpus().len().to_string());

        let mut disklist = vec![];
        let disks = sysinfo::Disks::new_with_refreshed_list();
        for disk in &disks {
            let mut info = BTreeMap::new();
            info.insert("MountPoint".to_owned(), disk.mount_point().to_string_lossy().to_string());
            info.insert("Name".to_owned(), disk.name().to_string_lossy().to_string());
            info.insert("FileSystem".to_owned(), disk.file_system().to_string_lossy().to_string());
            info.insert("Kind".to_owned(), disk.kind().to_string());
            info.insert("TotalSpace".to_owned(), disk.total_space().to_string());
            info.insert("AvailableSpace".to_owned(), disk.available_space().to_string());
            let info_str = self_t.build_obj(info);
            disklist.push(info_str);
        }
        mp.insert("DiskList".to_owned(),self_t.build_arr(disklist.iter().map(|x|x.as_str()).collect()));

        let networks = sysinfo::Networks::new_with_refreshed_list();
        let mut netlist = vec![];
        for (interface_name, data) in &networks {
            let mut info = BTreeMap::new();
            info.insert("TotalReceived".to_owned(), data.total_received().to_string());
            info.insert("TotalTransmitted".to_owned(), data.total_transmitted().to_string());
            info.insert("InterfaceName".to_owned(), interface_name.to_owned());
            let info_str = self_t.build_obj(info);
            netlist.push(info_str);
        }
        mp.insert("NetList".to_owned(),self_t.build_arr(netlist.iter().map(|x|x.as_str()).collect()));

        let ret = self_t.build_obj(mp);
        return Ok(Some(ret));
    });
    
    add_fun(vec!["渲染SVG","SVG渲染"],|self_t,params|{
        let svg_text = self_t.get_param(params, 0)?;
        let mut usvg_opt = usvg::Options::default();
        usvg_opt.fontdb_mut().load_system_fonts();
        let tree = resvg::usvg::Tree::from_str(&svg_text,&usvg_opt)?;
        // tree.fontdb().load_system_fonts();
        let width = (tree.size().width()+ 0.5)  as usize;
        let height = (tree.size().height() + 0.5) as usize;
        let transform = resvg::tiny_skia::Transform::identity();
        let sz = width * height * resvg::tiny_skia::BYTES_PER_PIXEL;
        let mut pix = vec![0; sz];
        let mut pixmapmut = resvg::tiny_skia::PixmapMut::from_bytes(pix.as_mut_slice(), width as u32, height as u32).ok_or("create pixmap err")?;
        resvg::render(&tree, transform,&mut pixmapmut);
        Ok(Some(self_t.build_bin(pixmapmut.to_owned().encode_png()?)))
    });

    add_fun(vec!["__MD转HTML"],|self_t,params|{
        let md_text = self_t.get_param(params, 0)?;
        let out_html = markdown::to_html(&md_text);
        Ok(Some(out_html))
    });

    add_fun(vec!["转繁体"],|self_t,params|{
        let msg = self_t.get_param(params, 0)?;
        let ret = crate::mytool::str_to_ft(msg.as_str());
        return Ok(Some(ret));
    });
    add_fun(vec!["转简体"],|self_t,params|{
        let msg = self_t.get_param(params, 0)?;
        let ret = crate::mytool::str_to_jt(msg.as_str());
        return Ok(Some(ret));
    });
    add_fun(vec!["github代理"],|_self_t,_params|{
        let config = crate::read_config().map_err(|_|"无法读取配置文件")?;
        let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
        let url = format!("http://localhost:{}/5350b16b-b5e2-425a-bba1-d33d92813ab4/",port);
        return Ok(Some(url));
    });
    add_fun(vec!["运行lua"],|self_t,params|{
        let code = self_t.get_param(params, 0)?;
        let lua = mlua::Lua::new();
        let s_t = RefCell::new(self_t);
        let lua_ret = lua.scope(|scope| {
            let func1 = scope.create_function_mut(|_, red_code:String| {
                let mut s_t = s_t.borrow_mut();
                let ret_str = s_t.parse(&red_code);
                if ret_str.is_err() {
                    return Err(mlua::Error::RuntimeError(ret_str.err().unwrap().to_string()));
                }
                Ok(ret_str.unwrap())
            })?;
            let func3 = scope.create_function_mut(|_, muti_params:MultiValue| {
                let params_len = muti_params.len();
                if params_len < 1 {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                }
                let cmd = muti_params.get(0).unwrap();
                let red_cmd = if cmd.is_string() {
                    cmd.as_str().unwrap()
                } else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let mut s_t = s_t.borrow_mut();
                let mut red_code = "【".to_string()+&red_cmd;
                for it in muti_params.iter().skip(1){
                    let it_t = if it.is_string() {
                        it.as_str().unwrap()
                    } else {
                        return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                    };
                    red_code += "@";
                    red_code += &s_t.parse_r_with_black(&it_t).unwrap();
                }
                red_code += "】";
                let ret_str = s_t.parse(&red_code);
                if ret_str.is_err() {
                    return Err(mlua::Error::RuntimeError(ret_str.err().unwrap().to_string()));
                }
                Ok(ret_str.unwrap())
            })?;
            lua.globals().set(
                "call_red",
                func1
            )?;
            lua.globals().set(
                "call_redcmd",
                func3
            )?;
            
            let lua_chunk = lua.load(&code);
            lua_chunk.eval::<String>()
        })?;
        return Ok(Some(lua_ret));
    });
    add_fun(vec!["运行超级lua"],|self_t,params|{
        let code = self_t.get_param(params, 0)?;
        let lua = unsafe { mlua::Lua::unsafe_new() };
        let s_t = RefCell::new(self_t);
        let lua_ret = lua.scope(|scope| {
            let func1 = scope.create_function_mut(|_, red_code:String| {
                let mut s_t = s_t.borrow_mut();
                let ret_str = s_t.parse(&red_code);
                if ret_str.is_err() {
                    return Err(mlua::Error::RuntimeError(ret_str.err().unwrap().to_string()));
                }
                Ok(ret_str.unwrap())
            })?;
            let func3 = scope.create_function_mut(|_, muti_params:MultiValue| {
                let params_len = muti_params.len();
                if params_len < 1 {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                }
                let cmd = muti_params.get(0).unwrap();
                let red_cmd = if cmd.is_string() {
                    cmd.as_str().unwrap()
                } else {
                    return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                };
                let mut s_t = s_t.borrow_mut();
                let mut red_code = "【".to_string()+&red_cmd;
                for it in muti_params.iter().skip(1){
                    let it_t = if it.is_string() {
                        it.as_str().unwrap()
                    } else {
                        return Err(mlua::Error::RuntimeError("参数错误".to_string()));
                    };
                    red_code += "@";
                    red_code += &s_t.parse_r_with_black(&it_t).unwrap();
                }
                red_code += "】";
                let ret_str = s_t.parse(&red_code);
                if ret_str.is_err() {
                    return Err(mlua::Error::RuntimeError(ret_str.err().unwrap().to_string()));
                }
                Ok(ret_str.unwrap())
            })?;
            lua.globals().set(
                "call_red",
                func1
            )?;
            lua.globals().set(
                "call_redcmd",
                func3
            )?;
            
            let lua_chunk = lua.load(&code);
            lua_chunk.eval::<String>()
        })?;
        return Ok(Some(lua_ret));
    });
    add_fun(vec!["TTS"],|self_t,params|{
        fn to_audio(text:&str,player:&str,proxy:&str) -> Result<Vec<u8>,Box<dyn std::error::Error>>{
            let voices;
            if proxy == "" {
                voices = msedge_tts::voice::get_voices_list()?;
            } else {
                voices = msedge_tts::voice::get_voices_list_proxy(proxy.parse()?,None,None)?;
            }
            for voice in &voices {
                if voice.name.contains(player) {
                    let config = msedge_tts::tts::SpeechConfig::from(voice);
                    if proxy == "" {
                        let mut tts = msedge_tts::tts::client::connect()?;
                        let audio: msedge_tts::tts::client::SynthesizedAudio = tts
                            .synthesize(text, &config)?;
                        let bt = audio.audio_bytes;
                        return Ok(bt);
                    } else {
                        let mut tts = msedge_tts::tts::client::connect_proxy(proxy.parse()?,None,None)?;
                        let audio: msedge_tts::tts::client::SynthesizedAudio = tts
                            .synthesize(text, &config)?;
                        let bt = audio.audio_bytes;
                        return Ok(bt);
                    }
                    
                }
            }
            return Err(format!("player `{player}` not found").into());
        }
        let proxy = self_t.get_coremap("代理")?;
        let text = self_t.get_param(params, 0)?;
        let mut player = self_t.get_param(params, 1)?;
        if player == "" {
            player = "YunyangNeural".to_owned();
        }
        let bin_mp3 = to_audio(&text,&player,&proxy)?;
        let ret = self_t.build_bin(bin_mp3);
        return Ok(Some(ret));
    });
    add_fun(vec!["执行频率"],|self_t,params|{

        self_t.set_coremap("剩余次数", &0.to_string())?;

        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);
        
        let name = self_t.get_param(params, 0)?;
        let time_str = self_t.get_param(params, 1)?;
        let times_str = self_t.get_param(params, 2)?;
        let time = time_str.parse::<i64>()?;
        let times = times_str.parse::<i64>()?;
        let key = format!("{name}245d51a6-5139-49ae-9d21-6d07065ee610{time}245d51a6-5139-49ae-9d21-6d07065ee610{times}");

        if times <= 0 {
            return Ok(Some(self_t.get_param(params, 3)?));
        }
        let times = times - 1;

        let can_run;
        {
            add_file_lock(&sql_file);

            let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
                del_file_lock(&sql_file);
            });
    
            let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
            let tm13 = tm.as_millis() as i64;
            
            let conn = rusqlite::Connection::open(sql_file)?;
            // 先创建表
            conn.execute("CREATE TABLE IF NOT EXISTS RUN_FREQ_TABLE (KEY TEXT PRIMARY KEY,VALUE TEXT);", [])?;
            // 再查询,VALUE格式:开始时间-剩余次数
            let ret_rst:Result<String,rusqlite::Error> = conn.query_row("SELECT VALUE FROM RUN_FREQ_TABLE WHERE KEY = ?", [key.to_owned()], |row| row.get(0));
            if let Ok(ret) =  ret_rst {
                // 查询到了,要判断是否过期
                let v = ret.split("-").collect::<Vec<&str>>();
                let start_time = v[0].parse::<i64>()?;
                let r_times = v[1].parse::<i64>()?;
                if tm13 - start_time > time {
                    // 过期了
                    can_run = true;
                    let value = format!("{tm13}-{times}");
                    conn.execute("REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)", [key,value])?;
                    self_t.set_coremap("剩余次数", &times.to_string())?;
                } else {
                    // 没过期
                    if r_times != 0 {
                        // 可以执行，剩余次数-1
                        can_run = true;
                        let r_times = r_times - 1;
                        let value = format!("{start_time}-{r_times}");
                        conn.execute("REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)", [key,value])?;
                        self_t.set_coremap("剩余次数", &r_times.to_string())?;
                    } else {
                        // 不能执行
                        self_t.set_coremap("剩余次数", &0.to_string())?;
                        can_run = false;
                    }
                }
            }else {
                // 没查询到,等效于没过期
                can_run = true;
                let value = format!("{tm13}-{times}");
                conn.execute("REPLACE INTO RUN_FREQ_TABLE (KEY,VALUE) VALUES (?,?)", [key,value])?;
                self_t.set_coremap("剩余次数", &times.to_string())?;
            }
        }
        if !can_run {
            // 不能执行
            return Ok(Some(self_t.get_param(params, 3)?));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["剩余次数"],|self_t,_params|{
        let count = self_t.get_coremap("剩余次数")?;
        if count == "" {
            return Ok(Some("0".to_string()));
        }
        Ok(Some(count))
    });
    add_fun(vec!["设置延迟触发"],|self_t,params|{
        // 【设置延迟触发@关键词@时间@传递数据】
        let keyword = self_t.get_param(params, 0)?;
        let mut tm = self_t.get_param(params, 1)?.parse::<i64>()?;
        let mut subdata = self_t.get_param(params, 2)?;
        if subdata.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
            subdata = get_raw_data(self_t,subdata)?;
        }
        let now_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as i64;
        tm += now_time;
        let mut lk = G_ONE_TIME_RUN.lock().unwrap();
        lk.push(OneTimeRunStruct {
             run_time: tm,
             pkg_name: self_t.pkg_name.to_owned(), 
             flag: keyword, 
             sub_data: subdata,
             data: (*self_t.exmap).borrow().clone() });
        Ok(Some("".to_string()))
    });
    fn zhuhe(s_len:usize,n:usize) -> Vec<Vec<usize>> {
        if s_len == 0 || n == 0 || s_len < n {
            return vec![];
        }
        let mut stack_vec:Vec<usize> = Vec::new();
        let mut out_vec:Vec<Vec<usize>> = Vec::new();
        stack_vec.push(0);
        loop {
            if stack_vec.len() == n {
                out_vec.push(stack_vec.clone());
                let last_num = stack_vec[stack_vec.len() - 1];
                if last_num == (s_len - 1) {
                    stack_vec.pop();
                    if stack_vec.len() == 0 {
                        break;
                    }
                    let last_num = stack_vec[stack_vec.len() - 1];
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                }else {
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                }
    
            } else {
                let last_num = stack_vec[stack_vec.len() - 1];
                if last_num == (s_len - 1) {
                    stack_vec.pop();
                    if stack_vec.len() == 0 {
                        break;
                    }
                    let last_num = stack_vec[stack_vec.len() - 1];
                    stack_vec.pop();
                    stack_vec.push(last_num + 1);
                } else {
                    stack_vec.push(last_num + 1)
                }
            }
        }
        out_vec
    }
    fn next_permutation(nums: &mut [usize]) -> bool {
        use std::cmp::Ordering;
        // or use feature(array_windows) on nightly
        let last_ascending = match nums.windows(2).rposition(|w| w[0] < w[1]) {
            Some(i) => i,
            None => {
                nums.reverse();
                return false;
            }
        };
    
        let swap_with = nums[last_ascending + 1..]
            .binary_search_by(|n| usize::cmp(&nums[last_ascending], n).then(Ordering::Less))
            .unwrap_err(); // cannot fail because the binary search will never succeed
        nums.swap(last_ascending, last_ascending + swap_with);
        nums[last_ascending + 1..].reverse();
        true
    }
    // 全排列
    fn quanpai(n:usize) -> Vec<Vec<usize>> {
        let mut out_vec:Vec<Vec<usize>> = Vec::new();
        let mut v = (0usize..n).collect::<Vec<usize>>();
        out_vec.push(v.clone());
        loop {
            let ret = next_permutation(&mut v);
            if ret == false {
                break;
            }
            out_vec.push(v.clone());
        }
        out_vec
    }
    add_fun(vec!["组合"],|self_t,params|{
        let s = self_t.get_param(params, 0)?;
        let s = RedLang::parse_arr(&s)?;
        let n = self_t.get_param(params, 1)?.parse::<usize>()?;
        let s_len = s.len();
        let out_vec = zhuhe(s_len,n);
        let mut out_vec_t:Vec<String> = Vec::new();
        for it in out_vec {
            let mut temp_vec = vec![];
            for it2 in it {
                temp_vec.push(s[it2]);
            }
            let red_vec = self_t.build_arr(temp_vec);
            out_vec_t.push(red_vec);
        }
        let out2 = out_vec_t.iter().map(|x|x.as_str()).collect::<Vec<&str>>();
        Ok(Some(self_t.build_arr(out2)))
    });
    add_fun(vec!["排列"],|self_t,params|{
        let s = self_t.get_param(params, 0)?;
        let s = RedLang::parse_arr(&s)?;
        let n = self_t.get_param(params, 1)?.parse::<usize>()?;
        let s_len = s.len();
        let out_zhuhe = zhuhe(s_len,n);
        let quanpai_vec = quanpai(n);
        let mut out_vec_t:Vec<String> = Vec::new();
        for it3 in out_zhuhe {
            for it4 in &quanpai_vec {
                let mut temp_vec = vec![];
                for it5 in it4 {
                    temp_vec.push(s[it3[*it5]]);
                }
                let red_vec = self_t.build_arr(temp_vec);
                out_vec_t.push(red_vec);
            }
        }
        let out2 = out_vec_t.iter().map(|x|x.as_str()).collect::<Vec<&str>>();
        Ok(Some(self_t.build_arr(out2)))
    });

    add_fun(vec!["HTML解析"],|self_t,params|{
        let mut html = self_t.get_param(params, 0)?;
        if self_t.get_type(&html)? == "字节集" {
            html = String::from_utf8(RedLang::parse_bin(&self_t.bin_pool, &html)?)?;
        }
        let css = self_t.get_param(params, 1)?;
        let document = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse(&css).map_err(|err| err.to_string())?;

        let mut retvec = vec![];

        for element in document.select(&selector) {
            let mut obj = BTreeMap::new();
            obj.insert("html".to_owned(), element.html());
            obj.insert("inner_html".to_owned(), element.inner_html());
            let text = element.text().collect::<Vec<&str>>();
            let mut text_str = String::new();
            for i in 0..text.len() {
                text_str.push_str(text[i]);
            }
            obj.insert("value".to_owned(),text_str);
            let atters_map = element.value().attrs();
            let mut atters_obj = BTreeMap::new();
            for (key, value) in atters_map {
                atters_obj.insert(key.to_string(), value.to_string());
            }
            let red_obj = self_t.build_obj(atters_obj);
            obj.insert("attrs".to_owned(), red_obj);
            retvec.push(self_t.build_obj(obj));
        }
        let out2 = retvec.iter().map(|x|x.as_str()).collect::<Vec<&str>>();
        Ok(Some(self_t.build_arr(out2)))
    });
    add_fun(vec!["计数"],|self_t,params|{
        let content = self_t.get_param(params, 0)?;
        let ele = self_t.get_param(params, 1)?;

        let tp = self_t.get_type(&content)?;
        let count = if tp == "文本" {
            if ele == "" {
                return Err(RedLang::make_err("计数的元素不能为空"));
            }
            let mut pos = 0;
            let mut text_count = 0;
            while let Some(found) = content[pos..].find(&ele) {
                text_count += 1;
                pos += found + ele.len();
            }
            text_count
        } else if tp == "字节集" {
            let bin_content = RedLang::parse_bin(&self_t.bin_pool, &content)?;
            let bin_ele = RedLang::parse_bin(&self_t.bin_pool, &ele)?;
            if bin_ele.len() == 0 {
                return Err(RedLang::make_err("计数的元素不能为空"));
            }
            let mut bin_count = 0;
            let mut pos = 0;
            while pos + bin_ele.len() <= bin_content.len() {
                if bin_content[pos..pos+bin_ele.len()] == bin_ele[..] {
                    bin_count += 1;
                    pos += bin_ele.len();
                } else {
                    pos += 1;
                }
            }
            bin_count
        } else if tp == "数组" {
            let arr_content = RedLang::parse_arr(&content)?;
            let arr_ele;
            if ele.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
                arr_ele = get_raw_data(self_t, ele)?;
            } else {
                arr_ele = ele;
            }
            arr_content.iter().filter(|x| {
                if x.contains("B96ad849c-8e7e-7886-7742-e4e896cc5b86") {
                    match get_raw_data(self_t, (*x).to_string()) {
                        Ok(raw_data) => arr_ele == raw_data,
                        Err(_) => false
                    }
                } else {
                    arr_ele == (*x).to_string()
                }
            }).count()
        } else {
            return Err(RedLang::make_err(&format!("计数不支持的类型:{}",tp)));
        };
        Ok(Some(count.to_string()))
    });
}




pub fn do_json_parse(json_val:&serde_json::Value,self_uid:&str) ->Result<String, Box<dyn std::error::Error>> {
    let err_str = "Json解析失败";
    if json_val.is_string() {
        return Ok(json_val.as_str().ok_or(err_str)?.to_string());
    }
    if json_val.is_object() {
        return Ok(do_json_obj(self_uid,&json_val)?);
    } 
    if json_val.is_array() {
        return Ok(do_json_arr(&self_uid,&json_val)?);
    }
    if json_val.is_null() {
        return Ok("".to_owned());
    }
    Err(None.ok_or(err_str)?)
}

fn do_json_string(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json字符串解析失败";
    return Ok(root.as_str().ok_or(err)?.to_string());
}

fn do_json_bool(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json布尔解析失败";
    let v_ret:String;
    if root.as_bool().ok_or(err)? {
        v_ret = "真".to_string();
    }else{
        v_ret = "假".to_string();
    }
    return Ok(v_ret);
}

fn do_json_number(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json数字解析失败";
    let v_ret:String;
    if root.is_u64() {
        v_ret = root.as_u64().ok_or(err)?.to_string();
    }else if root.is_i64() {
        v_ret = root.as_i64().ok_or(err)?.to_string();
    }else if root.is_f64() {
        v_ret = root.as_f64().ok_or(err)?.to_string();
    }else {
        return None.ok_or("不支持的Json类型")?;
    }
    return Ok(v_ret);
}

fn do_json_obj(self_uid:&str,root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json对象解析失败";
    let mut ret_str:BTreeMap<String,String> = BTreeMap::new();
    for it in root.as_object().ok_or(err)? {
        let k = it.0;
        let v = it.1;
        let v_ret:String;
        if v.is_string() {
            v_ret = do_json_string(v)?;
        } else if v.is_boolean() {
            v_ret = do_json_bool(v)?;
        }else if v.is_number() {
            v_ret = do_json_number(v)?
        }else if v.is_null() {
            v_ret = "".to_string();
        }else if v.is_object() {
            v_ret = do_json_obj(self_uid,v)?;
        }else if v.is_array() {
            v_ret = do_json_arr(self_uid,v)?;
        }else{
            return None.ok_or("不支持的Json类型")?;
        }
        ret_str.insert(k.to_string(), v_ret);
    }
    Ok(RedLang::build_obj_with_uid(self_uid, ret_str))
}

fn do_json_arr(self_uid: &str, root: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json数组解析失败";
    let mut ret_str:Vec<String> = vec![];
    for v in root.as_array().ok_or(err)? {
        let v_ret:String;
        if v.is_string() {
            v_ret = do_json_string(v)?;
        } else if v.is_boolean() {
            v_ret = do_json_bool(v)?;
        }else if v.is_number() {
            v_ret = do_json_number(v)?
        }else if v.is_null() {
            v_ret = "".to_string();
        }else if v.is_object() {
            v_ret = do_json_obj(self_uid,v)?;
        }else if v.is_array() {
            v_ret = do_json_arr(self_uid,v)?;
        }else{
            return None.ok_or("不支持的Json类型")?;
        }
        ret_str.push(v_ret);
    }
    Ok(RedLang::build_arr_with_uid(self_uid, ret_str.iter().map(AsRef::as_ref).collect()))
}

fn get_mid<'a>(s:&'a str,sub_begin:&str,sub_end:&str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
    let mut ret_vec:Vec<&str> = vec![];
    let mut s_pos = s;
    let err_str = "get_mid err";
    if sub_begin == "" || sub_end == "" {
        return Err(RedLang::make_err(err_str));
    }
    loop {
        let pos = s_pos.find(sub_begin);
        if let Some(pos_num) = pos {
            s_pos = s_pos.get((pos_num+sub_begin.len())..).ok_or(err_str)?;
            let pos_end = s_pos.find(sub_end);
            if let Some(pos_end_num) = pos_end {
                let val = s_pos.get(..pos_end_num).ok_or(err_str)?;
                ret_vec.push(val);
                s_pos = s_pos.get((pos_end_num+sub_end.len())..).ok_or(err_str)?;
            }else{
                break;
            }
        }else{
            break;
        }
    }
    return Ok(ret_vec);
}
