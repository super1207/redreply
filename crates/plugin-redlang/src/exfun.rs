use crate::core::astparser;
use crate::core::{MAX_VALUE_DEPTH, ParsedImage, RedLang};
use redlang::cq_add_log_w;
use redlang::cqapi::{cq_add_log, get_tmp_dir};
use crate::cronevent::OneTimeRunStruct;
use crate::lifecycle::pkg_can_run;
use crate::pyserver::call_py_block;
use crate::core::{add_fun, get_random, rv_array, rv_bin, rv_empty, rv_object, rv_text, RedValue};
use redlang::resource_manager::{add_file_lock, del_file_lock};
use redlang::runtime;
use redlang::RED_PY_DECODE;
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use chrono::TimeZone;
use encoding::Encoding;
use flate2::{
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
    Compression,
};
use ini::Ini;
use jsonpath_rust::JsonPath;
use md5::{Digest, Md5};
use mlua::MultiValue;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use rusttype::Scale;
use std::io::Write;
use std::rc::Rc;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
    vec,
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
use time::UtcOffset;

use ab_glyph::{FontRef, PxScale};
use image::{AnimationDecoder, EncodableLayout, GenericImageView, ImageBuffer, Rgba};
use imageproc::geometric_transformations::Interpolation;
use imageproc::geometric_transformations::{rotate_about_center, warp_with, Projection};
use std::io::Cursor;
use std::sync::Arc;

const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose =
    engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

/// 解压炸弹防护：限制解码输出总大小，超出上限返回错误而不是把内存耗尽
fn read_decompress_limited<R: std::io::Read>(
    mut r: R,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    const MAX_DECOMPRESSED: usize = 256 * 1024 * 1024;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = r.read(&mut chunk)?;
        if n == 0 {
            return Ok(buf);
        }
        if buf.len() + n > MAX_DECOMPRESSED {
            return Err(RedLang::make_err("解压输出超过大小上限"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// 获取 python 命令名，并把错误类型转换为 RedLang 命令通用的错误类型，
/// 避免环境异常时 panic，而是向脚本返回错误
fn get_python_cmd_name() -> Result<String, Box<dyn std::error::Error>> {
    crate::python_env::get_python_cmd_name().map_err(|err| err.to_string().into())
}

/// 脚本参数/SVG 属性提供的图片尺寸可被恶意构造为超大值，
/// 统一限制单边与总像素数，避免多 GB 分配把进程直接 abort
fn check_image_dims(width: usize, height: usize) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_SIDE: usize = 32768;
    const MAX_PIXELS: usize = 64 * 1024 * 1024;
    if width > MAX_SIDE
        || height > MAX_SIDE
        || width
            .checked_mul(height)
            .is_none_or(|px| px > MAX_PIXELS)
    {
        return Err(RedLang::make_err("图片尺寸过大"));
    }
    Ok(())
}

fn get_param_text_array(
    self_t: &mut RedLang,
    params: &[astparser::Ast],
    i: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    self_t
        .get_param_array(params, i)?
        .into_iter()
        .map(|v| v.expect_text_value())
        .collect::<Result<Vec<String>, _>>()
}

fn text_map_to_red_object(map: BTreeMap<String, String>) -> Rc<RedValue> {
    let out = map.into_iter().map(|(k, v)| (k, rv_text(v))).collect();
    rv_object(out)
}

fn coremap_value_to_text_map(
    self_t: &mut RedLang,
    key: &str,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    self_t.get_coremap_text_map(key)
}

fn browser_access(
    self_t: &mut RedLang,
    params: &[astparser::Ast],
    operation: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut path = self_t.get_param_text(params, 0)?;
    if !cfg!(target_os = "windows") && path.starts_with('/') {
        path = format!("file://{path}");
    }

    let selector = self_t.get_param_text(params, 1)?;
    let width = self_t
        .get_coremap("浏览器宽")
        .parse::<u32>()
        .unwrap_or(1920);
    let height = self_t
        .get_coremap("浏览器高")
        .parse::<u32>()
        .unwrap_or(1080);
    let timeout = match self_t.get_coremap("访问超时") {
        value if value.is_empty() => 30000,
        value => value.parse::<u64>()?,
    };
    let headers = if self_t.get_coremap("访问头").is_empty() {
        BTreeMap::new()
    } else {
        coremap_value_to_text_map(self_t, "访问头")?
    };
    let request = serde_json::json!({
        "operation": operation,
        "path": path,
        "selector": selector,
        "proxy": self_t.get_coremap("代理"),
        "width": width,
        "height": height,
        "headless": self_t.get_coremap("网页截图调试") != "真",
        "timeout_ms": timeout,
        "headers": headers,
    });
    let handle = redlang::framework_services()
        .and_then(|services| services.get::<redlang::plugin_ports::BrowserServiceHandle>("browser"))
        .ok_or("浏览器插件未启用")?;
    handle.execute_json(&request.to_string()).map_err(|error| {
        Box::<dyn std::error::Error>::from(error)
    })
}

fn get_param_image(
    self_t: &mut RedLang,
    params: &[astparser::Ast],
    i: usize,
) -> Result<ParsedImage, Box<dyn std::error::Error>> {
    RedLang::parse_bin_to_img_raw(self_t.get_param_bin(params, i)?)
}

fn make_round_square_image(img: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let width = img.width();
    let height = img.height();
    let size = width.min(height);
    let mut out: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(size, size);

    for pixel in out.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    let offset_x = if width > height {
        (width - size) / 2
    } else {
        0
    };
    let offset_y = if height > width {
        (height - size) / 2
    } else {
        0
    };
    let center = (size as i32) / 2;
    let radius = (size as i32) / 2;

    for x in 0..size {
        for y in 0..size {
            let dx = x as i32 - center;
            let dy = y as i32 - center;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }

            let src_x = offset_x + x;
            let src_y = offset_y + y;
            if src_x >= width || src_y >= height {
                continue;
            }

            let pixel = img.get_pixel(src_x, src_y);
            if let Some(dest) = out.get_pixel_mut_checked(x, y) {
                *dest = *pixel;
            }
        }
    }

    out
}

fn red_to_serde_value(value: &RedValue) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match value {
        RedValue::Text(s) => {
            if let Ok(num) = s.parse::<i64>() {
                Ok(serde_json::Value::Number(num.into()))
            } else if let Ok(num) = s.parse::<f64>() {
                Ok(serde_json::json!(num))
            } else if s.as_ref() == "真" {
                Ok(serde_json::Value::Bool(true))
            } else if s.as_ref() == "假" {
                Ok(serde_json::Value::Bool(false))
            } else {
                Ok(serde_json::Value::String(s.as_ref().clone()))
            }
        }
        RedValue::Array(arr) => {
            let mut json_arr = Vec::with_capacity(arr.len());
            for item in arr {
                json_arr.push(red_to_serde_value(item)?);
            }
            Ok(serde_json::Value::Array(json_arr))
        }
        RedValue::Object(obj) => {
            let mut json_map = serde_json::Map::new();
            for (key, value) in obj {
                json_map.insert(key.clone(), red_to_serde_value(value)?);
            }
            Ok(serde_json::Value::Object(json_map))
        }
        RedValue::Bin(_) => Err(RedLang::make_err("字节集不能直接转换为JSON值")),
        _ => Err(RedLang::make_err(&format!(
            "不支持的类型向JSON转换: {}",
            value.get_type_name()
        ))),
    }
}

fn red_to_python_wire_json(
    value: &RedValue,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match value {
        RedValue::Text(s) => Ok(serde_json::json!({
            "type": "text",
            "value": s.as_ref()
        })),
        RedValue::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(red_to_python_wire_json(item)?);
            }
            Ok(serde_json::json!({
                "type": "array",
                "items": out
            }))
        }
        RedValue::Object(obj) => {
            let mut out = serde_json::Map::new();
            for (key, value) in obj {
                out.insert(key.clone(), red_to_python_wire_json(value)?);
            }
            Ok(serde_json::json!({
                "type": "object",
                "items": out
            }))
        }
        RedValue::Bin(bin) => Ok(serde_json::json!({
            "type": "bin",
            "value_b64": BASE64_CUSTOM_ENGINE.encode(bin.as_ref())
        })),
        RedValue::Fun(_) => Err(RedLang::make_err("函数不能传递给Python")),
    }
}

fn red_to_python_wire_string(value: &RedValue) -> Result<String, Box<dyn std::error::Error>> {
    Ok(serde_json::to_string(&red_to_python_wire_json(value)?)?)
}

fn python_wire_json_to_red(
    value: &serde_json::Value,
) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    let value_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or("Python返回值缺少type字段")?;
    match value_type {
        "text" => Ok(rv_text(
            value
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        )),
        "bin" => {
            let encoded = value
                .get("value_b64")
                .and_then(|v| v.as_str())
                .ok_or("Python字节集返回值缺少value_b64字段")?;
            Ok(rv_bin(BASE64_CUSTOM_ENGINE.decode(encoded)?))
        }
        "array" => {
            let items = value
                .get("items")
                .and_then(|v| v.as_array())
                .ok_or("Python数组返回值缺少items字段")?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(python_wire_json_to_red(item)?);
            }
            Ok(rv_array(out))
        }
        "object" => {
            let items = value
                .get("items")
                .and_then(|v| v.as_object())
                .ok_or("Python对象返回值缺少items字段")?;
            let mut out = BTreeMap::new();
            for (key, value) in items {
                out.insert(key.clone(), python_wire_json_to_red(value)?);
            }
            Ok(Rc::new(RedValue::Object(out)))
        }
        other => Err(RedLang::make_err(&format!("未知Python返回值类型: {other}"))),
    }
}

fn python_wire_string_to_red(output: &str) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    if output.is_empty() {
        return Ok(rv_empty());
    }
    let wire: serde_json::Value = serde_json::from_str(output)?;
    python_wire_json_to_red(&wire)
}

fn python_base64_wire_output_to_red(
    output: &str,
) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    let content = BASE64_CUSTOM_ENGINE.decode(output)?;
    let wire = String::from_utf8(content)?;
    python_wire_string_to_red(&wire)
}

pub async fn http_post(
    url: &str,
    data: Vec<u8>,
    headers: &BTreeMap<String, String>,
    proxy_str: &str,
    method: &str,
) -> Result<(Vec<u8>, BTreeMap<String, String>), Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let uri = reqwest::Url::from_str(url)?;
    if proxy_str.is_empty() {
        if uri.scheme() == "http" {
            client = reqwest::Client::builder()
                .no_proxy()
                .http1_title_case_headers()
                .build()?;
        } else {
            client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .no_proxy()
                .http1_title_case_headers()
                .build()?;
        }
    } else {
        if uri.scheme() == "http" {
            let proxy = reqwest::Proxy::http(proxy_str)?;
            client = reqwest::Client::builder()
                .proxy(proxy)
                .http1_title_case_headers()
                .build()?;
        } else {
            let proxy = reqwest::Proxy::https(proxy_str)?;
            client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .http1_title_case_headers()
                .proxy(proxy)
                .build()?;
        }
    }
    let mut req;
    if method == "POST" {
        req = client.post(uri).body(reqwest::Body::from(data)).build()?;
    } else if method == "GET" {
        req = client.get(uri).body(reqwest::Body::from(data)).build()?;
    } else if method == "PUT" {
        req = client.put(uri).body(reqwest::Body::from(data)).build()?;
    } else if method == "PATCH" {
        req = client.patch(uri).body(reqwest::Body::from(data)).build()?;
    } else if method == "DELETE" {
        req = client.delete(uri).body(reqwest::Body::from(data)).build()?;
    } else if method == "HEAD" {
        req = client.head(uri).body(reqwest::Body::from(data)).build()?;
    } else {
        return Err(format!("不支持的访问方法:{method}").into());
    }
    for (key, val) in headers {
        req.headers_mut()
            .append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
    }
    let ret = client.execute(req).await?;
    let header_map_obj = ret
        .headers()
        .iter()
        .map(|(key, val)| {
            (
                key.as_str().to_string(),
                val.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect::<BTreeMap<String, String>>();
    let retbin = ret.bytes().await?.to_vec();
    Ok((retbin, header_map_obj))
}

/// 与【编码】(form_urlencoded::byte_serialize) 配套的解码：
/// %XX 还原为字节、'+' 还原为空格（W3C form 语义），其余字符（包括 '='、'&'）
/// 原样保留——不能像查询串解析那样按 '='/'&' 切分后拼回，否则会静默吞掉这些字符。
/// 解码结果按 UTF-8 宽松转换，非法字节序列替换为 U+FFFD。
fn decode_url_percent(input: &str) -> String {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let raw = input.as_bytes();
    let mut decoded = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'+' => {
                decoded.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < raw.len() => {
                if let (Some(hi), Some(lo)) = (hex_val(raw[i + 1]), hex_val(raw[i + 2])) {
                    decoded.push(hi * 16 + lo);
                    i += 3;
                } else {
                    // 非 %XX 序列（如 "100%" 末尾的 %）原样保留
                    decoded.push(b'%');
                    i += 1;
                }
            }
            b => {
                decoded.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

mod commands_file;
mod commands_image;
mod commands_misc;
mod commands_network;
mod commands_python;
mod commands_system;

pub fn init_ex_fun_map() {
    commands_network::register();
    commands_image::register();
    commands_file::register();
    commands_system::register();
    commands_python::register();
    commands_misc::register();
}

pub fn json_value_to_red(
    json_val: &serde_json::Value,
) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    let err_str = "Json解析失败";
    if json_val.is_string() {
        return Ok(rv_text(json_val.as_str().ok_or(err_str)?.to_string()));
    }
    if json_val.is_object() {
        return do_json_obj(json_val);
    }
    if json_val.is_array() {
        return do_json_arr(json_val);
    }
    if json_val.is_null() {
        return Ok(rv_empty());
    }
    if json_val.is_boolean() {
        return Ok(rv_text(
            if json_val.as_bool().ok_or(err_str)? {
                "真"
            } else {
                "假"
            }
            .to_string(),
        ));
    }
    if json_val.is_number() {
        return Ok(rv_text(json_val.to_string()));
    }
    Err(None.ok_or(err_str)?)
}

fn do_json_obj(root: &serde_json::Value) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    let err = "Json对象解析失败";
    let mut ret_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
    for (k, v) in root.as_object().ok_or(err)? {
        ret_obj.insert(k.to_string(), json_value_to_red(v)?);
    }
    Ok(Rc::new(RedValue::Object(ret_obj)))


}

fn do_json_arr(root: &serde_json::Value) -> Result<Rc<RedValue>, Box<dyn std::error::Error>> {
    let err = "Json数组解析失败";
    let mut ret_arr: Vec<Rc<RedValue>> = vec![];
    for v in root.as_array().ok_or(err)? {
        ret_arr.push(json_value_to_red(v)?);
    }
    Ok(rv_array(ret_arr))
}

fn get_mid<'a>(
    s: &'a str,
    sub_begin: &str,
    sub_end: &str,
) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
    let mut ret_vec: Vec<&str> = vec![];
    let mut s_pos = s;
    let err_str = "get_mid err";
    if sub_begin.is_empty() || sub_end.is_empty() {
        return Err(RedLang::make_err(err_str));
    }
    loop {
        let pos = s_pos.find(sub_begin);
        if let Some(pos_num) = pos {
            s_pos = s_pos.get((pos_num + sub_begin.len())..).ok_or(err_str)?;
            let pos_end = s_pos.find(sub_end);
            if let Some(pos_end_num) = pos_end {
                let val = s_pos.get(..pos_end_num).ok_or(err_str)?;
                ret_vec.push(val);
                s_pos = s_pos.get((pos_end_num + sub_end.len())..).ok_or(err_str)?;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    Ok(ret_vec)


}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn make_round_square_image_creates_square_output() {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(10, 6, |_, _| Rgba([255, 0, 0, 255]));
        let out = make_round_square_image(&img);

        assert_eq!(out.width(), 6);
        assert_eq!(out.height(), 6);
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(3, 3).0[3], 255);
    }

    #[test]
    fn decode_url_percent_keeps_separators() {
        // 旧实现用 form_urlencoded::parse 按查询串切分，'='、'&' 会被吞掉
        assert_eq!(decode_url_percent("a=1&b=2"), "a=1&b=2");
        assert_eq!(
            decode_url_percent("https://x.com/a?b=1&c=2"),
            "https://x.com/a?b=1&c=2"
        );
    }

    #[test]
    fn decode_url_percent_round_trips_with_byte_serialize() {
        let cases = [
            "你好世界",
            "a=1&b=2",
            "hello world",
            "100%纯棉",
            "特殊字符!@#$^*()[]{}",
        ];
        for case in cases {
            let encoded: String =
                url::form_urlencoded::byte_serialize(case.as_bytes()).collect();
            assert_eq!(decode_url_percent(&encoded), case, "case: {case}");
        }
    }

    #[test]
    fn decode_url_percent_edge_cases() {
        // '+' 还原为空格（W3C form 语义）
        assert_eq!(decode_url_percent("a+b"), "a b");
        // 未编码的中文字符原样保留
        assert_eq!(decode_url_percent("中文"), "中文");
        // 独立的 '%'（后面不是两位十六进制）原样保留
        assert_eq!(decode_url_percent("100%"), "100%");
        assert_eq!(decode_url_percent("%G1"), "%G1");
        // 大小写十六进制都可以
        assert_eq!(decode_url_percent("%2f%2F"), "//");
        // 非法 UTF-8 字节序列宽松转换
        assert_eq!(decode_url_percent("%FF"), "\u{FFFD}");
    }
}
