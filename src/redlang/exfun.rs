use std::{path::Path, io::Read, time::SystemTime, collections::HashMap};

use chrono::TimeZone;
use urlencoding::encode;
use base64;
use super::RedLang;

use crate::{redlang::cqexfun::cqexfun};


pub fn exfun(self_t:&mut RedLang,cmd: &str,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>> {
    
    let exret = cqexfun(self_t,cmd, params)?;
    if let Some(v) = exret{
        return Ok(Some(v));
    }
    if cmd == "访问" {
        let url = self_t.get_param(params, 0)?;
        let mut easy = curl::easy::Easy::new();
        easy.url(&url).unwrap();
        let mut header_list = curl::easy::List::new();
        header_list.append("User-Agent: Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36")?;
        let http_header_str = self_t.get_exmap("访问头")?;
        if http_header_str != "" {
            let http_header = self_t.parse_obj(&http_header_str)?;
            for it in http_header {
                header_list.append(&(it.0 + ": " + it.1))?;
            }
        }
        easy.http_headers(header_list)?;
        let mut content = Vec::new();
        {
            let mut transfer = easy.transfer();
            transfer.write_function(|data| {
                content.extend_from_slice(data);
                Ok(data.len())
            }).unwrap();
            transfer.perform()?;
        }
        return Ok(Some(self_t.build_bin(content)));
    }else if cmd == "POST访问" {
        let url = self_t.get_param(params, 0)?;
        let data_t = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&data_t)?;
        let data:Vec<u8>;
        if tp == "字节集" {
            data = self_t.parse_bin(&data_t)?;
        }else if tp == "文本" {
            data = data_t.as_bytes().to_vec();
        }else {
            return Err(self_t.make_err(&("不支持的post访问体类型:".to_owned()+&tp)));
        }
        let mut easy = curl::easy::Easy::new();
        easy.url(&url).unwrap();
        let mut header_list = curl::easy::List::new();
        header_list.append("User-Agent: Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36")?;
        let http_header_str = self_t.get_exmap("访问头")?;
        if http_header_str != "" {
            let http_header = self_t.parse_obj(&http_header_str)?;
            for it in http_header {
                header_list.append(&(it.0 + ": " + it.1))?;
            }
        }
        easy.http_headers(header_list)?;
        easy.post(true)?;
        easy.post_field_size(data.len() as u64).unwrap();
        let mut content = Vec::new();
        let mut dat = data.as_slice();
        {
            let mut transfer = easy.transfer();
            transfer.read_function(|buf| {
                Ok(dat.read(buf).unwrap_or(0))
            }).unwrap();
            transfer.write_function(|data| {
                content.extend_from_slice(data);
                Ok(data.len())
            }).unwrap();
            transfer.perform()?;
        }
        return Ok(Some(self_t.build_bin(content)));
    }else if cmd == "设置访问头"{
        let http_header = self_t.get_exmap("访问头")?.to_string();
        let mut http_header_map:HashMap<String, String> = HashMap::new();
        if http_header != "" {
            for (k,v) in self_t.parse_obj(&http_header)?{
                http_header_map.insert(k, v.to_string());
            }
        }
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        http_header_map.insert(k, v);
        self_t.set_exmap("访问头", &self_t.build_obj(http_header_map))?;
        return Ok(Some("".to_string()));
    }else if cmd == "编码" {
        let urlcode = self_t.get_param(params, 0)?;
        let encoded = encode(&urlcode);
        return Ok(Some(encoded.to_string()));
    }else if cmd == "随机取"{
        let arr_data = self_t.get_param(params, 0)?;
        let arr = self_t.parse_arr(&arr_data)?;
        if arr.len() == 0 {
            return Ok(Some(self_t.get_param(params, 1)?));
        }
        let index = self_t.parse(&format!("【取随机数@0@{}】",arr.len() - 1))?.parse::<usize>()?;
        let ret = arr.get(index).ok_or("数组下标越界")?;
        return Ok(Some(ret.to_string()))
    }else if cmd == "取中间"{
        let s = self_t.get_param(params, 0)?;
        let sub_begin = self_t.get_param(params, 1)?;
        let sub_end = self_t.get_param(params, 2)?;
        let ret_vec = get_mid(&s, &sub_begin, &sub_end)?;
        let mut ret_str:Vec<String> = vec![];
        for it in ret_vec {
            ret_str.push(it.to_string());
        }
        return Ok(Some(self_t.build_arr(ret_str)))
    }else if cmd == "Json解析"{
        let json_str = self_t.get_param(params, 0)?;
        let json_data_ret:serde_json::Value = serde_json::from_str(&json_str)?;
        let json_parse_out = do_json_parse(&json_data_ret,&self_t.type_uuid)?;
        return Ok(Some(json_parse_out));
    }else if cmd == "读文件"{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        let content = std::fs::read(path)?;
        return Ok(Some(self_t.build_bin(content)));
    }else if cmd == "分割"{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let split_ret:Vec<&str> = data_str.split(&sub_str).collect();
        let mut ret_str = format!("{}A",self_t.type_uuid);
        for it in split_ret {
            ret_str.push_str(&it.len().to_string());
            ret_str.push(',');
            ret_str.push_str(it);
        }
        return Ok(Some(ret_str));
    }else if cmd == "判含"{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&data_str)?;
        if tp == "文本" {
            if !data_str.contains(&sub_str){
                return Ok(Some(self_t.get_param(params, 2)?));
            }else{
                return Ok(Some(self_t.get_param(params, 3)?));
            }
        }else if tp == "数组" {
            let mut ret_str = format!("{}A",self_t.type_uuid);
            for it in self_t.parse_arr(&data_str)? {
                if it.contains(&sub_str){
                    ret_str.push_str(&it.len().to_string());
                    ret_str.push(',');
                    ret_str.push_str(it);
                }
            }
            return Ok(Some(ret_str)); 
        }else{
            return Err(self_t.make_err(&("对应类型不能使用判含:".to_owned()+&tp)));
        }
    }else if cmd == "正则"{
        let data_str = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param(params, 1)?;
        let re = fancy_regex::Regex::new(&sub_str)?;
        let mut sub_key_vec:Vec<String> = vec![];
        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            let mut temp_vec:Vec<String> = vec![];
            for i in 0..len {
                let s = cap.get(i).ok_or("regex cap访问越界")?.as_str();
                temp_vec.push(s.to_string());
            }
            sub_key_vec.push(self_t.build_arr(temp_vec));
        }
        return Ok(Some(self_t.build_arr(sub_key_vec)));
    }else if cmd == "转字节集"{
        let text = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&text)?;
        if tp != "文本" {
            return Err(self_t.make_err(&("转字节集不支持的类型:".to_owned()+&tp)));
        }
        let code_t = self_t.get_param(params, 1)?;
        let code = code_t.to_lowercase();
        let str_vec:Vec<u8>;
        if code == "" || code == "utf8" {
            str_vec = text.as_bytes().to_vec();
        }else if code == "gbk" {
            str_vec = encoding::Encoding::encode(encoding::all::GBK, &text, encoding::EncoderTrap::Ignore)?;
        }else{
            return Err(self_t.make_err(&("不支持的编码:".to_owned()+&code_t)));
        }
        return Ok(Some(self_t.build_bin(str_vec)));
    }else if cmd.to_uppercase() == "BASE64编码"{
        let text = self_t.get_param(params, 0)?;
        let bin = self_t.parse_bin(&text)?;
        let b64_str = base64::encode(bin);
        return Ok(Some(b64_str));
    }else if cmd.to_uppercase() == "BASE64解码"{
        let b64_str = self_t.get_param(params, 0)?;
        let content = base64::decode(b64_str)?;
        return Ok(Some(self_t.build_bin(content)));
    }else if cmd == "延时"{
        let mill = self_t.get_param(params, 0)?.parse::<u64>()?;
        let time_struct = core::time::Duration::from_millis(mill);
        std::thread::sleep(time_struct);
        return Ok(Some("".to_string()));
    }else if cmd == "序号"{
        if params.len() == 0 {
            let retnum = self_t.xuhao;
            self_t.xuhao += 1;
            return Ok(Some(retnum.to_string()));
        }
        let num = self_t.get_param(params, 0)?.parse::<usize>()?;
        self_t.xuhao = num;
        return Ok(Some(num.to_string()));
    }else if cmd == "时间戳" || cmd == "10位时间戳"{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_secs().to_string()));
    }else if cmd == "13位时间戳"{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_millis().to_string()));
    }else if cmd == "时间戳转文本"{
        let numstr = self_t.get_param(params, 0)?;
        let num = numstr.parse::<i64>()?;
        let datetime = chrono::prelude::Local.timestamp(num, 0);
        let newdate = datetime.format("%Y-%m-%d-%H-%M-%S");
        return Ok(Some(format!("{}",newdate)));
    }
    return Ok(None);
}


pub fn do_json_parse(json_val:&serde_json::Value,self_uid:&str) ->Result<String, Box<dyn std::error::Error>> {
    let err_str = "Json解析失败";
    if json_val.is_string() {
        return Ok(json_val.as_str().ok_or(err_str)?.to_string());
    }
    if json_val.is_object() {
        return Ok(do_json_obj(self_uid,&json_val)?);
    } 
    if json_val.is_array() {
        return Ok(do_json_obj(&self_uid,&json_val)?);
    }
    Err(None.ok_or(err_str)?)
}

fn do_json_string(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json字符串解析失败";
    return Ok(root.as_str().ok_or(err)?.to_string());
}

fn do_json_bool(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json布尔解析失败";
    let v_ret:String;
    if root.as_bool().ok_or(err)? {
        v_ret = "真".to_string();
    }else{
        v_ret = "假".to_string();
    }
    return Ok(v_ret);
}

fn do_json_number(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json数字解析失败";
    let v_ret:String;
    if root.is_u64() {
        v_ret = root.as_u64().ok_or(err)?.to_string();
    }else if root.is_i64() {
        v_ret = root.as_i64().ok_or(err)?.to_string();
    }else if root.is_f64() {
        v_ret = root.as_f64().ok_or(err)?.to_string();
    }else {
        return None.ok_or("不支持的Json类型")?;
    }
    return Ok(v_ret);
}

fn do_json_obj(self_uid:&str,root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json对象解析失败";
    let mut ret_str:HashMap<String,String> = HashMap::new();
    for it in root.as_object().ok_or(err)? {
        let k = it.0;
        let v = it.1;
        let v_ret:String;
        if v.is_string() {
            v_ret = do_json_string(v)?;
        } else if v.is_boolean() {
            v_ret = do_json_bool(v)?;
        }else if v.is_number() {
            v_ret = do_json_number(v)?
        }else if v.is_null() {
            v_ret = "".to_string();
        }else if v.is_object() {
            v_ret = do_json_obj(self_uid,v)?;
        }else if v.is_array() {
            v_ret = do_json_arr(self_uid,v)?;
        }else{
            return None.ok_or("不支持的Json类型")?;
        }
        ret_str.insert(k.to_string(), v_ret);
    }
    Ok(RedLang::build_obj_with_uid(self_uid, ret_str))
}

fn do_json_arr(self_uid: &str, root: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json数组解析失败";
    let mut ret_str:Vec<String> = vec![];
    for v in root.as_array().ok_or(err)? {
        let v_ret:String;
        if v.is_string() {
            v_ret = do_json_string(v)?;
        } else if v.is_boolean() {
            v_ret = do_json_bool(v)?;
        }else if v.is_number() {
            v_ret = do_json_number(v)?
        }else if v.is_null() {
            v_ret = "".to_string();
        }else if v.is_object() {
            v_ret = do_json_obj(self_uid,v)?;
        }else if v.is_array() {
            v_ret = do_json_arr(self_uid,v)?;
        }else{
            return None.ok_or("不支持的Json类型")?;
        }
        ret_str.push(v_ret);
    }
    Ok(RedLang::build_arr_with_uid(self_uid, ret_str))
}

fn get_mid<'a>(s:&'a str,sub_begin:&str,sub_end:&str) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
    let mut ret_vec:Vec<&str> = vec![];
    let mut s_pos = s;
    let err_str = "get_mid err";
    loop {
        let pos = s_pos.find(sub_begin);
        if let Some(pos_num) = pos {
            s_pos = s_pos.get((pos_num+sub_begin.len())..).ok_or(err_str)?;
            let pos_end = s_pos.find(sub_end);
            if let Some(pos_end_num) = pos_end {
                let val = s_pos.get(..pos_end_num).ok_or(err_str)?;
                ret_vec.push(val);
                s_pos = s_pos.get((pos_end_num+sub_end.len())..).ok_or(err_str)?;
            }else{
                break;
            }
        }else{
            break;
        }
    }
    return Ok(ret_vec);
}