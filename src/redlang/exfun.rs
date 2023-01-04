use std::{path::Path, io::Read, time::{SystemTime, Duration}, collections::BTreeMap, vec, fs};

use chrono::TimeZone;
use headless_chrome::Browser;
use md5::{Md5, Digest};
use urlencoding::encode;
use base64;
use super::RedLang;

use crate::cqapi::cq_add_log;

use image::{Rgba, ImageBuffer, EncodableLayout};
use imageproc::geometric_transformations::{Projection, warp_with, rotate_about_center};
use std::io::Cursor;
use image::io::Reader as ImageReader;
use imageproc::geometric_transformations::Interpolation;

pub fn init_ex_fun_map() {
    fn add_fun(k_vec:Vec<&str>,fun:fn(&mut RedLang,params: &[String]) -> Result<Option<String>, Box<dyn std::error::Error>>){
        let mut w = crate::G_CMD_FUN_MAP.write().unwrap();
        for it in k_vec {
            let k = it.to_string();
            if w.contains_key(&k) {
                let err_opt:Option<String> = None;
                err_opt.ok_or(&format!("不可以重复添加命令:{}",k)).unwrap();
            }
            w.insert(k, fun);
        }
    }
    add_fun(vec!["访问"],|self_t,params|{
        let url = self_t.get_param(params, 0)?;
        let mut easy = curl::easy::Easy::new();
        easy.url(&url)?;
        easy.ssl_verify_peer(false)?;
        easy.follow_location(true)?;
        let proxy = self_t.get_coremap("代理")?;
        if proxy != "" {
            easy.proxy(proxy)?;
        }
        let mut header_list = curl::easy::List::new();
        let http_header_str = self_t.get_coremap("访问头")?;
        if http_header_str != "" {
            let mut http_header = RedLang::parse_obj(&http_header_str)?;
            if !http_header.contains_key("User-Agent"){
                http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            for it in http_header {
                if it.1 != "" {
                    header_list.append(&(it.0 + ": " + &it.1))?;
                }
            }
        }else {
            header_list.append("User-Agent: Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36")?;
        }
        easy.http_headers(header_list)?;
        let mut content = Vec::new();
        {
            let mut transfer = easy.transfer();
            transfer.write_function(|data| {
                content.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["POST访问"],|self_t,params|{
        let url = self_t.get_param(params, 0)?;
        let data_t = self_t.get_param(params, 1)?;
        let tp = self_t.get_type(&data_t)?;
        let data:Vec<u8>;
        if tp == "字节集" {
            data = RedLang::parse_bin(&data_t)?;
        }else if tp == "文本" {
            data = data_t.as_bytes().to_vec();
        }else {
            return Err(RedLang::make_err(&("不支持的post访问体类型:".to_owned()+&tp)));
        }
        let mut easy = curl::easy::Easy::new();
        easy.url(&url)?;
        easy.ssl_verify_peer(false)?;
        easy.follow_location(true)?;
        let proxy = self_t.get_coremap("代理")?;
        if proxy != "" {
            easy.proxy(proxy)?;
        }
        let mut header_list = curl::easy::List::new();
        let http_header_str = self_t.get_coremap("访问头")?;
        if http_header_str != "" {
            let mut http_header = RedLang::parse_obj(&http_header_str)?;
            if !http_header.contains_key("User-Agent"){
                http_header.insert("User-Agent".to_string(),"Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            for it in http_header {
                if it.1 != "" {
                    header_list.append(&(it.0 + ": " + &it.1))?;
                }
            }
        }else {
            header_list.append("User-Agent: Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36")?;
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
            })?;
            transfer.write_function(|data| {
                content.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["设置访问头"],|self_t,params|{
        let http_header = self_t.get_coremap("访问头")?.to_string();
        let mut http_header_map:BTreeMap<String, String> = BTreeMap::new();
        if http_header != "" {
            for (k,v) in RedLang::parse_obj(&http_header)?{
                http_header_map.insert(k, v.to_string());
            }
        }
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        http_header_map.insert(k, v);
        self_t.set_coremap("访问头", &self_t.build_obj(http_header_map))?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["设置代理"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        self_t.set_coremap("代理", &k)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["编码"],|self_t,params|{
        let urlcode = self_t.get_param(params, 0)?;
        let encoded = encode(&urlcode);
        return Ok(Some(encoded.to_string()));
    });
    add_fun(vec!["随机取"],|self_t,params|{
        let arr_data = self_t.get_param(params, 0)?;
        let arr = RedLang::parse_arr(&arr_data)?;
        if arr.len() == 0 {
            return Ok(Some(self_t.get_param(params, 1)?));
        }
        let index = self_t.parse(&format!("【取随机数@0@{}】",arr.len() - 1))?.parse::<usize>()?;
        let ret = arr.get(index).ok_or("数组下标越界")?;
        return Ok(Some(ret.to_string()))
    });
    add_fun(vec!["取中间"],|self_t,params|{
        let s = self_t.get_param(params, 0)?;
        let sub_begin = self_t.get_param(params, 1)?;
        let sub_end = self_t.get_param(params, 2)?;
        let ret_vec = get_mid(&s, &sub_begin, &sub_end)?;
        let mut ret_str:Vec<String> = vec![];
        for it in ret_vec {
            ret_str.push(it.to_string());
        }
        return Ok(Some(self_t.build_arr(ret_str)))
    });
    add_fun(vec!["截取"],|self_t,params|{
        let content = self_t.get_param(params, 0)?;
        let begin = self_t.get_param(params, 1)?;
        let len = self_t.get_param(params, 2)?;
        let tp = self_t.get_type(&content)?;
        let ret:String;
        if tp == "文本" {
            let chs = content.chars().collect::<Vec<char>>();
            let begen_pos = begin.parse::<usize>()?;
            let sub_len:usize;
            if len == "" {
                sub_len = chs.len() - begen_pos;
            }else{
                sub_len = len.parse::<usize>()?;
            }
            let mut end_pos = begen_pos+sub_len;
            if end_pos > chs.len() {
                end_pos = chs.len();
            }
            ret = match chs.get(begen_pos..end_pos) {
                Some(value) => value.iter().collect::<String>(),
                None => "".to_string()
            };
        }else if tp == "数组" {
            let arr = RedLang::parse_arr(&content)?;
            let begen_pos = begin.parse::<usize>()?;
            let sub_len:usize;
            if len == "" {
                sub_len = arr.len() - begen_pos;
            }else{
                sub_len = len.parse::<usize>()?;
            }
            let mut end_pos = begen_pos+sub_len;
            if end_pos > arr.len() {
                end_pos = arr.len();
            }
            ret = match arr.get(begen_pos..end_pos) {
                Some(value) => {
                    let mut array:Vec<String> = vec![];
                    for it in value {
                        array.push(it.to_string());
                    }
                    self_t.build_arr(array)
                },
                None => self_t.build_arr(vec![])
            };
        }
        else{
            return Err(RedLang::make_err("截取命令目前仅支持文本或数组"));
        }
        
        return Ok(Some(ret))
    });
    add_fun(vec!["JSON解析"],|self_t,params|{
        let json_str = self_t.get_param(params, 0)?;
        let json_data_ret:serde_json::Value = serde_json::from_str(&json_str)?;
        let json_parse_out = do_json_parse(&json_data_ret,&self_t.type_uuid)?;
        return Ok(Some(json_parse_out));
    });
    add_fun(vec!["读文件"],|self_t,params|{
        let file_path = self_t.get_param(params, 0)?;
        let path = Path::new(&file_path);
        if !path.exists() {
            return Ok(Some(self_t.build_bin(vec![])));
        }
        let content = std::fs::read(path)?;
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["运行目录"],|_self_t,_params|{
        let exe_dir = std::env::current_exe()?;
        let exe_path = exe_dir.parent().ok_or("无法获得运行目录")?;
        let exe_path_str = exe_path.to_string_lossy().to_string() + "\\";
        return Ok(Some(exe_path_str));
    });
    add_fun(vec!["分割"],|self_t,params|{
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
    });
    add_fun(vec!["判含"],|self_t,params|{
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
            for it in RedLang::parse_arr(&data_str)? {
                if it.contains(&sub_str){
                    ret_str.push_str(&it.len().to_string());
                    ret_str.push(',');
                    ret_str.push_str(it);
                }
            }
            return Ok(Some(ret_str)); 
        }else{
            return Err(RedLang::make_err(&("对应类型不能使用判含:".to_owned()+&tp)));
        }
    });
    add_fun(vec!["正则"],|self_t,params|{
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
    });
    add_fun(vec!["转字节集"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let tp = self_t.get_type(&text)?;
        if tp != "文本" {
            return Err(RedLang::make_err(&("转字节集不支持的类型:".to_owned()+&tp)));
        }
        let code_t = self_t.get_param(params, 1)?;
        let code = code_t.to_lowercase();
        let str_vec:Vec<u8>;
        if code == "" || code == "utf8" {
            str_vec = text.as_bytes().to_vec();
        }else if code == "gbk" {
            str_vec = encoding::Encoding::encode(encoding::all::GBK, &text, encoding::EncoderTrap::Ignore)?;
        }else{
            return Err(RedLang::make_err(&("不支持的编码:".to_owned()+&code_t)));
        }
        return Ok(Some(self_t.build_bin(str_vec)));
    });
    add_fun(vec!["BASE64编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&text)?;
        let b64_str = base64::encode(bin);
        return Ok(Some(b64_str));
    });
    add_fun(vec!["BASE64解码"],|self_t,params|{
        let b64_str = self_t.get_param(params, 0)?;
        let content = base64::decode(b64_str)?;
        return Ok(Some(self_t.build_bin(content)));
    });
    add_fun(vec!["延时"],|self_t,params|{
        let mill = self_t.get_param(params, 0)?.parse::<u64>()?;
        let time_struct = core::time::Duration::from_millis(mill);
        std::thread::sleep(time_struct);
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["序号"],|self_t,params|{
        let k = self_t.get_param(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        if v != "" {
            // 说明是设置序号
            self_t.xuhao.insert(k.to_owned(), v.parse::<usize>()?);
            return Ok(Some("".to_string()));
        }else {
            // 说明是取序号
            let ret:usize;
            if self_t.xuhao.contains_key(&k) {
                let x = self_t.xuhao.get_mut(&k).unwrap();
                ret = *x;
                *x += 1;
            }else {
                self_t.xuhao.insert(k.to_owned(), 1);
                ret = 0;
            }
            return Ok(Some(ret.to_string()));
        }
    });
    add_fun(vec!["时间戳","10位时间戳"],|_self_t,_params|{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_secs().to_string()));
    });
    add_fun(vec!["13位时间戳"],|_self_t,_params|{
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        return Ok(Some(tm.as_millis().to_string()));
    });
    add_fun(vec!["时间戳转文本"],|self_t,params|{
        let numstr = self_t.get_param(params, 0)?;
        let num = numstr.parse::<i64>()?;
        let datetime_rst = chrono::prelude::Local.timestamp_opt(num, 0);
        if let chrono::LocalResult::Single(datetime) = datetime_rst {
            let newdate = datetime.format("%Y-%m-%d-%H-%M-%S");
            return Ok(Some(format!("{}",newdate)));
        }
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["MD5编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&text)?;
        let mut hasher = Md5::new();
        hasher.update(bin);
        let result = hasher.finalize();
        let mut content = String::new();
        for ch in result {
            content.push_str(&format!("{:02x}",ch));
        }
        return Ok(Some(content));
    });
    add_fun(vec!["RCNB编码"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let bin = RedLang::parse_bin(&text)?;
        let content = rcnb_rs::encode(bin);
        return Ok(Some(content));
    });
    add_fun(vec!["图片信息","图像信息"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let img_bin = RedLang::parse_bin(&text)?;
        let img = ImageReader::new(Cursor::new(img_bin)).with_guessed_format()?.decode()?.to_rgba8();
        let mut mp = BTreeMap::new();
        mp.insert("宽".to_string(), img.width().to_string());
        mp.insert("高".to_string(), img.height().to_string());
        let retobj = self_t.build_obj(mp);
        return Ok(Some(retobj));
    });
    add_fun(vec!["透视变换"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let img_bin = RedLang::parse_bin(&text1)?;
        let dst_t = RedLang::parse_arr(&text2)?;
        let img = ImageReader::new(Cursor::new(img_bin)).with_guessed_format()?.decode()?.to_rgba8();
        let img_width_str = img.width().to_string();
        let img_height_str = img.height().to_string();
        let src_t:Vec<&str>;
        if text3 == "" {
            src_t = vec!["0","0",&img_width_str,"0",&img_width_str,&img_height_str,"0",&img_width_str];
        }else{
            src_t = RedLang::parse_arr(&text3)?;
        }
        if dst_t.len() != 8 || src_t.len() != 8 {
            return Err(RedLang::make_err("透视变换参数错误1"));
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
    });
    add_fun(vec!["图片叠加","图像叠加"],|self_t,params|{
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
        let img_vec_big = RedLang::parse_bin(&text1)?;
        let img_vec_sub = RedLang::parse_bin(&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        let img_out = img_paste(img_vec_big,img_vec_sub,x,y)?;
        let ret = self_t.build_bin(img_out);
        return Ok(Some(ret));
    });
    add_fun(vec!["图片上叠加","图像上叠加"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let text4 = self_t.get_param(params, 3)?;
        let img_vec_big = RedLang::parse_bin(&text1)?;
        let img_vec_sub = RedLang::parse_bin(&text2)?;
        let x = text3.parse::<i64>()?;
        let y = text4.parse::<i64>()?;
        let mut img_big = ImageReader::new(Cursor::new(img_vec_big)).with_guessed_format()?.decode()?.to_rgba8();
        let img_sub = ImageReader::new(Cursor::new(img_vec_sub)).with_guessed_format()?.decode()?.to_rgba8();
        image::imageops::overlay(&mut img_big, &img_sub, x, y);
        let mut bytes: Vec<u8> = Vec::new();
        img_big.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    });
    add_fun(vec!["GIF合成"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let delay = text2.parse::<u64>()?;
        let img_arr_str = RedLang::parse_arr(&text1)?;
        let mut frame_vec:Vec<image::Frame> = vec![];
        for it in img_arr_str {
            let img_bin = RedLang::parse_bin(it)?;
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
    });
    add_fun(vec!["图片变圆","图像变圆"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = RedLang::parse_bin(&text1)?;
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
    });
    add_fun(vec!["图片变灰","图像变灰"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = RedLang::parse_bin(&text1)?;
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
    });
    add_fun(vec!["水平翻转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = RedLang::parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::flip_horizontal(&img);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    });
    add_fun(vec!["垂直翻转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let img_vec = RedLang::parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::flip_vertical(&img);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    });
    add_fun(vec!["图像旋转","图片旋转"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let img_vec = RedLang::parse_bin(&text1)?;
        let theta = text2.parse::<f32>()? / 360.0 * (2.0 * std::f32::consts::PI);
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = rotate_about_center(&img,theta,Interpolation::Bilinear,Rgba([0,0,0,0]));
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    });
    add_fun(vec!["图像大小调整","图片大小调整"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        let text2 = self_t.get_param(params, 1)?;
        let text3 = self_t.get_param(params, 2)?;
        let img_vec = RedLang::parse_bin(&text1)?;
        let img = ImageReader::new(Cursor::new(img_vec)).with_guessed_format()?.decode()?.to_rgba8();
        let img_out = image::imageops::resize(&img, text2.parse::<u32>()?, text3.parse::<u32>()?, image::imageops::FilterType::Nearest);
        let mut bytes: Vec<u8> = Vec::new();
        img_out.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Png)?;
        let ret = self_t.build_bin(bytes);
        return Ok(Some(ret));
    });
    add_fun(vec!["转大写"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        return Ok(Some(text1.to_uppercase()));
    });
    add_fun(vec!["转小写"],|self_t,params|{
        let text1 = self_t.get_param(params, 0)?;
        return Ok(Some(text1.to_lowercase()));
    });
    add_fun(vec!["打印日志"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        cq_add_log(&text).unwrap();
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["读目录"],|self_t,params|{
        let dir_name = self_t.get_param(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec:Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            let file_name = path.to_str().ok_or("获取目录文件异常")?;
            if path.is_dir() {
                ret_vec.push(format!("{}{}",file_name,std::path::MAIN_SEPARATOR));
            }else{
                ret_vec.push(file_name.to_string());
            }
            
        }
        let ret = self_t.build_arr(ret_vec);
        return Ok(Some(ret));
    });
    add_fun(vec!["读目录文件"],|self_t,params|{
        let dir_name = self_t.get_param(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec:Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            if path.is_file() {
                let file_name = path.to_str().ok_or("获取目录文件异常")?;
                ret_vec.push(file_name.to_string());
            }
        }
        let ret = self_t.build_arr(ret_vec);
        return Ok(Some(ret));
    });
    add_fun(vec!["目录分隔符"],|_self_t,_params|{
        return Ok(Some(std::path::MAIN_SEPARATOR.to_string()));
    });
    add_fun(vec!["去除开始空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim_start().to_string()));
    });
    add_fun(vec!["去除结尾空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim_end().to_string()));
    });
    add_fun(vec!["去除两边空白"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        return Ok(Some(text.trim().to_string()));
    });
    add_fun(vec!["数字转字符"],|self_t,params|{
        let text = self_t.get_param(params, 0)?;
        let num = text.parse::<u8>()?;
        if num > 127 || num < 1 {
            return Err(RedLang::make_err("在数字转字符中发生越界"));
        }
        return Ok(Some((num as char).to_string()));
    });
    add_fun(vec!["创建目录"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        fs::create_dir_all(path)?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["写文件"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let bin_data = self_t.get_param(params, 1)?;
        let parent_path = Path::new(&path).parent().ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        let mut f = fs::File::create(path)?;
        let bin = RedLang::parse_bin(&bin_data)?;
        std::io::Write::write_all(&mut f, bin.as_bytes())?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["追加文件"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let bin_data = self_t.get_param(params, 1)?;
        let parent_path = Path::new(&path).parent().ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        let mut f;
        if Path::new(&path).exists() {
            f = fs::OpenOptions::new().append(true).open(path)?
        }else {
            f = fs::File::create(path)?;
        }
        let bin = RedLang::parse_bin(&bin_data)?;
        std::io::Write::write_all(&mut f, bin.as_bytes())?;
        return Ok(Some("".to_string()));
    });
    add_fun(vec!["网页截图"],|self_t,params|{
        let path = self_t.get_param(params, 0)?;
        let sec = self_t.get_param(params, 1)?;
        let options = headless_chrome::LaunchOptions::default_builder()
            .window_size(Some((1920, 1080)))
            .build()?;
            let browser = Browser::new(options)?;
            let tab = browser.wait_for_initial_tab()?;
            tab.navigate_to(&path)?.wait_until_navigated()?;
        let el_html= tab.wait_for_element("html")?;
        let body_height = el_html.get_box_model()?.height;
        let body_width = el_html.get_box_model()?.width;
        tab.set_bounds(headless_chrome::types::Bounds::Normal { left: Some(0), top: Some(0), width:Some(body_width), height: Some(body_height) })?;
        let mut el = el_html;
        if sec != ""{
            el = tab.wait_for_element(&sec)?;
        }
        let png_data = tab.capture_screenshot(headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            Some(el.get_box_model()?.content_viewport()),
            true)?;
        return Ok(Some(self_t.build_bin(png_data)));
    });
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
    let mut ret_str:BTreeMap<String,String> = BTreeMap::new();
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