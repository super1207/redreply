use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
    sync::Arc,
    thread,
    time::SystemTime,
    vec,
};

use super::{
    exfun::json_value_to_red, rv_array, rv_bin, rv_empty, rv_object, rv_text, RedLang, RedValue,
    RedValueData,
};
use crate::{
    cqapi::{cq_add_log_w, cq_call_api, cq_get_app_directory1, cq_get_app_directory2},
    lifecycle::pkg_can_run,
    mytool::{cq_params_encode, cq_text_encode, json_to_cq_str, read_json_str},
    redlang::{add_fun, get_const_val, get_temp_const_val, set_const_val, set_temp_const_val},
    resource_manager::{add_file_lock, del_file_lock},
    runtime, runtime_context,
    status::{add_send_group_msg, add_send_private_msg},
    CLEAR_UUID, PAGING_UUID,
};
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use url::Url;
const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose =
    engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

pub fn get_app_dir(pkg_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let app_dir = if pkg_name.is_empty() {
        cq_get_app_directory2().unwrap()
    } else {
        let plus_dir_str = cq_get_app_directory1().unwrap();
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        pkg_dir
            .join(pkg_name)
            .to_str()
            .ok_or("获得应用目录失败")?
            .to_owned()
            + std::path::MAIN_SEPARATOR_STR
    };
    Ok(app_dir)
}

fn get_platform(self_t: &RedLang) -> String {
    let platform = self_t.get_exmap("机器人平台");
    (*platform).to_owned()
}

fn get_sub_id(rl: &RedLang, msg_type: &str) -> String {
    if msg_type == "group" || msg_type == "private_temp" {
        rl.get_exmap("群ID").to_string()
    } else {
        "".to_owned()
    }
}

fn hex_to_num(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_path_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut ret = Vec::with_capacity(bytes.len());
    let mut changed = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_to_num(bytes[i + 1]), hex_to_num(bytes[i + 2])) {
                ret.push(high * 16 + low);
                i += 3;
                changed = true;
                continue;
            }
        }
        ret.push(bytes[i]);
        i += 1;
    }
    if changed {
        String::from_utf8_lossy(&ret).to_string()
    } else {
        segment.to_string()
    }
}

fn infer_file_name_from_resource(resource: &str) -> Option<String> {
    if resource.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(resource) {
        if url.scheme() == "http" || url.scheme() == "https" {
            return url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(percent_decode_path_segment)
                .filter(|name| !name.is_empty());
        }
    }
    if resource.ends_with('/') || resource.ends_with('\\') {
        return None;
    }
    resource
        .rsplit(['/', '\\'])
        .next()
        .map(str::to_string)
        .filter(|name| !name.is_empty())
}

fn resolve_file_name(name: &str, resource: Option<&str>) -> String {
    if !name.is_empty() {
        return name.to_string();
    }
    resource
        .and_then(infer_file_name_from_resource)
        .unwrap_or_else(|| "未命名".to_string())
}

pub fn send_one_msg(rl: &RedLang, msg: &str) -> Result<String, Box<dyn std::error::Error>> {
    if msg.is_empty() {
        return Ok("".to_string());
    }

    let arr_msg = crate::mytool::str_msg_to_arr(&serde_json::json!(msg))?;
    let msg_type: &'static str = crate::cqevent::get_msg_type(rl);
    let sub_id = get_sub_id(rl, msg_type);

    // 没有设置输出流类型，所以不输出
    if msg_type.is_empty() {
        return Ok("".to_string());
    }
    let send_json: serde_json::Value;
    if msg_type == "group" {
        send_json = serde_json::json!({
            "action":"send_group_msg",
            "params":{
                "group_id":sub_id,
                "message":arr_msg
            }
        });
    } else if msg_type == "private" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":(*rl.get_exmap("发送者ID")),
                "message":arr_msg
            }
        });
    } else if msg_type == "private_temp" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":(*rl.get_exmap("发送者ID")),
                "group_id":sub_id,
                "message":arr_msg
            }
        });
    } else {
        return Err(RedLang::make_err(
            &("不支持的输出流:".to_string() + msg_type),
        ));
    }
    let self_id = rl.get_exmap("机器人ID");
    let platform = get_platform(rl);
    let passive_id = rl.get_exmap("消息ID");
    let remote_id = rl.get_exmap("远程MQTT客户端ID");

    // 数据统计
    {
        if msg_type == "group" {
            add_send_group_msg(&platform, &self_id)?;
        } else {
            add_send_private_msg(&platform, &self_id)?;
        }
    }

    let cq_ret = cq_call_api(
        &platform,
        &self_id,
        &passive_id,
        send_json.to_string().as_str(),
        &remote_id,
    );
    let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
    let err = "输出流调用失败,retcode 不为0";
    if ret_json.get("retcode").ok_or(err)?.as_i64().ok_or(err)? != 0 {
        return Err(RedLang::make_err(&format!("{}:{}", err, cq_ret)));
    }
    let err = "输出流调用失败，获取message_id失败";
    let msg_id = read_json_str(ret_json.get("data").ok_or(err)?, "message_id");
    let src_msg_id = rl.get_exmap("消息ID");
    let tm = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    runtime_context()
        .session
        .record_sent_message(&rl.pkg_name, &self_id, &src_msg_id, &msg_id, tm);
    if msg_type == "group" {
        let self_id = rl.get_exmap("机器人ID");
        crate::cqevent::do_group_msg::msg_id_map_insert(
            self_id.to_string(),
            self_id.to_string(),
            sub_id,
            msg_id.clone(),
        )?;
    }
    Ok(msg_id)
}

pub fn init_cq_ex_fun_map() {
    add_fun(vec!["取群列表", "群列表"], |self_t, _params| {
        let groups_id = &*self_t.get_exmap("群组ID");
        let send_json = if groups_id.is_empty() {
            serde_json::json!({
                "action":"get_group_list",
                "params":{}
            })
        } else {
            serde_json::json!({
                "action":"get_group_list",
                "params":{
                    "groups_id":groups_id
                }
            })
        };
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取群列表失败:{ret_json}");

        let group_list = ret_json.get("data").ok_or(err)?;

        let mut to_ret: Vec<serde_json::Value> = vec![];
        for group in group_list.as_array().ok_or("获取群列表失败")? {
            to_ret.push(serde_json::json!({
                "群ID":read_json_str(group, "group_id"),
                "群名":read_json_str(group, "group_name"),
            }));
        }
        return Ok(Some(json_value_to_red(&serde_json::json!(to_ret))?));
    });

    fn get_stranger_info(
        self_t: &mut RedLang,
        user_id: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let user_id_t = if user_id.is_empty() {
            (*self_t.get_exmap("发送者ID")).to_owned()
        } else {
            user_id.to_owned()
        };
        let send_json = serde_json::json!({
            "action":"get_stranger_info",
            "params":{
                "user_id":user_id_t
            }
        });

        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取发送者信息失败:{ret_json}");
        let data = ret_json.get("data").ok_or(err)?;
        Ok(data.to_owned())
    }
    add_fun(vec!["取发送者信息"], |self_t, _params| {
        let data = get_stranger_info(self_t, "")?;
        let nickname = read_json_str(&data, "nickname");
        self_t.set_exmap("发送者昵称", &nickname);
        let to_ret = serde_json::json!({
            "用户ID":read_json_str(&data, "user_id"),
            "用户名":nickname
        });
        return Ok(Some(json_value_to_red(&to_ret)?));
    });
    add_fun(vec!["取头像"], |self_t, params| {
        let user_id = self_t.get_param_text(params, 0).unwrap_or_default();
        let platform = get_platform(self_t);
        // 官方QQ平台：头像 CDN 支持用 AppID + OpenID 直接拼 URL
        if platform == "qqgroup_public"
            || platform == "qqguild_public"
            || platform == "qqguild_private"
        {
            // 官方QQ头像 CDN：https://q.qlogo.cn/qqapp/{AppID}/{OpenID}/{尺寸}
            // （honoka-bot 实测：返回 image/jpeg，无需调用任何 API）
            let appid = self_t.get_exmap("机器人ID").to_string();
            let openid = if user_id.is_empty() {
                self_t.get_exmap("发送者ID").to_string()
            } else {
                user_id.clone()
            };
            if !appid.is_empty() && !openid.is_empty() {
                return Ok(Some(rv_text(format!(
                    "https://q.qlogo.cn/qqapp/{}/{}/640",
                    appid, openid
                ))));
            }
            return Ok(Some(rv_text(
                "获取头像失败：缺少AppID或用户OpenID".to_string(),
            )));
        }
        let data = get_stranger_info(self_t, &user_id).unwrap_or_default();
        let avatar = read_json_str(&data, "avatar");
        Ok(Some(rv_text(avatar)))
    });
    add_fun(vec!["发送者ID", "发送者QQ"], |self_t, _params| {
        let qq = self_t.get_exmap("发送者ID");
        Ok(Some(rv_text(qq.to_string())))
    });
    add_fun(
        vec!["当前群号", "群号", "群ID"],
        |self_t, _params| {
            let group = self_t.get_exmap("群ID");
            Ok(Some(rv_text(group.to_string())))
        },
    );
    add_fun(
        vec!["当前群组", "群组号", "群组ID"],
        |self_t, _params| {
            let groups = self_t.get_exmap("群组ID");
            Ok(Some(rv_text(groups.to_string())))
        },
    );
    add_fun(vec!["发送者昵称"], |self_t, _params| {
        let mut nickname = &*self_t.get_exmap("发送者昵称");
        let card = &*self_t.get_exmap("发送者名片");
        if !card.is_empty() {
            nickname = card;
        }
        if nickname.is_empty() {
            if let Ok(data) = get_stranger_info(self_t, "") {
                let name = read_json_str(&data, "nickname");
                if !name.is_empty() {
                    self_t.set_exmap("发送者昵称", &name);
                    return Ok(Some(rv_text(name)));
                }
            }
        }
        Ok(Some(rv_text(nickname.to_string())))
    });
    add_fun(vec!["机器人QQ"], |self_t, _params| {
        let qq = self_t.get_exmap("机器人ID");
        Ok(Some(rv_text(qq.to_string())))
    });
    add_fun(vec!["机器人ID"], |self_t, _params| {
        let qq: String = self_t.get_exmap("机器人ID").to_string();
        Ok(Some(rv_text(qq)))
    });
    add_fun(vec!["机器人名字"], |self_t, _params| {
        let send_json = serde_json::json!({
            "action":"get_login_info",
            "params":{}
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let bot_name = &ret_json["data"]["nickname"];
        if bot_name.is_string() {
            let name = bot_name.as_str().unwrap().to_owned();
            if name.is_empty() {
                return Ok(Some(rv_text("露娜sama".to_owned())));
            }
            Ok(Some(rv_text(name)))
        } else {
            Ok(Some(rv_text("露娜sama".to_owned())))
        }
    });
    add_fun(vec!["权限", "发送者权限"], |self_t, _params| {
        let role = self_t.get_exmap("发送者权限");
        Ok(Some(rv_text(role.to_string())))
    });
    // add_fun(vec!["发送者名片"],|self_t,_params|{
    //     let card = self_t.get_exmap("发送者名片");
    //     return Ok(Some(card.to_string()));
    // });
    // add_fun(vec!["发送者专属头衔"],|self_t,_params|{
    //     let title = self_t.get_exmap("发送者专属头衔");
    //     return Ok(Some(title.to_string()));
    // });
    add_fun(vec!["消息ID"], |self_t, params| {
        let qq = self_t.get_param_text(params, 0)?;
        if qq.is_empty() {
            let msg_id = self_t.get_exmap("消息ID");
            Ok(Some(rv_text(msg_id.to_string())))
        } else {
            let group_id = self_t.get_exmap("群ID");
            let self_id = self_t.get_exmap("机器人ID");
            let message_ids = runtime_context().session.message_ids(&self_id, &group_id)?;
            let ret_arr = message_ids
                .into_iter()
                .filter(|(user_id, _)| user_id == &qq)
                .map(|(_, message_id)| rv_text(message_id))
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(ret_arr)))
        }
    });
    add_fun(vec!["群历史消息"], |self_t, params| {
        let num_str = self_t.get_param_text(params, 0)?;
        let num = num_str.parse::<usize>()?;
        let mut to_ret: Vec<String> = vec![];
        let group_id = self_t.get_exmap("群ID");
        let self_id = self_t.get_exmap("机器人ID");
        let message_ids = runtime_context().session.message_ids(&self_id, &group_id)?;
        for (_, message_id) in message_ids {
            to_ret.push(message_id);
            if to_ret.len() >= num {
                break;
            }
        }
        let ret = to_ret
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["设置图片参数"], |self_t, params| {
        let key = self_t.get_param_text(params, 0)?;
        let val = self_t.get_param_text(params, 1)?;
        if key.to_uppercase() == "SUMMARY" {
            self_t.set_exmap("图片SUMMARY", val.as_str());
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["图片"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        let summary = self_t.get_exmap("图片SUMMARY");
        if let Ok(bin) = pic_rv.expect_bin_value() {
            if bin.is_empty() {
                let _ = cq_add_log_w("图片字节集长度为0");
                return Ok(Some(rv_empty()));
            }
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            if !(*summary).is_empty() {
                ret = format!(
                    "[CQ:image,file=base64://{},summary={}]",
                    b64_str,
                    cq_params_encode(&summary)
                );
            } else {
                ret = format!("[CQ:image,file=base64://{}]", b64_str);
            }
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    if !(*summary).is_empty() {
                        ret = format!(
                            "[CQ:image,file={},cache=0,summary={}]",
                            cq_params_encode(&pic),
                            cq_params_encode(&summary)
                        );
                    } else {
                        ret = format!("[CQ:image,file={},cache=0]", cq_params_encode(&pic));
                    }
                } else {
                    if !(*summary).is_empty() {
                        ret = format!(
                            "[CQ:image,file={},summary={}]",
                            cq_params_encode(&pic),
                            cq_params_encode(&summary)
                        );
                    } else {
                        ret = format!("[CQ:image,file={}]", cq_params_encode(&pic));
                    }
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                if !(*summary).is_empty() {
                    ret = format!(
                        "[CQ:image,file=base64://{},summary={}]",
                        b64_str,
                        cq_params_encode(&summary)
                    );
                } else {
                    ret = format!("[CQ:image,file=base64://{}]", b64_str);
                }
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["语音"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:record,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    ret = format!("[CQ:record,file={},cache=0]", cq_params_encode(&pic));
                } else {
                    ret = format!("[CQ:record,file={}]", cq_params_encode(&pic));
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:record,file=base64://{}]", b64_str);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["文件"], |self_t, params| {
        let file_rv = self_t.get_param(params, 0)?;
        let name = self_t.get_param_text(params, 1)?;
        let mut ret: String = String::new();
        if let Ok(bin) = file_rv.expect_bin_value() {
            let name = cq_params_encode(&resolve_file_name(&name, None));
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:file,file=base64://{},name={}]", b64_str, name);
        } else if let Ok(file) = file_rv.expect_text_value() {
            let name = cq_params_encode(&resolve_file_name(&name, Some(&file)));
            if file.starts_with("http://") || file.starts_with("https://") {
                ret = format!("[CQ:file,file={},name={}]", cq_params_encode(&file), name);
            } else {
                let path = Path::new(&*file);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:file,file=base64://{},name={}]", b64_str, name);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["视频"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:video,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    ret = format!("[CQ:video,file={},cache=0]", cq_params_encode(&pic));
                } else {
                    ret = format!("[CQ:video,file={}]", cq_params_encode(&pic));
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:video,file=base64://{}]", b64_str);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["撤回"], |self_t, params| {
        let msg_id_rv = self_t.get_param(params, 0)?;
        let current_msg_id = self_t.get_exmap("消息ID");
        let msg_id_vec: Vec<String> = match &*msg_id_rv {
            RedValue::Text(s) if s.is_empty() => vec![current_msg_id.to_string()],
            RedValue::Text(s) => vec![s.to_string()],
            RedValue::Array(arr) => arr
                .iter()
                .filter_map(|item| item.expect_text_value().ok())
                .collect(),
            _ => vec![],
        };
        for it in msg_id_vec {
            let send_json = serde_json::json!({
                "action":"delete_msg",
                "params":{
                    "message_id":it
                }
            });
            let self_id = self_t.get_exmap("机器人ID");
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                send_json.to_string().as_str(),
                &remote_id,
            );
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["禁言"], |self_t, params| {
        let ban_time = self_t.get_param_text(params, 0)?;
        let user_id_str = self_t.get_exmap("发送者ID").to_string();
        let group_id_str = self_t.get_exmap("群ID").to_string();
        let send_json = serde_json::json!({
            "action":"set_group_ban",
            "params":{
                "group_id": group_id_str,
                "user_id": user_id_str,
                "duration":ban_time.parse::<usize>()?
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let _cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["表情回应"], |self_t, params| {
        let emoji_id = self_t.get_param_text(params, 0)?;
        let mut message_id = self_t.get_param_text(params, 1)?;
        if message_id.is_empty() {
            message_id = self_t.get_exmap("消息ID").to_string();
        }
        let emoji_num;
        if let Ok(emoji_id_t) = emoji_id.parse::<i32>() {
            emoji_num = emoji_id_t;
        } else {
            let emojis = emoji_id.chars().collect::<Vec<char>>();
            if emojis.len() != 1 {
                let _ = cq_add_log_w("表情回应失败，表情ID错误1");
                return Ok(Some(rv_empty()));
            }
            let emoji: char = emojis[0];
            emoji_num = emoji as i32;
        }
        let group_id = self_t.get_exmap("群ID");
        let send_json = serde_json::json!({
            "action":"set_msg_emoji_like",
            "params":{
                "message_id": *message_id,
                "emoji_id": emoji_num.to_string(),
                "group_id": *group_id
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let _cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["输出流"], |self_t, params| {
        let msg = self_t.get_param_text(params, 0)?;
        match send_one_msg(self_t, &msg) {
            Ok(msg_id) => Ok(Some(rv_text(msg_id))),
            Err(e) => {
                let _ = cq_add_log_w(&format!("输出流失败，{}", e));
                Ok(Some(rv_empty()))
            }
        }
    });
    add_fun(vec!["艾特"], |self_t, params| {
        let mut user_id = self_t.get_param_text(params, 0)?;
        if user_id.is_empty() {
            user_id = self_t.get_exmap("发送者ID").to_string();
        }
        if user_id.is_empty() {
            Ok(Some(rv_empty()))
        } else {
            Ok(Some(rv_text(format!(
                "[CQ:at,qq={}]",
                cq_params_encode(&user_id)
            ))))
        }
    });
    add_fun(vec!["合并转发"], |self_t, params| {
        let param0 = self_t.get_param(params, 0)?;
        let tp = param0.get_type_name();
        let mut ret_str = String::new();
        if tp == "数组" {
            let arr = param0.as_array().ok_or("合并转发:第1个参数不是数组")?;
            for it in arr {
                let content = it.expect_text_value()?;
                if content.is_empty() {
                    continue;
                }
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
        } else {
            let content = param0.expect_text_value()?;
            if !content.is_empty() {
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
            for i in 1..params.len() {
                let content = self_t.get_param_text(params, i)?;
                if content.is_empty() {
                    continue;
                }
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
        }
        if ret_str.is_empty() {
            return Ok(Some(rv_empty()));
        }
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["CQ码转义"], |self_t, params| {
        let cq_code = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(cq_params_encode(&cq_code))))
    });
    add_fun(vec!["CQ转义"], |self_t, params| {
        let cq_code = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(cq_text_encode(&cq_code))))
    });
    add_fun(vec!["子关键词"], |self_t, _params| {
        return Ok(Some(self_t.get_exmap_value("子关键词")?));
    });
    add_fun(vec!["事件内容"], |self_t, _params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json = serde_json::from_str(&raw_data)?;
        return Ok(Some(json_value_to_red(&raw_json)?));
    });
    add_fun(vec!["OB调用"], |self_t, params| {
        let content = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let call_ret = cq_call_api(&platform, &self_id, &passive_id, &content, &remote_id);
        let js_v = serde_json::from_str(&call_ret)?;
        return Ok(Some(json_value_to_red(&js_v)?));
    });
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
        let json_arr = crate::mytool::str_msg_to_arr(&serde_json::json!(data_str))?;
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
            crate::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
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
            crate::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
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
            crate::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
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
            crate::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
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
    add_fun(vec!["分页"], |_self_t, _params| {
        Ok(Some(rv_text(PAGING_UUID.to_string())))
    });
    add_fun(vec!["设置来源"], |self_t, params| {
        let key_t = self_t.get_param_text(params, 0)?;
        let val = self_t.get_param_text(params, 1)?;
        let key = crate::mytool::str_to_jt(&key_t);
        self_t.set_exmap(&key, &val);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["发送私聊"], |self_t, params| {
        let msg_type: &'static str = crate::cqevent::get_msg_type(self_t);
        if msg_type != "group" {
            return Ok(Some(rv_empty()));
        }
        let msg = self_t.get_param_text(params, 0)?;
        let user_id = self_t.get_param_text(params, 1)?;
        let old_group_id = self_t.get_exmap("群ID"); // 备份群ID
        let old_user_id = self_t.get_exmap("发送者ID"); // 备份用户ID
        self_t.set_exmap("群ID", "");
        if !user_id.is_empty() {
            self_t.set_exmap("发送者ID", &user_id);
        }
        match send_one_msg(self_t, &msg) {
            Ok(msg_id) => {
                self_t.set_exmap("群ID", &old_group_id); // 还原群ID
                self_t.set_exmap("发送者ID", &old_user_id); // 还原用户ID
                Ok(Some(rv_text(msg_id)))
            }
            Err(e) => {
                self_t.set_exmap("群ID", &old_group_id); // 还原群ID
                self_t.set_exmap("发送者ID", &old_user_id); // 还原用户ID
                let _ = cq_add_log_w(&format!("输出流失败，{}", e));
                Ok(Some(rv_empty()))
            }
        }
    });
    add_fun(vec!["清空"], |_self_t, _params| {
        Ok(Some(rv_text(CLEAR_UUID.to_string())))
    });
    add_fun(vec!["获取消息"], |self_t, params| {
        let msg_id = self_t.get_param_text(params, 0)?;
        let send_json = serde_json::json!({
            "action":"get_msg",
            "params":{
                "message_id":msg_id
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            &send_json.to_string(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取消息失败:{ret_json}");
        let raw_message = crate::mytool::json_to_cq_str(ret_json.get("data").ok_or(err)?)?;
        Ok(Some(rv_text(raw_message)))
    });
    add_fun(vec!["当前消息"], |self_t, _params| {
        let msg = self_t.get_exmap("当前消息");
        if (*msg).is_empty() {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            if let Ok(message) = json_to_cq_str(&raw_json) {
                return Ok(Some(rv_text(message)));
            }
        }
        Ok(Some(rv_text(msg.to_string())))
    });
    add_fun(vec!["输入流"], |self_t, params| {
        let tm = self_t.get_param_text(params, 0)?;
        let tm = tm.parse::<u64>().unwrap_or(15000);
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (
            std::sync::mpsc::Sender<String>,
            std::sync::mpsc::Receiver<String>,
        ) = std::sync::mpsc::channel();
        let ip = crate::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            echo: echo.clone(),
            stream_type: "输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        runtime_context().session.add_input_stream(ip);
        let _guard = scopeguard::guard(echo, |echo| {
            runtime_context().session.remove_input_stream(&echo);
        });
        let mut tm = tm;
        while tm > 1000 {
            if !pkg_can_run(&self_t.pkg_name, "输入流") {
                return Err("输入流终止，因用户要求退出".into());
            }
            let rv = rx.recv_timeout(std::time::Duration::from_secs(1));
            if let Ok(msg) = rv {
                return Ok(Some(rv_text(msg)));
            }
            tm -= 1000;
        }
        if !pkg_can_run(&self_t.pkg_name, "输入流") {
            return Err("输入流终止，因用户要求退出".into());
        }
        let rv = rx.recv_timeout(std::time::Duration::from_millis(tm));
        if let Ok(msg) = rv {
            return Ok(Some(rv_text(msg)));
        }
        let ret_str = String::new();
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["群输入流"], |self_t, params| {
        let tm = self_t.get_param_text(params, 0)?;
        let tm = tm.parse::<u64>().unwrap_or(15000);
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (
            std::sync::mpsc::Sender<String>,
            std::sync::mpsc::Receiver<String>,
        ) = std::sync::mpsc::channel();
        let ip = crate::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            echo: echo.clone(),
            stream_type: "群输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        runtime_context().session.add_input_stream(ip);
        let _guard = scopeguard::guard(echo, |echo| {
            runtime_context().session.remove_input_stream(&echo);
        });
        let mut ret_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        let mut tm = tm;
        while tm > 1000 {
            if !pkg_can_run(&self_t.pkg_name, "输入流") {
                return Err("输入流终止，因用户要求退出".into());
            }
            let rv = rx.recv_timeout(std::time::Duration::from_secs(1));
            if let Ok(msg) = rv {
                let js: serde_json::Value = serde_json::from_str(&msg)?;
                let js_obj = js.as_object().ok_or("解析JSON失败")?;
                let mut mp: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
                mp.insert(
                    "发送者ID".to_string(),
                    rv_text(
                        js_obj["发送者ID"]
                            .as_str()
                            .ok_or("解析JSON失败")?
                            .to_owned(),
                    ),
                );
                mp.insert(
                    "消息".to_string(),
                    rv_text(js_obj["消息"].as_str().ok_or("解析JSON失败")?.to_owned()),
                );
                mp.insert(
                    "消息ID".to_string(),
                    rv_text(js_obj["消息ID"].as_str().ok_or("解析JSON失败")?.to_owned()),
                );
                return Ok(Some(rv_object(mp)));
            }
            tm -= 1000;
        }
        if !pkg_can_run(&self_t.pkg_name, "输入流") {
            return Err("输入流终止，因用户要求退出".into());
        }
        let rv = rx.recv_timeout(std::time::Duration::from_millis(tm));
        if let Ok(msg) = rv {
            let js: serde_json::Value = serde_json::from_str(&msg)?;
            let js_obj = js.as_object().ok_or("解析JSON失败")?;
            ret_obj.insert(
                "发送者ID".to_string(),
                rv_text(
                    js_obj["发送者ID"]
                        .as_str()
                        .ok_or("解析JSON失败")?
                        .to_owned(),
                ),
            );
            ret_obj.insert(
                "消息".to_string(),
                rv_text(js_obj["消息"].as_str().ok_or("解析JSON失败")?.to_owned()),
            );
            ret_obj.insert(
                "消息ID".to_string(),
                rv_text(js_obj["消息ID"].as_str().ok_or("解析JSON失败")?.to_owned()),
            );
        }
        Ok(Some(rv_object(ret_obj)))
    });
    add_fun(vec!["BOT权限"], |self_t, _params| {
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("机器人ID");
        let send_json = serde_json::json!({
            "action":"get_group_member_info",
            "params":{
                "group_id":group_id.to_string(),
                "user_id":user_id.to_string()
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            &send_json.to_string(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取BOT权限失败:{ret_json}");
        let dat_json = ret_json.get("data").ok_or(err)?;
        let role = read_json_str(dat_json, "role");
        let role_ret;
        if role == "admin" {
            role_ret = "管理"
        } else if role == "owner" {
            role_ret = "群主"
        } else if role == "member" {
            role_ret = "群员"
        } else {
            return Err(RedLang::make_err("获取BOT权限失败:返回的json中无role字段"));
        }
        Ok(Some(rv_text(role_ret.to_string())))
    });
    add_fun(vec!["伪造OB事件"], |self_t, params| {
        let ob_event = self_t.get_param_text(params, 0)?;
        thread::spawn(move || {
            if let Err(e) = crate::cqevent::do_1207_event(&ob_event) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["同意"], |self_t, params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let remark = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add" || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":true,
                    "reason":"".to_owned()
                }
            });

            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        } else if request_type == "friend" {
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":true,
                    "remark":remark
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["拒绝"], |self_t, params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let reason = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add" || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":false,
                    "reason":reason
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        } else if request_type == "friend" {
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":false,
                    "remark":"".to_owned()
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["脚本输出"], |self_t, params| {
        let src_msg_id = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let key = format!("{}|{}|{}", self_t.pkg_name, self_id, src_msg_id);
        let msg_id_vec = runtime_context().session.related_message_ids(&key);
        if msg_id_vec.is_empty() {
            Ok(Some(rv_array(vec![])))
        } else {
            let ret_vec = msg_id_vec
                .iter()
                .map(|x| rv_text(x.to_string()))
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(ret_vec)))
        }
    });

    add_fun(vec!["脚本输出-增加ID"], |self_t, params| {
        let src_msg_id = self_t.get_exmap("消息ID"); // 来源消息ID
        let to_add_msg_id = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let key = format!("{}|{}|{}", self_t.pkg_name, self_id, src_msg_id);
        let tm = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        runtime_context()
            .session
            .add_related_message(key, &self_id, to_add_msg_id, tm);
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分-增加"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let add_score = self_t.get_param_text(params, 0)?.parse::<i64>()?;

        // 创建表
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        crate::resource_manager::configure_conn_busy_timeout(&conn);
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;

        // 查询积分
        let ret_rst: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?",
            [group_id.clone(), user_id.clone()],
            |row| row.get(0),
        );
        let mut ret_num: i64;
        if let Ok(ret) = ret_rst {
            ret_num = ret;
        } else {
            ret_num = 0;
        }

        // 积分变动
        ret_num = ret_num
            .checked_add(add_score)
            .ok_or_else(|| RedLang::make_err("积分-增加：积分溢出"))?;
        if ret_num < 0 {
            ret_num = 0;
        }

        // 积分设置
        conn.execute(
            "REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)",
            [group_id, user_id, ret_num.to_string()],
        )?;

        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分-设置"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let set_score = self_t.get_param_text(params, 0)?.parse::<u32>()?;

        // 创建表
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        crate::resource_manager::configure_conn_busy_timeout(&conn);
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;

        // 积分设置
        conn.execute(
            "REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)",
            [group_id, user_id, set_score.to_string()],
        )?;

        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分"], |self_t, _params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();

        // 查询积分
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        crate::resource_manager::configure_conn_busy_timeout(&conn);
        let ret_rst: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?",
            [group_id.clone(), user_id.clone()],
            |row| row.get(0),
        );
        let ret_num = ret_rst.unwrap_or(0);

        Ok(Some(rv_text(ret_num.to_string())))
    });

    add_fun(vec!["积分-排行"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);

        let limit = self_t.get_param_text(params, 0)?;
        let limit_num = if limit.is_empty() {
            10
        } else {
            limit.parse::<i32>()?
        };

        // 查询积分
        let app_dir = crate::redlang::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = crate::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        crate::resource_manager::configure_conn_busy_timeout(&conn);
        let mut stmt = conn.prepare(
            "SELECT USER_ID,VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? ORDER BY VALUE DESC LIMIT ?",
        )?;
        let mut rows = stmt.query(rusqlite::params_from_iter([
            group_id,
            limit_num.to_string(),
        ]))?;
        let mut vec: Vec<Rc<RedValue>> = vec![];
        while let Some(row) = rows.next()? {
            let mut v: Vec<Rc<RedValue>> = vec![];
            for i in 0..2 {
                let k = row.get_ref_unwrap(i);
                let dat = match k.data_type() {
                    rusqlite::types::Type::Null => rv_empty(),
                    rusqlite::types::Type::Integer => rv_text(k.as_i64().unwrap_or(0).to_string()),
                    rusqlite::types::Type::Real => rv_text(k.as_f64().unwrap_or(0.0).to_string()),
                    rusqlite::types::Type::Text => rv_text(match k.as_str() {
                        Ok(s) => s.to_owned(),
                        Err(_) => {
                            String::from_utf8_lossy(k.as_bytes().unwrap_or(&[])).to_string()
                        }
                    }),
                    rusqlite::types::Type::Blob => rv_bin(k.as_blob().unwrap_or(&[]).to_vec()),
                };
                v.push(dat);
            }
            vec.push(Rc::new(RedValue::Array(v)));
        }
        Ok(Some(Rc::new(RedValue::Array(vec))))
    });
    add_fun(vec!["机器人平台"], |self_t, _params| {
        let platform = get_platform(self_t);
        Ok(Some(rv_text(platform)))
    });
    add_fun(vec!["主人"], |self_t, params| {
        let params_len = params.len();
        let master = if params_len == 0 {
            self_t.get_exmap("发送者ID").as_str().to_owned()
        } else {
            self_t.get_param_text(params, 0)?
        };
        let master_arr_value =
            get_const_val(&self_t.pkg_name, "0b863263-484c-4447-9990-0469186b3d97")?;
        if let Some(master_arr_value) = master_arr_value {
            let master_arr_rv = master_arr_value.into_rc_value()?;
            let is_master = match &*master_arr_rv {
                RedValue::Array(arr) => arr
                    .iter()
                    .any(|item| item.as_text() == Some(master.as_str())),
                RedValue::Text(text) => text.as_str() == master.as_str(),
                _ => false,
            };
            if !is_master {
                // 等同于【返回】
                let fun_ret_vec_len = self_t.fun_ret_vec.len();
                self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
            }
        } else {
            // 等同于【返回】
            let fun_ret_vec_len = self_t.fun_ret_vec.len();
            self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["主人数组"], |self_t, _params| {
        if let Some(master_arr_value) =
            get_const_val(&self_t.pkg_name, "0b863263-484c-4447-9990-0469186b3d97")?
        {
            return Ok(Some(master_arr_value.into_rc_value()?));
        }
        Ok(Some(rv_array(vec![])))
    });
    add_fun(vec!["设置主人"], |self_t, params| {
        let master_arr = self_t.get_param_array(params, 0)?;
        // 这里仅仅为了验证是否规范
        for it in &master_arr {
            if it.get_type_name() != "文本" {
                return Err("主人数组的元素必须为全为文本类型".into());
            }
        }
        let master_arr_value = RedValueData::from_red_value(&RedValue::Array(master_arr))?;
        crate::redlang::set_const_val(
            &self_t.pkg_name,
            "0b863263-484c-4447-9990-0469186b3d97",
            master_arr_value,
        )?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["重定向"], |self_t, params| {
        let plus_name = self_t.get_param_text(params, 0)?;
        let script_json = runtime_context().scripts.read_scripts();
        for i in 0..script_json
            .as_array()
            .ok_or("script.json文件不是数组格式")?
            .len()
        {
            // 默认包可能没有包名，这个时候要认为是`""`
            let pkg_name = script_json[i]["pkg_name"].as_str().unwrap_or_default();
            let name = script_json[i]["name"].as_str().ok_or("name not str")?;
            if self_t.pkg_name != pkg_name || plus_name != name {
                continue;
            }
            let code = script_json[i]["content"]["code"]
                .as_str()
                .ok_or("脚本中code不是str")?
                .to_owned();
            let exmap = (*self_t.exmap).borrow().clone();
            let pkg_name = self_t.pkg_name.to_owned();
            let name = plus_name.to_owned();
            runtime().spawn_blocking(move || {
                let mut rl = RedLang::new();
                rl.exmap = Rc::new(RefCell::new(exmap));
                rl.pkg_name = pkg_name;
                rl.script_name = name;
                if let Err(e) = super::do_script(&mut rl, &code, "normal", false) {
                    let _ = cq_add_log_w(format!("err in do_group_msg:do_redlang:{}", e).as_str());
                }
            });
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["邮件主题"], |self_t, _params| {
        // 非 email 平台直接返回空
        let platform = get_platform(self_t);
        if platform != "email" {
            return Ok(Some(rv_empty()));
        }
        // 尝试从缓存获取
        let cached = self_t.get_exmap("邮件主题");
        if !(*cached).is_empty() {
            return Ok(Some(rv_text(cached.to_string())));
        }
        use base64::Engine;
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        let raw_message_b64 = raw_json
            .get("raw_message")
            .and_then(|v| v.as_str())
            .ok_or("raw_message 不存在或不是字符串")?;
        let raw_bytes = base64::engine::general_purpose::STANDARD.decode(raw_message_b64)?;
        let parsed = mail_parser::MessageParser::default()
            .parse(&raw_bytes)
            .ok_or("邮件解析失败")?;
        let subject = parsed.subject().unwrap_or("").to_string();
        // 缓存结果
        self_t.set_exmap("邮件主题", &subject);
        Ok(Some(rv_text(subject)))
    });
}
