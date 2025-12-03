use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::SystemTime;
use crate::cqapi::{cq_add_log, cq_call_api, cq_get_app_directory1_async, get_history_log};
use crate::cqevent::do_script;
use crate::httpevent::do_http_event;
use crate::mytool::read_json_str;
use crate::onebot11s::event_to_onebot;
use crate::pluscenter::PlusCenterPlusBase;
use crate::{initevent, read_config, set_gobal_filter_code, set_gobal_init_code, G_AUTO_CLOSE, G_PKG_NAME, G_SCRIPT};
use crate::redlang::RedLang;
use crate::{cqapi::cq_add_log_w, RT_PTR};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::service::service_fn;
use hyper::Response;
use serde_json::json;
use tokio_util::bytes::Buf;
use bytes::Bytes;
use futures_util::TryStreamExt;
use crate::status::get_status;


type GenericError = Box<dyn std::error::Error + Send + Sync>;
type BoxBody = http_body_util::combinators::BoxBody<Bytes,GenericError>;
type Result<T> = std::result::Result<T, GenericError>;

lazy_static! {
    static ref G_LOG_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<String>>> = tokio::sync::RwLock::new(HashMap::new());
    pub static ref G_PY_ECHO_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<String>>> = tokio::sync::RwLock::new(HashMap::new());
    pub static ref G_PY_HANDER:tokio::sync::RwLock<Option<tokio::sync::mpsc::Sender<String>>> = tokio::sync::RwLock::new(None);
    static ref G_PYSER_OPEN:AtomicBool = AtomicBool::new(false);
    static ref G_ONEBOT_WS_MAP:tokio::sync::RwLock<HashMap<String,(tokio::sync::mpsc::Sender<String>,String,String)>> = tokio::sync::RwLock::new(HashMap::new());
}

pub fn add_ws_log(log_msg:String) {
    RT_PTR.clone().spawn(async move {
        let lk = G_LOG_MAP.read().await;
        for (_,tx) in &*lk {
            let _foo = tx.send(log_msg.clone()).await;
        }
    });
}

async fn deal_api(request: hyper::Request<hyper::body::Incoming>,can_write:bool,can_read:bool) -> Result<hyper::Response<BoxBody>> {
    let url_path = request.uri().path();
    if url_path == "/get_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::read_code_cache() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path == "/backup_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::backup_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path == "/test_cron" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let def_str = String::new();
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let key = params.get("key").unwrap_or(&def_str);
        let schedule_rst = <cron::Schedule as std::str::FromStr>::from_str(&key);
        let ret: serde_json::Value;
        let mut timestamp_vec = vec![];
        if let Ok(schedule) = schedule_rst {
            let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;
            let datetime_rst = chrono::TimeZone::timestamp_opt(&chrono::prelude::Local, now_time, 0);
            
            if let chrono::LocalResult::Single(data) = datetime_rst {
                for datetime in schedule.after(&data).take(10) {
                    timestamp_vec.push(datetime.to_rfc3339());
                }
            }
            ret = json!({
                "retcode":0,
                "data":timestamp_vec
            });
        }else{
            ret = json!({
                "retcode":-1,
                "data":timestamp_vec
            });
        }
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    }
    else if url_path == "/get_pluscenter_list" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let info = crate::pluscenter::get_plus_list().await?;
        let ret = serde_json::json!({
            "retcode":0,
            "data":info
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    }
    else if url_path == "/get_pluscenter_info" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let def_str = String::new();
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let repo = params.get("repo").unwrap_or(&def_str);
        let branch = params.get("branch").unwrap_or(&def_str);
        let info = crate::pluscenter::get_plus_info(&PlusCenterPlusBase{
            repo:repo.to_owned(),
            branch:branch.to_owned()
        }).await?;
        let ret = serde_json::json!({
            "retcode":0,
            "data":info
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } 
    else if url_path == "/install_plus" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let def_str = String::new();
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let repo = params.get("repo").unwrap_or(&def_str);
        let name = params.get("name").unwrap_or(&def_str);
        let version = params.get("version").unwrap_or(&def_str);
        let info_rst = crate::pluscenter::install_plus(repo,name,version).await;
        let ret;
        if info_rst.is_err() {
            ret = serde_json::json!({
                "retcode":-1,
                "data":info_rst.err().unwrap().to_string()
            });
        }else{
            ret = serde_json::json!({
                "retcode":0,
                "data":{}
            });
        }
        
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    }
    else if url_path.starts_with("/5350b16b-b5e2-425a-bba1-d33d92813ab4/") { // github代理
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let sp_ret = url_path.split("5350b16b-b5e2-425a-bba1-d33d92813ab4").collect::<Vec<&str>>();
        let github_url = sp_ret.get(1).ok_or("url error")?;
        let git_proxy = crate::pluscenter::get_proxy().await?;
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().build().unwrap();
        let uri = <reqwest::Url as std::str::FromStr>::from_str(&(git_proxy.to_owned() + github_url))?;
        let req = client.get(uri).build().unwrap();
        if let Ok(ret) = client.execute(req).await {
            if ret.status() == reqwest::StatusCode::OK {
                let ct = ret.headers().get("Content-Type").ok_or("content type error")?.to_owned();
                let ret2 = ret.bytes().await?.to_vec();
                let mut res = hyper::Response::new(crate::httpserver::full(ret2));
                res.headers_mut().insert("Content-Type", ct);
                return Ok(res);
            }
        }
        let res = hyper::Response::new(full("access github error"));
        return Ok(res);
    }
    else if url_path == "/read_one_pkg" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name;
        if let Some(name) = params.get("pkg_name") {
            pkg_name = name.to_owned();
        }else {
            pkg_name = "".to_owned();
        }
        match crate::read_one_pkg(&pkg_name) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path == "/get_all_pkg_name" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::get_all_pkg_name_by_cache() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }else if url_path == "/get_config" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::read_config() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }else if url_path == "/set_ws_urls" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        match crate::set_ws_urls(js){
            Ok(_) => {
                let ret = json!({
                    "retcode":0,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(_) => {
                let ret = json!({
                    "retcode":-1,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
        }
    } else if url_path == "/set_gobal_filter_code" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if code.is_string() {
                
                let rst = set_gobal_filter_code(code.as_str().unwrap());
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                    }
                }     
            }else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/set_gobal_init_code" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if code.is_string() {
                
                let rst = set_gobal_init_code(code.as_str().unwrap());
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                    }
                }     
            }else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    } 
    else if url_path == "/get_gobal_filter_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::get_gobal_filter_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path == "/get_gobal_init_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::get_gobal_init_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path == "/set_code" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::save_code(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
            else {
                cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/save_one_pkg" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::save_one_pkg(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
            else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/rename_one_pkg" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let old_pkg_name = read_json_str(&js, "old_pkg_name");
        let new_pkg_name = read_json_str(&js, "new_pkg_name");
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::rename_one_pkg(&old_pkg_name,&new_pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
            else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/del_one_pkg" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js:serde_json::Value = serde_json::from_reader(body)?;
        let pkg_name = read_json_str(&js, "pkg_name");
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::del_one_pkg(&pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
            else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/run_code" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root:serde_json::Value = serde_json::from_reader(body)?;

        let bot_id = read_json_str(&root, "bot_id");
        let platform = read_json_str(&root, "platform");
        let user_id = read_json_str(&root, "user_id");
        let group_id = read_json_str(&root, "group_id");
        let code = read_json_str(&root, "content");
        let mut pkg_name = read_json_str(&root, "pkg_name");
        if pkg_name == "默认包" {
            pkg_name = "".to_owned();
        }
        let groups_id = read_json_str(&root, "groups_id");
        thread::spawn(move ||{
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id).unwrap();
            rl.set_exmap("群ID", &group_id).unwrap();
            rl.set_exmap("群组ID", &groups_id).unwrap();
            rl.set_exmap("发送者ID", &user_id).unwrap();
            rl.set_exmap("机器人平台", &platform).unwrap();
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            if let Err(err) = do_script(&mut rl, &code,"normal",false) {
                cq_add_log_w(&format!("{}",err)).unwrap();
            }
        });
        let mut res = hyper::Response::new(full(serde_json::json!({
            "retcode":0,
        }).to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/run_code_and_ret" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root:serde_json::Value = serde_json::from_reader(body)?;

        let bot_id = read_json_str(&root, "bot_id");
        let platform = read_json_str(&root, "platform");
        let user_id = read_json_str(&root, "user_id");
        let group_id = read_json_str(&root, "group_id");
        let code = read_json_str(&root, "content");
        let mut pkg_name = read_json_str(&root, "pkg_name");
        if pkg_name == "默认包" {
            pkg_name = "".to_owned();
        }
        let groups_id = read_json_str(&root, "groups_id");

        
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move ||{
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id).unwrap();
            rl.set_exmap("群ID", &group_id).unwrap();
            rl.set_exmap("群组ID", &groups_id).unwrap();
            rl.set_exmap("发送者ID", &user_id).unwrap();
            rl.set_exmap("机器人平台", &platform).unwrap();
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            let ret_rst = rl.parse(&code);
            let res;
            let ret;
            if let Ok(ret_str) = ret_rst {
                ret = ret_str;
                res = serde_json::json!({
                    "retcode":0,
                    "data":ret
                }).to_string();
            } else {
                ret = format!("{}",ret_rst.err().unwrap().to_string());
                res = serde_json::json!({
                    "retcode":-1,
                    "data":ret
                }).to_string();
            }
            let rst = tx.send(res);
            if rst.is_err() {
                cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
            }
        });
        let ret_str = rx.await?;
        let mut res = hyper::Response::new(full(ret_str));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)    
    }
    else if url_path == "/close" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        cq_add_log_w("收到退出指令，正在退出").unwrap();
        crate::wait_for_quit();
    }else if url_path == "/get_version" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let ret = json!({
            "retcode":0,
            "data":crate::get_version()
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    }else if url_path == "/login" {
        let body_len = request.headers().get("content-length").ok_or("/login 中没有content-length")?.to_str()?.parse::<usize>()?;
        let mut res = hyper::Response::new(full(vec![]));
        if body_len < 256 {
            let mut body_reader = request.collect().await?.aggregate().reader();
            let mut body = Vec::new();
            body_reader.read_to_end(&mut body)?;
            //let body = hyper::body::to_bytes(request.into_body()).await?;
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            let pass_cookie = format!("{};Max-Age=31536000",String::from_utf8(body.to_vec())?);
            res.headers_mut().append(hyper::header::SET_COOKIE, HeaderValue::from_str(&pass_cookie)?);
        }
        res.headers_mut().insert("Location", HeaderValue::from_static("/index.html"));
        Ok(res)
    }
    else if url_path == "/get_send_recv_status" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match get_status() {
            Ok(status) => {
                let ret = json!({
                    "retcode": 0,
                    "data": status
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }
    else if url_path.starts_with("/user") {
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let ret = do_http_event(request,can_write,can_read);
            if ret.is_ok() {
                let rst = tx.send(ret.unwrap());
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }else {
                let err_str = format!("Error:{:?}",ret);
                let mut res = hyper::Response::new(full(err_str));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
                let rst = tx.send(res);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}",rst.err())).unwrap();
                }
            }
        }).await?;
        let ret = rx.await?;
        // let t = http_body_util::combinators::BoxBody::new(ret);
        // let k = ret.map_err(|never| match never {});
        Ok(ret)
    } else if url_path == "/upload_pkg" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        
        let content_len = request.headers().get("content-length").ok_or("/upload_pkg 中没有content-length")?;
        let content_len = content_len.to_str()?.parse::<usize>()?;

        // 如果 content_len > 10MB，就拒绝上传
        if content_len > 11 * 1024 * 1024 {
            // 读取前1KB数据（避免完全预读）
            let mut body_stream = request.into_body();
            let mut received = 0;
            while received < 1024 {
                if let Some(frame_result) = body_stream.frame().await {
                    let frame = frame_result?;
                    if let Some(data) = frame.data_ref() {
                        received += data.len();
                    }
                }
            }
            cq_add_log_w(format!("文件太大,请上传小于10MB的文件").as_str()).unwrap();
            let res = hyper::Response::new(full("文件太大,请上传小于10MB的文件"));
            return Ok(res);
        }

        // 读取body
        let mut body_stream = request.into_body();
        let mut body = Vec::with_capacity(content_len);
        while let Some(frame_result) = body_stream.frame().await {
            let frame = frame_result?;
            if let Some(data) = frame.data_ref() {
                body.extend_from_slice(data);

                if body.len() > content_len {
                    cq_add_log_w("Client sent more data than content-length header specified.").unwrap();
                    let res = hyper::Response::new(full("Bad request: body size exceeds content-length"));
                    return Ok(res);
                }
            }
        }
        if body.len() != content_len {
            let res = hyper::Response::new(full("Bad request: body size does not match content-length"));
            return Ok(res);
        }

        // 将body解析为json,得到filename,file_size,file_content
        let json: serde_json::Value = tokio::task::spawn_blocking(move || -> std::result::Result<serde_json::Value, serde_json::Error> {
            serde_json::from_slice(&body)
        }).await??;
        let filename = read_json_str(&json, "filename");

        // 文件名可能是1.a.b.z.d.red.7z，我们应该取.red.7z前面的部分
        let name = filename.strip_suffix(".red.7z")
            // 如果 strip_suffix 返回 None，ok_or 会将其转换为 Err
            .ok_or("文件名格式错误：必须以 .red.7z 结尾")?
            // 将 &str 转换为拥有的 String
            .to_owned();


        let file_size = read_json_str(&json, "file_size");
        let file_content = read_json_str(&json, "file_content");

        // 将file_content解码
        // 将base64解码操作移到spawn_blocking中执行
        let file_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                &base64::alphabet::STANDARD,
                base64::engine::general_purpose::PAD), file_content)
                .map_err(|e| e.into())
        }).await??;

        // 验证file_content与file_size是否一致
        if file_content.len() != file_size.parse::<usize>()? {
            let res = hyper::Response::new(full("文件大小与文件内容不符"));
            return Ok(res);
        }

        // 如果文件目录不存在，就创建
        let tmp_dir = crate::cqapi::get_tmp_dir_async().await?;
        let file_dir = PathBuf::from(&tmp_dir).join("pkg");
        
        if !file_dir.exists() {
            tokio::fs::create_dir_all(&file_dir).await?;
        }
        // 生成一个随机文件名
        let uid = uuid::Uuid::new_v4().to_string();
        let tmp_file_name = format!("{}.red.7z", uid);
        let tmp_file_path = file_dir.join(&tmp_file_name);


        // 删除临时文件
        let _guard = scopeguard::guard(tmp_file_path.clone(), |path| {
            RT_PTR.spawn(async{
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        // 将文件写入文件
        tokio::fs::write(&tmp_file_path, &file_content).await?;


        // 解压文件
        let decompress_path = file_dir.join(&uid);

        // 删除临时目录
        let _guard = scopeguard::guard(decompress_path.clone(), |path| {
            RT_PTR.spawn(async{
                let _ = tokio::fs::remove_dir_all(path).await;
            });
        });

        // 将解压操作移到spawn_blocking中执行
        let tmp_file_path_clone = tmp_file_path.clone();
        let decompress_path_clone = decompress_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::decompress_file(&tmp_file_path_clone, &decompress_path_clone)?;
            Ok(())
        }).await??;

        // 判断其中有无 script.json 文件
        let script_json_path = decompress_path.join("script.json");
        if !script_json_path.exists() {
            let res = hyper::Response::new(full("文件中不存在script.json"));
            return Ok(res);
        }

        
        let plus_dir_str = cq_get_app_directory1_async().await?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let real_pkg_dir = pkg_dir.join(&name);

        // 判断 real_pkg_dir目前是否存在，存在则报错
        if real_pkg_dir.exists() {
            let res = hyper::Response::new(full("插件已经存在，无法重复安装"));
            return Ok(res);
        }

        // 将文件移动到 real_pkg_dir
        let real_pkg_dir_clone = real_pkg_dir.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::decompress_file(tmp_file_path, real_pkg_dir_clone)?;
            Ok(())
        }).await??;
        

        let mut new_script = vec![];
        // 读取已有脚本
        {
            let wk = G_SCRIPT.read().unwrap();
            for it in wk.as_array().ok_or("read G_SCRIPT err")? {
                let it_name = read_json_str(it, "pkg_name");
                if it_name != name {
                    new_script.push(it.to_owned());
                }
            }
        }
        // 更新新增脚本
        let ret_scripts = real_pkg_dir.join("script.json");
        let scripts_str = tokio::fs::read_to_string(ret_scripts).await?;
        let mut scripts:serde_json::Value = serde_json::from_str(&scripts_str)?;
        for it in scripts.as_array_mut().ok_or("script.json not array")? {
            let obj_mut = it.as_object_mut().ok_or("script obj not object")?;
            obj_mut.insert("pkg_name".to_owned(), serde_json::json!(name));
            new_script.push(it.to_owned());
        }
        {
            let mut wk = G_SCRIPT.write().unwrap();
            (*wk) = serde_json::Value::Array(new_script);
        }
        // 添加新脚本名
        G_PKG_NAME.write().unwrap().insert(name.to_owned());

        // 执行初始化脚本，不用等待
        let name_t = filename.to_owned();
        tokio::task::spawn_blocking(move ||{
            if let Err(err) = initevent::do_init_event(Some(&name_t)){
                cq_add_log_w(&err.to_string()).unwrap();
            }
        });

        cq_add_log_w(&format!("上传文件大小为{}",content_len)).unwrap();

        let res = hyper::Response::new(full("ok"));
        return Ok(res);
    }  else if url_path == "/download_pkg" {
        if can_write == false {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name;
        if let Some(name) = params.get("pkg_name") {
            pkg_name = name.to_owned();
        }else {
            pkg_name = "".to_owned();
        }
        let pkg_dir;
        let plus_dir = cq_get_app_directory1_async().await?;
        if pkg_name == "" {
            pkg_dir = PathBuf::from_str(&plus_dir)?.join("default_pkg_dir");
        } else {
            pkg_dir = PathBuf::from_str(&plus_dir)?.join("pkg_dir").join(&pkg_name);
        }

        // 递归计算pkg_dir大小
        async fn get_size(path: &PathBuf) -> Result<usize> {
            let mut size = 0;
            let mut entries = tokio::fs::read_dir(path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    size += Box::pin(get_size(&path)).await?;
                } else {
                    size += entry.metadata().await?.len() as usize;
                }
            }
            Ok(size)
        }
        let size = get_size(&pkg_dir).await?;

        // 大于20MB，报错
        if size > 20 * 1024 * 1024 {
            let res = hyper::Response::new(full("文件大小超过10MB"));
            return Ok(res);
        }

        let tmp_dir = crate::cqapi::get_tmp_dir_async().await?;
        let file_dir = PathBuf::from(&tmp_dir).join("pkg");
        
        if !file_dir.exists() {
            tokio::fs::create_dir_all(&file_dir).await?;
        }

        let uid = uuid::Uuid::new_v4().to_string();
        let tmp_file_name = format!("{}.red.7z", uid);
        let tmp_file_path = file_dir.join(&tmp_file_name);

        let _guard = scopeguard::guard(tmp_file_path.clone(), |path| {
            RT_PTR.spawn(async move {
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        let tmp_file_path_clone = tmp_file_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::compress_to_path(&pkg_dir, tmp_file_path_clone)?;
            Ok(())
        }).await??;
        

        let file_bin = tokio::fs::read(tmp_file_path).await?;

        let mut res = hyper::Response::new(full(file_bin));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/octet-stream"));
        // 设置文件名为pkg_name.red.7z，如果pkg_name为空，则为default.red.7z
        if pkg_name == "" {
            res.headers_mut().insert("Content-Disposition", HeaderValue::from_str(&format!("attachment; filename=default.red.7z"))?);
        } else {
            res.headers_mut().insert("Content-Disposition", HeaderValue::from_str(&format!("attachment; filename={}.red.7z", pkg_name))?);
        }
        return Ok(res);
    }
    else{
        let res = hyper::Response::new(full("api not found"));
        Ok(res)
    }
}

async fn deal_file(request: hyper::Request<hyper::body::Incoming>) -> Result<hyper::Response<BoxBody>> {
    let url_path = request.uri().path();
    let app_dir = cq_get_app_directory1_async().await?;
    let path = PathBuf::from(&app_dir);
    let path = path.join("webui");
    let url_path_t = url_path.replace("/", &std::path::MAIN_SEPARATOR.to_string());
    let file_path = path.join(url_path_t.get(1..).unwrap());
    let file = tokio::fs::File::open(file_path).await?;
    let reader_stream = tokio_util::io::ReaderStream::new(file);
    let stream_body = http_body_util::StreamBody::new(reader_stream.map_ok(hyper::body::Frame::data));
    let boxed_body: http_body_util::combinators::BoxBody<tokio_util::bytes::Bytes, std::io::Error> = BodyExt::boxed(stream_body);
    let kk = boxed_body.map_err(|e|Box::new(e) as Box<dyn std::error::Error + Send + Sync>).boxed();
    let mut res = hyper::Response::new(kk);
    *res.status_mut() = hyper::StatusCode::OK;
    if url_path.ends_with(".html") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
    }else if url_path.ends_with(".js") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/javascript; charset=utf-8"));
    }else if url_path.ends_with(".css") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/css; charset=utf-8"));
    }else if url_path.ends_with(".png") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("image/png"));
    }else if url_path.ends_with(".txt") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/plain"));
    }else if url_path.ends_with(".md") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/markdown"));
    }else if url_path.ends_with(".woff2") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("font/woff2"));
    }else if url_path.ends_with(".json") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
    }else if url_path.ends_with(".moc") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/octet-stream"));
    }else if url_path.ends_with(".mtn") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/octet-stream"));
    }else if url_path.ends_with(".vue") {
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("max-age=300"));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/plain; charset=utf-8"));
    }
    else {
        *res.status_mut() = hyper::StatusCode::NOT_FOUND;
    }
    // cq_add_log_w(&format!("{:?}",res));
    Ok(res)
}
/// 处理ws协议
async fn serve_websocket(websocket: hyper_tungstenite::HyperWebsocket,mut rx:tokio::sync::mpsc::Receiver<String>) -> Result<()> {
    
    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, mut _read_half ) = futures_util::StreamExt::split(ws_stream);
    
    while let Some(msg) = rx.recv().await { // 当所有tx被释放时,tx.recv会返回None
        // log::info!("recive:{}",msg);
        write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.into())).await?;
    }
    Ok(())
}


/// 处理ws协议
async fn serve_py_websocket(websocket: hyper_tungstenite::HyperWebsocket,mut rx:tokio::sync::mpsc::Receiver<String>) -> Result<()> {
    
    cq_add_log("connect to pyserver").unwrap();

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half ) = futures_util::StreamExt::split(ws_stream);
    
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await { // 当所有tx被释放时,tx.recv会返回None
            // log::info!("recive:{}",msg);
            let rst = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.into())).await;
            if rst.is_err() {
                let mut lk = G_PY_HANDER.write().await;
                (*lk) = None;
                cq_add_log_w(&format!("serve send1 of python err:{:?}",rst.err().unwrap() )).unwrap();
                break;
            }
        }
        cq_add_log_w("serve send2 of python err").unwrap();
    });

    async fn deal_py_msg(mut read_half:futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>>) -> Result<()> {
        while let Some(msg_t) = read_half.next().await {
            {
                let lk = G_PY_HANDER.read().await;
                if lk.is_none() {
                    break;
                }
            }
            let msg_text = msg_t?.to_text()?.to_owned();

            if msg_text == "opened" {
                G_PYSER_OPEN.store(true,std::sync::atomic::Ordering::Relaxed);
                cq_add_log("python环境已经连接！").unwrap();
                continue;
            }

            let js:serde_json::Value = serde_json::from_str(&msg_text)?;
            let echo = read_json_str(&js, "echo");
            let lk = G_PY_ECHO_MAP.read().await;
            if lk.contains_key(&echo) {
                lk.get(&echo).unwrap().send(read_json_str(&js,"data")).await?;
            }
        }
        Ok(())
    }
    let ret = deal_py_msg(read_half).await;
    let mut lk = G_PY_HANDER.write().await;
    if ret.is_err() {
        cq_add_log_w(&format!("serve recv of python err:{:?}",ret.err().unwrap())).unwrap();
    }
    (*lk) = None;
    
    Ok(())
}

pub async fn send_onebot_event(root:serde_json::Value) {
    let rst = event_to_onebot(&root);
    if rst.is_err() {
        cq_add_log_w(&format!("convert to onebot event err:{:?}",rst.err().unwrap())).unwrap();
        return;
    } 
    let (root,platform,self_id) = rst.unwrap();
    let lk = G_ONEBOT_WS_MAP.read().await;
    for (_uid,sender) in &*lk {
        if platform == sender.1 && self_id == sender.2{
            let _ = sender.0.send(root.to_string()).await;
        }
    }
}

pub async fn send_onebot_api_ret(root:serde_json::Value,platform:String,self_id:String) {
    let lk = G_ONEBOT_WS_MAP.read().await;
    for (_uid,sender) in &*lk {
        if platform == sender.1 && self_id == sender.2{
            let _ = sender.0.send(root.to_string()).await;
        }
    }
}


/// 处理ws协议
async fn serve_onebot_websocket(websocket: hyper_tungstenite::HyperWebsocket,mut rx:tokio::sync::mpsc::Receiver<String>,platform:String,self_id:String) -> Result<()> {
    
    cq_add_log("connect to onebot").unwrap();

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half ) = futures_util::StreamExt::split(ws_stream);
    
    // 发送生命周期事件
    let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let connect_event = serde_json::json!({
        "time":tm,
        "self_id":self_id,
        "post_type":"meta_event",
        "meta_event_type":"lifecycle",
        "sub_type":"connect"
    }).to_string();

    write_half.send(hyper_tungstenite::tungstenite::Message::Text(connect_event.into())).await?;

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await { // 当所有tx被释放时,tx.recv会返回None
            // log::info!("recive:{}",msg);
            let rst = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.into())).await;
            if rst.is_err() {
                let mut lk = G_PY_HANDER.write().await;
                (*lk) = None;
                cq_add_log_w("serve send1 of onebot err").unwrap();
                break;
            }
        }
        cq_add_log_w("serve send2 of onebot err").unwrap();
    });

    async fn deal_msg(mut read_half:futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>>,platform: String,self_id:String) -> Result<()> {
        while let Some(msg_t) = read_half.next().await {
            // 转json
            let msg_text = msg_t?.to_text()?.to_owned();
            let req_json_rst = serde_json::from_str(&msg_text);
            if req_json_rst.is_err() {
                cq_add_log_w(&format!("req json err:{:?}",req_json_rst.err().unwrap())).unwrap();
                continue;
            }
            // 转red请求
            let mut req_json = req_json_rst.unwrap();
            let root_rst = crate::onebot11s::request_to_red(&mut req_json);
            if root_rst.is_err() {
                cq_add_log_w(&format!("can't convert to red req:{:?}",root_rst.err().unwrap())).unwrap();
                continue;
            }
            let (root,passive_id) = root_rst.unwrap();

            
            let self_id_t = self_id.clone();
            let platform_t = platform.clone();
            
            tokio::spawn(async move {

                let self_id = self_id_t.clone();
                let platform = platform_t.clone();

                let root_str = root.to_string();
                
                let echo_t = root.get("echo");
                let echo;
                if echo_t.is_some() {
                    echo = Some(echo_t.unwrap().to_owned());
                }else{
                    echo = None;
                }
                let ret = tokio::task::spawn_blocking(move ||{
                    let ret = cq_call_api(&platform.clone(), &self_id.clone(), &passive_id, &root_str,"");
                    let ret_red_json:serde_json::Value = serde_json::from_str(&ret).unwrap();
                    let ret_ob_json = crate::onebot11s::red_ret_to_ob(ret_red_json,echo);
                    ret_ob_json
                }).await;
                if ret.is_ok() {
                    send_onebot_api_ret(ret.unwrap(),platform_t,self_id_t).await;
                }else{
                    cq_add_log_w(&format!("err:{:?}",ret.err().unwrap())).unwrap();
                }
            });
        }
        Ok(())
    }
    let ret = deal_msg(read_half,platform,self_id).await;
    if ret.is_err() {
        cq_add_log_w(&format!("serve recv of onebot err:{:?}",ret.err().unwrap())).unwrap();
    }
    
    Ok(())
}


async fn http_auth(request: &hyper::Request<Incoming>) -> Result<i32> {
    
    let web_pass_raw = tokio::task::spawn_blocking(move || -> Result<String> {
            Ok(crate::read_web_password()?)
    }).await??;
    
    if web_pass_raw == "" {
        return Ok(2)
    }
    let headers = request.headers();
    let cookie_str = headers.get("Cookie").ok_or("can not found Cookie")?.to_str()?;
    {
        let web_pass:String = url::form_urlencoded::byte_serialize(web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}",web_pass)) {
            return Ok(2)
        }
    }
    let read_only_web_pass_raw = tokio::task::spawn_blocking(move || -> Result<String> {
            Ok(crate::read_readonly_web_password()?)
    }).await??;
    if read_only_web_pass_raw == "" {
        return Ok(1)
    }
    {
        let web_pass:String = url::form_urlencoded::byte_serialize(read_only_web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}",web_pass)) {
            return Ok(1)
        }
    }
    return Err("password invaild".into());
}


fn is_users_api(url_path:&str) -> bool {
    if url_path.starts_with("/user") {
        return true;
    }
    return false;
}

fn rout_to_login() -> hyper::Response<BoxBody> {
    let mut res = hyper::Response::new(full(vec![]));
    *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
    res.headers_mut().insert("Location", HeaderValue::from_static("/login.html"));
    return res;
}

pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    http_body_util::Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}
async fn connect_handle(request: hyper::Request<hyper::body::Incoming>,is_local: bool) -> Result<Response<BoxBody>> {
    
    let url_path = request.uri().path();
        // cq_add_log_w(&format!("url:{url_path}")).unwrap();
    // 登录页面不进行身份验证
    if url_path == "/login.html" {
        return deal_file(request).await; 
    }

    // 登录API不进行身份验证
    if url_path == "/login" {
        return deal_api(request,false,false).await;
    }

    // 心跳接口无须身份验证
    if url_path == "/heartbeat" {
        let mut lk = G_AUTO_CLOSE.lock().unwrap();
        (*lk) = false;
        
        let res = hyper::Response::new(full("ok"));
        return Ok(res);
    }
    
    // onebot 接口走专有身份验证
    if url_path.starts_with("/onebot") {
        if hyper_tungstenite::is_upgrade_request(&request){

            
            let mut access_token = String::new();
            let params = crate::httpevent::get_params_from_uri(request.uri());
            if params.contains_key("access_token"){
                let ac = params.get("access_token").unwrap();
                access_token = ac.to_owned();
            }else if request.headers().contains_key("Authorization") {
                let au = request.headers().get("Authorization").unwrap();
                access_token = au.to_str()?.to_owned();
            }

            let web_pass_raw = crate::read_web_password()?;

            // 需要鉴权
            if web_pass_raw != "" && !is_local {
                if access_token.contains(&web_pass_raw) {
                    // 鉴权通过
                }else {
                    // 鉴权不通过
                    return Ok(hyper::Response::new(full("broken http")));
                }
            }
            
            let parts = url_path.split("/").collect::<Vec<&str>>();
            let platform_opt = parts.get(2);
            let mut platform = String::new();
            if platform_opt.is_some() {
                platform = platform_opt.unwrap().to_string();
            }
            let self_id_opt = parts.get(3);
            let mut self_id = String::new();
            if self_id_opt.is_some() {
                self_id = self_id_opt.unwrap().to_string();
            }
            
            let (response, websocket) = hyper_tungstenite::upgrade(request, None)?;
            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let (tx, rx) =  tokio::sync::mpsc::channel::<String>(60);
                let ws_uid = uuid::Uuid::new_v4().to_string();
                {
                    let mut lk = G_ONEBOT_WS_MAP.write().await;
                    lk.insert(ws_uid.to_owned(), (tx,platform.clone(),self_id.clone()));
                }
                if let Err(e) = serve_onebot_websocket(websocket,rx,platform,self_id).await {
                    cq_add_log_w(&format!("onebots Error in websocket connection: {}", e)).unwrap();
                }
                {
                    let mut lk = G_ONEBOT_WS_MAP.write().await;
                    lk.remove(&ws_uid)
                }
            });
            
            let headers = response.headers();
            let mut rrr = 
                Response::builder()
                .body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            return Ok(rrr);
        }else{
            return Ok(hyper::Response::new(full("broken http")));
        }
    }

    
    // 获取身份信息
    let can_write;
    let can_read;
    if !is_local {
    // if !addr.ip().is_loopback() {
        let http_auth_rst = http_auth(&request).await;
        if http_auth_rst.is_err() {
            can_read = false;
            can_write = false;
        }else {
            can_read = true;
            let auth_code = http_auth_rst.unwrap();
            if auth_code == 2 {
                can_write = true;
            }else {
                can_write = false;
            }
        }
    }else{ // 本机地址不进行身份验证
        can_write = true;
        can_read = true;
    }

    // 认证失败,且不是用户API,跳转登录页面
    if !can_read {
        if !is_users_api(url_path) {
            return Ok(rout_to_login());
        }
    }
    // 升级ws协议
    if hyper_tungstenite::is_upgrade_request(&request) {
        if url_path == "/watch_log" {

            // 没有写权限不允许访问log
            if can_write == false {
                let res = hyper::Response::new(full("api not found"));
                return Ok(res);
            }

            // ws协议升级返回
            let (response, websocket) = hyper_tungstenite::upgrade(request, None)?;
            let (tx, rx) =  tokio::sync::mpsc::channel::<String>(60);
            
            let history_log = get_history_log();
            for it in history_log {
                tx.send(it).await?;
            }

            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let uid = uuid::Uuid::new_v4().to_string();
                {
                    let mut lk = G_LOG_MAP.write().await;
                    lk.insert(uid.clone(), tx);
                }
                if let Err(e) = serve_websocket(websocket,rx).await {
                    cq_add_log_w(&format!("watchlog Error in websocket connection: {}", e)).unwrap();
                }
                // 线程结束，删除对应的entry
                let mut lk = G_LOG_MAP.write().await;
                lk.remove(&uid);
            });
            let headers = response.headers();
            let mut rrr = 
                Response::builder()
                .body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            return Ok(rrr);
        } else if url_path == "/pyserver" {
            // 没有写权限不允许访问pyserver
            if can_write == false {
                let res = hyper::Response::new(full("api not found"));
                return Ok(res);
            }
            // ws协议升级返回
            let mut conf = tungstenite::protocol::WebSocketConfig::default();
            conf.max_frame_size = Some(1024 * 1024 * 100); // 100MB

            let (response, websocket) = hyper_tungstenite::upgrade(request, Some(conf))?;

            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let (tx, rx) =  tokio::sync::mpsc::channel::<String>(60);
                {
                    let mut k = G_PY_HANDER.write().await;
                    *k = Some(tx);
                }
                if let Err(e) = serve_py_websocket(websocket,rx).await {
                    cq_add_log_w(&format!("pyserver Error in websocket connection: {}", e)).unwrap();
                }
            });
            
            let headers = response.headers();
            let mut rrr = 
                Response::builder()
                .body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            return Ok(rrr);
        }
        else {
            return Ok(hyper::Response::new(full("broken http")));
        }
    }else {
        // 处理HTTP协议
        if url_path == "/" {

            // 没有读权限不允许访问主页
            if !can_read {
                return Ok(rout_to_login());
            }
            let mut res = hyper::Response::new(full(vec![]));
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            res.headers_mut().insert("Location", HeaderValue::from_static("/index.html"));
            return Ok(res);
        } else if url_path == "/favicon.ico" {
            // 没有读权限不允许访问图标
            if !can_read {
                return Ok(rout_to_login());
            }
            let app_dir = cq_get_app_directory1_async().await?;
            let path = PathBuf::from(&app_dir);
            let path = path.join("webui");
            let url_path_t = "favicon.ico".to_owned();
            let file_path = path.join(url_path_t);
            let file_buf = tokio::fs::read(&file_path).await?;
            let mut res = hyper::Response::new(full(file_buf));
            res.headers_mut().insert("Content-Type", HeaderValue::from_static("image/x-icon"));
            return Ok(res);
        } else if !url_path.contains(".") || url_path.starts_with("/user") || url_path.contains("5350b16b-b5e2-425a-bba1-d33d92813ab4") {
            return deal_api(request,can_write,can_read).await;
        } else {
            // 没有读权限不允许访问文件
            if !can_read {
                return Ok(rout_to_login());
            }
            return deal_file(request).await;
        }
    }

    
}

pub fn init_http_server() -> Result<()> {
    let config = read_config()?;
    let mut host = config.get("web_host").ok_or("无法获取web_host")?.as_str().ok_or("无法获取web_host")?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    if host == "localhost" {
        host = "127.0.0.1";
    }
    let web_uri = format!("{host}:{port}");
    cq_add_log(&format!("webui访问地址：http://{web_uri}")).unwrap();
    let addr1 = web_uri.parse::<std::net::SocketAddr>().unwrap();

    RT_PTR.spawn(async move {

        let socket;
        if addr1.is_ipv4() {
            socket = tokio::net::TcpSocket::new_v4().unwrap();
        } else {
            socket = tokio::net::TcpSocket::new_v6().unwrap();
        } 
        
        // win 下不需要设置这个
        #[cfg(not(windows))]
        socket.set_reuseaddr(true).unwrap();

        socket.bind(addr1).unwrap();
        let listener = socket.listen(20).unwrap();
        

        loop {
            let (stream, remote_address) = listener.accept().await.unwrap();

            let io = hyper_util::rt::TokioIo::new(stream);
            
            let service = service_fn(move |req| {
                connect_handle(req,remote_address.ip().is_loopback())
            });
            
            tokio::task::spawn(async move {
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await
                {
                    cq_add_log_w(&format!("Error serving connection: {:?}", err)).unwrap();
                }
            });
        } 
    });
    
    if let Some(not_open_browser) = config.get("not_open_browser") {
        if not_open_browser == false {
            crate::show_ctrl_web()?;
        }
    }else {
        crate::show_ctrl_web()?;
    }
    
    Ok(())
}