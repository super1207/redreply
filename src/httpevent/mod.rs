use std::{str::FromStr, collections::BTreeMap};
use hyper::http::{HeaderValue, HeaderName};
use crate::RT_PTR;
use crate::cqapi::cq_add_log_w;
use crate::{redlang::RedLang, read_code};


fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json.get("name").ok_or("脚本中无name")?.as_str().ok_or("脚本中name不是str")?;
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let keyword = node.get("关键词").ok_or("脚本中无关键词")?.as_str().ok_or("脚本中关键词不是str")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    let ppfs = node.get("匹配方式").ok_or("脚本中无匹配方式")?.as_str().ok_or("脚本中匹配方式不是str")?;
    
    return Ok((keyword,cffs,code,ppfs,name,pkg_name));
}

fn get_params_from_uri(uri:&hyper::Uri) -> BTreeMap<String,String> {
    let mut ret_map = BTreeMap::new();
    if uri.query().is_none() {
        return ret_map;
    }
    let query_str = uri.query().unwrap();
    let query_vec = query_str.split("&");
    for it in query_vec {
        if it == "" {
            continue;
        }
        let index_opt = it.find("=");
        if index_opt.is_some() {
            let k_rst = urlencoding::decode(it.get(0..index_opt.unwrap()).unwrap());
            let v_rst = urlencoding::decode(it.get(index_opt.unwrap() + 1..).unwrap());
            if k_rst.is_err() || v_rst.is_err() {
                continue;
            }
            ret_map.insert(k_rst.unwrap().to_string(), v_rst.unwrap().to_string());
        }
        else {
            let k_rst = urlencoding::decode(it);
            if k_rst.is_err() {
                continue;
            }
            ret_map.insert(k_rst.unwrap().to_string(),"".to_owned());
        }
    }
    ret_map
}

pub fn do_http_event(mut req:hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error>> {
     // 获取pkg_name和pkg_key
    let url_path = req.uri().path();
    let true_url = url_path.get(5..).unwrap();
    let splited_url = true_url.split('/').into_iter().collect::<Vec<&str>>();
    let pkg_name = splited_url.get(1).ok_or("无法得到包名")?;
    let pkg_key = true_url.get(pkg_name.len() + 1..).unwrap();
    let pkg_name_t = urlencoding::decode(&pkg_name)?.to_string();
    let msg = urlencoding::decode(&pkg_key)?.to_string();
    let script_json = read_code()?;
    let method = req.method().to_string();
    let mut req_headers = BTreeMap::new();
    for it in req.headers() {
        req_headers.insert(it.0.as_str().to_owned(), it.1.to_str()?.to_owned());
    }
    let uri = req.uri();
    let req_params = get_params_from_uri(uri);
    let (body_tx1,mut body_rx1) =  tokio::sync::mpsc::channel(1);
    let (body_tx2, body_rx2) =  tokio::sync::mpsc::channel(1);
    RT_PTR.spawn(async move {
        let ret = body_rx1.recv().await;
        if ret.is_some() {
            let bdy = req.body_mut();
            let bt_rst: Result<hyper::body::Bytes, hyper::Error> = hyper::body::to_bytes(bdy).await;
            if let Ok(bt) = bt_rst {
                let _foo = body_tx2.send(bt.to_vec()).await;
            }else {
                cq_add_log_w(&format!("获取访问体失败:{bt_rst:?}")).unwrap();
                let _foo = body_tx2.send(vec![]).await;
            }
        }
    });
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "网络触发" && pkg_name == pkg_name_t && crate::cqevent::is_key_match(&mut rl,&ppfs,keyword,&msg)? {
            rl.set_coremap("网络-访问方法", &method)?;
            rl.set_coremap("网络-访问参数", &rl.build_obj(req_params))?;
            rl.set_coremap("网络-访问头", &rl.build_obj(req_headers))?;
            rl.req_tx = Some(body_tx1);
            rl.req_rx = Some(body_rx2);
            rl.pkg_name = pkg_name.to_owned();
            rl.script_name = name.to_owned();
            let rl_ret = crate::cqevent::do_script(&mut rl,code)?;
            let mut http_header = BTreeMap::new();
            let mut res:hyper::Response<hyper::Body>;
            if rl.get_type(&rl_ret)? == "字节集" {
                http_header.insert("Content-Type", "application/octet-stream");
                res = hyper::Response::new(hyper::Body::from(RedLang::parse_bin(&rl_ret)?));
            } else {
                http_header.insert("Content-Type", "text/html; charset=utf-8");
                res = hyper::Response::new(hyper::Body::from(rl_ret));
            }
            let http_header_str = rl.get_coremap("网络-返回头")?;
            if http_header_str != "" {
                let http_header_t = RedLang::parse_obj(&http_header_str)?;
                for (k,v) in &http_header_t {
                    http_header.insert(k, v);
                }
                for (key,val) in &http_header {
                    res.headers_mut().append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
                }
            }
            return Ok(res);
        }
    }
    let mut res:hyper::Response<hyper::Body> = hyper::Response::new(hyper::Body::from("api not found"));
    res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
    Ok(res)
}