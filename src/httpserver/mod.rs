use std::path::PathBuf;

use crate::cqapi::cq_get_app_directory1;
use crate::read_config;
use crate::{cqapi::cq_add_log_w, RT_PTR};
use hyper::http::HeaderValue;
use hyper::service::make_service_fn;
use serde_json::json;

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
    }else if url_path == "/set_code" {
        let body = hyper::body::to_bytes(request.into_body()).await?;
        let js:serde_json::Value = serde_json::from_slice(&body)?;
        match crate::save_code(&js.to_string()){
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
        
    }else{
        let res = hyper::Response::new(hyper::Body::from("api not found"));
        Ok(res)
    }
}

async fn deal_file(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error + Send + Sync>> {
    let url_path = request.uri().path();
    let app_dir = cq_get_app_directory1().unwrap();
    let path = PathBuf::from(&app_dir);
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
    }else {
        *res.status_mut() = hyper::StatusCode::NOT_FOUND;
    }
    // cq_add_log_w(&format!("{:?}",res));
    Ok(res)
}
async fn connect_handle(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, Box<dyn std::error::Error + Send + Sync>> {
    // 处理HTTP协议
    let url_path = request.uri().path();
    if url_path == "/" {
        let mut res = hyper::Response::new(hyper::Body::from(vec![]));
        *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
        res.headers_mut().insert("Location", HeaderValue::from_static("/index.html"));
        return Ok(res);
    }
    if !url_path.contains(".") {
        return deal_api(request).await;
    } else {
        return deal_file(request).await;
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
                // log::error!("{}",err);
                // std::process::exit(-1);
            }
        }
    });
    opener::open(format!("http://localhost:{port}"))?;
    
    Ok(())
}