use super::*;

pub(super) fn register() {
    add_fun(vec!["返回头", "取返回头"], |self_t, params| {
        let ret_headers = self_t.get_coremap_value("返回头");
        let key = self_t.get_param_text(params, 0)?;
        if key.is_empty() {
            Ok(Some(ret_headers))
        } else {
            match &*ret_headers {
                RedValue::Object(obj) => {
                    if let Some(val) = obj.get(&key) {
                        return Ok(Some(val.clone()));
                    }
                    Ok(Some(rv_empty()))
                }
                _ => Ok(Some(rv_empty())),
            }
        }
    });
    add_fun(vec!["访问"], |self_t, params| {
        fn access(
            self_t: &mut RedLang,
            url: &str,
        ) -> Result<Option<Rc<RedValue>>, Box<dyn std::error::Error>> {
            let proxy = self_t.get_coremap("代理");
            let mut timeout_str = self_t.get_coremap("访问超时");
            if timeout_str.is_empty() {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = coremap_value_to_text_map(self_t, "访问头")?;
            if !http_header.contains_key("User-Agent") {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let timeout = timeout_str.parse::<u64>()?;
            let content = runtime().block_on(async {
                let ret = tokio::select! {
                    val_rst = http_post(url,Vec::new(),&http_header,&proxy,"GET") => {
                        if let Ok(val) = val_rst {
                            val
                        } else {
                            let _ = cq_add_log_w(&format!("{:?}",val_rst));
                            (vec![],BTreeMap::new())
                        }
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                        let _ = cq_add_log_w(&format!("GET访问:`{}`超时",url));
                        (vec![],BTreeMap::new())
                    }
                };
                ret
            });
            self_t.set_coremap_value("返回头", text_map_to_red_object(content.1));
            Ok(Some(rv_bin(content.0)))
        }
        let url = self_t.get_param_text(params, 0)?;
        self_t.set_coremap_value("返回头", rv_empty());
        match access(self_t, &url) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                let _ = cq_add_log_w(&format!("{:?}", err));
                Ok(Some(rv_bin(vec![])))
            }
        }
    });
    fn do_http(
        method: &str,
        self_t: &mut RedLang,
        params: &[astparser::Ast],
    ) -> Result<Option<Rc<RedValue>>, Box<dyn std::error::Error>> {
        fn access(
            method: &str,
            self_t: &mut RedLang,
            url: &str,
            data: Vec<u8>,
        ) -> Result<Option<Rc<RedValue>>, Box<dyn std::error::Error>> {
            let proxy = self_t.get_coremap("代理");
            let mut timeout_str = self_t.get_coremap("访问超时");
            if timeout_str.is_empty() {
                timeout_str = "60000".to_owned();
            }
            let mut http_header = coremap_value_to_text_map(self_t, "访问头")?;
            if !http_header.contains_key("User-Agent") {
                http_header.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36".to_string());
            }
            let timeout = timeout_str.parse::<u64>()?;

            let content = runtime().block_on(async {
                let ret = tokio::select! {
                    val_rst = http_post(url,data,&http_header,&proxy,method) => {
                        if let Ok(val) = val_rst {
                            val
                        } else {
                            let _ = cq_add_log_w(&format!("{:?}",val_rst));
                            (vec![],BTreeMap::new())
                        }
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_millis(timeout)) => {
                        let _ = cq_add_log_w(&format!("{}访问:`{}`超时",method,url));
                        (vec![],BTreeMap::new())
                    }
                };
                ret
            });
            self_t.set_coremap_value("返回头", text_map_to_red_object(content.1));
            Ok(Some(rv_bin(content.0)))
        }
        let url = self_t.get_param_text(params, 0)?;
        let data_rv = self_t.get_param(params, 1)?;
        let data = if let Ok(b) = data_rv.expect_bin_value() {
            b
        } else if let Ok(text) = data_rv.expect_text_value() {
            text.into_bytes()
        } else {
            return Err(RedLang::make_err(
                &("不支持的访问体类型:".to_owned() + data_rv.get_type_name()),
            ));
        };
        self_t.set_coremap_value("返回头", rv_empty());
        match access(method, self_t, &url, data) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                let _ = cq_add_log_w(&format!("{:?}", err));
                Ok(Some(rv_bin(vec![])))
            }
        }
    }
    add_fun(vec!["POST访问"], |self_t, params| {
        do_http("POST", self_t, params)
    });
    add_fun(vec!["GET访问"], |self_t, params| {
        do_http("GET", self_t, params)
    });
    add_fun(vec!["PUT访问"], |self_t, params| {
        do_http("PUT", self_t, params)
    });
    add_fun(vec!["PATCH访问"], |self_t, params| {
        do_http("PATCH", self_t, params)
    });
    add_fun(vec!["DELETE访问"], |self_t, params| {
        do_http("DELETE", self_t, params)
    });
    add_fun(vec!["设置访问头"], |self_t, params| {
        let mut http_header_map = coremap_value_to_text_map(self_t, "访问头")?;
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param_text(params, 1)?;
        http_header_map.insert(k, v);
        self_t.set_coremap_value("访问头", text_map_to_red_object(http_header_map));
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["设置代理"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        self_t.set_coremap("代理", &k)?;
        Ok(Some(rv_empty()))
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["系统代理"], |_self_t, _params| {
        if cfg!(target_os = "windows") {
            const HKEY_CURRENT_USER: winreg::HKEY = 0x80000001u32 as usize as winreg::HKEY;
            let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
            let internet_setting: winreg::RegKey = hkcu
                .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
            // ensure the proxy is enable, if the value doesn't exist, an error will returned.
            let proxy_enable: u32 = internet_setting.get_value("ProxyEnable")?;
            let proxy_server: String = internet_setting.get_value("ProxyServer")?;
            let mut proxy = "".to_string();
            if proxy_enable == 1 {
                if !(proxy_server.starts_with("http://") || proxy_server.starts_with("https://")) {
                    proxy = format!("http://{}", proxy_server);
                } else {
                    proxy = proxy_server;
                }
            }
            Ok(Some(rv_text(proxy)))
        } else {
            Ok(Some(rv_empty()))
        }
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["IE代理"], |_self_t, _params| {
        if cfg!(target_os = "windows") {
            const HKEY_CURRENT_USER: winreg::HKEY = 0x80000001u32 as usize as winreg::HKEY;
            let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
            let internet_setting: winreg::RegKey = hkcu
                .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
            let proxy_server_rst = internet_setting.get_value("ProxyServer");
            let mut proxy = "".to_string();
            if let Ok(proxy_server) = proxy_server_rst {
                if !(proxy.starts_with("http://") || proxy.starts_with("https://")) {
                    proxy = format!("http://{}", proxy_server);
                } else {
                    proxy = proxy_server;
                }
            }
            Ok(Some(rv_text(proxy)))
        } else {
            Ok(Some(rv_empty()))
        }
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["设置桌面背景"], |self_t, params| {
        let image_path = self_t.get_param_text(params, 0)?;
        let image_path = std::ffi::CString::new(image_path)
            .map_err(|e| RedLang::make_err(&format!("路径包含空字符: {e}")))?;
        let image_path_c_ptr: *mut winapi::ctypes::c_void =
            image_path.as_ptr() as *mut winapi::ctypes::c_void;
        unsafe {
            winapi::um::winuser::SystemParametersInfoA(
                winapi::um::winuser::SPI_SETDESKWALLPAPER,
                0,
                image_path_c_ptr,
                winapi::um::winuser::SPIF_UPDATEINIFILE,
            );
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["设置访问超时"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let tm = k.parse::<u64>()?;
        if tm <= 1000 {
            return Err(RedLang::make_err("访问超时时间不能小于1000毫秒"));
        }
        self_t.set_coremap("访问超时", &k)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["编码"], |self_t, params| {
        let urlcode = self_t.get_param_text(params, 0)?;
        let encoded: String = url::form_urlencoded::byte_serialize(urlcode.as_bytes()).collect();
        Ok(Some(rv_text(encoded)))
    });
    add_fun(vec!["解码"], |self_t, params| {
        let urlcode = self_t.get_param_text(params, 0)?;
        let decoded = decode_url_percent(&urlcode);
        Ok(Some(rv_text(decoded)))
    });
    add_fun(vec!["随机取"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        if arr.is_empty() {
            return Ok(Some(self_t.get_param(params, 1)?));
        }
        let index = get_random()? % arr.len();
        let ret = arr.get(index).ok_or("数组下标越界")?.clone();
        Ok(Some(ret))
    });
    add_fun(vec!["取中间"], |self_t, params| {
        let s = self_t.get_param_text(params, 0)?;
        let sub_begin = self_t.get_param_text(params, 1)?;
        let sub_end = self_t.get_param_text(params, 2)?;
        let ret_vec = get_mid(&s, &sub_begin, &sub_end)?;
        let ret_arr = ret_vec
            .into_iter()
            .map(|it| rv_text(it.to_string()))
            .collect();
        Ok(Some(rv_array(ret_arr)))
    });
    add_fun(vec!["截取"], |self_t, params| {
        let content = self_t.get_param(params, 0)?;
        let begin = self_t.get_param_text(params, 1)?;
        let len = self_t.get_param_text(params, 2)?;
        if let Ok(text) = content.expect_text_value() {
            let chs = text.chars().collect::<Vec<char>>();
            let begen_pos;
            if begin.starts_with("-") {
                let pos_rev = begin.get(1..).ok_or("参数错误")?.parse::<usize>()?;
                if pos_rev > chs.len() {
                    return Ok(Some(rv_empty()));
                } else {
                    begen_pos = chs.len() - pos_rev;
                }
            } else {
                begen_pos = begin.parse::<usize>()?;
            }
            let sub_len: usize;
            if len.is_empty() {
                if chs.len() < begen_pos {
                    sub_len = 0;
                } else {
                    sub_len = chs.len() - begen_pos;
                }
            } else {
                sub_len = len.parse::<usize>()?;
            }
            // 长度参数来自消息文本，begen_pos + sub_len 可能超出 usize，用饱和加法
            let mut end_pos = begen_pos.saturating_add(sub_len);
            if end_pos > chs.len() {
                end_pos = chs.len();
            }
            let ret = match chs.get(begen_pos..end_pos) {
                Some(value) => value.iter().collect::<String>(),
                None => "".to_string(),
            };
            Ok(Some(rv_text(ret)))
        } else if let Ok(arr) = content.expect_array_value() {
            let begen_pos;
            if begin.starts_with("-") {
                let pos_rev = begin.get(1..).ok_or("参数错误")?.parse::<usize>()?;
                if pos_rev > arr.len() {
                    return Ok(Some(rv_array(vec![])));
                } else {
                    begen_pos = arr.len() - pos_rev;
                }
            } else {
                begen_pos = begin.parse::<usize>()?;
            }
            let sub_len: usize;
            if len.is_empty() {
                if arr.len() < begen_pos {
                    sub_len = 0;
                } else {
                    sub_len = arr.len() - begen_pos;
                }
            } else {
                sub_len = len.parse::<usize>()?;
            }
            // 长度参数来自消息文本，begen_pos + sub_len 可能超出 usize，用饱和加法
            let mut end_pos = begen_pos.saturating_add(sub_len);
            if end_pos > arr.len() {
                end_pos = arr.len();
            }
            let ret = arr
                .get(begen_pos..end_pos)
                .map(|value| value.to_vec())
                .unwrap_or_default();
            Ok(Some(rv_array(ret)))
        } else {
            Err(RedLang::make_err("截取命令目前仅支持文本或数组"))
        }
    });
    add_fun(vec!["JSON解析"], |self_t, params| {
        let json_obj = self_t.get_param(params, 0)?;

        let json_data: serde_json::Value;

        if let Ok(u8_vec) = json_obj.expect_bin_value() {
            let json_str = match String::from_utf8(u8_vec) {
                Ok(s) => s,
                Err(_) => return Ok(Some(rv_empty())),
            };
            match serde_json::from_str(&json_str) {
                Ok(v) => json_data = v,
                Err(_) => return Ok(Some(rv_empty())),
            }
        } else if matches!(&*json_obj, RedValue::Object(_) | RedValue::Array(_)) {
            json_data = red_to_serde_value(&json_obj)?;
        } else if let Ok(json_str) = json_obj.expect_text_value() {
            match serde_json::from_str(&json_str) {
                Ok(v) => json_data = v,
                Err(_) => return Ok(Some(rv_empty())),
            }
        } else {
            return Err(RedLang::make_err(&format!(
                "不支持的JSON解析类型: {}",
                json_obj.get_type_name()
            )));
        }

        let jsonpath = self_t.get_param_text(params, 1)?;
        if !jsonpath.is_empty() {
            if let Ok(v) = json_data.query(&jsonpath) {
                return Ok(Some(json_value_to_red(&serde_json::json!(v))?));
            } else {
                let _ = cq_add_log_w("jsonpath解析失败");
                return Ok(Some(rv_empty()));
            }
        }
        return Ok(Some(json_value_to_red(&json_data)?));
    });
    add_fun(vec!["读文件"], |self_t, params| {
        let file_path = self_t.get_param_text(params, 0)?;
        let file_path = redlang::mytool::path_to_os_str(&file_path);

        let path = Path::new(&file_path);
        if !path.exists() {
            return Ok(Some(rv_bin(vec![])));
        }
        add_file_lock(&file_path);
        let _guard = scopeguard::guard(file_path.clone(), |file_path| {
            del_file_lock(&file_path);
        });
        let content = std::fs::read(path)?;
        Ok(Some(rv_bin(content)))
    });
    add_fun(vec!["运行目录"], |_self_t, _params| {
        let exe_dir = std::env::current_exe()?;
        let exe_path = exe_dir.parent().ok_or("无法获得运行目录")?;
        let mut exe_path_str = exe_path.to_string_lossy().to_string();
        if !exe_path_str.ends_with(std::path::MAIN_SEPARATOR) {
            exe_path_str.push(std::path::MAIN_SEPARATOR);
        }
        Ok(Some(rv_text(
            redlang::mytool::deal_path_str(&exe_path_str).to_string(),
        )))
    });
    add_fun(vec!["分割"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let sub_str = self_t.get_param_text(params, 1)?;
        let mut split_ret: Vec<&str> = data_str.split(&sub_str).collect();
        if sub_str.is_empty() {
            split_ret = split_ret
                .get(1..split_ret.len() - 1)
                .ok_or("无法获得分割结果")?
                .to_vec();
        }
        let ret = split_ret
            .into_iter()
            .map(|it| rv_text(it.to_string()))
            .collect();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["判含"], |self_t, params| {
        let data = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param_text(params, 1)?;
        if let Ok(data_str) = data.expect_text_value() {
            if !data_str.contains(&sub_str) {
                return Ok(Some(self_t.get_param(params, 2)?));
            } else {
                return Ok(Some(self_t.get_param(params, 3)?));
            }
        } else if let Ok(arr) = data.expect_array_value() {
            let mut ret = Vec::new();
            for it in arr {
                if it.expect_text_value()?.contains(&sub_str) {
                    ret.push(it);
                }
            }
            Ok(Some(rv_array(ret)))
        } else {
            Err(RedLang::make_err(
                &("对应类型不能使用判含:".to_owned() + data.get_type_name()),
            ))
        }
    });
    add_fun(vec!["正则判含"], |self_t, params| {
        let data = self_t.get_param(params, 0)?;
        let sub_str = self_t.get_param_text(params, 1)?;
        let re = fancy_regex::Regex::new(&sub_str)?;
        if let Ok(data_str) = data.expect_text_value() {
            let re = fancy_regex::Regex::new(&sub_str)?;
            let is_have = re.captures_iter(&data_str).next().is_some();
            if !is_have {
                return Ok(Some(self_t.get_param(params, 2)?));
            } else {
                return Ok(Some(self_t.get_param(params, 3)?));
            }
        } else if let Ok(arr) = data.expect_array_value() {
            let mut ret = Vec::new();
            for it in arr {
                let it_text = it.expect_text_value()?;
                let is_have = re.captures_iter(&it_text).next().is_some();
                if is_have {
                    ret.push(it);
                }
            }
            Ok(Some(rv_array(ret)))
        } else {
            Err(RedLang::make_err(
                &("对应类型不能使用正则判含:".to_owned() + data.get_type_name()),
            ))
        }
    });
    add_fun(vec!["正则"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let sub_str = self_t.get_param_text(params, 1)?;
        let mut index0_str = self_t.get_param_text(params, 2)?;
        if index0_str == "*" {
            index0_str = "".to_string();
        }
        let mut index1_str = self_t.get_param_text(params, 3)?;
        if index1_str == "*" {
            index1_str = "".to_string();
        }
        let mode;
        if index0_str.parse::<usize>().is_ok() && index1_str.parse::<usize>().is_ok() {
            mode = 1; // 两个索引都在
        } else if index0_str.parse::<usize>().is_ok() && index1_str.is_empty() {
            mode = 2; // 只有第一个索引
        } else if index0_str.is_empty() && index1_str.parse::<usize>().is_ok() {
            mode = 3; // 只有第二个索引
        } else if index0_str.is_empty() && index1_str.is_empty() {
            mode = 0; // 两个索引都为空
        } else {
            return Err(RedLang::make_err("正则索引参数错误"));
        }
        let re = fancy_regex::Regex::new(&sub_str)?;
        let mut sub_key_vec: Vec<Vec<String>> = vec![];
        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            let mut temp_vec: Vec<String> = vec![];
            for i in 0..len {
                if let Some(s) = cap.get(i) {
                    temp_vec.push(s.as_str().to_owned());
                }
            }
            sub_key_vec.push(temp_vec);
        }
        if mode == 0 {
            let out = sub_key_vec
                .iter()
                .map(|temp_vec| {
                    let inner = temp_vec
                        .iter()
                        .map(|s| rv_text(s.clone()))
                        .collect::<Vec<Rc<RedValue>>>();
                    rv_array(inner)
                })
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(out)))
        } else if mode == 1 {
            let index0 = index0_str.parse::<usize>()?;
            let index1 = index1_str.parse::<usize>()?;
            if let Some(v0) = sub_key_vec.get(index0) {
                if let Some(v1) = v0.get(index1) {
                    return Ok(Some(rv_text(v1.to_owned())));
                }
            }
            Ok(Some(rv_empty()))
        } else if mode == 2 {
            let index0 = index0_str.parse::<usize>()?;
            if let Some(v0) = sub_key_vec.get(index0) {
                let out = v0
                    .iter()
                    .map(|s| rv_text(s.clone()))
                    .collect::<Vec<Rc<RedValue>>>();
                return Ok(Some(rv_array(out)));
            }
            Ok(Some(rv_empty()))
        } else {
            // mode == 3
            let ret_vec = sub_key_vec
                .iter()
                .map(|temp_vec| {
                    if let Some(v1) = temp_vec.get(index1_str.parse::<usize>().unwrap_or(usize::MAX)) {
                        v1.to_owned()
                    } else {
                        "".to_owned()
                    }
                })
                .collect::<Vec<String>>();
            let out = ret_vec
                .iter()
                .map(|s| rv_text(s.clone()))
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(out)))
        }
    });
    add_fun(vec!["转字节集"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        let code_t = self_t.get_param_text(params, 1)?;
        let code = code_t.to_lowercase();
        let str_vec: Vec<u8>;
        if code.is_empty() || code == "utf8" || code == "utf-8" {
            str_vec = text.as_bytes().to_vec();
        } else if code == "gbk" {
            str_vec = encoding::Encoding::encode(
                encoding::all::GBK,
                &text,
                encoding::EncoderTrap::Ignore,
            )?;
        } else {
            return Err(RedLang::make_err(&("不支持的编码:".to_owned() + &code_t)));
        }
        Ok(Some(rv_bin(str_vec)))
    });
    add_fun(vec!["BASE64编码"], |self_t, params| {
        let bin = self_t.get_param_bin(params, 0)?;
        let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
        Ok(Some(rv_text(b64_str)))
    });
    add_fun(vec!["BASE64解码"], |self_t, params| {
        let b64_str = self_t.get_param_text(params, 0)?;
        if let Ok(content) = base64::Engine::decode(
            &base64::engine::GeneralPurpose::new(
                &base64::alphabet::STANDARD,
                base64::engine::general_purpose::PAD,
            ),
            b64_str,
        ) {
            Ok(Some(rv_bin(content)))
        } else {
            // 解码出错返回空字节集
            Ok(Some(rv_bin(Vec::new())))
        }
    });
    add_fun(vec!["GZIP解码"], |self_t, params| {
        let gzip_bin = self_t.get_param_bin(params, 0)?;
        let d = GzDecoder::new(gzip_bin.as_slice());
        match read_decompress_limited(d) {
            Ok(buf) => Ok(Some(rv_bin(buf))),
            Err(_) => {
                // 解码出错/输出超限返回空字节集
                Ok(Some(rv_bin(Vec::new())))
            }
        }
    });
    add_fun(vec!["GZIP编码"], |self_t, params| {
        let gzip_bin = self_t.get_param_bin(params, 0)?;
        let mut e = GzEncoder::new(vec![], Compression::default());
        e.write_all(&gzip_bin)?;
        let buf = e.finish()?;
        Ok(Some(rv_bin(buf)))
    });
    add_fun(vec!["ZLIB解码"], |self_t, params| {
        let zlib_bin = self_t.get_param_bin(params, 0)?;
        let d = ZlibDecoder::new(zlib_bin.as_slice());
        match read_decompress_limited(d) {
            Ok(buf) => Ok(Some(rv_bin(buf))),
            Err(_) => {
                // 解码出错/输出超限返回空字节集
                Ok(Some(rv_bin(Vec::new())))
            }
        }
    });
    add_fun(vec!["ZLIB编码"], |self_t, params| {
        let zlib_bin = self_t.get_param_bin(params, 0)?;
        let mut e = ZlibEncoder::new(vec![], Compression::default());
        e.write_all(&zlib_bin)?;
        let buf = e.finish()?;
        Ok(Some(rv_bin(buf)))
    });
    add_fun(vec!["BR解码"], |self_t, params| {
        let br_bin = self_t.get_param_bin(params, 0)?;
        let d = brotli::Decompressor::new(br_bin.as_slice(), 4096);
        match read_decompress_limited(d) {
            Ok(buf) => Ok(Some(rv_bin(buf))),
            Err(_) => Ok(Some(rv_bin(Vec::new()))),
        }
    });
    add_fun(vec!["BR编码"], |self_t, params| {
        let br_bin = self_t.get_param_bin(params, 0)?;
        let mut c = brotli::CompressorReader::new(br_bin.as_slice(), 4096, 11, 22);
        let mut buf = vec![];
        c.read_to_end(&mut buf)?;
        Ok(Some(rv_bin(buf)))
    });
    add_fun(vec!["延时"], |self_t, params| {
        let mut mill = self_t.get_param_text(params, 0)?.parse::<u64>()?;
        while mill > 1000 {
            let time_struct = core::time::Duration::from_secs(1);
            std::thread::sleep(time_struct);
            mill -= 1000;
            if !pkg_can_run(&self_t.pkg_name, "延时") {
                return Err("延时终止，因用户要求退出".into());
            }
        }
        if !pkg_can_run(&self_t.pkg_name, "延时") {
            return Err("延时终止，因用户要求退出".into());
        }
        let time_struct = core::time::Duration::from_millis(mill);
        std::thread::sleep(time_struct);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["序号"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param_text(params, 1)?;
        if !v.is_empty() {
            // 说明是设置序号
            self_t.xuhao.insert(k.to_owned(), v.parse::<usize>()?);
            Ok(Some(rv_empty()))
        } else {
            // 说明是取序号
            let ret = if let Some(x) = self_t.xuhao.get_mut(&k) {
                let ret = *x;
                *x += 1;
                ret
            } else {
                self_t.xuhao.insert(k.to_owned(), 1);
                0
            };
            Ok(Some(rv_text(ret.to_string())))
        }
    });
    add_fun(vec!["时间戳", "10位时间戳"], |_self_t, _params| {
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        Ok(Some(rv_text(tm.as_secs().to_string())))
    });
    add_fun(vec!["13位时间戳"], |_self_t, _params| {
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        Ok(Some(rv_text(tm.as_millis().to_string())))
    });
    add_fun(vec!["开始时间"], |_self_t, _params| {
        Ok(Some(rv_text(crate::control_plane::control().start_time())))
    });
    add_fun(vec!["时间戳转文本"], |self_t, params| {
        let numstr = self_t.get_param_text(params, 0)?;
        if numstr.len() > 10 {
            return Ok(Some(rv_empty()));
        }
        let num = numstr.parse::<i64>()?;
        let datetime_rst = chrono::prelude::Local.timestamp_opt(num, 0);
        if let chrono::LocalResult::Single(datetime) = datetime_rst {
            let newdate = datetime.format("%Y-%m-%d-%H-%M-%S");
            return Ok(Some(rv_text(format!("{}", newdate))));
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["文本转时间戳"], |self_t, params| {
        let time_str = self_t.get_param_text(params, 0)?;
        const FORMAT: &str = "%Y-%m-%d-%H-%M-%S";
        let utc_offset;
        if let Ok(v) = UtcOffset::current_local_offset() {
            utc_offset = v;
        } else {
            utc_offset = UtcOffset::from_hms(8, 0, 0).unwrap_or(UtcOffset::UTC);
        }
        let tm = chrono::NaiveDateTime::parse_from_str(&time_str, FORMAT)?
            .and_utc()
            .timestamp()
            - utc_offset.whole_seconds() as i64;
        Ok(Some(rv_text(tm.to_string())))
    });

    add_fun(vec!["MD5编码"], |self_t, params| {
        let bin = self_t.get_param_bin(params, 0)?;
        let mut hasher = Md5::new();
        hasher.update(bin);
        let result = hasher.finalize();
        let mut content = String::new();
        for ch in result {
            content.push_str(&format!("{:02x}", ch));
        }
        Ok(Some(rv_text(content)))
    });
    add_fun(vec!["RCNB编码"], |self_t, params| {
        let bin = self_t.get_param_bin(params, 0)?;
        let content = rcnb_rs::encode(bin);
        Ok(Some(rv_text(content)))
    });
}
