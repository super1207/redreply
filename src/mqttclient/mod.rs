use crate::cqapi::{cq_add_log, cq_add_log_w, cq_call_api};
use crate::mytool::read_json_str;
use crate::runtime;
use rumqttc::v5::mqttbytes::v5::Filter;
use rumqttc::v5::mqttbytes::v5::Packet::ConnAck;
use rumqttc::v5::mqttbytes::v5::Packet::Publish;
use rumqttc::v5::mqttbytes::v5::Subscribe;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{Client, MqttOptions};
use std::time::Duration;

use crate::runtime_context;

fn get_mqtt_client() -> Option<Client> {
    runtime_context().services.mqtt_client()
}

fn get_mqtt_client_id() -> String {
    runtime_context().services.mqtt_client_id()
}

pub fn call_mqtt_remote(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    mut playload: serde_json::Value,
    remote_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let topic = format!("bot/{remote_id}/{platform}/{self_id}/api");
    let client = get_mqtt_client();
    if let Some(client) = client {
        let uuid_t = uuid::Uuid::new_v4().to_string();
        let uuid_b = uuid_t.as_bytes();
        let resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties {
            content_type: Some("application/json".to_string()),
            response_topic: Some(format!("plus/{}/response", get_mqtt_client_id())),
            correlation_data: Some(bytes::Bytes::copy_from_slice(uuid_b)),
            ..Default::default()
        };
        let (tx_ay, rx_ay) = std::sync::mpsc::channel::<serde_json::Value>();
        runtime_context()
            .services
            .add_mqtt_echo(uuid_t.clone(), tx_ay);
        let _guard = scopeguard::guard(uuid_t, |echo| {
            runtime_context().services.remove_mqtt_echo(&echo);
        });
        let payload_obj = playload
            .as_object_mut()
            .ok_or("MQTT远程调用参数必须是JSON对象")?;
        payload_obj["message_id"] = serde_json::json!(passive_id);
        let payload = playload.to_string();
        cq_add_log(&format!("mqtt client send: {}", payload)).unwrap();
        client.try_publish_with_properties(topic, QoS::ExactlyOnce, false, payload, resp_props)?;
        let rst = match rx_ay.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(v) => v.to_string(),
            Err(_) => {
                return Err("超时".into());
            }
        };
        Ok(rst)
    } else {
        Err("没有MQTT Client".into())
    }
}

pub fn publish_mqtt_event(
    evt: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let from_mqtt = !read_json_str(evt, "mqtt_client_id").is_empty();
    if from_mqtt {
        // 如果是来自MQTT的事件，则不进行处理
        return Ok(());
    }
    let client = get_mqtt_client();
    if let Some(client) = client {
        let platform = evt
            .get("platform")
            .ok_or("缺少platform")?
            .as_str()
            .ok_or("platform不是字符串")?;
        let self_id = evt
            .get("self_id")
            .ok_or("缺少self_id")?
            .as_str()
            .ok_or("self_id不是字符串")?;
        let payload = evt.to_string();
        let client_id = get_mqtt_client_id();
        let resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties {
            content_type: Some("application/json".to_string()),
            ..Default::default()
        };
        client.try_publish_with_properties(
            format!("bot/{client_id}/{platform}/{self_id}/event"),
            QoS::ExactlyOnce,
            false,
            payload,
            resp_props,
        )?;
    }
    Ok(())
}

fn deal_pushlish(
    p: rumqttc::v5::mqttbytes::v5::Publish,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let playload = serde_json::from_slice::<serde_json::Value>(&p.payload)?;
    let playload_str = playload.to_string();
    let topic = String::from_utf8_lossy(&p.topic).to_string();
    cq_add_log(&format!(
        "mqtt client received: {} topic:{}",
        playload_str, topic
    ))
    .unwrap();
    deal_api_callback(&p, &playload, &topic)?;
    deal_publish_api(&p, &playload, &playload_str, &topic)?;
    deal_remote_event(p, playload, &topic)?;
    Ok(())
}

fn deal_api_callback(
    p: &rumqttc::v5::mqttbytes::v5::Publish,
    playload: &serde_json::Value,
    topic: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !(topic.starts_with(&"plus/".to_string()) && topic.ends_with("/response")) {
        return Ok(());
    }
    let topic_parts = topic.split('/').collect::<Vec<&str>>();
    if topic_parts.len() != 3 {
        return Err(format!("topic not match1: {}", topic).into());
    }
    if topic_parts[1] != get_mqtt_client_id() {
        return Err(format!("topic not match2: {}", topic).into());
    }
    let correlation_data;
    if let Some(properties) = &p.properties {
        let correlation_data_t = &properties.correlation_data;
        correlation_data = correlation_data_t;
    } else {
        return Err(format!("topic not match3: {}", topic).into());
    }
    if correlation_data.is_none() {
        return Err(format!("topic not match4: {}", topic).into());
    }
    let correlation_data = correlation_data.clone().unwrap();
    let uuid_t: String = String::from_utf8(correlation_data.to_vec())?;
    runtime_context()
        .services
        .send_mqtt_echo(&uuid_t, playload.clone())?;
    Ok(())
}

fn deal_remote_event(
    _p: rumqttc::v5::mqttbytes::v5::Publish,
    mut playload: serde_json::Value,
    topic: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !(topic.starts_with(&"bot/".to_string()) && topic.ends_with("/event")) {
        return Ok(());
    }
    let topic_parts = topic.split('/').collect::<Vec<&str>>();
    if topic_parts.len() != 5 {
        return Err(format!("topic not match: {}", topic).into());
    }
    let remote_client_id = topic_parts[1];
    let platform = topic_parts[2];
    let self_id = topic_parts[3];
    // 远程载荷可能是任意 JSON(数字/数组/字符串)，对非对象做下标会 panic，
    // release 下 panic=abort 会直接杀死整个进程，必须先校验
    if !playload.is_object() {
        crate::cqapi::cq_add_log_w(
            format!(
                "收到非对象MQTT事件载荷，已忽略: topic={}, payload={}",
                topic,
                playload
            )
            .as_str(),
        )
        .unwrap();
        return Ok(());
    }
    playload["platform"] = serde_json::json!(platform);
    playload["self_id"] = serde_json::json!(self_id);
    playload["mqtt_client_id"] = serde_json::json!(remote_client_id);
    if let Err(e) = crate::cqevent::do_1207_event(&playload.to_string()) {
        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
    }
    Ok(())
}

fn deal_publish_api(
    p: &rumqttc::v5::mqttbytes::v5::Publish,
    playload: &serde_json::Value,
    playload_str: &str,
    topic: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client_id = get_mqtt_client_id();
    if !(topic.starts_with(&format!("bot/{client_id}/")) && topic.ends_with("/api")) {
        return Ok(());
    }
    let topic_parts = topic.split('/').collect::<Vec<&str>>();
    if topic_parts.len() != 5 {
        return Err(format!("topic not match: {}", topic).into());
    }
    // bot:0
    // client_id:1
    // platform:2
    // self_id:3
    // api:4
    let platform = topic_parts[2];
    let self_id = topic_parts[3];
    // 远程载荷可能是任意 JSON，对非对象做下标会 panic，与 deal_remote_event 同理
    let passive_id = playload
        .get("message_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let resp = cq_call_api(platform, self_id, passive_id, playload_str, "");
    let response_topic;
    let correlation_data;
    if let Some(properties) = &p.properties {
        let response_topic_t = &properties.response_topic;
        if response_topic_t.is_none() {
            return Ok(());
        }
        response_topic = response_topic_t.clone().unwrap();
        let correlation_data_t = &properties.correlation_data;
        correlation_data = correlation_data_t;
    } else {
        return Ok(());
    }

    let resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties {
        content_type: Some("application/json".to_string()),
        correlation_data: correlation_data.clone(),
        ..Default::default()
    };
    let client = get_mqtt_client();
    if let Some(client) = client {
        cq_add_log(&format!("mqtt client send: {}", resp)).unwrap();
        client.publish_with_properties(
            response_topic,
            QoS::ExactlyOnce,
            false,
            resp,
            resp_props,
        )?;
    } else {
        return Err("没有MQTT Client".into());
    }
    Ok(())
}

fn validate_mqtt_client_id(
    client_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Check if client ID is empty
    if client_id.is_empty() {
        return Err("MQTT client ID不能为空".into());
    }

    // Check client ID length (MQTT v5 allows up to 65535 bytes, but we'll use a reasonable limit)
    if client_id.len() > 65535 {
        return Err("MQTT client ID长度超过限制 (最大65535字节)".into());
    }

    // Check for valid characters in client ID
    // MQTT v5 allows UTF-8 strings, but it's better to restrict to safe characters
    if !client_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err("MQTT client ID只能包含字母、数字、下划线和连字符".into());
    }

    Ok(())
}

pub fn init_mqttclient() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config_manager::startup_config()?;
    let Some(mqtt_config) = config.mqtt_client else {
        // 没有配置MQTT Client，直接返回
        return Ok(());
    };
    let mqtt_config = mqtt_config.as_object().ok_or("mqtt_client不是对象")?;
    let broker_host = mqtt_config
        .get("broker_host")
        .ok_or("broker_host not found")?
        .as_str()
        .ok_or("broker_host not string")?
        .to_string();
    let broker_port = mqtt_config
        .get("broker_port")
        .ok_or("broker_port not found")?
        .as_u64()
        .ok_or("broker_port not number")?;

    if let Some(remote_clients) = mqtt_config.get("remote_clients") {
        if remote_clients.is_array() {
            let mut remote_clients_vec = vec![];
            for client in remote_clients.as_array().unwrap() {
                let client_str = client.as_str().ok_or("remote_client not string")?;
                remote_clients_vec.push(client_str.to_string());
            }
            runtime_context()
                .services
                .set_mqtt_remote_clients(remote_clients_vec);
        }
    }

    let mqtt_client_id = mqtt_config
        .get("client_id")
        .ok_or("client_id not found")?
        .as_str()
        .ok_or("client_id not string")?
        .to_string();

    // Validate the client ID
    validate_mqtt_client_id(&mqtt_client_id)?;

    let mut broker_username: Option<String> = None;
    let mut broker_password: Option<String> = None;
    if let (Some(u), Some(p)) = (
        mqtt_config.get("broker_username"),
        mqtt_config.get("broker_password"),
    ) {
        if !u.is_null() && !p.is_null() {
            broker_username = Some(u.as_str().ok_or("username not string")?.to_string());
            broker_password = Some(p.as_str().ok_or("password not string")?.to_string());
        }
    }

    {
        runtime_context()
            .services
            .set_mqtt_client_id(mqtt_client_id.clone());
    }

    cq_add_log("MQTT 推送已开启！").unwrap();
    cq_add_log(&format!("MQTT Client ID: {}", mqtt_client_id)).unwrap();
    cq_add_log(&format!("MQTT Broker: {}:{}", broker_host, broker_port)).unwrap();

    std::thread::spawn(move || {
        loop {
            let mut mqttoptions =
                MqttOptions::new(&mqtt_client_id, &broker_host, broker_port as u16);
            mqttoptions
                .set_keep_alive(Duration::from_secs(30))
                .set_clean_start(true)
                .set_max_packet_size(Some(268435456));
            if let (Some(u), Some(p)) = (&broker_username, &broker_password) {
                mqttoptions.set_credentials(u, p);
            }
            let (client, mut connection) = Client::new(mqttoptions, 10);
            {
                runtime_context()
                    .services
                    .set_mqtt_client(Some(client.clone()));
            }
            for notification in connection.iter().flatten() {
                if let rumqttc::v5::Event::Incoming(Publish(p)) = notification {
                    runtime().spawn_blocking(move || {
                        if let Err(e) = deal_pushlish(p) {
                            cq_add_log_w(&format!("deal_publish_api error: {}", e)).unwrap();
                        }
                    });
                } else if let rumqttc::v5::Event::Incoming(ConnAck(p)) = notification {
                    let code: rumqttc::v5::mqttbytes::v5::ConnectReturnCode = p.code;
                    if code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
                        let mqtt_client_id = get_mqtt_client_id();
                        // 请求通道容量有限：满时短暂重试，避免订阅静默丢失
                        // （丢失后远端事件直到下次重连都不会到达）
                        let mut last_err = None;
                        for _ in 0..100 {
                            match client.try_subscribe(
                                format!("bot/{mqtt_client_id}/+/+/api"),
                                QoS::ExactlyOnce,
                            ) {
                                Ok(()) => {
                                    last_err = None;
                                    break;
                                }
                                Err(err) => {
                                    last_err = Some(err);
                                    std::thread::sleep(Duration::from_millis(20));
                                }
                            }
                        }
                        if let Some(err) = last_err {
                            cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
                        }
                        let mut last_err = None;
                        for _ in 0..100 {
                            match client.try_subscribe(
                                format!("plus/{mqtt_client_id}/response"),
                                QoS::ExactlyOnce,
                            ) {
                                Ok(()) => {
                                    last_err = None;
                                    break;
                                }
                                Err(err) => {
                                    last_err = Some(err);
                                    std::thread::sleep(Duration::from_millis(20));
                                }
                            }
                        }
                        if let Some(err) = last_err {
                            cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
                        }
                        let remote_clients = { runtime_context().services.mqtt_remote_clients() };
                        for remote_client in remote_clients {
                            let topic = format!("bot/{remote_client}/+/+/event");
                            let mut last_err = None;
                            for _ in 0..100 {
                                let mut filter = Filter::new(topic.clone(), QoS::ExactlyOnce);
                                filter.nolocal = true;
                                let subscribe = Subscribe::new(filter, None);
                                match client.client.request_tx.try_send(subscribe.into()) {
                                    Ok(()) => {
                                        last_err = None;
                                        break;
                                    }
                                    Err(err) => {
                                        last_err = Some(err);
                                        std::thread::sleep(Duration::from_millis(20));
                                    }
                                }
                            }
                            if let Some(err) = last_err {
                                cq_add_log_w(&format!("MQTT Client subscribe error: {}", err))
                                    .unwrap();
                            }
                        }
                    }
                }
            }
            // 连接断开后清理并重连
            {
                runtime_context().services.set_mqtt_client(None);
            }
            cq_add_log_w("MQTT 连接已断开，5秒后重连...").unwrap();
            std::thread::sleep(Duration::from_secs(5));
        }
    });
    Ok(())
}
