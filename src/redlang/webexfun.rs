use std::collections::BTreeMap;

use crate::redlang::RedLang;

pub fn init_web_ex_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string();
            let k_t = crate::mytool::cmd_to_jt(&k);
            if k == k_t {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
            }else {
                if w.contains_key(&k) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
                }
                w.insert(k, fun);
                if w.contains_key(&k_t) {
                    let err_opt:Option<String> = None;
                    err_opt.ok_or(&format!("不可以重复添加命令:{}",k_t)).unwrap();
                }
                w.insert(k_t, fun);
            }
        }
    }
    add_fun(vec!["网络-设置返回头"],|self_t,params|{
        let http_header = self_t.get_coremap("网络-返回头")?.to_string();
        let mut http_header_map:BTreeMap<String, String> = BTreeMap::new();
        if http_header != "" {
            for (k,v) in RedLang::parse_obj(&http_header)?{
                http_header_map.insert(k, v.to_string());
            }
        }
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        http_header_map.insert(k, v);
        self_t.set_coremap("网络-返回头", &self_t.build_obj(http_header_map))?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["网络-访问参数"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问参数")?;
        return Ok(Some(ret.to_owned()));
    });
    add_fun(vec!["网络-访问方法"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问方法")?;
        return Ok(Some(ret.to_owned()));
    });
    add_fun(vec!["网络-访问头"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问头")?;

        return Ok(Some(ret.to_owned()));
    });
}