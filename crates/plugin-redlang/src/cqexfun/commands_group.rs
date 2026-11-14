use super::*;
use crate::bot_commands::send_options;

pub(super) fn register() {
    add_fun(vec!["取群列表", "群列表"], |self_t, params| {
        let groups_id = self_t.get_exmap("群组ID");
        let send_json = if get_platform(self_t) == "kook" {
            let explicit = self_t.get_param_text(params, 0)?;
            let guild_id = if explicit.is_empty() {
                groups_id.as_str()
            } else {
                &explicit
            };
            serde_json::json!({
                "action":"get_group_list",
                "params":{"groups_id":guild_id,"group_id":self_t.get_exmap("群ID").as_str()},
            })
        } else if groups_id.is_empty() {
            serde_json::json!({
                "action":"get_group_list",
                "params":{}
            })
        } else {
            serde_json::json!({
                "action":"get_group_list",
                "params":{
                    "groups_id":groups_id.as_str()
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
        if platform == "telegram" && self_t.get_param_text(params, 1)? == "文件ID列表" {
            let offset = self_t.get_param_text(params, 2)?;
            let limit = self_t.get_param_text(params, 3)?;
            return crate::bot_commands::management::telegram_avatars(
                self_t, &user_id, &offset, &limit,
            );
        }
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
        if let Some(name) = bot_name.as_str() {
            if name.is_empty() {
                return Ok(Some(rv_text("露娜sama".to_owned())));
            }
            Ok(Some(rv_text(name.to_owned())))
        } else {
            Ok(Some(rv_text("露娜sama".to_owned())))
        }
    });
    add_fun(vec!["权限", "发送者权限"], |self_t, _params| {
        let user = self_t.get_exmap("发送者ID");
        if get_platform(self_t) == "telegram"
            && !self_t.get_exmap("群ID").is_empty()
            && user.parse::<i64>().is_ok_and(|id| id > 0)
        {
            return group_member_role(self_t, &user);
        }
        Ok(Some(rv_text(self_t.get_exmap("发送者权限").to_string())))
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
        let option = (get_platform(self_t) == "telegram" && params.len() > 1)
            .then(|| self_t.get_param_text(params, 1))
            .transpose()?;
        if option.as_deref() == Some("便笺") {
            return telegram_media::segment(self_t, params, &pic_rv, "voice");
        }
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:record,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = match option {
                    Some(option) => option,
                    None => self_t.get_param_text(params, 1)?,
                };
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
    add_fun(vec!["媒体组"], |rl, params| {
        management_platform(rl)?;
        let items = rl.get_param(params, 0)?;
        let group = native_args::media_group(&items)?;
        let content = cq_params_encode(&group.to_string());
        Ok(Some(rv_text(format!("[CQ:media_group,content={content}]"))))
    });
    add_fun(vec!["贴纸"], |rl, params| {
        if get_platform(rl) != "telegram" {
            return Err(RedLang::make_err("【贴纸】需要 Telegram 机器人来源"));
        }
        let source = rl.get_param(params, 0)?;
        let filename = rl.get_param_text(params, 1)?;
        let emoji = rl.get_param_text(params, 2)?;
        let data = if let RedValue::Bin(bytes) = source.as_ref() {
            let lower = filename.to_ascii_lowercase();
            let mime = if lower.ends_with(".webp") {
                "image/webp"
            } else if lower.ends_with(".tgs") {
                "application/x-tgsticker"
            } else if lower.ends_with(".webm") {
                "video/webm"
            } else {
                return Err(RedLang::make_err(
                    "Telegram 贴纸上传文件名需以 .webp、.tgs 或 .webm 结尾",
                ));
            };
            let mut data =
                serde_json::json!({"upload":native_args::file_part(&filename, bytes, Some(mime))});
            if !emoji.is_empty() {
                data["emoji"] = emoji.into();
            }
            data
        } else {
            let file = source.expect_text_value()?;
            if file.is_empty() || !filename.is_empty() || !emoji.is_empty() {
                return Err(RedLang::make_err(
                    "文件 ID 或链接只填第一项，文件名和表情用于上传字节集",
                ));
            }
            serde_json::json!({"file_id":file})
        };
        Ok(Some(rv_text(format!(
            "[CQ:sticker,content={}]",
            cq_params_encode(&data.to_string())
        ))))
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
        let option = (get_platform(self_t) == "telegram" && params.len() > 1)
            .then(|| self_t.get_param_text(params, 1))
            .transpose()?;
        if option.as_deref() == Some("便笺") {
            return telegram_media::segment(self_t, params, &pic_rv, "video_note");
        }
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:video,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = match option {
                    Some(option) => option,
                    None => self_t.get_param_text(params, 1)?,
                };
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
    add_fun(vec!["动画"], |rl, params| {
        let source = rl.get_param(params, 0)?;
        telegram_media::segment(rl, params, &source, "animation")
    });
    add_fun(vec!["撤回"], |self_t, params| {
        let msg_id_rv = self_t.get_param(params, 0)?;
        let platform = get_platform(self_t);
        if platform == "telegram" && params.len() > 1 {
            let business_id = self_t.get_param_text(params, 1)?;
            if business_id == "临时" {
                let id = msg_id_rv.expect_text_value()?;
                let receiver = self_t.get_param_text(params, 2)?;
                let chat = self_t.get_param_text(params, 3)?;
                let body = send_options::ephemeral_target(self_t, &id, &receiver, &chat)?;
                let result = call_platform_api(
                    self_t,
                    "telegram",
                    "telegram_api",
                    serde_json::json!({
                        "method":"deleteEphemeralMessage","body":body
                    }),
                )?;
                return check_native_command(result, "撤回临时消息");
            }
            if !business_id.is_empty() {
                let ids = message_handle_list(&msg_id_rv)?;
                if !(1..=100).contains(&ids.len()) {
                    return Err(RedLang::make_err(
                        "按平台ID撤回商务消息需要 1 至 100 条同一会话消息",
                    ));
                }
                let ids = ids
                    .into_iter()
                    .map(|id| {
                        id.parse::<i64>()
                            .ok()
                            .filter(|id| *id > 0)
                            .ok_or_else(|| RedLang::make_err("平台消息ID需要正整数"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let result = call_platform_api(
                    self_t,
                    "telegram",
                    "telegram_api",
                    serde_json::json!({
                        "method":"deleteBusinessMessages","body":{"business_connection_id":business_id,"message_ids":ids}
                    }),
                )?;
                return check_native_command(result, "撤回商务消息");
            }
        }
        if matches!(platform.as_str(), "kook" | "telegram")
            && matches!(msg_id_rv.as_ref(), RedValue::Array(_))
        {
            let ids = message_handle_list(&msg_id_rv)?;
            return call_platform_api(
                self_t,
                &platform,
                "delete_msgs",
                serde_json::json!({"message_ids":ids}),
            );
        }
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
    add_fun(vec!["编辑消息"], |rl, params| {
        let platform = management_platform(rl)?;
        let mut id = rl.get_param_text(params, 0)?;
        let explicit_id = !id.is_empty();
        if id.is_empty() {
            id = rl.get_exmap("消息ID").to_string();
        }
        let content = rl.get_param(params, 1)?;
        let kind = rl.get_param_text(params, 2)?;
        if platform == "kook" {
            let (mode, mut body) = match kind.as_str() {
                "" | "文本" => (
                    "text",
                    serde_json::json!({"content": content.expect_text_value()?}),
                ),
                "卡片" => (
                    "card",
                    serde_json::json!({"content": crate::bot_commands::card_for_edit(&content)?}),
                ),
                "模板" => (
                    "template",
                    crate::bot_commands::management::templates::template_message(&content)?,
                ),
                _ => return Err(RedLang::make_err("KOOK 支持编辑文本、卡片或模板")),
            };
            let input = rl.get_param(params, 3)?;
            let options = match input.as_ref() {
                RedValue::Object(_) => native_args::object(&input)?,
                RedValue::Text(value) if !value.is_empty() => native_args::object(&input)?,
                _ => crate::bot_commands::management::kook_message_options(rl, &input, true)?,
            };
            for (key, value) in options.as_object().ok_or("消息选项需要数组")? {
                body[key] = value.clone();
            }
            return call_platform_api(
                rl,
                &platform,
                "edit_msg",
                serde_json::json!({"message_id":id,"kind":mode,"body":body}),
            );
        }
        let mut body = if params.len() > 3 {
            let options = rl.get_param(params, 3)?;
            match options.as_ref() {
                RedValue::Text(text) if text.is_empty() => serde_json::json!({}),
                RedValue::Array(_) => send_options::edit_fields(&options, &kind)?,
                RedValue::Text(text) if platform == "telegram" => {
                    let mode = match text.as_str() {
                        "Markdown" => "MarkdownV2",
                        "HTML" => "HTML",
                        _ => return Err(RedLang::make_err("格式请填 Markdown、HTML 或留空")),
                    };
                    serde_json::json!({"parse_mode":mode})
                }
                _ => native_args::object(&options)?,
            }
        } else {
            serde_json::json!({})
        };
        let mut upload = None;
        let (kind, field, value) = match (platform.as_str(), kind.as_str()) {
            ("telegram", "富文本") => {
                let data = crate::bot_commands::rich::decode(&content)?;
                body = serde_json::json!({});
                if data["files"]
                    .as_object()
                    .is_some_and(|files| !files.is_empty())
                {
                    upload = Some(data["files"].clone());
                }
                ("text", "rich_message", data["body"].clone())
            }
            ("telegram", "" | "文本") => ("text", "text", content.expect_text_value()?.into()),
            ("telegram", "说明") => ("caption", "caption", content.expect_text_value()?.into()),
            ("telegram", "键盘") => (
                "keyboard",
                "reply_markup",
                crate::bot_commands::edit_keyboard(&content)?,
            ),
            ("telegram", "媒体") if content.as_array().is_some() => {
                let (mut media, files) = native_args::media_item(&content, 0, true)?;
                if let Some(mode) = body
                    .as_object_mut()
                    .and_then(|obj| obj.remove("parse_mode"))
                {
                    media["parse_mode"] = mode;
                }
                if let Some(above) = body
                    .as_object_mut()
                    .and_then(|obj| obj.remove("show_caption_above_media"))
                {
                    if !matches!(
                        media["type"].as_str(),
                        Some("photo" | "animation" | "video")
                    ) {
                        return Err("说明位置适用于图片、动画和视频".into());
                    }
                    media["show_caption_above_media"] = above;
                }
                if !files.is_empty() {
                    upload = Some(serde_json::Value::Object(files));
                }
                ("media", "media", media)
            }
            ("telegram", "媒体") => ("media", "media", native_args::object(&content)?),
            ("telegram", "清单") => ("checklist", "checklist", business::checklist(&content)?),
            ("telegram", "实时位置") => {
                let values = content
                    .as_array()
                    .ok_or("新位置请用【数组@经度@纬度@精度米数@有效秒数@方向角@接近提醒米数】")?;
                body = crate::bot_commands::management::location::live_edit_fields(values)?;
                ("live_location", "", serde_json::Value::Null)
            }
            ("telegram", "停止位置") => {
                if !matches!(content.as_ref(), RedValue::Text(text) if text.is_empty()) {
                    return Err(RedLang::make_err("停止实时位置时第二项留空"));
                }
                ("stop_location", "", serde_json::Value::Null)
            }
            _ => return Err(RedLang::make_err("当前平台不支持这种编辑类型")),
        };
        if !field.is_empty() {
            body[field] = value;
        }
        let mut request = serde_json::json!({"message_id":id,"kind":kind,"body":body});
        if platform == "telegram" {
            let attachments = rl.get_param(params, 4)?;
            match attachments.as_ref() {
                RedValue::Text(text) if text.is_empty() => (),
                RedValue::Object(fields) if fields.contains_key("inline_keyboard") => {
                    request["body"]["reply_markup"] =
                        crate::bot_commands::edit_keyboard(&attachments)?;
                }
                _ if upload.is_some() => return Err(RedLang::make_err("媒体数组已经包含上传内容")),
                _ => upload = native_args::files_value(&attachments)?,
            }
            if let Some(files) = upload {
                request["files"] = files;
            }
        }
        match rl.get_param_text(params, 5)?.as_str() {
            "" => (),
            "临时" if platform == "telegram" => {
                let receiver = rl.get_param_text(params, 6)?;
                let target = rl.get_param_text(params, 7)?;
                let id = if explicit_id { id.as_str() } else { "" };
                let addresses = send_options::ephemeral_target(rl, id, &receiver, &target)?;
                for (key, value) in addresses.as_object().ok_or("临时消息目标格式错误")? {
                    request["body"][key] = value.clone();
                }
                let method = match kind {
                    "text" => "editEphemeralMessageText",
                    "caption" => "editEphemeralMessageCaption",
                    "keyboard" => "editEphemeralMessageReplyMarkup",
                    "media" => "editEphemeralMessageMedia",
                    _ => return Err("临时消息可编辑文本、说明、键盘或媒体".into()),
                };
                request["method"] = method.into();
                return call_platform_api(rl, &platform, "telegram_api", request);
            }
            "行内" if platform == "telegram" => {
                if !explicit_id {
                    return Err(RedLang::make_err("编辑行内消息需填写平台行内消息ID"));
                }
                if request.get("files").is_some()
                    || crate::bot_commands::rich::requires_upload(&request["body"]["rich_message"])
                {
                    return Err(RedLang::make_err(
                        "Telegram 行内媒体编辑请使用已有文件ID或直链",
                    ));
                }
                if kind == "checklist" {
                    return Err(RedLang::make_err("清单编辑需要商务消息的 RedReply 消息ID"));
                }
                let method = match kind {
                    "text" => "editMessageText",
                    "caption" => "editMessageCaption",
                    "keyboard" => "editMessageReplyMarkup",
                    "live_location" => "editMessageLiveLocation",
                    "stop_location" => "stopMessageLiveLocation",
                    _ => "editMessageMedia",
                };
                let mut body = request["body"].take();
                body["inline_message_id"] = id.into();
                return call_platform_api(
                    rl,
                    &platform,
                    "telegram_api",
                    serde_json::json!({"method":method,"body":body}),
                );
            }
            _ => {
                return Err(RedLang::make_err(
                    "Telegram 编辑目标类型请留空或填行内、临时",
                ))
            }
        }
        call_platform_api(rl, &platform, "edit_msg", request)
    });
    add_fun(vec!["转发消息"], |rl, params| {
        let platform = management_platform(rl)?;
        let source = rl.get_param(params, 0)?;
        let ids = match source.as_ref() {
            RedValue::Text(text) if text.is_empty() => vec![rl.get_exmap("消息ID").to_string()],
            _ => message_handle_list(&source)?,
        };
        let mut target = rl.get_param_text(params, 1)?;
        let kind = rl.get_param_text(params, 2)?;
        let is_private = match kind.as_str() {
            "" => crate::cqevent::get_msg_type(rl) == "private",
            "群聊" | "频道" => false,
            "私聊" => true,
            _ => return Err(RedLang::make_err("转发目标类型需为群聊、频道或私聊")),
        };
        if target.is_empty() {
            target = rl
                .get_exmap(if is_private { "发送者ID" } else { "群ID" })
                .to_string();
        }
        let options = rl.get_param(params, 3)?;
        let options = match options.as_ref() {
            RedValue::Text(copy) => {
                let silent = rl.get_param_text(params, 4)?;
                for value in [copy.as_str(), silent.as_str()] {
                    if !matches!(value, "" | "真" | "假") {
                        return Err(RedLang::make_err("复制和静默请填真或假"));
                    }
                }
                serde_json::json!({"复制":copy.as_str(),"静默":silent,
                    "话题ID":rl.get_param_text(params, 5)?,"话题类型":rl.get_param_text(params, 6)?})
            }
            _ => native_args::object(&options)?,
        };
        if ids.is_empty() || ids.iter().any(|id| id.is_empty()) || target.is_empty() {
            return Err(RedLang::make_err("转发需要消息 ID 和目标聊天"));
        }
        let mut request = serde_json::json!({
            "message_ids":ids,"target_id":target,"private":is_private,
            "copy":options["复制"] == "真","silent":options["静默"] == "真",
        });
        if platform == "telegram" {
            let read_option = |key| {
                options
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            };
            if let Some((field, id)) =
                telegram_output_topic(read_option("话题ID"), read_option("话题类型"))?
            {
                request[field] = id.into();
            } else if let Ok(event) = serde_json::from_str(&rl.get_exmap("原始事件")) {
                apply_telegram_topic(&event, &rl.get_exmap("机器人ID"), &target, &mut request);
            }
            let extra = send_options::parse(rl, params, 7)?;
            if extra.get("ephemeral_message_parameters").is_some()
                || extra.get("reply_parameters").is_some()
            {
                return Err("转发和复制的发送选项不支持临时消息或高级引用".into());
            }
            request["send_options"] = extra;
        }
        call_platform_api(rl, &platform, "forward_msgs", request)
    });
}
