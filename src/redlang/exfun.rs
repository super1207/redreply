use std::{path::Path, time::SystemTime};

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
        let base64text = base64::encode(content);
        let mut ret_str = String::new();
        ret_str.push_str(&self_t.type_uuid);
        ret_str.push('B');
        ret_str.push_str(&base64text);
        return Ok(Some(ret_str));
    }else if cmd == "设置访问头"{
        let mut http_header = self_t.get_exmap("访问头")?.to_string();
        if http_header == "" {
            http_header.push_str(&self_t.type_uuid);
            http_header.push('O');
        }
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        http_header.push_str(&k.len().to_string());
        http_header.push(',');
        http_header.push_str(&k);
        http_header.push_str(&v.len().to_string());
        http_header.push(',');
        http_header.push_str(&v);
        self_t.set_exmap("访问头", &http_header)?;
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
        let mut ret_str = String::new();
        ret_str.push_str(&self_t.type_uuid);
        ret_str.push('A');
        for it in ret_vec {
            ret_str.push_str(&it.len().to_string());
            ret_str.push(',');
            ret_str.push_str(it);
        }
        return Ok(Some(ret_str))
    }else if cmd == "Json解析"{
        let json_str = self_t.get_param(params, 0)?;
        let json_data_ret:serde_json::Value = serde_json::from_str(&json_str)?;
        let json_parse_out = do_json_parse(&json_data_ret,&self_t.type_uuid)?;
        return Ok(Some(json_parse_out));
    }else if cmd == "读文件"{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        let content = std::fs::read(path)?;
        let mut ret_str = format!("{}B",self_t.type_uuid);
        ret_str.push_str(&base64::encode(content));
        return Ok(Some(ret_str));
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
        let mut sub_key_vec = String::new();
        sub_key_vec.push_str(&self_t.type_uuid);
        sub_key_vec.push('A');
        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            let mut temp_vec = String::new();
            temp_vec.push_str(&self_t.type_uuid);
            temp_vec.push('A');
            for i in 0..len {
                let s = cap.get(i).ok_or("regex cap访问越界")?.as_str();
                temp_vec.push_str(&s.len().to_string());
                temp_vec.push(',');
                temp_vec.push_str(s);
            }
            sub_key_vec.push_str(&temp_vec.len().to_string());
            sub_key_vec.push(',');
            sub_key_vec.push_str(&temp_vec);
        }
        return Ok(Some(sub_key_vec));
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
        let mut temp_bytes = String::new();
        temp_bytes.push_str(&self_t.type_uuid);
        temp_bytes.push('B');
        let s = base64::encode(str_vec);
        temp_bytes.push_str(&s);
        return Ok(Some(temp_bytes));
    }else if cmd == "BASE64编码"{
        let text = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&text)?;
        if tp != "字节集" {
            return Err(self_t.make_err(&("BASE64编码不支持的类型:".to_owned()+&tp)));
        }
        let b64_str = text.get(37..).ok_or("获取字节集失败")?;
        return Ok(Some(b64_str.to_string()));
    }else if cmd == "BASE64解码"{
        let b64_str = self_t.get_param(params, 0)?;
        let mut temp_bytes = String::new();
        temp_bytes.push_str(&self_t.type_uuid);
        temp_bytes.push('B');
        temp_bytes.push_str(&b64_str);
        return Ok(Some(temp_bytes));
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
    let mut ret_str = String::new();
    ret_str.push_str(self_uid);
    ret_str.push('O');
    for it in root.as_object().ok_or(err)? {
        let k = it.0;
        let v = it.1;
        ret_str.push_str(&k.len().to_string());
        ret_str.push(',');
        ret_str.push_str(&k);
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
        ret_str.push_str(&v_ret.len().to_string());
        ret_str.push(',');
        ret_str.push_str(&v_ret);
    }
    Ok(ret_str)
}

fn do_json_arr(self_uid: &str, root: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let err = "Json数组解析失败";
    let mut ret_str = String::new();
    ret_str.push_str(self_uid);
    ret_str.push('A');
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
        ret_str.push_str(&v_ret.len().to_string());
        ret_str.push(',');
        ret_str.push_str(&v_ret);
    }
    Ok(ret_str)
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