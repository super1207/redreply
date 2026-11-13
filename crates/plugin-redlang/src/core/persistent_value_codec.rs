use std::{collections::BTreeMap, rc::Rc};

use super::{astparser, RedLang, RedValue};

/// 序列化最大嵌套深度：与运行期值树深度上限一致，
/// 防止畸形 DB 数据在反序列化时递归爆栈。
const MAX_PERSIST_DEPTH: usize = 256;

pub fn to_persistent_legacy_string(
    value: &RedValue,
) -> Result<Rc<String>, Box<dyn std::error::Error>> {
    fn encode(value: &RedValue, depth: usize) -> Result<Rc<String>, Box<dyn std::error::Error>> {
        if depth > MAX_PERSIST_DEPTH {
            return Err(RedLang::make_err("持久常量嵌套过深"));
        }
        Ok(match value {
            RedValue::Text(s) => s.clone(),
            RedValue::Array(arr) => {
                let mut ret = String::new();
                ret.push_str(super::PERSISTENT_VALUE_MARKER);
                ret.push('A');
                for item in arr {
                    let s = encode(item, depth + 1)?;
                    ret.push_str(&s.len().to_string());
                    ret.push(',');
                    ret.push_str(&s);
                }
                Rc::new(ret)
            }
            RedValue::Object(obj) => {
                let mut ret = String::new();
                ret.push_str(super::PERSISTENT_VALUE_MARKER);
                ret.push('O');
                for (k, v) in obj {
                    ret.push_str(&k.len().to_string());
                    ret.push(',');
                    ret.push_str(k);
                    let vs = encode(v, depth + 1)?;
                    ret.push_str(&vs.len().to_string());
                    ret.push(',');
                    ret.push_str(&vs);
                }
                Rc::new(ret)
            }
            RedValue::Bin(bin) => {
                let mut ret = String::new();
                ret.push_str(super::PERSISTENT_VALUE_MARKER);
                ret.push('B');
                for ch in bin.iter() {
                    ret.push_str(&format!("{:02X}", ch));
                }
                Rc::new(ret)
            }
            RedValue::Fun(ast) => {
                let mut ret = String::new();
                ret.push_str(super::PERSISTENT_VALUE_MARKER);
                ret.push('F');
                ret.push_str(&astparser::ast_to_string(ast));
                Rc::new(ret)
            }
        })
    }
    encode(value, 1)
}

pub fn from_persistent_legacy_string(s: &str) -> Result<RedValue, Box<dyn std::error::Error>> {
    decode(s, 1)
}

fn decode(s: &str, depth: usize) -> Result<RedValue, Box<dyn std::error::Error>> {
    if depth > MAX_PERSIST_DEPTH {
        return Err(RedLang::make_err("持久常量嵌套过深"));
    }
    let marker = super::PERSISTENT_VALUE_MARKER;
    if !s.starts_with(marker) {
        return Ok(RedValue::Text(Rc::new(s.to_string())));
    }
    let marker_len = marker.len();
    let tp = s
        .get(marker_len..marker_len + 1)
        .ok_or("类型解析错误,无类型标识")?;
    match tp {
        "A" => {
            let mut ret_arr: Vec<Rc<RedValue>> = vec![];
            let mut arr = s.get(marker_len + 1..).ok_or("不能获得数组类型")?;
            loop {
                let spos_opt = arr.find(',');
                let Some(spos_num) = spos_opt else {
                    // 尾部残留非空说明数据被截断，不能静默接受
                    if !arr.is_empty() {
                        return Err(RedLang::make_err("数组类型数据不完整"));
                    }
                    break;
                };
                let num_str = arr.get(0..spos_num).ok_or("不能获得数组类型")?;
                let num = num_str.parse::<usize>()?;
                // checked_add 防止恶意长度前缀造成 usize 溢出(debug panic)
                let start = spos_num.checked_add(1).ok_or("不能获得数组类型")?;
                let end = start.checked_add(num).ok_or("不能获得数组类型")?;
                let str_val = arr.get(start..end).ok_or("不能获得数组类型")?;
                ret_arr.push(Rc::new(decode(str_val, depth + 1)?));
                arr = arr.get(end..).ok_or("不能获得数组类型")?;
            }
            Ok(RedValue::Array(ret_arr))
        }
        "O" => {
            let mut ret_map: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
            let mut arr_strs: Vec<&str> = vec![];
            let mut arr = s.get(marker_len + 1..).ok_or("不能获得对象类型")?;
            loop {
                let spos_opt = arr.find(',');
                let Some(spos_num) = spos_opt else {
                    // 尾部残留非空说明数据被截断，不能静默接受
                    if !arr.is_empty() {
                        return Err(RedLang::make_err("对象类型数据不完整"));
                    }
                    break;
                };
                let num_str = arr.get(0..spos_num).ok_or("不能获得对象类型")?;
                let num = num_str.parse::<usize>()?;
                // checked_add 防止恶意长度前缀造成 usize 溢出(debug panic)
                let start = spos_num.checked_add(1).ok_or("不能获得对象类型")?;
                let end = start.checked_add(num).ok_or("不能获得对象类型")?;
                let str_val = arr.get(start..end).ok_or("不能获得对象类型")?;
                arr_strs.push(str_val);
                arr = arr.get(end..).ok_or("不能获得对象类型")?;
            }
            if !arr_strs.len().is_multiple_of(2) {
                return Err(RedLang::make_err("不能获得对象类型"));
            }
            for i in 0..(arr_strs.len() / 2) {
                let k = arr_strs[i * 2].to_string();
                let v = decode(arr_strs[i * 2 + 1], depth + 1)?;
                ret_map.insert(k, Rc::new(v));
            }
            Ok(RedValue::Object(ret_map))
        }
        "B" => {
            let content_text = s
                .get(marker_len + 1..)
                .ok_or("不能获得字节集类型")?
                .as_bytes();
            if content_text.len() % 2 != 0 {
                return Err(RedLang::make_err("不能获得字节集类型"));
            }
            let mut content2: Vec<u8> = vec![];
            for pos in 0..(content_text.len() / 2) {
                // 用 to_digit 解析：既兼容大小写，非法字符(如 ':'、'@'、非hex符号)
                // 也不会因 u8 下溢 panic(debug)/静默解错(release)
                let ch1 = content_text[pos * 2] as char;
                let ch2 = content_text[pos * 2 + 1] as char;
                let h1 = ch1
                    .to_digit(16)
                    .ok_or_else(|| RedLang::make_err(&format!("不能获得字节集类型:非法十六进制字符`{ch1}`")))?;
                let h2 = ch2
                    .to_digit(16)
                    .ok_or_else(|| RedLang::make_err(&format!("不能获得字节集类型:非法十六进制字符`{ch2}`")))?;
                content2.push(((h1 as u8) << 4) + h2 as u8);
            }
            Ok(RedValue::Bin(Rc::new(content2)))
        }
        "F" => {
            let body = s
                .get(marker_len + 1..)
                .ok_or("不能获得函数类型")?
                .to_string();
            let ast = astparser::parse_to_ast(&body).map_err(|e| RedLang::make_err(&e))?;
            Ok(RedValue::Fun(ast))
        }
        _ => Err(RedLang::make_err(&format!("错误的类型标识:`{}`", tp))),
    }
}
