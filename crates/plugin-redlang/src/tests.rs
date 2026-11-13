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
        count += 1;
    }
    assert!(count >= 13);
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
