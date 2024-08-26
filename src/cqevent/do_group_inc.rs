use std::collections::HashSet;

use crate::{cqapi::cq_add_log_w, read_code_cache, redlang::RedLang};

use super::{get_script_info, set_normal_evt_info};


fn do_redlang(root: &serde_json::Value,ban_pkgs:&HashSet<String>) -> Result< (), Box<dyn std::error::Error>> {
    let script_json = read_code_cache()?;
    for i in 0..script_json.as_array().ok_or("script.json文件不是数组格式")?.len(){
        let (_keyword,cffs,code,_ppfs,name,pkg_name) = get_script_info(&script_json[i])?;
        if ban_pkgs.contains(pkg_name) {
            continue;
        }
        let mut rl = RedLang::new();
        if cffs == "群成员增加" {
            set_normal_evt_info(&mut rl, root)?;
            rl.pkg_name = pkg_name.to_owned();
            rl.script_name = name.to_owned();
            if let Err(e) = super::do_script(&mut rl,code,"normal") {
                cq_add_log_w(format!("err in do_group_increase:do_group_increase:{}", e.to_string()).as_str()).unwrap();
            }
        }
    }
    Ok(())
}


// 处理群成员增加事件
pub fn do_group_inc(root: &serde_json::Value,ban_pkgs:&HashSet<String>) {
    if let Err(e) = do_redlang(&root,ban_pkgs) {
        cq_add_log_w(format!("err in do_group_increase:do_group_increase:{}", e.to_string()).as_str()).unwrap();
    }
}
