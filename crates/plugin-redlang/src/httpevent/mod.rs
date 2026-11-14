use crate::cqevent::do_script;
use http_body_util::BodyExt;
use hyper::http::{HeaderName, HeaderValue};
use std::{collections::BTreeMap, str::FromStr};

use crate::core::{rv_object, rv_text, RedLang, RedValue};
use hyper::body::Bytes;
use redlang::cqapi::cq_add_log_w;
use redlang::runtime;

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type BoxBody = http_body_util::combinators::BoxBody<Bytes, GenericError>;
type ScriptInfo<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a str);

fn get_script_info(
    script_json: &serde_json::Value,
) -> Result<ScriptInfo<'_>, Box<dyn std::error::Error>> {
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json
        .get("name")
        .ok_or("脚本中无name")?
        .as_str()
        .ok_or("脚本中name不是str")?;
    let node = script_json
        .get("content")
        .ok_or("script.json文件缺少content字段")?;
    let keyword = node
        .get("关键词")
        .ok_or("脚本中无关键词")?
        .as_str()
        .ok_or("脚本中关键词不是str")?;
    let cffs = node
        .get("触发方式")
        .ok_or("脚本中无触发方式")?
        .as_str()
        .ok_or("脚本中触发方式不是str")?;
    let code = node
        .get("code")
        .ok_or("脚本中无code")?
        .as_str()
        .ok_or("脚本中code不是str")?;
    let ppfs = node
        .get("匹配方式")
        .ok_or("脚本中无匹配方式")?
        .as_str()
        .ok_or("脚本中匹配方式不是str")?;

    Ok((keyword, cffs, code, ppfs, name, pkg_name))
}

pub fn get_params_from_uri(uri: &hyper::Uri) -> BTreeMap<String, String> {
    let mut ret_map = BTreeMap::new();
    let Some(query_str) = uri.query() else {
        return ret_map;
    };
    let query_vec = query_str.split("&");
    for it in query_vec {
        if it.is_empty() {
            continue;
        }
        if let Some(index) = it.find('=') {
            // 用 x=... 包装，避免值中的 '=' 被 form_urlencoded 拆成多对
            let k_raw = &it[..index];
            let v_raw = &it[index + 1..];
            let k_rst = url::form_urlencoded::parse(format!("x={}", k_raw).as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_else(|| k_raw.to_owned());
            let v_rst = url::form_urlencoded::parse(format!("x={}", v_raw).as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_else(|| v_raw.to_owned());
            ret_map.insert(k_rst, v_rst);
        } else {
            let k_rst = url::form_urlencoded::parse(format!("x={}", it).as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_else(|| it.to_owned());
            ret_map.insert(k_rst, "".to_owned());
        }
    }
    ret_map
}

pub fn do_http_event(
    req: hyper::Request<hyper::body::Incoming>,
    can_write: bool,
    can_read: bool,
) -> Result<hyper::Response<BoxBody>, Box<dyn std::error::Error>> {
    // 获取pkg_name和pkg_key
    let url_path = req.uri().path();
    // 短路径(如 /、/user、/user/)没有合法的包名和关键词，直接返回错误而不是 panic
    let true_url = url_path.get(5..).ok_or("无法得到网络触发路径")?;
    let splited_url = true_url.split('/').collect::<Vec<&str>>();
    let pkg_name = splited_url.get(1).ok_or("无法得到包名")?;
    let pkg_key = true_url
        .get(pkg_name.len() + 1..)
        .ok_or("无法得到网络触发关键词")?;
    let pkg_name_t: String = url::form_urlencoded::parse(pkg_name.as_bytes())
        .map(|(key, val)| [key, val].concat())
        .collect::<String>();
    let msg: String = url::form_urlencoded::parse(pkg_key.as_bytes())
        .map(|(key, val)| [key, val].concat())
        .collect::<String>();
    let script_json = crate::core::scripts().read_scripts();
    let method = req.method().to_string();
    let mut req_headers = BTreeMap::new();
    for it in req.headers() {
        // 单个非法（非 UTF-8）头跳过即可，不让整个网络触发请求失败
        if let Ok(header_value) = it.1.to_str() {
            req_headers.insert(it.0.as_str().to_owned(), header_value.to_owned());
        }
    }
    let uri = req.uri();
    let req_params = get_params_from_uri(uri);
    let (body_tx1, mut body_rx1) = tokio::sync::mpsc::channel(1);
    let (body_tx2, body_rx2) = tokio::sync::mpsc::channel(1);
    runtime().spawn(async move {
        let ret = body_rx1.recv().await;
        if ret.is_some() {
            let mut req = req;
            // 网络触发请求体无大小上限会被恶意大 body 打爆内存(OOM 即 abort 进程)。
            // 注意不能先 collect() 再截断（那已经全量进内存了），必须逐帧流式累计。
            const MAX_HTTP_EVENT_BODY: usize = 32 * 1024 * 1024;
            let mut body: Vec<u8> = Vec::new();
            let mut over_limit = false;
            loop {
                match req.body_mut().frame().await {
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data() {
                            if body.len() + data.len() > MAX_HTTP_EVENT_BODY {
                                over_limit = true;
                                break;
                            }
                            body.extend_from_slice(&data);
                        }
                    }
                    Some(Err(e)) => {
                        let _ = cq_add_log_w(&format!("获取访问体失败:{e}"));
                        body.clear();
                        break;
                    }
                    None => break,
                }
            }
            if over_limit {
                let _ = cq_add_log_w("网络触发请求体超出大小限制(32MB)");
                let _foo = body_tx2.send(vec![]).await;
            } else {
                let _foo = body_tx2.send(body).await;
            }
        }
    });
    let web_access;
    if can_write {
        web_access = "可写";
    } else if can_read {
        web_access = "只读";
    } else {
        web_access = "";
    }
    for i in 0..script_json
        .as_array()
        .ok_or("script.json文件不是数组格式")?
        .len()
    {
        let (keyword, cffs, code, ppfs, name, pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = RedLang::new();
        if cffs == "网络触发"
            && pkg_name == pkg_name_t
            && crate::cqevent::is_key_match(&mut rl, ppfs, keyword, &msg)?
        {
            rl.set_coremap("网络-访问方法", &method)?;
            let req_params_value = req_params
                .into_iter()
                .map(|(k, v)| (k, rv_text(v)))
                .collect();
            rl.set_coremap_value("网络-访问参数", rv_object(req_params_value));
            let req_headers_value = req_headers
                .into_iter()
                .map(|(k, v)| (k, rv_text(v)))
                .collect();
            rl.set_coremap_value("网络-访问头", rv_object(req_headers_value));
            rl.set_coremap("网络-权限", web_access)?;
            rl.req_tx = Some(body_tx1);
            rl.req_rx = Some(body_rx2);
            rl.pkg_name = pkg_name.to_owned();
            rl.script_name = name.to_owned();
            rl.can_wrong = true;
            let rl_ret = do_script(&mut rl, code, "normal", true)?;
            let mut http_header: BTreeMap<String, String> = BTreeMap::new();
            let mut res: hyper::Response<BoxBody>;
            match &*rl_ret {
                RedValue::Bin(bin) => {
                    http_header.insert(
                        "Content-Type".to_string(),
                        "application/octet-stream".to_string(),
                    );
                    res = hyper::Response::new(redlang::httpserver::full(bin.as_ref().clone()));
                }
                RedValue::Text(text) => {
                    http_header.insert(
                        "Content-Type".to_string(),
                        "text/html; charset=utf-8".to_string(),
                    );
                    res = hyper::Response::new(redlang::httpserver::full(text.as_ref().clone()));
                }
                other => {
                    return Err(RedLang::make_err(&format!(
                        "网络触发不能返回{}类型",
                        other.get_type_name()
                    )));
                }
            }
            let http_header_value = rl.get_coremap_value("网络-返回头");
            if let RedValue::Object(http_header_t) = &*http_header_value {
                for (k, v) in http_header_t {
                    http_header.insert(k.clone(), v.expect_text_value()?);
                }
                for (key, val) in &http_header {
                    res.headers_mut()
                        .append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
                }
            } else {
                for (key, val) in &http_header {
                    res.headers_mut()
                        .append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
                }
            }
            return Ok(res);
        }
    }
    let mut res: hyper::Response<BoxBody> =
        hyper::Response::new(redlang::httpserver::full("api not found"));
    res.headers_mut().insert(
        "Content-Type",
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    Ok(res)
}
