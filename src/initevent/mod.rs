use crate::{read_code, cqapi::cq_add_log_w};


fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str), Box<dyn std::error::Error>>{
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    return Ok((cffs,code));
}

// 处理init事件
pub fn do_init_event() -> Result<i32, Box<dyn std::error::Error>> {
    let script_json = read_code()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (cffs,code) = get_script_info(&script_json[i])?;
        let mut rl = crate::redlang::RedLang::new();
        if cffs == "框架初始化" {
            let out = rl.parse(code)?;
            let ret = crate::cqevent::do_script(&mut rl,&out);
            if let Err(err) = ret{
                cq_add_log_w(&format!("{}",err)).unwrap();
            }
        }
    }
    Ok(0)
}