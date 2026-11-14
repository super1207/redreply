use super::*;

pub(super) fn register() {
    add_fun(vec!["CQ码解析"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let pos1 = data_str.find(",").ok_or("CQ码解析失败")?;
        let tp = data_str.get(4..pos1).ok_or("CQ码解析失败")?;
        let mut sub_key_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        sub_key_obj.insert("type".to_string(), rv_text(tp.to_string()));
        let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])")?;

        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            if len == 3 {
                let key = &cap[1];
                let val = &cap[2];
                let key = key.replace("&#91;", "[");
                let key = key.replace("&#93;", "]");
                let key = key.replace("&#44;", ",");
                let key = key.replace("&amp;", "&");
                let val = val.replace("&#91;", "[");
                let val = val.replace("&#93;", "]");
                let val = val.replace("&#44;", ",");
                let val = val.replace("&amp;", "&");
                sub_key_obj.insert(key, rv_text(val));
            }
        }
        Ok(Some(rv_object(sub_key_obj)))
    });
    add_fun(vec!["CQ解析"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let json_arr = redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?;
        return Ok(Some(json_value_to_red(&json_arr)?));
    });
    add_fun(vec!["CQ反转义"], |self_t, params| {
        let content = self_t.get_param_text(params, 0)?;
        let content = content.replace("&#91;", "[");
        let content = content.replace("&#93;", "]");
        let content = content.replace("&amp;", "&");
        Ok(Some(rv_text(content)))
    });
    add_fun(vec!["定义常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        set_const_val(&self_t.pkg_name, &k, RedValueData::from_red_value(&v)?)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["常量"], |self_t, params| {
        let params_len = params.len();
        if params_len == 1 {
            // 取当前包的常量
            let k = self_t.get_param_text(params, 0)?;
            if let Some(v) = get_const_val(&self_t.pkg_name, &k)? {
                return Ok(Some(v.into_rc_value()?));
            }
            Ok(Some(rv_empty()))
        } else {
            // 取其它包的常量
            let pkg_name = self_t.get_param_text(params, 0)?;
            let k = self_t.get_param_text(params, 1)?;
            if let Some(v) = get_const_val(&pkg_name, &k)? {
                return Ok(Some(v.into_rc_value()?));
            }
            Ok(Some(rv_empty()))
        }
    });
    add_fun(vec!["定义临时常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        let mut tm = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        tm += self_t.get_param_text(params, 2)?.parse::<u128>()?;
        set_temp_const_val(&self_t.pkg_name, &k, RedValueData::from_red_value(&v)?, tm)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["临时常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        if let Some(v) = get_temp_const_val(&self_t.pkg_name, &k)? {
            return Ok(Some(v.into_rc_value()?));
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["进程ID"], |_self_t, _params| {
        Ok(Some(rv_text(std::process::id().to_string())))
    });
    add_fun(vec!["读词库文件"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        let path_t = path.clone();
        let file_dat = fs::read_to_string(path)?;
        let file_dat_without_r = file_dat.replace('\r', "");
        let words_list = file_dat_without_r.split("\n\n");
        let mut dict_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        let err = format!("词库文件格式错误:`{}`", &path_t);
        for words in words_list {
            if words.trim().is_empty() {
                continue;
            }
            let word_list = words.split('\n').collect::<Vec<&str>>();
            let key: &str = word_list.first().ok_or(err.clone())?;
            let word_list_t = word_list.get(1..).ok_or(err.clone())?;
            let mut arr_val: Vec<Rc<RedValue>> = vec![];
            for word in word_list_t {
                arr_val.push(rv_text((*word).to_string()));
            }
            dict_obj.insert(key.to_owned(), rv_array(arr_val));
        }
        Ok(Some(rv_object(dict_obj)))
    });
    add_fun(vec!["应用目录"], |self_t, _params| {
        let app_dir = get_app_dir(&self_t.pkg_name)?;
        Ok(Some(rv_text(app_dir)))
    });
    add_fun(vec!["取艾特"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        let mut ret_vec: Vec<&str> = vec![];
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "at" {
                let qq = it
                    .get("data")
                    .ok_or(err)?
                    .get("qq")
                    .ok_or(err)?
                    .as_str()
                    .ok_or(err)?;
                ret_vec.push(qq);
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(|x| rv_text(x.to_string()))
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["取回复ID"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "reply" {
                let id = it
                    .get("data")
                    .ok_or(err)?
                    .get("id")
                    .ok_or(err)?
                    .as_str()
                    .ok_or(err)?;
                return Ok(Some(rv_text(id.to_owned())));
            }
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["取图片"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };

        let mut ret_vec: Vec<String> = vec![];
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "image" || tp == "mface" {
                let data = it.get("data").ok_or("data not found in image cq code")?;
                let mut url = read_json_str(data, "url");
                if url.is_empty() {
                    url = read_json_str(data, "file");
                }
                if url.starts_with("http://") || url.starts_with("https://") {
                    ret_vec.push(url);
                }
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["取文本"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        let mut ret_vec: Vec<String> = vec![];
        let mut should_new = 1;
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "text" {
                let data = it.get("data").ok_or("data not found in text cq code")?;
                let text = read_json_str(data, "text");
                if should_new == 1 {
                    ret_vec.push(text);
                } else {
                    let len = ret_vec.len();
                    ret_vec[len - 1].push_str(&text);
                }
                should_new = 0;
            } else {
                should_new = 1;
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
}
