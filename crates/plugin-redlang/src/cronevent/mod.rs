use std::{cell::RefCell, rc::Rc, thread, time::SystemTime};

use chrono::TimeZone;

use redlang::cqapi::cq_add_log_w;
use crate::cqevent::is_key_match;
use crate::core::RedLang;
use redlang::runtime;

pub(crate) use crate::core::scheduler_store::OneTimeRunStruct;

type ScriptInfo<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);
type ScriptInfoWithMatch<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a str);

fn get_script_info(
    script_json: &serde_json::Value,
) -> Result<ScriptInfo<'_>, Box<dyn std::error::Error>> {
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json
        .get("name")
        .ok_or("脚本中无name")?
        .as_str()
        .ok_or("脚本中name不是str")?;
    let node = script_json
        .get("content")
        .ok_or("script.json文件缺少content字段")?;
    let keyword = node
        .get("关键词")
        .ok_or("脚本中无关键词")?
        .as_str()
        .ok_or("脚本中关键词不是str")?;
    let cffs = node
        .get("触发方式")
        .ok_or("脚本中无触发方式")?
        .as_str()
        .ok_or("脚本中触发方式不是str")?;
    let code = node
        .get("code")
        .ok_or("脚本中无code")?
        .as_str()
        .ok_or("脚本中code不是str")?;
    Ok((keyword, cffs, code, name, pkg_name))
}

fn get_script_info2(
    script_json: &serde_json::Value,
) -> Result<ScriptInfoWithMatch<'_>, Box<dyn std::error::Error>> {
    let pkg_name_opt = script_json.get("pkg_name");
    let mut pkg_name = "";
    if let Some(val) = pkg_name_opt {
        pkg_name = val.as_str().ok_or("pkg_name不是字符串")?;
    }
    let name = script_json
        .get("name")
        .ok_or("脚本中无name")?
        .as_str()
        .ok_or("脚本中name不是str")?;
    let node = script_json
        .get("content")
        .ok_or("script.json文件缺少content字段")?;
    let keyword = node
        .get("关键词")
        .ok_or("脚本中无关键词")?
        .as_str()
        .ok_or("脚本中关键词不是str")?;
    let cffs = node
        .get("触发方式")
        .ok_or("脚本中无触发方式")?
        .as_str()
        .ok_or("脚本中触发方式不是str")?;
    let code = node
        .get("code")
        .ok_or("脚本中无code")?
        .as_str()
        .ok_or("脚本中code不是str")?;
    let ppfs = node
        .get("匹配方式")
        .ok_or("脚本中无匹配方式")?
        .as_str()
        .ok_or("脚本中匹配方式不是str")?;

    Ok((keyword, cffs, code, ppfs, name, pkg_name))
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
        let (baseline, trigger_threshold, clock_shift) =
            crate::core::scheduler().cron_window(now_time);
        if let Some((large, difference)) = clock_shift {
            if large {
                let _ = cq_add_log_w(&format!(
                    "检测到时间大幅倒流 ({} 秒)，进行补偿触发",
                    difference
                ));
            } else {
                let _ = cq_add_log_w(&format!(
                    "检测到时间小幅倒流 ({} 秒)，不补偿触发",
                    difference
                ));
            }
        }
        (baseline, trigger_threshold)
    };

    let script_json = crate::core::scripts().read_scripts();
    for i in 0..script_json
        .as_array()
        .ok_or("script.json文件不是数组格式")?
        .len()
    {
        // 只有 CRON 定时器脚本才需要解析触发方式/关键词等字段；
        // 其它脚本字段缺失时静默跳过，避免每 500ms 刷屏告警
        let is_cron = script_json[i]
            .get("content")
            .and_then(|node| node.get("触发方式"))
            .and_then(serde_json::Value::as_str)
            == Some("CRON定时器");
        if !is_cron {
            continue;
        }
        let (keyword, cffs, code, name, pkg_name) = match get_script_info(&script_json[i]) {
            Ok(v) => v,
            Err(err) => {
                let _ = cq_add_log_w(&format!("读取CRON脚本信息失败: {}", err));
                continue;
            }
        };
        if cffs == "CRON定时器" {
            let schedule = match <cron::Schedule as std::str::FromStr>::from_str(keyword) {
                Ok(s) => s,
                Err(err) => {
                    let _ = cq_add_log_w(&format!(
                        "无效的CRON表达式 `{}` (脚本:{}): {}",
                        keyword, name, err
                    ));
                    continue;
                }
            };

            // 以上次处理时间为基准，获取在这段时间内是否有触发时间
            let dt_baseline = match chrono::Local.timestamp_opt(baseline, 0).single() {
                Some(dt) => dt,
                None => {
                    let _ = cq_add_log_w("Invalid baseline timestamp");
                    continue;
                }
            };
            let upcoming = schedule.after(&dt_baseline);

            // 持续触发期间内所有调度点。
            // 补偿触发数量设上限：时钟大幅回拨（NTP 纠偏、虚拟机快照恢复）时，
            // 每秒级 CRON 会产生成千上万个漏触发点，逐点开线程会瞬间耗尽线程资源
            const MAX_CATCHUP: usize = 100;
            let mut fired = 0usize;
            let mut skipped_by_cap = false;
            for next_datetime in upcoming {
                if next_datetime.timestamp() <= trigger_threshold {
                    if fired >= MAX_CATCHUP {
                        skipped_by_cap = true;
                        break;
                    }
                    fired += 1;
                    // 触发任务，连续触发多个漏掉的调度点。
                    // 用运行时的阻塞线程池复用线程，避免逐点开 OS 线程瞬时耗尽线程资源
                    let pkg_name_t = pkg_name.to_string();
                    let name_t = name.to_string();
                    let code_t = code.to_string();
                    redlang::runtime().spawn_blocking(move || {
                        let mut rl = crate::core::RedLang::new();
                        rl.pkg_name = pkg_name_t;
                        rl.script_name = name_t;
                        if let Err(err) =
                            crate::cqevent::do_script(&mut rl, &code_t, "normal", false)
                        {
                            let _ = cq_add_log_w(&format!("{}", err));
                        }
                    });
                    // 不退出循环，继续检测是否有下一个漏掉的调度点
                } else {
                    break;
                }
            }
            if skipped_by_cap {
                let _ = cq_add_log_w(&format!(
                    "CRON补偿触发已达上限({MAX_CATCHUP}个)，剩余漏掉的调度点全部跳过 (脚本:{name})"
                ));
            }
        }
    }
    Ok(0)
}

// 负责延迟触发
fn do_timer_event_t2() -> Result<i32, Box<dyn std::error::Error>> {
    let now_time = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    let run_vec = crate::core::scheduler().take_due_runs(now_time);
    // cq_add_log_w(&format!("{:?}",run_vec)).unwrap();
    for it in &run_vec {
        let script_json = crate::core::scripts().read_scripts();
        let Some(script_arr) = script_json.as_array() else {
            let _ = cq_add_log_w("script.json文件不是数组格式,放弃本批延迟触发");
            break;
        };
        for i in 0..script_arr.len() {
            // 只有延迟触发脚本才解析字段，其它脚本缺字段时静默跳过
            let is_delay = script_arr[i]
                .get("content")
                .and_then(|node| node.get("触发方式"))
                .and_then(serde_json::Value::as_str)
                == Some("延迟触发");
            if !is_delay {
                continue;
            }
            // 单条脚本信息损坏只跳过该条，不再因 `?` 传播导致整批到期任务丢失
            let (keyword, cffs, code, ppfs, name, pkg_name) =
                match get_script_info2(&script_arr[i]) {
                    Ok(v) => v,
                    Err(err) => {
                        let _ = cq_add_log_w(&format!("读取延迟触发脚本信息失败: {}", err));
                        continue;
                    }
                };
            if cffs == "延迟触发" && it.pkg_name == pkg_name {
                let mut rl = RedLang::new();
                let is_match = match is_key_match(&mut rl, ppfs, keyword, &it.flag) {
                    Ok(m) => m,
                    Err(err) => {
                        let _ = cq_add_log_w(&format!("延迟触发匹配失败: {}", err));
                        continue;
                    }
                };
                if is_match {
                    let mut data = it.data.to_owned();
                    // 把匹配阶段得到的子关键词传给执行脚本，修复前缀/正则匹配丢失
                    if let Some(sub_key) = (*rl.exmap).borrow().get("子关键词").cloned() {
                        data.insert("子关键词".to_owned(), sub_key);
                    }
                    let sub_data = it.sub_data.to_owned();
                    let code = code.to_owned();
                    let name = name.to_owned();
                    let pkg_name_t = pkg_name.to_owned();
                    runtime().spawn_blocking(move || {
                        let mut rl = RedLang::new();
                        let exmap = data;
                        let code_t = code.to_owned();
                        let script_name_t = name.to_owned();
                        rl.exmap = Rc::new(RefCell::new(exmap.clone()));
                        rl.pkg_name = pkg_name_t.to_owned();
                        rl.script_name = script_name_t.to_owned();
                        if let Err(err) = rl.set_coremap("隐藏", &sub_data) {
                            let _ = cq_add_log_w(&format!("设置隐藏变量失败:{err}"));
                        }
                        if let Err(err) =
                            crate::cqevent::do_script(&mut rl, &code_t, "normal", false)
                        {
                            let _ = cq_add_log_w(&format!("{}", err));
                        }
                    });
                }
            }
        }
    }
    Ok(0)
}

pub fn do_cron_event() -> Result<i32, Box<dyn std::error::Error>> {
    thread::spawn(|| loop {
        if let Err(err) = do_cron_event_t2() {
            let _ = cq_add_log_w(&err.to_string());
        }
        if let Err(err) = do_timer_event_t2() {
            let _ = cq_add_log_w(&err.to_string());
        }
        if let Err(err) = crate::status::the_500ms_timer() {
            let _ = cq_add_log_w(&err.to_string());
        }
        let time_struct = core::time::Duration::from_millis(500);
        std::thread::sleep(time_struct);
    });
    Ok(0)
}
