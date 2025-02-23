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

// 负责cron触发
fn do_cron_event_t2() -> Result<i32, Box<dyn std::error::Error>> {
    // 获得当前时间（单位为秒）
    let now_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    // 使用上一次处理时间作为基准，并更新为当前时间，同时处理系统时间倒流的情况
    // 如果时间倒流在30秒以内，则不重新触发已经执行过的 cron；如果超过30秒，则补偿触发
    let (baseline, trigger_threshold) = {
        let mut last_time_lk = G_LAST_RUN_TIME.lock()?;
        if let Some(prev_time) = *last_time_lk {
            if now_time < prev_time {
                let diff = prev_time - now_time;
                if diff > 30 {
                    cq_add_log_w(&format!("检测到时间大幅倒流 ({} 秒)，进行补偿触发", diff)).unwrap();
                    let baseline = now_time;
                    let trigger_threshold = prev_time;
                    *last_time_lk = Some(prev_time);
                    (baseline, trigger_threshold)
                } else {
                    cq_add_log_w(&format!("检测到时间小幅倒流 ({} 秒)，不补偿触发", diff)).unwrap();
                    let baseline = prev_time;
                    let trigger_threshold = now_time;
                    *last_time_lk = Some(prev_time);
                    (baseline, trigger_threshold)
                }
            } else {
                let baseline = prev_time;
                let trigger_threshold = now_time;
                *last_time_lk = Some(now_time);
                (baseline, trigger_threshold)
            }
        } else {
            *last_time_lk = Some(now_time);
            (now_time, now_time)
        }
    };

    let script_json = read_code_cache()?;
    for i in 0..script_json
        .as_array()
        .ok_or("script.json文件不是数组格式")?
        .len()
    {
        let (keyword, cffs, code, name, pkg_name) = get_script_info(&script_json[i])?;
        if cffs == "CRON定时器" {
            let schedule = <cron::Schedule as std::str::FromStr>::from_str(&keyword)?;

            // 以上次处理时间为基准，获取在这段时间内是否有触发时间
            let dt_baseline = chrono::Local.timestamp_opt(baseline, 0)
                .single()
                .ok_or("Invalid baseline timestamp")?;
            let mut upcoming = schedule.after(&dt_baseline);

            // 持续触发期间内所有调度点
            while let Some(next_datetime) = upcoming.next() {
                if next_datetime.timestamp() <= trigger_threshold {
                    // 触发任务，连续触发多个漏掉的调度点
                    let pkg_name_t = pkg_name.to_string();
                    let name_t = name.to_string();
                    let code_t = code.to_string();
                    thread::spawn(move || {
                        let mut rl = crate::redlang::RedLang::new();
                        rl.pkg_name = pkg_name_t;
                        rl.script_name = name_t;
                        if let Err(err) = crate::cqevent::do_script(&mut rl, &code_t, "normal", false) {
                            cq_add_log_w(&format!("{}", err)).unwrap();
                        }
                    });
                    // 不退出循环，继续检测是否有下一个漏掉的调度点
                } else {
                    break;
                }
            }
        }
    }
    Ok(0)
}

// 负责延迟触发
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
            if let Err(err) = crate::status::the_500ms_timer() {
                cq_add_log_w(&err.to_string()).unwrap();
            }
            let time_struct = core::time::Duration::from_millis(500);
            std::thread::sleep(time_struct);
        }
    });
    Ok(0)
}