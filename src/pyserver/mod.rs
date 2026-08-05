// 此页代码为进行任何测试，暂时不可用

use crate::cqapi::cq_add_log_w;
use crate::runtime;
use crate::runtime_context;

async fn send_to_ser(code: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let hand = runtime_context()
        .session
        .python_handler()
        .await
        .ok_or("not have python env")?;
    hand.send(code).await?;
    Ok(())
}

pub fn call_py_block(code: &str, input: &str) -> String {
    runtime().block_on(async {
        let rst = call_py(code, input).await;
        match rst {
            Ok(s) => s.to_string(),
            Err(err) => {
                cq_add_log_w(&err.to_string()).unwrap();
                "".to_string()
            }
        }
    })
}

async fn call_py(
    code: &str,
    input: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let uid = uuid::Uuid::new_v4().to_string();
    let send_json = serde_json::json!({
        "echo":uid,
        "code":code,
        "input":input
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(1);
    {
        runtime_context()
            .session
            .add_python_echo(uid.clone(), tx)
            .await;
    }
    let _guard = scopeguard::guard(uid, |uid| {
        runtime().spawn(async move {
            runtime_context().session.remove_python_echo(&uid).await;
        });
    });
    let ret = send_to_ser(send_json.to_string()).await;
    if ret.is_err() {
        cq_add_log_w(&format!("call_py err:{:?}", ret.err())).unwrap();
        return Ok(serde_json::Value::String(String::new()));
    }
    tokio::select! {
        std::option::Option::Some(val) = rx.recv() => {
            Ok(val)
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(90)) => {
            cq_add_log_w("接收python返回超时").unwrap();
            Ok(serde_json::Value::String(String::new()))
        }
    }
}
