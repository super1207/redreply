use std::sync::Once;

fn init_redlang_test_funs() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        crate::core::init_core_fun_map();
        crate::cqexfun::init_cq_ex_fun_map();
        crate::exfun::init_ex_fun_map();
    });
}

#[test]
fn test_cqstr_to_arr() {
    use redlang::mytool::str_msg_to_arr;
    let js = serde_json::json!("hello,world[CQ:image,file=xxx.png]hello,world");
    let ret = str_msg_to_arr(&js).unwrap();
    println!("test_cqstr_to_arr ret:`{}`", ret);
}

#[test]
fn test_cqparse() {
    use std::collections::BTreeMap;
    let data_str = "[CQ:image,file=620a6c143114a4feaaf9e89cc83162b6.image,subType=0,url=https://gchat.qpic.cn/]";
    let pos1 = data_str.find(",").ok_or("CQ码解析失败").unwrap();
    let tp = data_str.get(4..pos1).ok_or("CQ码解析失败").unwrap();
    let mut sub_key_obj: BTreeMap<String, String> = BTreeMap::new();
    sub_key_obj.insert("type".to_string(), tp.to_string());
    let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])").unwrap();

    for cap_iter in re.captures_iter(data_str) {
        let cap = cap_iter.unwrap();
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
            sub_key_obj.insert(key, val);
        }
    }
    println!("{:?}", sub_key_obj);
}

#[test]
fn test_structured_const_keeps_value_shape() {
    init_redlang_test_funs();
    let pkg_name = "test_structured_const_keeps_value_shape";
    crate::script_manager::del_pkg_memory(pkg_name);

    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut rl = crate::core::RedLang::new();
        rl.pkg_name = pkg_name.to_string();
        let ret = rl.parse(
            "【定义常量@结构常量@【对象@列表@【数组@甲@乙】@对象@【对象@键@值】】】\
             【取类型@【取元素@【常量@结构常量】@列表】】|\
             【取元素@【取元素@【常量@结构常量】@列表】@1】|\
             【取元素@【取元素@【常量@结构常量】@对象】@键】",
        )?;
        match &*ret {
            crate::core::RedValue::Text(s) => Ok(s.to_string()),
            other => Err(format!("expected text result, got {}", other.get_type_name()).into()),
        }
    })();

    crate::script_manager::del_pkg_memory(pkg_name);
    assert_eq!(result.unwrap(), "A|乙|值");
}

#[test]
fn test_structured_runtime_operations() {
    init_redlang_test_funs();
    let pkg_name = "test_structured_runtime_operations";
    crate::script_manager::del_pkg_memory(pkg_name);

    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut rl = crate::core::RedLang::new();
        rl.pkg_name = pkg_name.to_string();
        let ret = rl.parse(
            "【取长度@【截取@【数组@1@2@3】@1@2】】|\
             【取元素@【取元素@【JSON解析@【对象@列表@【数组@甲@乙】】】@列表】@1】|\
             【计数@【数组@甲@乙@甲】@甲】|\
             【取元素@【翻转@【数组@甲@乙】】@0】",
        )?;
        match &*ret {
            crate::core::RedValue::Text(s) => Ok(s.to_string()),
            other => Err(format!("expected text result, got {}", other.get_type_name()).into()),
        }
    })();

    crate::script_manager::del_pkg_memory(pkg_name);
    assert_eq!(result.unwrap(), "2|乙|2|乙");
}

#[test]
fn test_parse_deep_nesting_returns_err_not_crash() {
    init_redlang_test_funs();
    let deep = format!("{}{}", "【甲@".repeat(1000), "】".repeat(1000));
    let mut rl = crate::core::RedLang::new();
    let ret = rl.parse(&deep);
    assert!(
        ret.is_err(),
        "深嵌套输入应返回错误而不是栈溢出崩溃"
    );
}

#[test]
fn test_run_script_shares_call_depth_counter() {
    init_redlang_test_funs();
    // 自递归脚本：所有实参都读取“参数@1”，子实例帧大小恒定且 code 永远取到 body 本身，
    // 递归无衰减。共享深度计数后应在 501 层报错，而不是栈溢出崩溃。
    let body = "【运行脚本@【参数@1】@【参数@1】@【参数@1】】";
    let esc_body = body
        .replace('\\', "\\\\")
        .replace('【', "\\【")
        .replace('】', "\\】")
        .replace('@', "\\@");
    let code = format!("【运行脚本@{esc_body}@{esc_body}】");
    let mut rl = crate::core::RedLang::new();
    let ret = rl.parse(&code);
    assert!(
        ret.is_err(),
        "嵌套“运行脚本”应受共享深度计数保护，返回错误而不是栈溢出崩溃"
    );
}

// #[test]
// fn test_wav_to_pcm() {
//     let wav_info = redlang::mytool::wav_to_pcm::WavFormat::decode("D:\\青雀语音\\71201001_cn.wav").unwrap();

//    let bits_per_sample = u16::from_le_bytes(wav_info.bits_per_sample) / 8 * u16::from_le_bytes(wav_info.num_channels);
//    let sample_gap = (u32::from_le_bytes(wav_info.sampling_rate) as f64) / 32000.0;
//    let mut real_pos = 0f64;
//    let mut new_data = vec![];
//    loop {
//     let index = ((real_pos as usize) / 2) * 2;
//     let index2 = index+(bits_per_sample as usize);
//     if index2 > wav_info.data.len() {
//         break;
//     }
//     let sample = &wav_info.data[index..index2];

//     for i in 0..bits_per_sample {
//         let d = sample[i as usize];
//         new_data.push(d);
//     }
//     real_pos += bits_per_sample as f64 * sample_gap;
//    }

//    println!("wav_info:{:?}",new_data.len());
//    let mut f = std::fs::File::create("D:\\青雀语音\\71201001_cn.pcm").unwrap();
//     std::io::Write::write_all(&mut f, &new_data).unwrap();
//     let input = std::fs::read("D:\\青雀语音\\71201001_cn.pcm").unwrap();
//     let output = silk_rs::encode_silk(input, 32000, 32000, true).unwrap();
//     std::fs::write("D:\\青雀语音\\71201001_cn.silk", output).unwrap();
// }



#[test]
fn native_platform_commands_are_registered_and_validate_their_inputs() {
    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    for command in [
        "【Telegram调用@getMe】",
        "【Telegram下载文件@file-id】",
        "【KOOK调用@GET@voice/list】",
        "【KOOK共享播放@channel@https://example.org/song.mp3@65】",
        "【KOOK停止播放】",
        "【KOOK播放状态】",
    ] {
        let error = rl.parse(command).unwrap_err();
        assert!(error.to_string().contains("机器人来源"), "{error}");
    }
    rl.set_exmap("机器人平台", "telegram");
    let error = rl.parse("【Telegram调用@setMyCommands@【@[]】】").unwrap_err();
    assert!(error.to_string().contains("JSON 对象"), "{error}");
    let error = rl.parse("【Telegram调用@setMyCommands@【@{\"commands\":[]}】】").unwrap_err();
    assert!(error.to_string().contains("指定机器人ID"), "{error}");
}

#[test]
fn documented_native_platform_examples_parse() {
    let document = include_str!("../../../docs/adaptar/native-bot-api.md");
    let mut count = 0;
    for block in document.split("```text\n").skip(1) {
        let code = block.split_once("```").unwrap().0;
        assert!(crate::core::astparser::parse_to_ast(code).is_ok(), "{code}");
        for raw in ["【Telegram调用", "【KOOK调用", "【OB调用", "【接口值"] {
            assert!(!code.contains(raw), "{code}");
        }
        count += 1;
    }
    assert!(count >= 13);
}

#[test]
fn rich_inline_and_media_edits_build_positional_requests() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::json;
    use std::{cell::RefCell,rc::Rc};
    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台","telegram");
    rl.set_exmap("机器人ID","7");
    rl.set_exmap("发送者ID","42");
    let records = Rc::new(RefCell::new(Vec::new()));
    let copy = records.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |_, _, args| {
            copy.borrow_mut().push(args);
            json!({"retcode":0,"data":{"message_id":"sent","id":"prepared","expiration_date":10}})
        }));
    });
    let run = |rl: &mut crate::core::RedLang, script: &str| rl.parse(script).unwrap_or_else(|error| panic!("{script}: {error}"));
    run(&mut rl,"【定义变量@表@【数组@表格@【数组@【数组@日期@地点】@【数组@周六@码头】】@真】】");
    run(&mut rl,"【定义变量@体@【Telegram富文本@【数组@【变量@表】@【数组@地图@114@22@10】@【数组@视频@file-id】】@内容块】】");
    run(&mut rl,"【Telegram应答查询@【数组@【数组@event@活动@【变量@体】@@@富文本@【Telegram键盘@【数组@【数组@【数组@报名@join】】】】】】@@@@query-id】");
    assert!(rl.parse("【Telegram应答查询@【数组@【数组@event@活动@【Telegram富文本@【数组@【数组@视频@https://example.org/video】】@内容块】@@@富文本】】@@@@query-id】").is_err());
    run(&mut rl,"【Telegram准备行内消息@【数组@event@活动@【变量@体】@@@富文本】@@真】");
    rl.set_exmap("原始事件",&json!({"platform":"telegram","sub_type":"chosen_inline_result","telegram":{
        "result_id":"event","inline_message_id":"inline-id","query":"query","from":{"id":42}}}).to_string());
    assert_eq!(run(&mut rl,"【取元素@【事件内容@行内结果】@结果ID】").expect_text_value().unwrap(),"event");
    run(&mut rl,"【编辑消息@@【变量@体】@富文本@@@行内】");
    run(&mut rl,"【编辑消息@old@【数组@视频@file-id@说明@@【数组@30@640@360@真】】@媒体】");
    run(&mut rl,"【转发消息@12@-1001@群聊@真@@@@@【数组@10@新版说明@@@@@-1002】】");
    rl.set_exmap("机器人平台","kook");
    rl.set_exmap("原始事件",&json!({"platform":"kook","kook":{"type":3,"extra":{"attachments":{
        "name":"movie.mp4","url":"https://example.org/movie.mp4","width":640,"height":360,"duration":2.5}}}}).to_string());
    assert_eq!(run(&mut rl,"【取元素@【事件内容@视频】@时长】").expect_text_value().unwrap(),"2.5");
    rl.set_exmap("原始事件",&json!({"platform":"kook","kook":{"type":10,"content":json!([{"modules":[{
        "type":"audio","src":"https://example.org/music","title":"歌曲","cover":"https://example.org/art"}]}]).to_string()}}).to_string());
    assert_eq!(run(&mut rl,"【取元素@【事件内容@媒体列表】@0@标题】").expect_text_value().unwrap(),"歌曲");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler=None);
    let calls = records.borrow();
    assert_eq!(calls.len(),5);
    let result = &calls[0]["body"]["results"][0];
    assert_eq!(result["input_message_content"]["rich_message"]["blocks"][0]["cells"][0][0]["text"],"日期");
    assert_eq!(result["input_message_content"]["rich_message"]["blocks"][2]["video"]["media"],"file-id");
    assert_eq!(result["reply_markup"]["inline_keyboard"][0][0]["callback_data"],"join");
    assert_eq!(calls[1]["method"],"savePreparedInlineMessage");
    assert_eq!(calls[2]["body"]["inline_message_id"],"inline-id");
    assert_eq!(calls[3]["body"]["media"]["has_spoiler"],true);
    assert_eq!(calls[4]["forward_options"],json!({"video_start_timestamp":10,"caption":"新版说明","from_chat_id":-1002}));
}

#[test]
fn rich_messages_replies_and_kook_fields_use_positional_scripts() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台", "telegram");
    rl.set_exmap("机器人ID", "7");
    rl.set_exmap("发送者ID", "42");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |_, action, args| {
            let data = match args["method"].as_str() {
                Some("getMyCommands") => json!([{"command":"help","description":"帮助","is_ephemeral":true}]),
                _ if args["endpoint"] == "message/view" => json!({"id":"88","embeds":[{"type":"bili-video","title":"视频"}]}),
                _ => json!({"message_id":"sent"}),
            };
            record.borrow_mut().push((action.to_owned(), args));
            json!({"retcode":0,"data":data})
        }));
    });
    let run = |rl: &mut crate::core::RedLang, script: &str| {
        rl.parse(script).unwrap_or_else(|error| panic!("{script}: {error}"))
    };
    run(&mut rl, "【Telegram命令菜单@设置@【数组@【数组@help@帮助@真】】@所有群聊】");
    assert_eq!(run(&mut rl, "【取元素@【Telegram命令菜单@查询详情@所有群聊】@0@2】").expect_text_value().unwrap(), "真");
    run(&mut rl, "【输出流@【Telegram富文本@**文字**】@@@@@@【数组@@@@@@【数组@99@@文字@0】】】");
    assert!(rl.parse("【输出流@回复@@@@@@【数组@@@@@@【数组@99@@😀@@@@@@@【数组@【数组@粗体@1@1】】】】】").is_err());
    run(&mut rl, "【输出流@回复@@@@@@【数组@@@@@@【数组@99@@😀@@@@@@@【数组@【数组@粗体@0@2】】】】】");
    run(&mut rl, "【Telegram富文本草稿@【Telegram富文本@【数组@【数组@标题@准备中@2】】@内容块】@101@真】");
    run(&mut rl, "【编辑消息@sent@【Telegram富文本@<b>新内容</b>@HTML】@富文本】");
    run(&mut rl, "【编辑消息@sent@新内容@文本@【数组@@【数组@真】】】");
    rl.set_exmap("原始事件", &json!({"platform":"telegram","sub_type":"rich_message",
        "telegram":{"rich_message":{"blocks":[{"type":"heading","text":[{"type":"bold","text":"标题"}]},{"type":"paragraph","text":"正文"}]}}}).to_string());
    assert_eq!(run(&mut rl, "【取元素@【事件内容@富文本】@正文】").expect_text_value().unwrap(), "标题\n正文");

    rl.set_exmap("机器人平台", "kook");
    rl.set_exmap("原始事件", &json!({"platform":"kook","sub_type":"guild_member_online",
        "kook":{"user_id":"42","guilds":["77"],"event_time":1234}}).to_string());
    assert_eq!(run(&mut rl, "【取元素@【事件内容@服务器事件】@共同服务器ID列表@0】").expect_text_value().unwrap(), "77");
    assert_eq!(run(&mut rl, "【取元素@【KOOK频道消息@详情@88】@链接预览@0@类型】").expect_text_value().unwrap(), "哔哩哔哩视频");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let calls = calls.borrow();
    assert_eq!(calls.len(), 8);
    assert_eq!(calls[0].1["body"]["commands"][0]["is_ephemeral"], true);
    let rich: Value = serde_json::from_str(calls[2].1["message"][0]["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(rich["body"]["markdown"], "**文字**");
    assert_eq!(calls[2].1["send_options"]["reply_parameters"], json!({"message_id":99,"quote":"文字","quote_position":0}));
    assert_eq!(calls[3].1["send_options"]["reply_parameters"]["quote_entities"], json!([{"type":"bold","offset":0,"length":2}]));
    assert_eq!(calls[4].1["body"]["rich_message"]["blocks"][0], json!({"type":"heading","text":"准备中","size":2}));
    assert_eq!(calls[5].1["body"]["rich_message"]["html"], "<b>新内容</b>");
    assert_eq!(calls[6].1["body"]["link_preview_options"]["is_disabled"], true);
}

#[test]
fn redlang_send_options_keep_targeting_and_platform_fields() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("群ID", "-100123");
    rl.set_exmap("发送者ID", "49");
    rl.set_exmap("机器人ID", "7");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |platform, action, args| {
            let data = match args["endpoint"].as_str() {
                Some("channel/create") => json!({"id":"123"}),
                Some("guild/user-list" | "direct-message/list") => json!({"items":[]}),
                _ => json!({"message_id":"sent"}),
            };
            record.borrow_mut().push((platform.to_owned(), action.to_owned(), args));
            json!({"retcode":0,"data":data})
        }));
    });
    let run = |rl: &mut crate::core::RedLang, script: &str| {
        rl.parse(script).unwrap_or_else(|error| panic!("{script}: {error}"))
    };
    rl.set_exmap("机器人平台", "telegram");
    run(&mut rl, "【输出流@更新@@@@@@【数组@真@真】】");
    run(&mut rl, "【输出流@建议@@456@频道私信@@@【数组@@@@【数组@星币@25】】】");
    run(&mut rl, "【输出流@限当前用户@@@@@@【数组@@@@@【数组@49】】】");
    assert!(rl.parse("【输出流@建议@@456@频道私信@@@【数组@@@@【数组@星币@1】】】").is_err());
    rl.set_exmap("原始事件", &json!({"self_id":"7","chat_id":"-100123","sub_type":"callback_query",
        "telegram":{"id":"token","from":{"id":49}}}).to_string());
    run(&mut rl, "【输出流@回调@@@@@@【数组@@@@@【数组@@当前@真】】】");
    rl.set_exmap("原始事件", &json!({"self_id":"7","chat_id":"-100123","sub_type":"ephemeral_message",
        "telegram":{"chat":{"id":-100123},"ephemeral_message_id":91,"receiver_user":{"id":49},"from":{"id":49}}}).to_string());
    run(&mut rl, "【输出流@普通回复不会公开发送】");
    run(&mut rl, "【输出流@临时回复@@@@@@【数组@@@@@【数组】】】");
    run(&mut rl, "【编辑消息@@新消息@@@@临时】");
    run(&mut rl, "【撤回@@临时】");
    rl.set_exmap("原始事件", &json!({"sub_type":"suggested_post_info",
        "telegram":{"chat":{"id":-100123},"message_id":21,"suggested_post_info":{"state":"pending"}}}).to_string());
    assert_eq!(run(&mut rl, "【取元素@【事件内容@建议帖】@状态】").expect_text_value().unwrap(), "待审核");
    run(&mut rl, "【同意】");
    run(&mut rl, "【拒绝@请重试】");

    rl.set_exmap("机器人平台", "kook");
    rl.set_exmap("群组ID", "77");
    rl.set_exmap("原始事件", &json!({"kook_message_id":"native-123"}).to_string());
    run(&mut rl, "【输出流@限当前用户@【数组@当前@任务标记@当前】】");
    run(&mut rl, "【发送私聊@回执@@【数组@@回执@@会话码】】");
    run(&mut rl, "【编辑消息@old@新内容@文本@【数组@当前@清空@当前】】");
    run(&mut rl, "【KOOK频道@创建@活动语音@语音@@@30@高质量】");
    run(&mut rl, "【KOOK频道@编辑@频道ID@@@@@20@40@流畅@清空】");
    run(&mut rl, "【群成员@列表@@1@@@30@已认证@降序@升序@123】");
    run(&mut rl, "【KOOK频道消息@最新@10@@@私聊@会话码@2】");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 16);
    assert_eq!(calls[0].2["send_options"], json!({"disable_notification":true,"protect_content":true}));
    assert_eq!(calls[1].2["direct_messages_topic_id"], 456);
    assert_eq!(calls[1].2["send_options"]["suggested_post_parameters"]["price"], json!({"currency":"XTR","amount":25}));
    assert_eq!(calls[2].2["send_options"]["ephemeral_message_parameters"], json!({"receiver_user_id":49}));
    assert_eq!(calls[3].2["send_options"]["ephemeral_message_parameters"], json!({"callback_query_id":"token","replace_callback_query_message":true}));
    assert_eq!(calls[4].2["send_options"]["ephemeral_reply_id"], 91);
    assert_eq!(calls[5].2["method"], "editEphemeralMessageText");
    assert_eq!(calls[6].2["method"], "deleteEphemeralMessage");
    assert_eq!(calls[7].2["method"], "approveSuggestedPost");
    assert_eq!(calls[8].2["body"]["comment"], "请重试");
    assert_eq!(calls[9].2["send_options"], json!({"temp_target_id":"49","nonce":"任务标记","reply_msg_id":"native-123"}));
    assert_eq!(calls[10].2["send_options"]["chat_code"], "会话码");
    assert_eq!(calls[11].2["body"], json!({"content":"新内容","temp_target_id":"49","quote":"","reply_msg_id":"native-123"}));
    assert_eq!(calls[12].2["body"], json!({"guild_id":"77","name":"活动语音","type":2,"limit_amount":30,"voice_quality":"3"}));
    assert_eq!(calls[13].2["body"], json!({"channel_id":"频道ID","level":20,"limit_amount":40,"voice_quality":"1","password":""}));
    assert_eq!(calls[14].2["body"], json!({"guild_id":"77","page":1,"page_size":30,"mobile_verified":1,"active_time":1,"joined_at":0,"channel_id":"123"}));
    assert_eq!(calls[15].2["body"], json!({"chat_code":"会话码","page_size":10,"page":2}));
}

#[test]
fn kook_intimacy_badge_and_activity_use_positional_values() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台", "kook");
    rl.set_exmap("发送者ID", "49");
    rl.set_exmap("群组ID", "77");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |platform, action, args| {
            assert_eq!(platform, "kook");
            let data = match (action, args["endpoint"].as_str()) {
                ("kook_guild_badge", _) => json!({"base64":"YWJj"}),
                (_, Some("intimacy/index")) => json!({"score":0,"img_list":[{"id":"0001","url":"image"}]}),
                (_, Some("game")) => json!({"items":[{"id":123,"name":"活动","type":0}]}),
                (_, Some("game/create" | "game/update")) => json!({"id":123,"name":"活动","type":0}),
                _ => json!({}),
            };
            record.borrow_mut().push((action.to_owned(), args));
            json!({"retcode":0,"data":data})
        }));
    });
    let run = |rl: &mut crate::core::RedLang, script| rl.parse(script).unwrap();
    assert_eq!(
        run(&mut rl, "【取元素@【KOOK亲密度@查询】@形象列表@0@形象ID】").expect_text_value().unwrap(),
        "0001"
    );
    assert!(rl.parse("【KOOK亲密度@更新@@2201】").is_err());
    run(&mut rl, "【KOOK亲密度@更新@@0@@0001】");
    run(&mut rl, "【KOOK亲密度@清空社交信息】");
    assert_eq!(run(&mut rl, "【KOOK服务器@徽章@@在线及总人数】").expect_bin_value().unwrap().as_slice(), b"abc");
    assert_eq!(
        run(&mut rl, "【取元素@【KOOK游戏@列表@系统创建@2@20】@0@游戏ID】").expect_text_value().unwrap(),
        "123"
    );
    run(&mut rl, "【KOOK游戏@创建@活动】");
    run(&mut rl, "【KOOK游戏@更新@123@@https://example.org/icon.png】");
    run(&mut rl, "【KOOK动态@游戏@123】");
    run(&mut rl, "【KOOK动态@音乐@歌名@歌手】");
    run(&mut rl, "【KOOK动态@停止音乐】");
    run(&mut rl, "【KOOK动态@停止游戏】");
    run(&mut rl, "【KOOK游戏@删除@123】");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let calls = calls.borrow();
    assert_eq!(calls.len(), 12);
    assert_eq!(calls[0].1["body"], json!({"user_id":"49"}));
    assert_eq!(calls[1].1["body"], json!({"user_id":"49","score":0,"img_id":"0001"}));
    assert_eq!(calls[2].1["body"], json!({"user_id":"49","social_info":""}));
    assert_eq!(calls[3], ("kook_guild_badge".into(), json!({"guild_id":"77","style":2})));
    assert_eq!(calls[4].1, json!({"http_method":"GET","endpoint":"game","body":{"type":"2","page":2,"page_size":20}}));
    assert_eq!(calls[6].1["body"], json!({"id":123,"icon":"https://example.org/icon.png"}));
    assert_eq!(calls[7].1["body"], json!({"data_type":1,"id":123}));
    assert_eq!(calls[8].1["body"], json!({"data_type":2,"music_name":"歌名","singer":"歌手","software":"cloudmusic"}));
    assert_eq!(calls[9].1["body"], json!({"data_type":2}));
    assert_eq!(calls[10].1["body"], json!({"data_type":1}));
    assert_eq!(calls[11].1["endpoint"], "game/delete");
}

#[test]
fn telegram_notes_and_animation_work_through_redlang_messages_and_events() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台", "telegram");
    rl.set_exmap("发送者ID", "49");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |platform, action, args| {
            assert_eq!(platform, "telegram");
            record.borrow_mut().push((action.to_owned(), args));
            json!({"retcode":0,"data":{"message_id":"sent"}})
        }));
    });
    let run = |rl: &mut crate::core::RedLang, script| rl.parse(script).unwrap().expect_text_value().unwrap();
    assert!(run(&mut rl, "【语音@https://example.org/test.ogg@假】").starts_with("[CQ:record,"));
    assert!(run(&mut rl, "【视频@【BASE64解码@YWJj】】").starts_with("[CQ:video,"));
    run(&mut rl, "【输出流@【语音@voice-id@便笺@@问候@2】】");
    run(&mut rl, "【输出流@【视频@【BASE64解码@YWJj】@便笺@note.mp4@30@360@【BASE64解码@/9j/】】】");
    run(&mut rl, "【输出流@【动画@https://example.org/welcome.gif@@欢迎动画@@@@@真】】");
    rl.parse("【编辑消息@old@【数组@动画@animation-id@新说明】@媒体】").unwrap();
    assert!(rl.parse("【视频@https://example.org/note.mp4@便笺】").is_err());
    assert!(rl.parse("【视频@video-id@便笺@@61】").is_err());
    assert!(rl.parse("【媒体组@【数组@【数组@动画@file-one】@【数组@图片@file-two】】】").is_err());
    rl.set_exmap("原始事件", &json!({"platform":"telegram","notice_type":"telegram","sub_type":"edited_animation",
        "telegram":{"animation":{"file_id":"anim","width":320,"file_name":"anim.gif"},"caption":"已更新"}}).to_string());
    assert_eq!(run(&mut rl, "【取元素@【事件内容@动画】@说明】"), "已更新");
    assert_eq!(run(&mut rl, "【取元素@【事件内容@动画】@文件ID】"), "anim");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let calls = calls.borrow();
    assert_eq!(calls.len(), 4);
    let media = |index: usize| -> Value {
        assert_eq!(calls[index].0, "send_private_msg");
        assert_eq!(calls[index].1["message"][0]["type"], "telegram_media");
        serde_json::from_str(calls[index].1["message"][0]["data"]["content"].as_str().unwrap()).unwrap()
    };
    assert_eq!(media(0)["body"], json!({"voice":"voice-id","caption":"问候","duration":2}));
    assert_eq!(media(1)["body"], json!({"video_note":"attach://media","duration":30,"length":360,"thumbnail":"attach://preview"}));
    assert_eq!(media(1)["files"]["media"]["filename"], "note.mp4");
    assert_eq!(media(1)["files"]["preview"]["mime_type"], "image/jpeg");
    assert_eq!(media(2)["body"]["show_caption_above_media"], true);
    assert_eq!(calls[3].1["body"]["media"], json!({"type":"animation","media":"animation-id","caption":"新说明"}));
}

#[test]
fn positional_payments_and_kook_templates_build_expected_calls() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |platform, action, args| {
            let data = match (action,args["endpoint"].as_str(),args["method"].as_str()) {
                ("send_private_msg",_,_) => json!({"message_id":"sent"}),
                (_,Some("template/create"),_) => json!({"model":{"id":"239"}}),
                ("kook_pipe_message",_,_) => json!({"msg_id":"sent-native"}),
                (_,_,Some("createInvoiceLink")) => json!("https://t.me/test"),
                _ => Value::Null,
            };
            record.borrow_mut().push((platform.to_owned(), action.to_owned(), args));
            json!({"retcode":0,"data":data})
        }));
    });
    let read = |rl: &mut crate::core::RedLang, script| rl.parse(script).unwrap().expect_text_value().unwrap();
    rl.set_exmap("机器人平台","kook");
    rl.set_exmap("机器人ID","bot");
    rl.set_exmap("发送者ID","49");
    assert_eq!(read(&mut rl,"【KOOK模板管理@创建@通知@正文@文字@【数组@测试】】"),"239");
    read(&mut rl,"【KOOK模板管理@创建@卡片@【KOOK卡片@通知】@卡片】");
    read(&mut rl,"【输出流@【KOOK模板消息@239@【数组@001@测试】】】");
    rl.parse("【编辑消息@old@【KOOK模板消息@239】@模板】").unwrap();
    assert_eq!(read(&mut rl,"【KOOK管道消息@令牌@【数组@周末】@模板】"),"sent-native");
    read(&mut rl,"【KOOK管道消息@令牌@通知@文字@频道】");

    rl.set_exmap("机器人平台","telegram");
    rl.set_exmap("机器人ID","1");
    assert!(rl.parse("【Telegram发票@商品@说明@order@【数组@【数组@甲@1】@【数组@乙@2】】】").is_err());
    assert_eq!(read(&mut rl,"【Telegram发票链接@会员@说明@member@25@@@【数组@@@@@@真】】"),"https://t.me/test");
    read(&mut rl,"【输出流@【Telegram发票@手册@纸质手册@001@100@HKD@令牌@【数组@【数组@运费询价】】】@【Telegram键盘@【数组@【数组@【数组@立即支付@@支付】】】】】");
    read(&mut rl,"【输出流@【Telegram付费媒体@5@【数组@【数组@图片@【BASE64解码@YWJj】@@预览.png】】@media】】");
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"1","notice_type":"telegram","sub_type":"shipping_query",
        "telegram":{"id":"ship","from":{"id":49},"invoice_payload":"001","shipping_address":{"country_code":"HK"}}}).to_string());
    assert_eq!(read(&mut rl,"【取元素@【事件内容@支付信息】@地址@国家代码】"),"HK");
    read(&mut rl,"【同意@【数组@【数组@pickup@自提@0】】】");
    assert!(rl.parse("【同意@【数组@【数组@pickup@自提@0】@【数组@pickup@重复@0】】】").is_err());
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"1","notice_type":"telegram","sub_type":"pre_checkout_query",
        "telegram":{"id":"check","from":{"id":49}}}).to_string());
    read(&mut rl,"【同意】");
    assert!(rl.parse("【拒绝】").is_err());
    read(&mut rl,"【拒绝@库存不足】");
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"1","notice_type":"telegram","sub_type":"successful_payment",
        "telegram":{"from":{"id":49},"successful_payment":{"currency":"XTR","telegram_payment_charge_id":"paid-1","total_amount":25}}}).to_string());
    read(&mut rl,"【Telegram星币@退款】");
    read(&mut rl,"【Telegram星币@恢复续订@49@paid-1】");
    assert!(rl.parse("【Telegram位置@NaN@22.3193】").is_err());
    read(&mut rl,"【输出流@【Telegram位置@114.1694@22.3193@@600】】");
    rl.parse("【编辑消息@live@【数组@114.1701@22.3199】@实时位置】").unwrap();
    rl.parse("【编辑消息@live@@停止位置】").unwrap();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let calls = calls.borrow();
    assert_eq!(calls.len(),17);
    let template: Value = serde_json::from_str(calls[0].2["body"]["test_data"].as_str().unwrap()).unwrap();
    assert_eq!(template,json!({"参数1":"测试"}));
    assert_eq!(calls[1].2["body"]["msgtype"],2);
    assert_eq!(calls[2].2["message"][0]["type"],"kook_template");
    assert_eq!(calls[3].2["body"]["template_id"],"239");
    assert_eq!(calls[4].2, json!({"token":"令牌","body":{"参数1":"周末"}}));
    assert_eq!(calls[5].2["type"],9);
    assert_eq!(calls[6].2["body"]["subscription_period"],2_592_000);
    let invoice: Value = serde_json::from_str(calls[7].2["message"][0]["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(invoice["payload"],"001");
    assert_eq!(invoice["need_shipping_address"],true);
    assert_eq!(calls[7].2["reply_markup"]["inline_keyboard"][0][0]["pay"],true);
    let media: Value = serde_json::from_str(calls[8].2["message"][0]["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(media["body"]["media"][0]["media"],"attach://media_0");
    assert_eq!(media["files"]["media_0"]["filename"],"预览.png");
    assert_eq!(calls[9].2["body"]["shipping_options"][0]["prices"][0]["amount"],0);
    assert_eq!(calls[10].2["body"],json!({"pre_checkout_query_id":"check","ok":true}));
    assert_eq!(calls[11].2["body"]["error_message"],"库存不足");
    assert_eq!(calls[12].2["body"],json!({"user_id":49,"telegram_payment_charge_id":"paid-1"}));
    assert_eq!(calls[13].2["body"]["is_canceled"],false);
    let live: Value = serde_json::from_str(calls[14].2["message"][0]["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(live["live_period"],600);
    assert_eq!(calls[15].2["body"]["longitude"],114.1701);
    assert_eq!(calls[16].2["kind"],"stop_location");
}

#[test]
fn positional_bot_commands_build_wire_requests_from_real_scripts() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let record = requests.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = Some(Box::new(move |platform, action, args| {
        let data = match (args["method"].as_str(), args["endpoint"].as_str()) {
            (Some("getChatAdministrators"), _) => json!([
                {"user":{"id":45,"first_name":"小明","is_bot":false},"status":"creator"}
            ]),
            (Some("createForumTopic"), _) => json!({"message_thread_id":55}),
            (Some("createChatSubscriptionInviteLink"), _) => json!({"invite_link":"https://t.me/+paid"}),
            (Some("getUserProfilePhotos"), _) => json!({"photos":[[
                {"file_id":"smaller","width":32,"height":32},
                {"file_id":"large-photo","width":128,"height":128}
            ]]}),
            (_, Some("thread/create")) => json!({"id":"post-id"}),
            (_, Some("message/list")) => json!({"items":[{"id":"original-id","author":{"id":"99"},"content":"报名"}]}),
            (_, Some("asset/create")) => json!({"url":"https://example.org/upload.pdf"}),
            _ if action == "kook_voice_status" => json!({"state":"playing","track":2,"total":4}),
            _ => Value::Null,
        };
        record.borrow_mut().push((platform.to_owned(), action.to_owned(), args));
        json!({"retcode":0,"data":data})
    })));
    let text = |rl: &mut crate::core::RedLang, script| rl.parse(script).unwrap().expect_text_value().unwrap();

    rl.set_exmap("机器人平台", "telegram");
    rl.set_exmap("群ID", "-100123");
    rl.set_exmap("原始事件", &json!({"notice_type":"telegram","sub_type":"inline_query","telegram":
        {"id":"query-id","query":"搜索","offset":"two"}}).to_string());
    assert_eq!(text(&mut rl,"【事件内容@查询内容】"),"搜索");
    assert_eq!(text(&mut rl,"【事件内容@查询偏移】"),"two");
    text(&mut rl,"【Telegram应答查询@【数组@【数组@001@帮助@欢迎使用红色问答】】】");
    assert!(rl.parse("【Telegram应答查询@【数组@【数组@a@甲@内容】@【数组@a@乙@内容】】】").is_err());
    assert_eq!(text(&mut rl,"【取长度@【群成员@管理员】】"),"1");
    assert_eq!(text(&mut rl,"【取元素@【群成员@管理员@@真】@0@身份】"),"群主");
    assert_eq!(text(&mut rl,"【Telegram话题@创建@周末活动@@蓝色】"),"55");
    text(&mut rl,"【Telegram群权限@设置@【数组@文本@文件】】");
    rl.parse("【编辑消息@handle@【数组@图片@【BASE64解码@YWJj】@说明@新图.png】@媒体@HTML】").unwrap();
    rl.parse("【转发消息@handle@-100456@群聊@真@真@15】").unwrap();

    rl.set_exmap("机器人平台", "kook");
    rl.set_exmap("群ID", "channel");
    rl.set_exmap("群组ID", "guild");
    text(&mut rl,"【KOOK频道@编辑@@@活动说明@5】");
    assert_eq!(text(&mut rl,"【KOOK帖子@发布@活动报名@【KOOK卡片@请回复报名】】"),"post-id");
    assert_eq!(text(&mut rl,"【取元素@【KOOK频道消息@之前@10@@older-id】@0@正文】"),"报名");
    assert_eq!(text(&mut rl,"【KOOK上传文件@【BASE64解码@YWJj】@说明.pdf】"),"https://example.org/upload.pdf");
    text(&mut rl,"【KOOK频道权限@设置@身份组@702@【数组@查看频道】@【数组@发布消息】】");
    assert!(rl.parse("【KOOK频道权限@设置@身份组@702@【数组@查看频道】@2048】").is_err());
    assert_eq!(text(&mut rl,"【KOOK播放状态@状态】"),"播放中");
    rl.set_exmap("机器人平台","telegram");
    assert_eq!(text(&mut rl,"【取元素@【取头像@123@文件ID列表@0@1】@0】"),"large-photo");
    rl.parse("【编辑消息@inline-id@活动已开始@@@@行内】").unwrap();
    text(&mut rl,"【邀请链接@创建付费@100@活动会员@-100123】");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let requests = requests.borrow();
    assert_eq!(requests.len(), 16);
    assert_eq!(requests[0].2, json!({"method":"answerInlineQuery","body":{"inline_query_id":"query-id",
        "cache_time":0,"is_personal":false,"next_offset":"","results":[{"type":"article","id":"001",
        "title":"帮助","input_message_content":{"message_text":"欢迎使用红色问答"}}]}}));
    assert_eq!(requests[1].2["body"], json!({"chat_id":-100123}));
    assert_eq!(requests[2].2["body"]["return_bots"], true);
    assert_eq!(requests[3].2["body"]["icon_color"], 0x6FB9F0);
    assert_eq!(requests[4].2["body"]["permissions"]["can_send_documents"],true);
    assert_eq!(requests[4].2["body"]["permissions"]["can_send_photos"],false);
    assert_eq!(requests[5].2["body"]["media"]["parse_mode"],"HTML");
    assert_eq!(requests[5].2["files"]["media_0"]["filename"],"新图.png");
    assert_eq!(requests[6].2["message_thread_id"],15);
    assert_eq!(requests[6].2["copy"],true);
    assert_eq!(requests[7].2["body"],json!({"channel_id":"channel","topic":"活动说明","slow_mode":5000}));
    let card: Value = serde_json::from_str(requests[8].2["body"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(card[0]["modules"][0]["text"]["content"],"请回复报名");
    assert_eq!(requests[9].2["body"],json!({"target_id":"channel","page_size":10,"flag":"before","msg_id":"older-id"}));
    assert_eq!(requests[10].2["files"]["file"],json!({"filename":"说明.pdf","base64":"YWJj"}));
    assert_eq!(requests[11].2["body"],json!({"channel_id":"channel","type":"role_id","value":"702","allow":2048,"deny":4096}));
    assert_eq!(requests[12].1,"kook_voice_status");
    assert_eq!(requests[13].2["body"],json!({"user_id":123,"offset":0,"limit":1}));
    assert_eq!(requests[14].2,json!({"method":"editMessageText","body":{"inline_message_id":"inline-id","text":"活动已开始"}}));
    assert_eq!(requests[15].2["body"]["subscription_period"],2_592_000);
}

#[test]
fn business_messages_and_kook_moderation_use_positional_parameters() {
    use crate::cqexfun::TEST_PLATFORM_CALL;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let record = requests.clone();
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| {
        *handler = Some(Box::new(move |_, action, args| {
            let data = match (action,args["method"].as_str(),args["endpoint"].as_str()) {
                ("send_private_msg",_,_) => json!({"message_id":"sent-handle"}),
                (_,Some("getBusinessConnection"),_) => json!({"id":"business-a","user":{"id":11},"is_enabled":true,
                    "rights":{"can_reply":true,"can_read_messages":true}}),
                (_,Some("getBusinessAccountGifts"),_) => json!({"total_count":1,"gifts":[{"type":"unique","owned_gift_id":"gift-a"}]}),
                (_,Some("postStory"),_) => json!({"id":36}),
                (_,_,Some("blacklist/list")) => json!({"items":[{"user_id":"002","created_time":120,"remark":"刷屏","user":{"nickname":"小明"}}]}),
                (_,_,Some("guild-mute/list")) => json!({"mic":{"user_ids":["002"]},"headset":{"user_ids":["003"]}}),
                _ => Value::Null,
            };
            record.borrow_mut().push((action.to_owned(),args));
            json!({"retcode":0,"data":data})
        }));
    });
    let text = |rl: &mut crate::core::RedLang, script| rl.parse(script).unwrap().expect_text_value().unwrap();
    rl.set_exmap("机器人平台","telegram");
    rl.set_exmap("机器人ID","1");
    rl.set_exmap("发送者ID","11");
    rl.set_exmap("消息ID","incoming-handle");
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"1","sub_type":"business_message","user_id":"11",
        "telegram":{"business_connection_id":"business-a","chat":{"id":22,"type":"private"},"message_id":5,"text":"客户消息"}}).to_string());
    assert_eq!(text(&mut rl,"【事件内容@商务会话ID】"),"22");
    assert_eq!(text(&mut rl,"【事件内容@消息文字】"),"客户消息");
    assert_eq!(text(&mut rl,"【取元素@【Telegram商务账户@查询】@已启用】"),"真");
    assert_eq!(text(&mut rl,"【输出流@【Telegram任务清单@订单@【数组@确认@发货】】】"),"sent-handle");
    text(&mut rl,"【输出流@普通回复@@@@机器人】");
    text(&mut rl,"【输出流@指定客户@@@@business-b@77】");
    text(&mut rl,"【Telegram商务已读】");
    text(&mut rl,"【Telegram商务已读@5@22】");
    rl.parse("【编辑消息@sent-handle@【数组@订单@【数组@【数组@2@等待发货】】】@清单】").unwrap();
    assert!(rl.parse("【Telegram任务清单@订单@【数组@【数组@2@确认】@发货】】").is_err());
    text(&mut rl,"【撤回@【数组@5@6】@business-a】");
    text(&mut rl,"【Telegram聊天动作@正在输入】");
    assert_eq!(text(&mut rl,"【Telegram商务动态@发布@【BASE64解码@YWJj】@图.jpg@图片@活动】"),"36");
    assert_eq!(text(&mut rl,"【取元素@【Telegram商务礼物@列表@@@5@价格@【数组@未展示】】@礼物@0@持有礼物ID】"),"gift-a");
    assert!(rl.parse("【Telegram商务礼物@转星币@0】").is_err());
    rl.set_exmap("机器人ID","2");
    assert!(rl.parse("【Telegram商务账户@查询】").is_err());
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"2","sub_type":"business_connection",
        "telegram":{"id":"business-c","user":{"id":11},"is_enabled":true,"rights":{"can_read_messages":true}}}).to_string());
    assert_eq!(text(&mut rl,"【事件内容@商务连接ID】"),"business-c");
    assert_eq!(text(&mut rl,"【取元素@【事件内容@商务连接】@权限@0】"),"标记消息已读");
    rl.set_exmap("原始事件",&json!({"platform":"telegram","self_id":"2","sub_type":"deleted_business_messages",
        "message_ids":["old-handle"],"telegram":{"business_connection_id":"business-c","chat":{"id":22,"type":"private"},
            "message_ids":[5]}}).to_string());
    assert_eq!(text(&mut rl,"【取元素@【事件内容@已删除平台消息ID】@0】"),"5");
    assert_eq!(text(&mut rl,"【取元素@【事件内容@已删除消息ID】@0】"),"old-handle");

    rl.set_exmap("机器人平台","kook");
    rl.set_exmap("群组ID","guild-a");
    rl.set_exmap("发送者ID","001");
    assert_eq!(text(&mut rl,"【取元素@【群管理@黑名单@@2@20】@0@原因】"),"刷屏");
    assert!(rl.parse("【群管理@黑名单@@1@51】").is_err());
    text(&mut rl,"【群管理@封禁@002@@刷屏@3】");
    assert_eq!(text(&mut rl,"【取元素@【取元素@【KOOK语音管理@限制列表】@静音】@0】"),"003");
    text(&mut rl,"【KOOK语音管理@闭麦】");
    text(&mut rl,"【KOOK语音管理@解除静音@003】");
    TEST_PLATFORM_CALL.with_borrow_mut(|handler| *handler = None);

    let requests = requests.borrow();
    assert_eq!(requests.len(),16);
    assert_eq!(requests[0].1["body"]["business_connection_id"],"business-a");
    assert_eq!(requests[1].1["user_id"],"22");
    assert_eq!(requests[1].1["business_connection_id"],"business-a");
    let segment = &requests[1].1["message"][0];
    assert_eq!(segment["type"],"telegram_checklist");
    let checklist: Value = serde_json::from_str(segment["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(checklist["tasks"][1],json!({"id":2,"text":"发货"}));
    assert_eq!(requests[2].1["business_connection_id"],Value::Null);
    assert_eq!(requests[3].1["user_id"],"77");
    assert_eq!(requests[3].1["business_connection_id"],"business-b");
    assert_eq!(requests[4],("telegram_read_business_message".into(),json!({"message_id":"incoming-handle"})));
    assert_eq!(requests[5].1["body"],json!({"business_connection_id":"business-a","chat_id":22,"message_id":5}));
    assert_eq!(requests[6].1["body"]["checklist"]["tasks"][0]["id"],2);
    assert_eq!(requests[7].1["body"]["message_ids"],json!([5,6]));
    assert_eq!(requests[8].1["body"]["chat_id"],22);
    assert_eq!(requests[9].1["body"]["active_period"],86400);
    assert_eq!(requests[9].1["files"]["story_file"]["filename"],"图.jpg");
    assert_eq!(requests[10].1["body"]["exclude_unsaved"],true);
    assert_eq!(requests[11].1["body"],json!({"guild_id":"guild-a","page":2,"page_size":20}));
    assert_eq!(requests[12].1["body"]["del_msg_days"],3);
    assert_eq!(requests[13].1["body"]["return_type"],"detail");
    assert_eq!(requests[14].1["body"],json!({"guild_id":"guild-a","user_id":"001","type":1}));
    assert_eq!(requests[15].1["body"],json!({"guild_id":"guild-a","user_id":"003","type":2}));
}

#[test]
fn telegram_topic_follows_only_the_original_bot_and_chat() {
    use crate::cqexfun::apply_telegram_topic;
    use serde_json::json;
    let event = json!({
        "self_id":"123", "message_thread_id":9,
        "telegram":{"message":{"chat":{"id":-100234}}}
    });
    let mut payload = json!({});
    apply_telegram_topic(&event, "123", "-100000", &mut payload);
    apply_telegram_topic(&event, "456", "-100234", &mut payload);
    assert_eq!(payload, json!({}));
    apply_telegram_topic(&event, "123", "-100234", &mut payload);
    assert_eq!(payload, json!({"message_thread_id":9}));
}


#[test]
fn native_objects_keep_text_and_explicit_values_separate() {
    use crate::native_args;
    use serde_json::json;

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    let value = rl.parse(
        "【对象@text@假@callback_data@001@reply_markup@【对象@inline_keyboard@【数组@【数组@【对象@text@123】】】】@is_anonymous@【接口值@逻辑@假】@offset@【接口值@整数@-2】@optional@【接口值@空值】】"
    ).unwrap();
    assert_eq!(native_args::object(&value).unwrap(), json!({
        "text":"假", "callback_data":"001", "reply_markup":{"inline_keyboard":[[{"text":"123"}]]},
        "is_anonymous":false, "offset":-2, "optional":null
    }));
    assert!(rl.parse("【接口值@整数@1.5】").is_err());
    assert!(rl.parse("【接口值@小数@NaN】").is_err());

    let files = crate::core::astparser::parse_to_ast(
        "【对象@file@【对象@filename@说明.txt@内容@【BASE64解码@YWJj】】】"
    ).unwrap();
    assert_eq!(native_args::files(&mut rl, &[files], 0).unwrap(), Some(json!({
        "file":{"filename":"说明.txt","base64":"YWJj"}
    })));
    let invalid = crate::core::astparser::parse_to_ast(
        "【对象@file@【对象@filename@说明.txt@内容@abc】】"
    ).unwrap();
    assert!(native_args::files(&mut rl, &[invalid], 0).is_err());

    let card = rl.parse("【KOOK卡片@【数组@【对象@type@card@modules@【数组@【对象@type@section@text@测试】】】】】").unwrap();
    let message = redlang::mytool::str_msg_to_arr(&json!(card.expect_text_value().unwrap())).unwrap();
    assert_eq!(message[0]["type"], "kook_card");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(message[0]["data"]["content"].as_str().unwrap()).unwrap(),
        json!([{"type":"card","modules":[{"type":"section","text":"测试"}]}])
    );
}

#[test]
fn telegram_join_request_reuses_existing_approval_and_refusal() {
    use crate::cqexfun::telegram_join_request;
    use serde_json::json;

    let event = json!({
        "notice_type":"telegram", "sub_type":"chat_join_request",
        "telegram":{"chat":{"id":-100234},"from":{"id":42},"user_chat_id":1000}
    });
    let expected = json!({"chat_id":-100234,"user_id":42});
    assert_eq!(telegram_join_request(&event, true), Some(("approveChatJoinRequest", expected.clone())));
    assert_eq!(telegram_join_request(&event, false), Some(("declineChatJoinRequest", expected)));
    assert!(telegram_join_request(&json!({"notice_type":"telegram","sub_type":"chat_join_request"}), true).is_none());
}


#[test]
fn telegram_explicit_topics_and_join_request_queries() {
    use crate::cqexfun::{telegram_join_request, telegram_output_topic};
    use serde_json::json;

    assert_eq!(telegram_output_topic("", "").unwrap(), None);
    assert_eq!(telegram_output_topic("23", "").unwrap(), Some(("message_thread_id", 23)));
    assert_eq!(telegram_output_topic("9", "频道私信").unwrap(), Some(("direct_messages_topic_id", 9)));
    assert!(telegram_output_topic("0", "").is_err());
    assert!(telegram_output_topic("2", "语音").is_err());

    let event = json!({
        "notice_type":"telegram","sub_type":"chat_join_request",
        "telegram":{"query_id":"q-42","chat":{"id":-100234},"from":{"id":42}}
    });
    assert_eq!(telegram_join_request(&event, false), Some((
        "answerChatJoinRequestQuery",
        json!({"chat_join_request_query_id":"q-42","result":"decline"}),
    )));
    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    assert!(rl.parse("【置顶消息】").unwrap_err().to_string().contains("机器人来源"));
    rl.set_exmap("机器人平台", "telegram");
    assert!(rl.parse("【输出流@消息@@0】").unwrap_err().to_string().contains("正整数"));
}


#[test]
fn message_management_accepts_redlang_objects_and_checks_invalid_arguments() {
    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台", "telegram");
    for script in [
        "【编辑消息@消息ID@新的内容】",
        "【编辑消息@消息ID@【对象@inline_keyboard@【数组】】@键盘】",
        "【转发消息@【数组@消息一@消息二】@-100123@群聊@【对象@复制@真】】",
        "【撤回@【数组@消息一@消息二】】",
    ] {
        let error = rl.parse(script).unwrap_err();
        assert!(error.to_string().contains("指定机器人ID"), "{error}");
    }
    assert!(rl.parse("【转发消息@【数组】@-100123】").is_err());
    assert!(rl.parse("【撤回@【数组@消息一@【对象】】】").is_err());
    rl.set_exmap("机器人平台", "kook");
    assert!(rl.parse("【编辑消息@消息ID@文本@卡片】").is_err());
}


#[test]
fn media_and_sticker_commands_preserve_binary_attachments_and_text_ids() {
    use serde_json::{json, Value};

    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    rl.set_exmap("机器人平台", "telegram");
    let output = rl.parse("【媒体组@【数组@【数组@图片@0001】@【数组@图片@【BASE64解码@YWJj】@@测试.png】】】").unwrap();
    let segments = redlang::mytool::str_msg_to_arr(&json!(output.expect_text_value().unwrap())).unwrap();
    let group: Value = serde_json::from_str(segments[0]["data"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(group["media"][0]["media"], "0001");
    assert_eq!(group["media"][1]["media"], "attach://media_1");
    assert_eq!(group["files"]["media_1"], json!({"filename":"测试.png","base64":"YWJj"}));
    assert!(rl.parse("【媒体组@【数组】】").is_err());
    assert!(rl.parse("【贴纸@【BASE64解码@YWJj】@sticker.jpg】").is_err());
    let error = rl.parse("【表情回应@123@@真@自定义@真】").unwrap_err();
    assert!(error.to_string().contains("指定机器人ID"));
}

#[test]
fn simplified_card_and_event_commands_use_redlang_values() {
    init_redlang_test_funs();
    let mut rl = crate::core::RedLang::new();
    let card = rl.parse("【KOOK卡片@活动@【数组@【数组@【数组@报名@001】】】@绿色@小】").unwrap();
    let payload = crate::bot_commands::card_for_edit(&card).unwrap();
    assert_eq!(payload[0]["theme"], "success");
    assert_eq!(payload[0]["modules"][1]["elements"][0]["value"], "001");
    assert_eq!(payload[0]["modules"][1]["elements"][0]["click"], "return-val");

    rl.set_exmap("原始事件", r#"{"platform":"kook","notice_type":"kook","sub_type":"message_btn_click","kook":{"value":"001"}}"#);
    assert_eq!(rl.parse("【事件内容@按钮值】").unwrap().expect_text_value().unwrap(), "001");
    rl.set_exmap("原始事件", r#"{"platform":"telegram","notice_type":"telegram","sub_type":"poll_answer","telegram":{"option_ids":[0,2]}}"#);
    let choices = rl.parse("【事件内容@投票选项】").unwrap().expect_array_value().unwrap();
    assert_eq!(choices[0].expect_text_value().unwrap(), "1");
    assert_eq!(choices[1].expect_text_value().unwrap(), "3");

    rl.set_exmap("机器人平台", "telegram");
    rl.set_exmap("群ID", "-100123456789");
    for code in [
        "【Telegram命令菜单@设置@【数组@【数组@start@开始使用】】@所有私聊@zh】",
        "【Telegram菜单按钮@小程序@@打开服务@https://example.org/app】",
        "【Telegram投票@下次活动选哪天？@【数组@周六@周日】@假】",
        "【Telegram结束投票@消息ID】",
        "【邀请链接@创建@@@3600@@活动报名@真】",
    ] {
        assert!(rl.parse(code).unwrap_err().to_string().contains("指定机器人ID"), "{code}");
    }
}
