use chrono::TimeZone;
use tokio::sync::RwLock;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use crate::cqapi::cq_add_log_w;
use crate::{RT_PTR, read_config};
use crate::cq_add_log;
use tokio_postgres::NoTls;

lazy_static::lazy_static! {
    static ref USE_PGSQL: AtomicBool = AtomicBool::new(false);
    static ref CLIENT: RwLock<Option<Arc<tokio_postgres::Client>>> = RwLock::new(None);
}

async fn manage_connection(conn_str: &str) {
	let mut backoff = 1u64;
	loop {
		match tokio_postgres::connect(conn_str, NoTls).await {
			Ok((client, connection)) => {
				{
					let mut w = CLIENT.write().await;
					*w = Some(Arc::new(client));
				}
                cq_add_log(&format!("数据库连接成功:{}", conn_str)).unwrap();
				// 等待 connection 结束，结束后清理 client 并重试
				if let Err(e) = connection.await {
                    cq_add_log_w(&format!("数据库连接错误：{}", e)).unwrap();
				} else {
                    cq_add_log("数据库连接已断开").unwrap();
				}
				{
					let mut w = CLIENT.write().await;
					*w = None;
				}
				// 连接断开后立即尝试重连，但加入短暂退避
				backoff = 1;
				tokio::time::sleep(Duration::from_secs(backoff)).await;
			}
			Err(e) => {
                cq_add_log_w(&format!("连接数据库失败：{}，{}秒后重试", e, backoff)).unwrap();
				tokio::time::sleep(Duration::from_secs(backoff)).await;
				backoff = std::cmp::min(backoff * 2, 60); // 指数退避上限 60s
			}
		}
	}
}


async fn get_client(timeout_secs: u64) -> Option<Arc<tokio_postgres::Client>> {
    // 在给定超时时间内轮询 CLIENT，超时则返回 None（不再无限等待）
    let dur = Duration::from_secs(timeout_secs);
    match tokio::time::timeout(dur, async {
        loop {
            {
                let r = CLIENT.read().await;
                if let Some(c) = r.as_ref() {
                    return c.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }).await {
        Ok(client) => Some(client),
        Err(_) => None,
    }
}

async fn init_postgresql_db_async(conn_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn_str = conn_str.to_string();

    tokio::spawn(async move {
        manage_connection(&conn_str).await;
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
            cq_add_log_w(&format!("数据库初始化失败，创建或检查 message 表失败：{}", e)).unwrap();
            return Err(Box::new(e));
        }
        Ok(())
    } else {
        cq_add_log_w("数据库客户端在初始化时不可用（超时），取消创建表操作").ok();
        return Err(Box::new(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout waiting for pg client")));
    }
}


pub fn do_insert(root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !USE_PGSQL.load(std::sync::atomic::Ordering::SeqCst) {
        // 未启用pgsql，直接返回
        return Ok(());
    }
    let root = root.to_owned();
    
    RT_PTR.spawn(async move {
        if let Err(err) = do_insert_async(
            &root
        ).await {
            cq_add_log_w(&format!("插入消息记录失败：{}", err)).unwrap();
        }
    });
    Ok(())
}

async fn do_insert_async (
	root:&serde_json::Value
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    let message_type = root["message_type"].as_str().unwrap_or("");
    if message_type != "group" && message_type != "private" {
        // 仅存储群消息和私聊消息
        return Ok(());
    }
    let msg_type: i16 = if message_type == "group" {0} else {1};
    let group_id = root["group_id"].as_str();
    let groups_id = root["groups_id"].as_str();
    let platform = root["platform"].as_str().unwrap_or("");
    let self_id = root["self_id"].as_str().unwrap_or("");
    let user_id = root["user_id"].as_str();
    let msg_id = root["message_id"].as_str().unwrap_or("");
    let card = root["sender"]["card"].as_str();
    let nickname = root["sender"]["nickname"].as_str();
    let time_t = root["time"].as_i64().unwrap_or(0); // 10 位时间戳

    let datetime_rst = chrono::prelude::Local.timestamp_opt(time_t, 0);
    let time_str;
    if let chrono::LocalResult::Single(datetime) = datetime_rst {
        let newdate = datetime.format("%Y-%m-%d %H:%M:%S");
        time_str = format!("{}",newdate);
    } else {
        time_str = "1970-01-01 00:00:00".to_string();
    }

    let msg_arr = &root["message"];   // json value

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
    let cfg = read_config()?;
    let postgresql_cfg = cfg["postgresql"].to_owned();
    if postgresql_cfg.is_null() {
        // 未配置postgresql，跳过初始化
        return Ok(());
    }
    USE_PGSQL.store(true, std::sync::atomic::Ordering::SeqCst);
    let host = postgresql_cfg["host"].as_str().unwrap_or("localhost");
    let port = postgresql_cfg["port"].as_u64().unwrap_or(5432);
    let user = postgresql_cfg["user"].as_str().unwrap_or("postgres");
    let password = postgresql_cfg["password"].as_str().unwrap_or("");
    let dbname = postgresql_cfg["dbname"].as_str().unwrap_or("postgres");
    let conn_str = format!("host={} port={} user={} password={} dbname={}", host, port, user, password, dbname);
    RT_PTR.spawn(async move {
        if let Err(err) = init_postgresql_db_async(&conn_str).await {
            cq_add_log_w(&format!("初始化PostgreSQL数据库失败：{}", err)).unwrap();
        } else {
            cq_add_log("PostgreSQL数据库初始化成功！").unwrap();
        }
    });
    Ok(())
}