use redlang::cqapi::cq_add_log_w;
use crate::script_manager::get_gobal_init_code;

fn get_script_info(
    script_json: &serde_json::Value,
) -> Result<(&str, &str, &str, &str), Box<dyn std::error::Error>> {
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
    Ok((cffs, code, name, pkg_name))
}

pub fn do_gobal_init_event(pkg_name_opt: Option<&str>) -> Result<i32, Box<dyn std::error::Error>> {
    let code = get_gobal_init_code()?;
    if code.is_empty() {
        return Ok(0);
    }
    if let Some(pkg_name) = pkg_name_opt {
        let mut rl = crate::core::RedLang::new();
        rl.script_name = "9dbc0f69-d736-4e07-8a75-ec462fecd387".to_owned();
        rl.pkg_name = pkg_name.to_owned();
        let ret = crate::cqevent::do_script(&mut rl, &code, "init", false);
        if let Err(err) = ret {
            let _ = cq_add_log_w(&format!("{}", err));
        }
    } else {
        // 为所有包执行预初始化
        let mut pkg_names = crate::core::scripts().package_names();
        pkg_names.push("".to_owned()); // 追加一个默认包
        for pkg_name in pkg_names {
            let mut rl = crate::core::RedLang::new();
            rl.script_name = "9dbc0f69-d736-4e07-8a75-ec462fecd387".to_owned();
            rl.pkg_name = pkg_name.to_owned();
            let ret = crate::cqevent::do_script(&mut rl, &code, "init", false);
            if let Err(err) = ret {
                let _ = cq_add_log_w(&format!("{}", err));
            }
        }
    }
    Ok(0)
}

// 处理init事件
pub fn do_init_event(pkg_name_opt: Option<&str>) -> Result<i32, Box<dyn std::error::Error>> {
    // 这里处理预初始化逻辑
    do_gobal_init_event(pkg_name_opt)?;

    let script_json = crate::core::scripts().read_scripts();
    for i in 0..script_json
        .as_array()
        .ok_or("script.json文件不是数组格式")?
        .len()
    {
        let (cffs, code, name, pkg_name) = get_script_info(&script_json[i])?;
        let mut rl = crate::core::RedLang::new();
        if cffs == "框架初始化" {
            rl.pkg_name = pkg_name.to_owned();
            if pkg_name_opt.is_none_or(|value| value == rl.pkg_name) {
                rl.script_name = name.to_owned();
                let ret = crate::cqevent::do_script(&mut rl, code, "init", false);
                if let Err(err) = ret {
                    let _ = cq_add_log_w(&format!("{}", err));
                }
            }
        }
    }
    Ok(0)
}
