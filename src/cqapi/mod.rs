use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_char;

use encoding::all::GBK;
use encoding::{DecoderTrap, EncoderTrap, Encoding};

// 在这里链接CQ的函数
#[link(name = "CQP")]
extern "system" {
    pub fn CQ_callApi(ac: i32, msg: *const c_char) -> *const c_char;
    pub fn CQ_getAppDirectory(ac: i32) -> *const c_char;
    pub fn CQ_addLog(ac: i32, log_level: i32, category: *const c_char, log_msg: *const c_char) -> i32;
}

// 用于CQ识别插件的标记
static AUTH_CODE: AtomicI32 = AtomicI32::new(0);

pub fn get_auth_code() -> i32 {
    AUTH_CODE.load(Ordering::SeqCst)
}

pub fn set_auth_code(auth_code:i32) {
    AUTH_CODE.store(auth_code, Ordering::SeqCst);
}

// 获取插件的目录，绝对路径，末尾有'\',utf8编码
#[allow(dead_code)]
pub fn cq_get_app_directory() -> Result<String, Box<dyn std::error::Error>> {
    unsafe {
        let c_str = CQ_getAppDirectory(get_auth_code());
        let u8_str = GBK.decode(CStr::from_ptr(c_str).to_bytes(), DecoderTrap::Ignore)?;
        Ok(u8_str)
    }
}

// 用于发送Onebot原始数据，返回OneBot原始数据，utf8编码
#[allow(dead_code)]
pub fn cq_call_api(json_str: &str) -> Result<String, Box<dyn std::error::Error>> {
    let c_json_str = CString::new(json_str)?;
    unsafe {
        let c_str = CQ_callApi(get_auth_code(), c_json_str.as_ptr());
        let ret_json = CStr::from_ptr(c_str).to_str()?;
        Ok(ret_json.to_string())
    }
}

#[allow(dead_code)]
fn cq_add_log_t(log_level:i32,log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let c_category = CString::new("")?;
    let gbk_vec = GBK.encode(log_msg, EncoderTrap::Ignore)?;
    let c_log_msg = CString::new(gbk_vec)?;
    unsafe {
        let ret = CQ_addLog(
            get_auth_code(),
            log_level,
            c_category.as_ptr(),
            c_log_msg.as_ptr(),
        );
        Ok(ret)
    }
}

// 打印日志，utf8编码
#[allow(dead_code)]
pub fn cq_add_log(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(cq_add_log_t(0,log_msg)?)
}

// 打印日志，utf8编码
#[allow(dead_code)]
pub fn cq_add_log_w(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(cq_add_log_t(20,log_msg)?)
}