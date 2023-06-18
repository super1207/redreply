use std::collections::HashMap;
use std::path::PathBuf;

use crate::cqapi::{cq_get_app_directory1, get_history_log};
use crate::httpevent::do_http_event;
use crate::read_config;
use crate::redlang::RedLang;
use crate::{cqapi::cq_add_log_w, RT_PTR};
use futures_util::SinkExt;
use hyper::http::HeaderValue;
use hyper::service::make_service_fn;
use serde_json::json;

lazy_static! {
    static ref G_LOG_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<String>>> = tokio::sync::RwLock::new(HashMap::new());
}

pub fn add_ws_log(log_msg:String) {
    RT_PTR.clone().spawn(async move {
        let lk = G_LOG_MAP.read().await;
        for (_,tx) in &*lk {
            let _foo = tx.send(log_msg.clone()).await;
        }
    });
}

async fn deal_api(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error + Send + Sync>> {
    let url_path = request.uri().path();
    if url_path == "/get_code" {
        match crate::read_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(hyper::Body::from(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }else if url_path == "/get_all_pkg_name" {
        match crate::get_all_pkg_name() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(hyper::Body::from(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }else if url_path == "/get_config" {
        match crate::read_config() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(err) => {
                let mut res = hyper::Response::new(hyper::Body::from(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            },
        }
    }else if url_path == "/set_ws_urls" {
        let body = hyper::body::to_bytes(request.into_body()).await?;
        let js:serde_json::Value = serde_json::from_slice(&body)?;
        match crate::set_ws_urls(js){
            Ok(_) => {
                let ret = json!({
                    "retcode":0,
                });
                let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
            Err(_) => {
                let ret = json!({
                    "retcode":-1,
                });
                let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            },
        }
        
    }else if url_path == "/set_code" {
        let body = hyper::body::to_bytes(request.into_body()).await?;
        let js:serde_json::Value = serde_json::from_slice(&body)?;
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::save_code(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                tx.send(ret).unwrap();
            }
            else {
                let ret = json!({
                    "retcode":-1,
                });
                tx.send(ret).unwrap();
            }
        }).await?;
        let ret = rx.await.unwrap();
        let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
        
    }else if url_path == "/close" {
        cq_add_log_w("收到退出指令，正在退出").unwrap();
        crate::wait_for_quit();
    }else if url_path == "/get_version" {
        let ret = json!({
            "retcode":0,
            "data":crate::get_version()
        });
        let mut res = hyper::Response::new(hyper::Body::from(ret.to_string()));
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    }else if url_path == "/login" {
        let body = hyper::body::to_bytes(request.into_body()).await?;
        let mut res = hyper::Response::new(hyper::Body::from(vec![]));
        *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
        let pass_cookie = format!("{};Max-Age=31536000",String::from_utf8(body.to_vec())?);
        res.headers_mut().append(hyper::header::SET_COOKIE, HeaderValue::from_str(&pass_cookie)?);
        res.headers_mut().insert("Location", HeaderValue::from_static("/index.html"));
        Ok(res)
    }
    else if url_path.starts_with("/user") {
        let (tx, rx) =  tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let ret = do_http_event(request);
            if ret.is_ok() {
                tx.send(ret.unwrap()).unwrap();
            }else {
                let err_str = format!("Error:{:?}",ret);
                let mut res:hyper::Response<hyper::Body> = hyper::Response::new(hyper::Body::from(err_str));
                res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
                tx.send(res).unwrap();
            }
        }).await?;
        let ret = rx.await?;
        Ok(ret)    
    }
    else{
        let res = hyper::Response::new(hyper::Body::from("api not found"));
        Ok(res)
    }
}

async fn deal_file(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error + Send + Sync>> {
    let url_path = request.uri().path();
    let app_dir = cq_get_app_directory1().unwrap();
    let path = PathBuf::from(&app_dir);
    let path = path.join("webui");
    let url_path_t = url_path.replace("/", &std::path::MAIN_SEPARATOR.to_string());
    let file_path = path.join(url_path_t.get(1..).unwrap());
    let file_buf = tokio::fs::read(&file_path).await?;
    let mut res = hyper::Response::new(hyper::Body::from(file_buf));
    *res.status_mut() = hyper::StatusCode::OK;
    if url_path.ends_with(".html") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
    }else if url_path.ends_with(".js") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/javascript; charset=utf-8"));
    }else if url_path.ends_with(".css") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/css; charset=utf-8"));
    }else if url_path.ends_with(".png") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("image/png"));
    }else if url_path.ends_with(".txt") {
        res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/plain"));
    }else {
        *res.status_mut() = hyper::StatusCode::NOT_FOUND;
    }
    // cq_add_log_w(&format!("{:?}",res));
    Ok(res)
}
/// 处理ws协议
async fn serve_websocket(websocket: hyper_tungstenite::HyperWebsocket,mut rx:tokio::sync::mpsc::Receiver<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    
    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, mut _read_half ) = futures_util::StreamExt::split(ws_stream);
    
    while let Some(msg) = rx.recv().await { // 当所有tx被释放时,tx.recv会返回None
        // log::info!("recive:{}",msg);
        write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string())).await?;
    }
    Ok(())
}

fn http_auth(request: &hyper::Request<hyper::Body>) -> Result<(), Box<dyn std::error::Error>> {
    let web_pass_raw = crate::read_web_password()?;
    if web_pass_raw == "" {
        return Ok(())
    }
    let headers = request.headers();
    let cookie_str = headers.get("Cookie").ok_or("can not found Cookie")?.to_str()?;
    let web_pass = urlencoding::encode(&web_pass_raw);
    if cookie_str.contains(&format!("password={}",web_pass)) {
        return Ok(())
    }
    return Err(RedLang::make_err("password invaild"));
}

async fn connect_handle(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error + Send + Sync>> {
    
    let url_path = request.uri().path();

    // 登录页面不进行身份验证
    if url_path == "/login.html" {
        return deal_file(request).await; 
    }
    if url_path == "/login" {
        return deal_api(request).await;
    }
    
    // 身份验证
    if let Err(err) = http_auth(&request) {
        // 认证失败，跳转登录页面
        cq_add_log_w(&format!("{}",err)).unwrap();
        let mut res = hyper::Response::new(hyper::Body::from(vec![]));
        *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
        res.headers_mut().insert("Location", HeaderValue::from_static("/login.html"));
        return Ok(res);
    }

    // 升级ws协议
    if hyper_tungstenite::is_upgrade_request(&request) {
        if url_path == "/watch_log" {
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
                    log::warn!("Error in websocket connection: {}", e);
                }
                // 线程结束，删除对应的entry
                let mut lk = G_LOG_MAP.write().await;
                lk.remove(&uid);
            });
            return Ok(response);
        }else {
            return Ok(hyper::Response::new(hyper::Body::from("broken http")));
        }
    }else {
        // 处理HTTP协议
        if url_path == "/" {
            let mut res = hyper::Response::new(hyper::Body::from(vec![]));
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            res.headers_mut().insert("Location", HeaderValue::from_static("/index.html"));
            return Ok(res);
        }
        if url_path == "/readme.html" {
            let app_dir = cq_get_app_directory1().unwrap();
            let path = PathBuf::from(&app_dir);
            let path = path.join("webui");
            let url_path_t = "readme.md".to_owned();
            let file_path = path.join(url_path_t);
            let file_buf = tokio::fs::read(&file_path).await?;
            let ret_str = String::from_utf8(file_buf)?;
            let html = markdown::to_html_with_options(&ret_str, &markdown::Options::gfm())?;
            let html = html.replace("&lt;font color=&quot;red&quot;&gt;", "<font color=\"red\">");
            let html = html.replace("&lt;/font&gt;", "</font>");
            let mut res = hyper::Response::new(hyper::Body::from(html));
            res.headers_mut().insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
            return Ok(res);
        }else if url_path == "/favicon.ico" {
            let app_dir = cq_get_app_directory1().unwrap();
            let path = PathBuf::from(&app_dir);
            let path = path.join("webui");
            let url_path_t = "favicon.ico".to_owned();
            let file_path = path.join(url_path_t);
            let file_buf = tokio::fs::read(&file_path).await?;
            let mut res = hyper::Response::new(hyper::Body::from(file_buf));
            res.headers_mut().insert("Content-Type", HeaderValue::from_static("image/x-icon"));
            return Ok(res);
        } else if !url_path.contains(".") || url_path.starts_with("/user") {
            return deal_api(request).await;
        } else {
            return deal_file(request).await;
        }
    }

    
}

pub fn init_http_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = read_config()?;
    let mut host = config.get("web_host").ok_or("无法获取web_host")?.as_str().ok_or("无法获取web_host")?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    if host == "localhost" {
        host = "127.0.0.1";
    }
    let web_uri = format!("{host}:{port}");
    let addr = web_uri.parse::<std::net::SocketAddr>()?;
    RT_PTR.clone().spawn(async move {
        let bd_rst = hyper::Server::try_bind(&addr);
        if bd_rst.is_ok() {
            // 启动服务
            let ret = bd_rst.unwrap().serve(make_service_fn(|_conn| async {
                Ok::<_, std::convert::Infallible>(hyper::service::service_fn(connect_handle))
            })).await;
            if let Err(err)  = ret{
                cq_add_log_w(&format!("绑定端口号失败：{}",err)).unwrap();
            }
        }
    });
    if let Some(not_open_browser) = config.get("not_open_browser") {
        if not_open_browser == false {
            opener::open(format!("http://localhost:{port}"))?;
        }
    }else {
        opener::open(format!("http://localhost:{port}"))?;
    }
    
    
    Ok(())
}