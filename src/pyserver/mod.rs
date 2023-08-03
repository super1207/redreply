// 此页代码为进行任何测试，暂时不可用

use crate::RT_PTR;
use crate::cqapi::cq_add_log_w;
use crate::httpserver::G_PY_HANDER;
use crate::httpserver::G_PY_ECHO_MAP;

#[allow(dead_code)]
async fn send_to_ser(code:String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let lk = G_PY_HANDER.read().await;
    let hand = lk.clone().ok_or("not have python env")?;
    hand.send(code).await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn call_py(code:String) -> Result<String, Box<dyn std::error::Error + Send + Sync>>{
    let uid = uuid::Uuid::new_v4().to_string();
    let send_json = serde_json::json!({
        "echo":uid,
        "code":code
    });
    let (tx, mut rx) =  tokio::sync::mpsc::channel::<String>(1);
    {
        let mut lk = G_PY_ECHO_MAP.write().await;
        lk.insert(uid.clone(), tx);
    }
    let _guard = scopeguard::guard(uid, |uid| {
        RT_PTR.spawn(async move {
            G_PY_ECHO_MAP.write().await.remove(&uid);
        });
    });
    let ret = send_to_ser(send_json.to_string()).await;
    if ret.is_err() {
        cq_add_log_w(&format!("call_py err:{:?}",ret.err())).unwrap();
    }
    tokio::select! {
        std::option::Option::Some(val) = rx.recv() => {
            return Ok(val);
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
            cq_add_log_w(&format!("接收python返回超时")).unwrap();
            return Ok("".to_string());
        }
    }
}