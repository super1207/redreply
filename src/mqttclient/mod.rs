use std::collections::HashMap;
use std::time::Duration;
use rumqttc::v5::mqttbytes::v5::Filter;
use rumqttc::v5::mqttbytes::v5::Packet::Publish;
use rumqttc::v5::mqttbytes::v5::Packet::ConnAck;
use rumqttc::v5::mqttbytes::v5::Subscribe;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{Client, MqttOptions};
use crate::cqapi::{cq_add_log, cq_add_log_w, cq_call_api};
use crate::mytool::read_json_str;
use crate::{read_config, RT_PTR};


use std::sync::Mutex;

lazy_static! {
    static ref MQTT_CLIENT: Mutex<Option<Client>> = Mutex::new(None);
    static ref CLIENT_ID:Mutex<String> = Mutex::new("".to_string());
    static ref REMOTE_CLIENTS:Mutex<Vec<String>> = Mutex::new(vec![]);
    static ref G_ECHO_MAP:std::sync::RwLock<HashMap<String,std::sync::mpsc::Sender<serde_json::Value>>> = std::sync::RwLock::new(HashMap::new());
}


fn get_mqtt_client() -> Option<Client> {
    let lk = MQTT_CLIENT.lock().unwrap();
    if let Some(client) = &*lk {
        return Some(client.clone());
    }
    None
}

fn get_mqtt_client_id() -> String {
    let lk = CLIENT_ID.lock().unwrap();
    (*lk).to_string()
}

pub fn call_mqtt_remote(platform:&str,self_id:&str,passive_id:&str,mut playload: serde_json::Value,remote_id:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let topic = format!("bot/{remote_id}/{platform}/{self_id}/api");
    let client = get_mqtt_client();
    if let Some(client) = client {
        let mut resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties::default();
        resp_props.content_type = Some("application/json".to_string());
        resp_props.response_topic = Some(format!("plus/{}/response", get_mqtt_client_id()));
        let uuid_t = uuid::Uuid::new_v4().to_string();
        let uuid_b = uuid_t.as_bytes();
        resp_props.correlation_data = Some(bytes::Bytes::copy_from_slice(uuid_b));
        let (tx_ay, rx_ay) =  std::sync::mpsc::channel::<serde_json::Value>();
        G_ECHO_MAP.write().unwrap().insert(uuid_t.clone(), tx_ay);
        let _guard = scopeguard::guard(uuid_t, |echo| {
            RT_PTR.spawn(async move {
                G_ECHO_MAP.write().unwrap().remove(&echo);
            });
        });
        playload["message_id"] = serde_json::json!(passive_id);
        let payload = playload.to_string();
        cq_add_log(&format!("mqtt client send: {}", payload)).unwrap();
        client.try_publish_with_properties(topic, QoS::ExactlyOnce, false, payload,resp_props)?;
        let rst;
        match rx_ay.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(v) => {
                rst = v.to_string();
            },
            Err(_) => {
                return Err("超时".into());
            }
        }
        return Ok(rst);
    } else {
        return Err("没有MQTT Client".into());
    }
}


pub fn publish_mqtt_event(evt:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let from_mqtt = read_json_str(evt, "mqtt_client_id") != "";
    if from_mqtt {
        // 如果是来自MQTT的事件，则不进行处理
        return Ok(());   
    }
    let client = get_mqtt_client();
    if let Some(client) = client {
        let platform = evt.get("platform").ok_or("缺少platform")?.as_str().ok_or("platform不是字符串")?;
        let self_id = evt.get("self_id").ok_or("缺少self_id")?.as_str().ok_or("self_id不是字符串")?;
        let payload = evt.to_string();
        let client_id = get_mqtt_client_id();
        let mut resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties::default();
        resp_props.content_type = Some("application/json".to_string());
        client.try_publish_with_properties(format!("bot/{client_id}/{platform}/{self_id}/event"), QoS::ExactlyOnce, false, payload,resp_props)?;
    }
    Ok(())
}

fn deal_pushlish(p:rumqttc::v5::mqttbytes::v5::Publish) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let playload = serde_json::from_slice::<serde_json::Value>(&p.payload)?;
    let playload_str = playload.to_string();
    let topic = String::from_utf8_lossy(&p.topic).to_string();
    cq_add_log(&format!("mqtt client received: {} topic:{}", playload_str,topic)).unwrap();
    deal_api_callback(&p,&playload,&topic)?;
    deal_publish_api(&p,&playload,&playload_str,&topic)?;
    deal_remote_event(p,playload,&topic)?;
    Ok(())
}

fn deal_api_callback(p:&rumqttc::v5::mqttbytes::v5::Publish,playload:&serde_json::Value,topic:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !(topic.starts_with(&format!("plus/")) && topic.ends_with("/response")) {
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
    let uuid_t:String = String::from_utf8(correlation_data.to_vec())?;
    let lk = G_ECHO_MAP.write().unwrap();
    let tx = lk.get(&uuid_t);
    if tx.is_none() {
        return Ok(());
    }
    let tx = tx.unwrap();
    tx.send(playload.clone())?;
    Ok(())
}

fn deal_remote_event(_p:rumqttc::v5::mqttbytes::v5::Publish,mut playload:serde_json::Value,topic:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !(topic.starts_with(&format!("bot/")) && topic.ends_with("/event")) {
        return Ok(());
    }
    let topic_parts = topic.split('/').collect::<Vec<&str>>();
    if topic_parts.len() != 5 {
        return Err(format!("topic not match: {}", topic).into());
    }
    let remote_client_id = topic_parts[1];
    let platform = topic_parts[2];
    let self_id = topic_parts[3];
    playload["platform"] = serde_json::json!(platform);
    playload["self_id"] = serde_json::json!(self_id);
    playload["mqtt_client_id"] = serde_json::json!(remote_client_id);
    if let Err(e) = crate::cqevent::do_1207_event(&playload.to_string()) {
        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
    }
    Ok(())
}


fn deal_publish_api(p:&rumqttc::v5::mqttbytes::v5::Publish,playload:&serde_json::Value,playload_str:&str,topic:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

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
    let passive_id = playload["message_id"].as_str().unwrap_or("");
    let resp = cq_call_api(platform, self_id, passive_id, &playload_str,"");
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

    let mut resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties::default();
    resp_props.content_type = Some("application/json".to_string());
    resp_props.correlation_data = correlation_data.clone();
    let client = get_mqtt_client();
    if let Some(client) = client {
        cq_add_log(&format!("mqtt client send: {}", resp)).unwrap();
        client.publish_with_properties(
            response_topic,
            QoS::ExactlyOnce,
            false,
            resp,
            resp_props
        )?;
        
    } else {
        return Err("没有MQTT Client".into());
    }
    Ok(())
}

fn validate_mqtt_client_id(client_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    if !client_id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err("MQTT client ID只能包含字母、数字、下划线和连字符".into());
    }
    
    Ok(())
}

pub fn init_mqttclient() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = read_config()?;
    let mqtt_config = &config["mqtt_client"];
    if !mqtt_config.is_object() {
        // 没有配置MQTT Client，直接返回
        return Ok(());
    }
    let mqtt_config = mqtt_config.as_object().unwrap();
    let broker_host = mqtt_config.get("broker_host").ok_or("broker_host not found")?.as_str().ok_or("broker_host not string")?;
    let broker_port = mqtt_config.get("broker_port").ok_or("broker_port not found")?.as_u64().ok_or("broker_port not number")?;
    
    if let Some(remote_clients) = mqtt_config.get("remote_clients") {
        if remote_clients.is_array() {
            let mut remote_clients_vec = vec![];
            for client in remote_clients.as_array().unwrap() {
                let client_str = client.as_str().ok_or("remote_client not string")?;
                remote_clients_vec.push(client_str.to_string());
            }
            let mut lk = REMOTE_CLIENTS.lock().unwrap();
            *lk = remote_clients_vec;
        }
    }
    
    let broker_username_opt = mqtt_config.get("broker_username");
    let broker_password_opt = mqtt_config.get("broker_password");
    let mqtt_client_id = mqtt_config.get("client_id").ok_or("client_id not found")?.as_str().ok_or("client_id not string")?;
    
    // Validate the client ID
    validate_mqtt_client_id(mqtt_client_id)?;
    
    let mut mqttoptions = MqttOptions::new(mqtt_client_id, broker_host, broker_port.try_into()?);
    mqttoptions
    .set_keep_alive(Duration::from_secs(30))
    .set_clean_start(true)
    .set_max_packet_size(Some(268435456));
    if broker_username_opt.is_some() && broker_password_opt.is_some() {
        let broker_username = broker_username_opt.unwrap();
        let broker_password = broker_password_opt.unwrap();
        if !broker_password.is_null() && !broker_username.is_null() {
            let broker_username = broker_username.as_str().ok_or("username not string")?;
            let broker_password = broker_password.as_str().ok_or("password not string")?;
            mqttoptions.set_credentials(broker_username, broker_password);
        }
    }
    let (client, mut connection) = Client::new(mqttoptions, 10);
    {
        let mut lk = CLIENT_ID.lock().unwrap();
        *lk = mqtt_client_id.to_string();
    }
    {
        let mut lk = MQTT_CLIENT.lock().unwrap();
        *lk = Some(client.clone());
    }
    std::thread::spawn(move ||{
        for (_, notification) in connection.iter().enumerate() {
            if let Ok(notification) = notification {
                if let rumqttc::v5::Event::Incoming(Publish(p)) = notification {
                    RT_PTR.spawn_blocking(move ||{
                        if let Err(e) = deal_pushlish(p) {
                            cq_add_log_w(&format!("deal_publish_api error: {}", e)).unwrap();
                        }
                    });
                } else if let rumqttc::v5::Event::Incoming(ConnAck(p)) = notification {
                    let code: rumqttc::v5::mqttbytes::v5::ConnectReturnCode = p.code;
                    if code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
                        let mqtt_client_id = get_mqtt_client_id();
                        let resp = client.try_subscribe(format!("bot/{mqtt_client_id}/+/+/api"), QoS::ExactlyOnce);
                        if let Err(err) = resp {
                            cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
                        }
                        let resp = client.try_subscribe(format!("plus/{mqtt_client_id}/response"), QoS::ExactlyOnce);
                        if let Err(err) = resp {
                            cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
                        }
                        let remote_clients;
                        {
                            let lk = REMOTE_CLIENTS.lock().unwrap();
                            remote_clients = (*lk).clone();
                        }
                        for remote_client in remote_clients {
                            let topic = format!("bot/{remote_client}/+/+/event");
                            let qos = QoS::ExactlyOnce;
                            let mut filter = Filter::new(topic, qos);
                            filter.nolocal = true;
                            let subscribe = Subscribe::new(filter, None);
                            let resp = client.client.request_tx.try_send(subscribe.into());
                            if let Err(err) = resp {
                                cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
                            }
                        }
                    }
                }
            } 
        }
    });
    cq_add_log("MQTT 推送已开启！").unwrap();
    cq_add_log(&format!("MQTT Client ID: {}", mqtt_client_id)).unwrap();
    cq_add_log(&format!("MQTT Broker: {}:{}", broker_host, broker_port)).unwrap();
    Ok(())
}