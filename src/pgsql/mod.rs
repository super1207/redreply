use crate::cq_add_log;
use crate::cqapi::cq_add_log_w;
use crate::{runtime, runtime_context};
use chrono::TimeZone;
use std::sync::Arc;
use std::time::Duration;
use tokio_postgres::NoTls;

async fn manage_connection(config: tokio_postgres::Config, display: &str) {
    let mut backoff = 1u64;
    loop {
        // 用 Config 构建器逐字段设置，密码中的空格/引号等特殊字符不会再破坏解析，
        // 也杜绝通过字段内容注入额外连接参数
        // 用 Config 构建器逐字段设置，密码中的空格/引号等特殊字符不会再破坏解析，
        // 也杜绝通过字段内容注入额外连接参数
        match config.connect(NoTls).await {
            Ok((client, connection)) => {
                {
                    runtime_context()
                        .services
                        .set_pgsql_client(Some(Arc::new(client)))
                        .await;
                }
                let _ = cq_add_log(&format!("数据库连接成功:{display}"));
                // 等待 connection 结束，结束后清理 client 并重试
                if let Err(e) = connection.await {
                    let _ = cq_add_log_w(&format!("数据库连接错误：{}", e));
                } else {
                    let _ = cq_add_log("数据库连接已断开");
                }
                {
                    runtime_context().services.set_pgsql_client(None).await;
                }
                // 连接断开后立即尝试重连，但加入短暂退避
                backoff = 1;
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
            Err(e) => {
                let _ = cq_add_log_w(&format!("连接数据库失败：{}，{}秒后重试", e, backoff));
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = std::cmp::min(backoff * 2, 60); // 指数退避上限 60s
            }
        }
    }
}

/// 数字型 ID（OneBot11 标准里 message_id/group_id 常为数字）统一转字符串，
/// 否则 as_str() 取不到导致 NOT NULL 列插入失败
fn json_id_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

async fn get_client(timeout_secs: u64) -> Option<Arc<tokio_postgres::Client>> {
    // 在给定超时时间内轮询 CLIENT，超时则返回 None（不再无限等待）
    let dur = Duration::from_secs(timeout_secs);
    tokio::time::timeout(dur, async {
        loop {
            {
                if let Some(client) = runtime_context().services.pgsql_client().await {
                    return client;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .ok()
}

async fn init_postgresql_db_async(
    config: tokio_postgres::Config,
    display: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::spawn(async move {
        manage_connection(config, &display).await;
    });

    let create_table_sql = r#"
        CREATE TABLE IF NOT EXISTS public.message (
            msg_type smallint NOT NULL,
            group_id text,
            groups_id text,
            platform text NOT NULL,
            self_id text NOT NULL,
            user_id text,
            msg_id text NOT NULL,
            card text,
            nickname text,
            time timestamp(0),
            msg jsonb,
            PRIMARY KEY (platform, self_id, msg_id)
        );
    "#;

    // 有限等待客户端可用，超时则返回错误
    if let Some(client) = get_client(10).await {
        if let Err(e) = client.batch_execute(create_table_sql).await {
            let _ = cq_add_log_w(&format!(
                "数据库初始化失败，创建或检查 message 表失败：{}",
                e
            ));
            return Err(Box::new(e));
        }
        Ok(())
    } else {
        cq_add_log_w("数据库客户端在初始化时不可用（超时），取消创建表操作").ok();
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timeout waiting for pg client",
        )))
    }
}

pub fn do_insert(root: &serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !runtime_context().services.pgsql_enabled() {
        // 未启用pgsql，直接返回
        return Ok(());
    }
    let root = root.to_owned();

    runtime().spawn(async move {
        if let Err(err) = do_insert_async(&root).await {
            let _ = cq_add_log_w(&format!("插入消息记录失败：{}", err));
        }
    });
    Ok(())
}

async fn do_insert_async(
    root: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let message_type = root["message_type"].as_str().unwrap_or("");
    if message_type != "group" && message_type != "private" {
        // 仅存储群消息和私聊消息
        return Ok(());
    }
    let msg_type: i16 = if message_type == "group" { 0 } else { 1 };
    let group_id = json_id_string(&root["group_id"]);
    let groups_id = json_id_string(&root["groups_id"]);
    let platform = root["platform"].as_str().unwrap_or("");
    let self_id = root["self_id"].as_str().unwrap_or("");
    let user_id = json_id_string(&root["user_id"]);
    let msg_id = json_id_string(&root["message_id"]).unwrap_or_default();
    let card = root["sender"]["card"].as_str();
    let nickname = root["sender"]["nickname"].as_str();
    let time_t = root["time"].as_i64().unwrap_or(0); // 10 位时间戳

    let datetime_rst = chrono::prelude::Local.timestamp_opt(time_t, 0);
    let time_str;
    if let chrono::LocalResult::Single(datetime) = datetime_rst {
        let newdate = datetime.format("%Y-%m-%d %H:%M:%S");
        time_str = format!("{}", newdate);
    } else {
        time_str = "1970-01-01 00:00:00".to_string();
    }

    let msg_arr = &root["message"]; // json value

    // 从全局 CLIENT 获取 client（有限等待）
    if let Some(client) = get_client(3).await {
        client.execute(
            "INSERT INTO public.message (msg_type, group_id, groups_id, platform, self_id, user_id, msg_id, card, nickname, \"time\", msg) \
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,to_timestamp($10,'YYYY-MM-DD HH24:MI:SS'),$11) \
            ON CONFLICT (platform, self_id, msg_id) DO UPDATE SET \
                msg_type = EXCLUDED.msg_type, \
                group_id = EXCLUDED.group_id, \
                groups_id = EXCLUDED.groups_id, \
                user_id = EXCLUDED.user_id, \
                card = EXCLUDED.card, \
                nickname = EXCLUDED.nickname, \
                \"time\" = EXCLUDED.time, \
                msg = EXCLUDED.msg",
            &[
                &msg_type,
                &group_id,
                &groups_id,
                &platform,
                &self_id,
                &user_id,
                &msg_id,
                &card,
                &nickname,
                &time_str,
                msg_arr,
            ],
        ).await?;
    } else {
        // 客户端不可用，记录并跳过插入
        cq_add_log_w("数据库客户端不可用，跳过插入消息记录（超时）").ok();
    }

    Ok(())
}

pub fn init_postgresql_db() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config_manager::startup_config()?;
    let Some(postgresql_cfg) = config.postgresql else {
        // 未配置postgresql，跳过初始化
        return Ok(());
    };
    runtime_context().services.set_pgsql_enabled(true);
    let host = postgresql_cfg["host"].as_str().unwrap_or("localhost");
    let port = postgresql_cfg["port"].as_u64().unwrap_or(5432);
    let user = postgresql_cfg["user"].as_str().unwrap_or("postgres");
    let password = postgresql_cfg["password"].as_str().unwrap_or("");
    let dbname = postgresql_cfg["dbname"].as_str().unwrap_or("postgres");

    let mut pg_config = tokio_postgres::Config::new();
    pg_config.host(host);
    pg_config.port(u16::try_from(port).unwrap_or(5432));
    pg_config.user(user);
    pg_config.password(password);
    pg_config.dbname(dbname);

    // 日志只输出非敏感字段，密码不落日志
    let display = format!("host={host} port={port} user={user} dbname={dbname}");
    runtime().spawn(async move {
        if let Err(err) = init_postgresql_db_async(pg_config, display).await {
            let _ = cq_add_log_w(&format!("初始化PostgreSQL数据库失败：{}", err));
        } else {
            let _ = cq_add_log("PostgreSQL数据库初始化成功！");
        }
    });
    Ok(())
}
