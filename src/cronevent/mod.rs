use std::{cell::RefCell, collections::{HashMap, HashSet}, rc::Rc, sync::Arc, thread, time::SystemTime};

use chrono::TimeZone;

use crate::{cqapi::cq_add_log_w, cqevent::is_key_match, read_code_cache, redlang::RedLang, RT_PTR};

// 【设置延迟触发@关键词@时间@传递数据】
#[derive(Clone,Debug)]
pub struct OneTimeRunStruct {
    pub run_time:i64,
    pub pkg_name:String,
    pub flag:String,
    pub sub_data:String,
    pub data:HashMap<String, Arc<String>>,
}

lazy_static! {
    static ref G_LAST_RUN_TIME:std::sync::Mutex<Option<i64>> = std::sync::Mutex::new(None);
    pub static ref G_ONE_TIME_RUN:std::sync::Mutex<Vec<OneTimeRunStruct>> = std::sync::Mutex::new(vec![]);
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

fn get_script_info2<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
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
    let ppfs = node.get("匹配方式").ok_or("脚本中无匹配方式")?.as_str().ok_or("脚本中匹配方式不是str")?;
    
    return Ok((keyword,cffs,code,ppfs,name,pkg_name));
}

fn do_cron_event_t2() -> Result<i32, Box<dyn std::error::Error>> {
    

    // 获得当前时间
    let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;
    
    // 获得当前时间与上一次时间之间经过的时间
    let mut to_deal_time: Vec<i64> = vec![];
    {
        let mut last_time_lk = G_LAST_RUN_TIME.lock()?;
        if last_time_lk.is_none() {
            to_deal_time.push(now_time);
        }else if now_time > last_time_lk.unwrap() {
            for i in ((*last_time_lk).unwrap()) .. now_time{
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
                    // 获得以指定时间为基准，定时器下一次触发的时间
                    let mut timestamp_vec:Vec<i64> = vec![];
                    for datetime in schedule.after(&data).take(1) {
                        timestamp_vec.push(datetime.timestamp());
                    }
                    if timestamp_vec.len() != 0 {
                        let dst_time = timestamp_vec[0] as i64;
                        // 如果下一次触发时间在当前时间之前，则触发
                        if dst_time <= now_time {
                            let pkg_name_t = pkg_name.to_string();
                            let name_t = name.to_string();
                            let code_t = code.to_string();
                            thread::spawn(move ||{
                                let mut rl = crate::redlang::RedLang::new();
                                rl.pkg_name = pkg_name_t;
                                rl.script_name = name_t;
                                if let Err(err) = crate::cqevent::do_script(&mut rl,&code_t,"normal",false) {
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

fn do_timer_event_t2() -> Result<i32, Box<dyn std::error::Error>> {
    let now_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as i64;
    let mut run_vec = vec![];
    {
        let mut lk = G_ONE_TIME_RUN.lock().unwrap();
        let mut can_run_index_vec = HashSet::<usize>::new();
        for it in 0..lk.len() {
            if lk[it].run_time <= now_time {
                can_run_index_vec.insert(it);
            }
        }
        for it in can_run_index_vec.iter() {
            run_vec.push(lk[*it].clone());
        }
        let mut new_one_run_time = vec![];
        for it in 0..lk.len() {
            let index = it;
            if !can_run_index_vec.contains(&index) {
                new_one_run_time.push(lk[it].clone());
            }
        }
        *lk = new_one_run_time;
    }
    // cq_add_log_w(&format!("{:?}",run_vec)).unwrap();
    for it in &run_vec {
        let script_json = read_code_cache()?;
        for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
            let (keyword,cffs,code,ppfs,name,pkg_name) = get_script_info2(&script_json[i])?;
            if cffs == "延迟触发" && it.pkg_name == pkg_name{
                let mut rl = RedLang::new();
                if is_key_match(&mut rl,&ppfs,keyword,&it.flag)? {
                    let data = it.data.to_owned();
                    let sub_data = it.sub_data.to_owned();
                    let code = code.to_owned();
                    let name = name.to_owned();
                    let pkg_name_t = pkg_name.to_owned();
                    RT_PTR.spawn_blocking(move ||{
                        let mut rl = RedLang::new();
                        let exmap = data;
                        let code_t = code.to_owned();
                        let script_name_t = name.to_owned();
                        rl.exmap = Rc::new(RefCell::new(exmap.clone()));
                        rl.pkg_name = pkg_name_t.to_owned();
                        rl.script_name = script_name_t.to_owned();
                        rl.set_coremap("隐藏", &sub_data).unwrap();
                        if let Err(err) = crate::cqevent::do_script(&mut rl,&code_t,"normal",false) {
                            cq_add_log_w(&format!("{}",err)).unwrap();
                        }
                    });
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
            if let Err(err) = do_timer_event_t2(){
                cq_add_log_w(&err.to_string()).unwrap();
            }
            let time_struct = core::time::Duration::from_millis(500);
            std::thread::sleep(time_struct);
        }
    });
    Ok(0)
}