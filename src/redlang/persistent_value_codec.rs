use std::{collections::BTreeMap, rc::Rc};

use super::{astparser, RedLang, RedValue};

pub fn to_persistent_legacy_string(value: &RedValue) -> Rc<String> {
    match value {
        RedValue::Text(s) => s.clone(),
        RedValue::Array(arr) => {
            let mut ret = String::new();
            ret.push_str(&crate::PERSISTENT_VALUE_MARKER);
            ret.push('A');
            for item in arr {
                let s = to_persistent_legacy_string(item);
                ret.push_str(&s.len().to_string());
                ret.push(',');
                ret.push_str(&s);
            }
            Rc::new(ret)
        }
        RedValue::Object(obj) => {
            let mut ret = String::new();
            ret.push_str(&crate::PERSISTENT_VALUE_MARKER);
            ret.push('O');
            for (k, v) in obj {
                ret.push_str(&k.len().to_string());
                ret.push(',');
                ret.push_str(k);
                let vs = to_persistent_legacy_string(v);
                ret.push_str(&vs.len().to_string());
                ret.push(',');
                ret.push_str(&vs);
            }
            Rc::new(ret)
        }
        RedValue::Bin(bin) => {
            let mut ret = String::new();
            ret.push_str(&crate::PERSISTENT_VALUE_MARKER);
            ret.push('B');
            for ch in bin.iter() {
                ret.push_str(&format!("{:02X}", ch));
            }
            Rc::new(ret)
        }
        RedValue::Fun(ast) => {
            let mut ret = String::new();
            ret.push_str(&crate::PERSISTENT_VALUE_MARKER);
            ret.push('F');
            ret.push_str(&astparser::ast_to_string(ast));
            Rc::new(ret)
        }
    }
}

pub fn from_persistent_legacy_string(s: &str) -> Result<RedValue, Box<dyn std::error::Error>> {
    let marker = crate::PERSISTENT_VALUE_MARKER.as_str();
    if !s.starts_with(marker) {
        return Ok(RedValue::Text(Rc::new(s.to_string())));
    }
    let marker_len = marker.len();
    let tp = s.get(marker_len..marker_len + 1).ok_or("类型解析错误,无类型标识")?;
    match tp {
        "A" => {
            let mut ret_arr: Vec<Rc<RedValue>> = vec![];
            let mut arr = s.get(marker_len + 1..).ok_or("不能获得数组类型")?;
            loop {
                let spos_opt = arr.find(',');
                if spos_opt.is_none() {
                    break;
                }
                let spos_num = spos_opt.unwrap();
                let num_str = arr.get(0..spos_num).ok_or("不能获得数组类型")?;
                let num = num_str.parse::<usize>()?;
                let str_val = arr.get(spos_num + 1..spos_num + 1 + num).ok_or("不能获得数组类型")?;
                ret_arr.push(Rc::new(from_persistent_legacy_string(str_val)?));
                arr = arr.get(spos_num + 1 + num..).ok_or("不能获得数组类型")?;
            }
            Ok(RedValue::Array(ret_arr))
        }
        "O" => {
            let mut ret_map: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
            let mut arr_strs: Vec<&str> = vec![];
            let mut arr = s.get(marker_len + 1..).ok_or("不能获得对象类型")?;
            loop {
                let spos_opt = arr.find(',');
                if spos_opt.is_none() {
                    break;
                }
                let spos_num = spos_opt.unwrap();
                let num_str = arr.get(0..spos_num).ok_or("不能获得对象类型")?;
                let num = num_str.parse::<usize>()?;
                let str_val = arr.get(spos_num + 1..spos_num + 1 + num).ok_or("不能获得对象类型")?;
                arr_strs.push(str_val);
                arr = arr.get(spos_num + 1 + num..).ok_or("不能获得对象类型")?;
            }
            if arr_strs.len() % 2 != 0 {
                return Err(RedLang::make_err("不能获得对象类型"));
            }
            for i in 0..(arr_strs.len() / 2) {
                let k = arr_strs[i * 2].to_string();
                let v = from_persistent_legacy_string(arr_strs[i * 2 + 1])?;
                ret_map.insert(k, Rc::new(v));
            }
            Ok(RedValue::Object(ret_map))
        }
        "B" => {
            let content_text = s.get(marker_len + 1..).ok_or("不能获得字节集类型")?.as_bytes();
            if content_text.len() % 2 != 0 {
                return Err(RedLang::make_err("不能获得字节集类型"));
            }
            let mut content2: Vec<u8> = vec![];
            for pos in 0..(content_text.len() / 2) {
                let mut ch1 = content_text[pos * 2];
                let mut ch2 = content_text[pos * 2 + 1];
                if ch1 < 0x3A { ch1 -= 0x30; } else { ch1 -= 0x41; ch1 += 10; }
                if ch2 < 0x3A { ch2 -= 0x30; } else { ch2 -= 0x41; ch2 += 10; }
                content2.push((ch1 << 4) + ch2);
            }
            Ok(RedValue::Bin(Rc::new(content2)))
        }
        "F" => {
            let body = s.get(marker_len + 1..).ok_or("不能获得函数类型")?.to_string();
            let ast = astparser::parse_to_ast(&body).map_err(|e| RedLang::make_err(&e))?;
            Ok(RedValue::Fun(ast))
        }
        _ => Err(RedLang::make_err(&format!("错误的类型标识:`{}`", tp))),
    }
}
