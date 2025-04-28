use std::time::Duration;
use rumqttc::v5::mqttbytes::v5::Packet::Publish;
use rumqttc::v5::mqttbytes::v5::Packet::ConnAck;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{Client, MqttOptions};
use crate::cqapi::{cq_add_log, cq_add_log_w, cq_call_api};
use crate::{read_config, RT_PTR};


use std::sync::Mutex;

lazy_static! {
    static ref MQTT_CLIENT: Mutex<Option<Client>> = Mutex::new(None);
    static ref CLIENT_ID:Mutex<String> = Mutex::new("".to_string());
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

pub fn publish_mqtt_event(evt:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = get_mqtt_client();
    if let Some(client) = client {
        let platform = evt.get("platform").ok_or("缺少platform")?.as_str().ok_or("platform不是字符串")?;
        let self_id = evt.get("self_id").ok_or("缺少self_id")?.as_str().ok_or("self_id不是字符串")?;
        let payload = evt.to_string();
        let client_id = get_mqtt_client_id();
        let mut resp_props = rumqttc::v5::mqttbytes::v5::PublishProperties::default();
        resp_props.content_type = Some("application/json".to_string());
        client.publish_with_properties(format!("bot/{client_id}/{platform}/{self_id}/event"), QoS::ExactlyOnce, false, payload,resp_props)?;
    }
    Ok(())
}

fn deal_publish_api(p:rumqttc::v5::mqttbytes::v5::Publish) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    
    let playload = serde_json::from_slice::<serde_json::Value>(&p.payload)?;
    let playload_str = playload.to_string();
    cq_add_log(&format!("mqtt client received: {}", playload_str)).unwrap();
    let topic = String::from_utf8_lossy(&p.topic).to_string();
    let client_id = get_mqtt_client_id();
    if !(topic.starts_with(&format!("bot/{client_id}/")) && topic.ends_with("/api")) {
        return Err(format!("topic not match: {}", topic).into());
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
    let resp = cq_call_api(platform, self_id, passive_id, &playload_str);
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

    let broker_username_opt = mqtt_config.get("broker_username");
    let broker_password_opt = mqtt_config.get("broker_password");
    let mqtt_client_id = mqtt_config.get("client_id").ok_or("client_id not found")?.as_str().ok_or("client_id not string")?;
    let mut mqttoptions = MqttOptions::new(mqtt_client_id, broker_host, broker_port.try_into()?);
    mqttoptions.set_keep_alive(Duration::from_secs(5));
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
                        if let Err(e) = deal_publish_api(p) {
                            cq_add_log_w(&format!("deal_publish_api error: {}", e)).unwrap();
                        }
                    });
                } else if let rumqttc::v5::Event::Incoming(ConnAck(p)) = notification {
                    let code: rumqttc::v5::mqttbytes::v5::ConnectReturnCode = p.code;
                    if code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
                        let mqtt_client_id = get_mqtt_client_id();
                        let resp = client.subscribe(format!("bot/{mqtt_client_id}/+/+/api"), QoS::ExactlyOnce);
                        if let Err(err) = resp {
                            cq_add_log_w(&format!("MQTT Client subscribe error: {}", err)).unwrap();
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