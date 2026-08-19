use std::collections::BTreeMap;

use std::rc::Rc;

use crate::{
    redlang::{add_fun, rv_bin, rv_empty, rv_object, rv_text, RedLang, RedValue},
    runtime,
};

pub fn init_web_ex_fun_map() {
    add_fun(vec!["网络-设置返回头"], |self_t, params| {
        let http_header = self_t.get_coremap_value("网络-返回头");
        let mut http_header_map: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        if let RedValue::Object(obj) = &*http_header {
            for (k, v) in obj {
                http_header_map.insert(k.to_string(), v.clone());
            }
        }
        let k = self_t.get_param_text_rc(params, 0)?;
        let v = self_t.get_param_text_rc(params, 1)?;
        http_header_map.insert(k.to_string(), rv_text(v.to_string()));
        self_t.set_coremap_value("网络-返回头", rv_object(http_header_map));
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["网络-访问参数"], |self_t, _params| {
        Ok(Some(self_t.get_coremap_value("网络-访问参数")))
    });
    add_fun(vec!["网络-访问方法"], |self_t, _params| {
        let ret = self_t.get_coremap("网络-访问方法");
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["网络-访问头"], |self_t, _params| {
        Ok(Some(self_t.get_coremap_value("网络-访问头")))
    });
    add_fun(vec!["网络-权限"], |self_t, _params| {
        let ret = self_t.get_coremap("网络-权限");
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["网络-访问体"], |self_t, _params| {
        if self_t.req_tx.is_none() || self_t.req_rx.is_none() {
            return Ok(Some(self_t.get_coremap_value("网络-访问体")));
        }
        let ret_vec: Option<Vec<u8>> = runtime().block_on(async {
            // 客户端断开后后台任务已退出，send 会失败；recv 会返回 None
            if self_t.req_tx.clone().unwrap().send(true).await.is_err() {
                return None;
            }
            self_t.req_rx.as_mut().unwrap().recv().await
        });
        // 无论成败都清空通道，避免重复访问一个已断开的请求体
        self_t.req_rx = None;
        self_t.req_tx = None;
        let Some(ret_vec) = ret_vec else {
            return Err(RedLang::make_err("网络-访问体获取失败:请求方已断开连接"));
        };
        let ret = rv_bin(ret_vec);
        self_t.set_coremap_value("网络-访问体", ret.clone());
        Ok(Some(ret))
    });
}
