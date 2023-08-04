// 此页代码为进行任何测试，暂时不可用

use crate::RT_PTR;
use crate::cqapi::cq_add_log_w;
use crate::httpserver::G_PY_HANDER;
use crate::httpserver::G_PY_ECHO_MAP;


async fn send_to_ser(code:String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let lk = G_PY_HANDER.read().await;
    let hand = lk.clone().ok_or("not have python env")?;
    hand.send(code).await?;
    Ok(())
}


pub fn call_py_block(code:&str,input:&str) -> String {
    RT_PTR.block_on(async {
        let rst = call_py(code,input).await;
        match rst {
            Ok(s) => s,
            Err(err) => {
                cq_add_log_w(&err.to_string()).unwrap();
                "".to_string()
            }
        }
    })
}

async fn call_py(code:&str,input:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>{
    let uid = uuid::Uuid::new_v4().to_string();
    let send_json = serde_json::json!({
        "echo":uid,
        "code":code,
        "input":input
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
        return Ok("".to_string());
    }
    tokio::select! {
        std::option::Option::Some(val) = rx.recv() => {
            return Ok(val);
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(90)) => {
            cq_add_log_w(&format!("接收python返回超时")).unwrap();
            return Ok("".to_string());
        }
    }
}