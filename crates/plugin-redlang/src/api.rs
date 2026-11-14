//! 脚本 HTTP API：由宿主 httpserver 迁移而来，路由已剥离 `/redlang/api` 前缀。

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::http::HeaderValue;
use serde_json::json;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use tokio_util::bytes::Buf;

use crate::core::RedLang;
use crate::cqevent::do_script;
use crate::httpevent::do_http_event;
use crate::initevent;
use crate::script_manager::{set_gobal_filter_code, set_gobal_init_code};
use crate::status::get_status;
use redlang::config_manager::{get_ban_pkgs, set_ban_pkg};
use redlang::cqapi::{cq_add_log_w, cq_get_app_directory1_async};
use redlang::lifecycle::wait_for_quit;
use redlang::mytool::read_json_str;
use redlang::resource_manager::get_version;
use redlang::runtime;

pub type GenericError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, GenericError>;
pub type Result<T> = std::result::Result<T, GenericError>;

pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    http_body_util::Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}
pub async fn deal_api(
    request: hyper::Request<hyper::body::Incoming>,
    can_write: bool,
    can_read: bool,
) -> Result<hyper::Response<BoxBody>> {
    let url_path = request.uri().path();

    if url_path == "/get_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match Ok::<_, GenericError>(crate::core::scripts().read_scripts()) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/backup_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::backup_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/test_cron" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let def_str = String::new();
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let key = params.get("key").unwrap_or(&def_str);
        let schedule_rst = <cron::Schedule as std::str::FromStr>::from_str(key);
        let ret: serde_json::Value;
        let mut timestamp_vec = vec![];
        if let Ok(schedule) = schedule_rst {
            let now_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;
            let datetime_rst =
                chrono::TimeZone::timestamp_opt(&chrono::prelude::Local, now_time, 0);

            if let chrono::LocalResult::Single(data) = datetime_rst {
                for datetime in schedule.after(&data).take(10) {
                    timestamp_vec.push(datetime.to_rfc3339());
                }
            }
            ret = json!({
                "retcode":0,
                "data":timestamp_vec
            });
        } else {
            ret = json!({
                "retcode":-1,
                "data":timestamp_vec
            });
        }
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path.starts_with("/5350b16b-b5e2-425a-bba1-d33d92813ab4/") {
        // github代理
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let sp_ret = url_path
            .split("5350b16b-b5e2-425a-bba1-d33d92813ab4")
            .collect::<Vec<&str>>();
        let github_url = sp_ret.get(1).ok_or("url error")?;
        let git_proxy = redlang::mytool::get_github_proxy().await?;
        // 不再禁用证书校验：代理地址是动态获取的外部服务，关闭校验等于
        // 允许中间人替换下载内容；同时加请求超时
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        let uri =
            <reqwest::Url as std::str::FromStr>::from_str(&(git_proxy.to_owned() + github_url))?;
        let req = client.get(uri).build()?;
        if let Ok(ret) = client.execute(req).await {
            if ret.status() == reqwest::StatusCode::OK {
                let ct = ret
                    .headers()
                    .get("Content-Type")
                    .ok_or("content type error")?
                    .to_owned();
                // 流式接收并限制总量，避免大文件把内存吃爆
                use futures_util::StreamExt;
                const MAX_PROXY_BODY: usize = 64 * 1024 * 1024;
                let mut buf: Vec<u8> = Vec::new();
                let mut intact = true;
                let mut stream = ret.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            if buf.len() + bytes.len() > MAX_PROXY_BODY {
                                intact = false;
                                break;
                            }
                            buf.extend_from_slice(&bytes);
                        }
                        Err(_) => {
                            intact = false;
                            break;
                        }
                    }
                }
                if intact {
                    let mut res = hyper::Response::new(full(buf));
                    res.headers_mut().insert("Content-Type", ct);
                    return Ok(res);
                }
            }
        }
        let res = hyper::Response::new(full("access github error"));
        Ok(res)
    } else if url_path == "/read_one_pkg" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name;
        if let Some(name) = params.get("pkg_name") {
            pkg_name = name.to_owned();
        } else {
            pkg_name = "".to_owned();
        }
        match crate::script_manager::read_one_pkg(&pkg_name) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_all_pkg_name" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match Ok::<_, GenericError>(crate::core::scripts().package_names()) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_ban_pkgs" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match get_ban_pkgs() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/set_ban_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let pkg_name = read_json_str(&js, "pkg_name");
        let banned = js.get("banned").and_then(|v| v.as_bool()).unwrap_or(false);
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = set_ban_pkg(&pkg_name, banned);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/set_gobal_filter_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if let Some(code) = code.as_str() {
                let rst = set_gobal_filter_code(code);
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                } else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                    }
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/set_gobal_init_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if let Some(code) = code.as_str() {
                let rst = set_gobal_init_code(code);
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                } else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                    }
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/get_gobal_filter_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::get_gobal_filter_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_gobal_init_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::get_gobal_init_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/set_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::save_code(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            } else {
                let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/save_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::save_one_pkg(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/rename_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let old_pkg_name = read_json_str(&js, "old_pkg_name");
        let new_pkg_name = read_json_str(&js, "new_pkg_name");
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::rename_one_pkg(&old_pkg_name, &new_pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/del_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let pkg_name = read_json_str(&js, "pkg_name");
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::del_one_pkg(&pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/run_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root: serde_json::Value = serde_json::from_reader(body)?;

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
        thread::spawn(move || {
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id);
            rl.set_exmap("群ID", &group_id);
            rl.set_exmap("群组ID", &groups_id);
            rl.set_exmap("发送者ID", &user_id);
            rl.set_exmap("机器人平台", &platform);
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            if let Err(err) = do_script(&mut rl, &code, "normal", false) {
                let _ = cq_add_log_w(&format!("{}", err));
            }
        });
        let mut res = hyper::Response::new(full(
            serde_json::json!({
                "retcode":0,
            })
            .to_string(),
        ));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/run_code_and_ret" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root: serde_json::Value = serde_json::from_reader(body)?;

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

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id);
            rl.set_exmap("群ID", &group_id);
            rl.set_exmap("群组ID", &groups_id);
            rl.set_exmap("发送者ID", &user_id);
            rl.set_exmap("机器人平台", &platform);
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            let ret_rst = rl.parse_to_string(&code);
            let (ret, retcode) = match ret_rst {
                Ok(ret_str) => (ret_str, 0),
                Err(err) => (format!("{err}"), -1),
            };
            let res = serde_json::json!({
                "retcode":retcode,
                "data":ret
            })
            .to_string();
            let rst = tx.send(res);
            if rst.is_err() {
                let _ = cq_add_log_w(&format!("Error:{:?}", rst.err()));
            }
        });
        let ret_str = rx.await?;
        let mut res = hyper::Response::new(full(ret_str));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/close" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let _ = cq_add_log_w("收到退出指令，正在退出");
        wait_for_quit();
    } else if url_path == "/get_version" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let ret = json!({
            "retcode":0,
            "data":get_version()
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/login" {
        let body_len = request
            .headers()
            .get("content-length")
            .ok_or("/login 中没有content-length")?
            .to_str()?
            .parse::<usize>()?;
        let mut res = hyper::Response::new(full(vec![]));
        if body_len < 256 {
            let mut body_reader = request.collect().await?.aggregate().reader();
            let mut body = Vec::new();
            body_reader.read_to_end(&mut body)?;
            //let body = hyper::body::to_bytes(request.into_body()).await?;
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            let body_str = String::from_utf8(body.to_vec())?;
            // 从表单体中取出 password 字段并解码（表单编码中 + 表示空格），
            // 再按认证侧 http_auth 相同的 byte_serialize 编码写入 Cookie：
            // 之前直接回写原始 body，含空格/符号/中文的密码永远无法通过比对
            if let Some((_, pass)) = url::form_urlencoded::parse(body_str.as_bytes())
                .into_iter()
                .find(|(k, _)| k == "password")
            {
                let encoded: String =
                    url::form_urlencoded::byte_serialize(pass.as_bytes()).collect();
                let pass_cookie = format!("password={encoded};Max-Age=31536000");
                res.headers_mut().append(
                    hyper::header::SET_COOKIE,
                    HeaderValue::from_str(&pass_cookie)?,
                );
            }
        }
        res.headers_mut()
            .insert("Location", HeaderValue::from_static("/index.html"));
        Ok(res)
    } else if url_path == "/get_send_recv_status" {
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
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path.starts_with("/user") {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || match do_http_event(request, can_write, can_read) {
            Ok(response) => {
                if let Err(err) = tx.send(response) {
                    let _ = cq_add_log_w(&format!("Error:{err:?}"));
                }
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                res.headers_mut().insert(
                    "Content-Type",
                    HeaderValue::from_static("text/html; charset=utf-8"),
                );
                if let Err(send_err) = tx.send(res) {
                    let _ = cq_add_log_w(&format!("Error:{send_err:?}"));
                }
            }
        })
        .await?;
        let ret = rx.await?;
        // let t = http_body_util::combinators::BoxBody::new(ret);
        // let k = ret.map_err(|never| match never {});
        Ok(ret)
    } else if url_path == "/upload_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }

        let content_len = request
            .headers()
            .get("content-length")
            .ok_or("/upload_pkg 中没有content-length")?;
        let content_len = content_len.to_str()?.parse::<usize>()?;

        // 如果 content_len > 11MB，就拒绝上传
        if content_len > 11 * 1024 * 1024 {
            // 排空剩余 body 以复用 keep-alive 连接；但设置排空总量上限，
            // 超过则直接放弃（hyper 会关闭该连接），防止恶意慢速大上传占用资源
            const MAX_DRAIN_BYTES: usize = 32 * 1024 * 1024;
            let mut body_stream = request.into_body();
            let mut drained = 0usize;
            while let Some(frame_result) = body_stream.frame().await {
                if drained > MAX_DRAIN_BYTES {
                    break;
                }
                match frame_result {
                    Ok(frame) => {
                        drained += frame.data_ref().map(|data| data.len()).unwrap_or(0);
                    }
                    Err(_) => break,
                }
            }
            let _ = cq_add_log_w("文件太大,请上传小于11MB的文件");
            let res = hyper::Response::new(full("文件太大,请上传小于11MB的文件"));
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
                    let _ =
                        cq_add_log_w("Client sent more data than content-length header specified.");
                    let res =
                        hyper::Response::new(full("Bad request: body size exceeds content-length"));
                    return Ok(res);
                }
            }
        }
        if body.len() != content_len {
            let res =
                hyper::Response::new(full("Bad request: body size does not match content-length"));
            return Ok(res);
        }

        // 将body解析为json,得到filename,file_size,file_content
        let json: serde_json::Value = tokio::task::spawn_blocking(
            move || -> std::result::Result<serde_json::Value, serde_json::Error> {
                serde_json::from_slice(&body)
            },
        )
        .await??;
        let filename = read_json_str(&json, "filename");

        // 文件名可能是1.a.b.z.d.red.7z，我们应该取.red.7z前面的部分
        let name = filename
            .strip_suffix(".red.7z")
            // 如果 strip_suffix 返回 None，ok_or 会将其转换为 Err
            .ok_or("文件名格式错误：必须以 .red.7z 结尾")?
            // 将 &str 转换为拥有的 String
            .to_owned();

        let file_size = read_json_str(&json, "file_size");
        let file_content = read_json_str(&json, "file_content");

        // 将file_content解码
        // 将base64解码操作移到spawn_blocking中执行
        let file_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            base64::Engine::decode(
                &base64::engine::GeneralPurpose::new(
                    &base64::alphabet::STANDARD,
                    base64::engine::general_purpose::PAD,
                ),
                file_content,
            )
            .map_err(|e| e.into())
        })
        .await??;

        // 验证file_content与file_size是否一致
        if file_content.len() != file_size.parse::<usize>()? {
            let res = hyper::Response::new(full("文件大小与文件内容不符"));
            return Ok(res);
        }

        // 如果文件目录不存在，就创建
        let tmp_dir = redlang::cqapi::get_tmp_dir_async().await?;
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
            runtime().spawn(async {
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        // 将文件写入文件
        tokio::fs::write(&tmp_file_path, &file_content).await?;

        // 解压文件
        let decompress_path = file_dir.join(&uid);

        // 删除临时目录
        let _guard = scopeguard::guard(decompress_path.clone(), |path| {
            runtime().spawn(async {
                let _ = tokio::fs::remove_dir_all(path).await;
            });
        });

        // 将解压操作移到spawn_blocking中执行
        let tmp_file_path_clone = tmp_file_path.clone();
        let decompress_path_clone = decompress_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::decompress_file(&tmp_file_path_clone, &decompress_path_clone)?;
            Ok(())
        })
        .await??;

        // 判断其中有无 script.json 文件
        let script_json_path = decompress_path.join("script.json");
        if !script_json_path.exists() {
            let res = hyper::Response::new(full("文件中不存在script.json"));
            return Ok(res);
        }

        let plus_dir_str = cq_get_app_directory1_async().await?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let real_pkg_dir = pkg_dir.join(&name);

        // 与保存/删除/改名共用同一把保存锁：上传对脚本缓存做读-改-写，
        // 不加锁会与并发保存互相丢数据。整个临界区放进阻塞线程执行，
        // 避免在异步上下文持有非 Send 的 std MutexGuard
        let real_pkg_dir_for_install = real_pkg_dir.clone();
        let name_for_install = name.to_owned();
        let installed = tokio::task::spawn_blocking(move || -> Result<bool> {
            let _save_guard = crate::script_manager::lock_save()?;
            if real_pkg_dir_for_install.exists() {
                return Ok(false);
            }
            sevenz_rust2::decompress_file(&tmp_file_path, &real_pkg_dir_for_install)?;
            let mut new_script = vec![];
            let current_scripts = crate::core::scripts().read_scripts();
            for it in current_scripts.as_array().ok_or("read script cache err")? {
                let it_name = read_json_str(it, "pkg_name");
                if it_name != name_for_install {
                    new_script.push(it.to_owned());
                }
            }
            let scripts_str =
                std::fs::read_to_string(real_pkg_dir_for_install.join("script.json"))?;
            let mut scripts: serde_json::Value = serde_json::from_str(&scripts_str)?;
            for it in scripts.as_array_mut().ok_or("script.json not array")? {
                let obj_mut = it.as_object_mut().ok_or("script obj not object")?;
                obj_mut.insert(
                    "pkg_name".to_owned(),
                    serde_json::json!(name_for_install.as_str()),
                );
                new_script.push(it.to_owned());
            }
            crate::core::scripts().replace_scripts(serde_json::Value::Array(new_script));
            crate::core::scripts().add_package_name(name_for_install);
            Ok(true)
        })
        .await??;
        if !installed {
            let res = hyper::Response::new(full("插件已经存在，无法重复安装"));
            return Ok(res);
        }

        // 执行初始化脚本，不用等待
        let name_t = name.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Err(err) = initevent::do_init_event(Some(&name_t)) {
                let _ = cq_add_log_w(&err.to_string());
            }
        });

        let _ = cq_add_log_w(&format!("上传文件大小为{}", content_len));

        let res = hyper::Response::new(full("ok"));
        Ok(res)
    } else if url_path == "/download_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name = params.get("pkg_name").cloned().unwrap_or_default();
        let plus_dir = cq_get_app_directory1_async().await?;
        let pkg_dir = if pkg_name.is_empty() {
            PathBuf::from_str(&plus_dir)?.join("default_pkg_dir")
        } else {
            PathBuf::from_str(&plus_dir)?
                .join("pkg_dir")
                .join(&pkg_name)
        };

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
            let res = hyper::Response::new(full("文件大小超过20MB"));
            return Ok(res);
        }

        let tmp_dir = redlang::cqapi::get_tmp_dir_async().await?;
        let file_dir = PathBuf::from(&tmp_dir).join("pkg");

        if !file_dir.exists() {
            tokio::fs::create_dir_all(&file_dir).await?;
        }

        let uid = uuid::Uuid::new_v4().to_string();
        let tmp_file_name = format!("{}.red.7z", uid);
        let tmp_file_path = file_dir.join(&tmp_file_name);

        let _guard = scopeguard::guard(tmp_file_path.clone(), |path| {
            runtime().spawn(async move {
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        let tmp_file_path_clone = tmp_file_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::compress_to_path(&pkg_dir, tmp_file_path_clone)?;
            Ok(())
        })
        .await??;

        let file_bin = tokio::fs::read(tmp_file_path).await?;

        let mut res = hyper::Response::new(full(file_bin));
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("application/octet-stream"),
        );
        // 设置文件名为pkg_name.red.7z，如果pkg_name为空，则为default.red.7z
        if pkg_name.is_empty() {
            res.headers_mut().insert(
                "Content-Disposition",
                HeaderValue::from_str("attachment; filename=default.red.7z")?,
            );
        } else {
            res.headers_mut().insert(
                "Content-Disposition",
                HeaderValue::from_str(&format!("attachment; filename={}.red.7z", pkg_name))?,
            );
        }
        Ok(res)
    } else {
        let res = hyper::Response::new(full("api not found"));
        Ok(res)
    }
}
