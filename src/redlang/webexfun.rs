use std::collections::BTreeMap;

use std::rc::Rc;

use crate::{redlang::{add_fun, rv_bin, rv_empty, rv_text, RedLang, RedValue}, RT_PTR};

fn rv_from_legacy(s: &str) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    Ok(Rc::new(RedValue::from_legacy_string(s)?))
}

pub fn init_web_ex_fun_map() {
    add_fun(vec!["网络-设置返回头"],|self_t,params|{
        let http_header = self_t.get_coremap("网络-返回头");
        let mut http_header_map:BTreeMap<String, String> = BTreeMap::new();
        if http_header != "" {
            for (k,v) in RedLang::parse_obj(&http_header)?{
                http_header_map.insert(k, v.to_string());
            }
        }
        let k = self_t.get_param_text_rc(params, 0)?;
        let v = self_t.get_param_text_rc(params, 1)?;
        http_header_map.insert(k.to_string(), v.to_string());
        self_t.set_coremap("网络-返回头", &self_t.build_obj(http_header_map))?;
        return Ok(Some(rv_empty()));
    });
    add_fun(vec!["网络-访问参数"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问参数");
        return Ok(Some(rv_from_legacy(&ret)?));
    });
    add_fun(vec!["网络-访问方法"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问方法");
        return Ok(Some(rv_text(ret)));
    });
    add_fun(vec!["网络-访问头"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-访问头");
        return Ok(Some(rv_from_legacy(&ret)?));
    });
    add_fun(vec!["网络-权限"],|self_t,_params|{
        let ret = self_t.get_coremap("网络-权限");
        return Ok(Some(rv_text(ret)));
    });
    add_fun(vec!["网络-访问体"],|self_t,_params|{
        if self_t.req_tx.is_none() ||  self_t.req_rx.is_none() {
            let ret = self_t.get_coremap("网络-访问体");
            return Ok(Some(rv_from_legacy(&ret)?));
        }
        let ret_vec:Vec<u8> = RT_PTR.block_on(async {
            self_t.req_tx.clone().unwrap().send(true).await.unwrap();
            let k =  self_t.req_rx.as_mut().unwrap().recv().await.unwrap();
            return k;
        });
        self_t.req_rx = None;
        self_t.req_tx = None;
        let ret = self_t.build_bin(ret_vec.clone());
        self_t.set_coremap("网络-访问体", &ret)?;
        return Ok(Some(rv_bin(ret_vec)));
    });
}
