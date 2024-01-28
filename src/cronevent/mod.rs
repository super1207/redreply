use std::thread;

use chrono::TimeZone;

use crate::{read_code_cache, cqapi::cq_add_log_w};


lazy_static! {
    static ref G_LAST_RUN_TIME:std::sync::Mutex<Option<i64>> = std::sync::Mutex::new(None);
}

fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json.get("name").ok_or("脚本中无name")?.as_str().ok_or("脚本中name不是str")?;
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let keyword = node.get("关键词").ok_or("脚本中无关键词")?.as_str().ok_or("脚本中关键词不是str")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    return Ok((keyword,cffs,code,name,pkg_name));
}

fn do_cron_event_t2() -> Result<i32, Box<dyn std::error::Error>> {
    let mut to_deal_time: Vec<i64> = vec![];

    // 
    let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64 - 1;
    {
        let mut last_time_lk = G_LAST_RUN_TIME.lock()?;
        if last_time_lk.is_none() {
            to_deal_time.push(now_time);
        }else if now_time > last_time_lk.unwrap() {
            for i in ((*last_time_lk).unwrap() + 1) .. now_time + 1 {
                to_deal_time.push(i);
            }
        }
        (*last_time_lk) =  Some(now_time);
    }

    let script_json = read_code_cache()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (keyword,cffs,code,name,pkg_name) = get_script_info(&script_json[i])?;
        if cffs == "CRON定时器" {
            let schedule = <cron::Schedule as std::str::FromStr>::from_str(&keyword)?;
            for timestamp in to_deal_time.clone() {
                let datetime_rst = chrono::prelude::Local.timestamp_opt(timestamp, 0);
                if let chrono::LocalResult::Single(data) = datetime_rst {
                    let mut timestamp_vec:Vec<i64> = vec![];
                    for datetime in schedule.after(&data).take(1) {
                        timestamp_vec.push(datetime.timestamp() - 1);
                    }
                    if timestamp_vec.len() != 0 {
                        let dst_time = timestamp_vec[0] as i64;
                        let pkg_name_t = pkg_name.to_string();
                        let name_t = name.to_string();
                        let code_t = code.to_string();
                        if dst_time == timestamp {
                            thread::spawn(move ||{
                                let mut rl = crate::redlang::RedLang::new();
                                rl.pkg_name = pkg_name_t;
                                rl.script_name = name_t;
                                if let Err(err) = crate::cqevent::do_script(&mut rl,&code_t) {
                                    cq_add_log_w(&format!("{}",err)).unwrap();
                                }
                            });
                        }
                    }
                }
            }
        }      
    }
    Ok(0)
}

pub fn do_cron_event() -> Result<i32, Box<dyn std::error::Error>> {
    thread::spawn(||{
        loop {
            if let Err(err) = do_cron_event_t2(){
                cq_add_log_w(&err.to_string()).unwrap();
            }
            let time_struct = core::time::Duration::from_millis(500);
            std::thread::sleep(time_struct);
        }
    });
    Ok(0)
}