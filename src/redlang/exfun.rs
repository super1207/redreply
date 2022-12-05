use std::{path::Path, io::Read, time::{SystemTime, Duration}, collections::HashMap};

use chrono::TimeZone;
use md5::{Md5, Digest};
use urlencoding::encode;
use base64;
use super::RedLang;

use crate::{redlang::cqexfun::cqexfun};

use image::{Rgba, ImageBuffer};
use imageproc::geometric_transformations::{Projection, warp_with, rotate_about_center};
use std::io::Cursor;
use image::io::Reader as ImageReader;
use imageproc::geometric_transformations::{Interpolation};


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
    }else if cmd == "运行目录"{
        let exe_dir = std::env::current_exe()?;
        let exe_path = exe_dir.parent().ok_or("无法获得运行目录")?;
        let exe_path_str = exe_path.to_string_lossy().to_string() + "\\";
        return Ok(Some(exe_path_str));
    }
    else if cmd == "分割"{
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
        let datetime_rst = chrono::prelude::Local.timestamp_opt(num, 0);
        if let chrono::LocalResult::Single(datetime) = datetime_rst {
            let newdate = datetime.format("%Y-%m-%d-%H-%M-%S");
            return Ok(Some(format!("{}",newdate)));
        }
        return Ok(Some("".to_string()));
    }else if cmd.to_uppercase() == "MD5编码"{
        let text = self_t.get_param(params, 0)?;
        let bin = self_t.parse_bin(&text)?;
        let mut hasher = Md5::new();
        hasher.update(bin);
        let result = hasher.finalize();
        let mut content = String::new();
        for ch in result {
            content.push_str(&format!("{:02x}",ch));
        }
        return Ok(Some(content));
    }else if cmd.to_uppercase() == "RCNB编码"{
        let text = self_t.get_param(params, 0)?;
        let bin = self_t.parse_bin(&text)?;
        let content = rcnb_rs::encode(bin);
        return Ok(Some(content));
    }else if cmd == "图片信息" || cmd == "图像信息"{
        let text = self_t.get_param(params, 0)?;
        let img_bin = self_t.parse_bin(&text)?;
        let img = ImageReader::new(Cursor::new(img_bin)).with_guessed_format()?.decode()?.to_rgba8();
        let mut mp = HashMap::new();
        mp.insert("宽".to_string(), img.width().to_string());
        mp.insert("高".to_string(), img.height().to_string());
        let retobj = self_t.build_obj(mp);
        return Ok(Some(retobj));
    }else if cmd == "透视变换"{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let img_bin = self_t.parse_bin(&text1)?;
        let dst_t = self_t.parse_arr(&text2)?;
        let img = ImageReader::new(Cursor::new(img_bin)).with_guessed_format()?.decode()?.to_rgba8();
        let img_width_str = img.width().to_string();
        let img_height_str = img.height().to_string();
        let src_t:Vec<&str>;
        if text3 == "" {
            src_t = vec!["0","0",&img_width_str,"0",&img_width_str,&img_height_str,"0",&img_width_str];
        }else{
            src_t = self_t.parse_arr(&text3)?;
        }
        if dst_t.len() != 8 || src_t.len() != 8 {
            return Err(self_t.make_err("透视变换参数错误1"));
        }
        fn cv(v:Vec<&str>) -> Result<[(f32,f32);4], Box<dyn std::error::Error>> {
            let v_ret = [
                (v[0].parse::<f32>()?,v[1].parse::<f32>()?),
                (v[2].parse::<f32>()?,v[3].parse::<f32>()?),
                (v[4].parse::<f32>()?,v[5].parse::<f32>()?),
                (v[6].parse::<f32>()?,v[7].parse::<f32>()?)
            ];
            return Ok(v_ret);
        }
        let dst = cv(dst_t)?;
        let src = cv(src_t)?;
        let p = Projection::from_control_points(src, dst).ok_or("Could not compute projection matrix")?.invert();
        let mut img2 = warp_with(
            &img,
            |x, y| p * (x, y),
            Interpolation::Bilinear,
            Rgba([0,0,0,0]),
        );
        fn m_min(v:Vec<f32>) -> f32 {
            if v.len() == 0 {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i < m {
                    m = i;
                }
            }
            m
        }
        fn m_max(v:Vec<f32>) -> f32 {
            if v.len() == 0 {
                return 0f32;
            }
            let mut m = v[0];
            for i in v {
                if i > m {
                    m = i;
                }
            }
            m
        }
        let x_min = m_min(vec![dst[0].0,dst[1].0,dst[2].0,dst[3].0]);
        let x_max = m_max(vec![dst[0].0,dst[1].0,dst[2].0,dst[3].0]);
        let y_min = m_min(vec![dst[0].1,dst[1].1,dst[2].1,dst[3].1]);
        let y_max = m_max(vec![dst[0].1,dst[1].1,dst[2].1,dst[3].1]);
        let img_out = image::imageops::crop(&mut img2,x_min as u32,y_min as u32,(x_max - x_min) as u32,(y_max - y_min) as u32);
        let mm = img_out.to_image();
        let mut bytes: Vec<u8> = Vec::new();
        mm.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd == "图片叠加" || cmd == "图像叠加"{
        fn img_paste(img_vec_big:Vec<u8>,img_vec_sub:Vec<u8>,x:i64,y:i64) -> Result<Vec<u8>, Box<dyn std::error::Error>>{
            let img1 = ImageReader::new(Cursor::new(img_vec_big)).with_guessed_format()?.decode()?.to_rgba8();
            let img2 = ImageReader::new(Cursor::new(img_vec_sub)).with_guessed_format()?.decode()?.to_rgba8();
            let w = img1.width();
            let h = img1.height();
            let mut img:ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
            image::imageops::overlay(&mut img, &img2, x, y);
            image::imageops::overlay(&mut img, &img1, 0, 0);
            let mut bytes: Vec<u8> = Vec::new();
            img.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
            Ok(bytes)
        }
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let img_vec_big = self_t.parse_bin(&text1)?;
        let img_vec_sub = self_t.parse_bin(&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        let img_out = img_paste(img_vec_big,img_vec_sub,x,y)?;
        let ret = self_t.build_bin(img_out);
        return Ok(Some(ret));
    }else if cmd.to_uppercase() == "GIF合成"{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let delay = text2.parse::<u64>()?;
        let img_arr_str = self_t.parse_arr(&text1)?;
        let mut frame_vec:Vec<image::Frame> = vec![];
        for it in img_arr_str {
            let img_bin = self_t.parse_bin(it)?;
            let img = ImageReader::new(Cursor::new(img_bin)).with_guessed_format()?.decode()?.to_rgba8();
            let fm = image::Frame::from_parts(img, 0, 0, image::Delay::from_saturating_duration(Duration::from_millis(delay)));
            frame_vec.push(fm);
        }
        let mut v:Vec<u8> = vec![];
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut v);
            encoder.encode_frames(frame_vec)?;
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
        }
        let ret = self_t.build_bin(v);
        return Ok(Some(ret));
    }else if cmd == "图片变圆" || cmd == "图像变圆" {
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let mut img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let width = img.width();
        let height = img.height();
        let r:u32;
        if width < height {
            r = width / 2;
        }else{
            r = height / 2;
        }
        for x in 0..width {
            for y in 0..height {
                if (x - r)*(x - r) + (y - r)*(y - r) > r * r {
                    let mut pix = img.get_pixel_mut(x, y);
                    pix.0[3] = 0;
                }
            }
        }
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd == "图片变灰" || cmd == "图像变灰"{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let mut img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let width = img.width();
        let height = img.height();
        for x in 0..width {
            for y in 0..height {
                let mut pix = img.get_pixel_mut(x, y);
                let red = pix.0[0] as f32  * 0.3;
                let green = pix.0[1] as f32  * 0.589;
                let blue = pix.0[2] as f32  * 0.11;
                let color = (red + green + blue) as u8;
                pix.0[0] = color;
                pix.0[1] = color;
                pix.0[2] = color;
            }
        }
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd.to_uppercase() == "水平翻转"{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::flip_horizontal(&img);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd.to_uppercase() == "垂直翻转"{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::flip_vertical(&img);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd == "图像旋转" || cmd == "图片旋转"{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = rotate_about_center(&img,theta,Interpolation::Bilinear,Rgba([0,0,0,0]));
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    }else if cmd == "图像大小调整" || cmd == "图片大小调整"{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let img_vec = self_t.parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::resize(&img, text2.parse::<u32>()?, text3.parse::<u32>()?, image::imageops::FilterType::Nearest);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
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