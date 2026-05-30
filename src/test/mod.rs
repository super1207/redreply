

use std::sync::Once;

fn init_redlang_test_funs() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        crate::redlang::init_core_fun_map();
        crate::redlang::cqexfun::init_cq_ex_fun_map();
        crate::redlang::exfun::init_ex_fun_map();
    });
}


#[test]
fn test_cqstr_to_arr() {


    use crate::mytool::str_msg_to_arr;
    let js = serde_json::json!("hello,world[CQ:image,file=xxx.png]hello,world");
    let ret = str_msg_to_arr(&js).unwrap();
    println!("test_cqstr_to_arr ret:`{}`",ret.to_string());
}


#[test]
fn test_cqparse() {
    use std::collections::BTreeMap;
    let data_str = "[CQ:image,file=620a6c143114a4feaaf9e89cc83162b6.image,subType=0,url=https://gchat.qpic.cn/]";
    let pos1 = data_str.find(",").ok_or("CQ码解析失败").unwrap();
    let tp = data_str.get(4..pos1).ok_or("CQ码解析失败").unwrap();
    let mut sub_key_obj:BTreeMap<String,String> = BTreeMap::new();
    sub_key_obj.insert("type".to_string(), tp.to_string());
    let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])").unwrap();

    for cap_iter in re.captures_iter(&data_str) {
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
    println!("{:?}",sub_key_obj);
}

#[test]
fn test_redformat() {
    // fn is_black_char(ch: char) -> bool {
    //     ch == ' ' || ch == '\r' || ch == '\n' || ch == '\t'
    // }
    let test_str = "【定义变量@使用情况数组@【数组@0@0@0】】";
    let content = test_str.chars().collect::<Vec<char>>();
    let mut out_content = String::new();
    let mut index = 0;
    while index < content.len() {
        if content[index] != '【' {
            out_content.push(content[index]);
            index += 1;
            continue;
        }
        else {
            let next_char = content.get(index + 1).ok_or("syntax error").unwrap();
            if next_char.to_owned() == '@' {
                let mut num = 1;
                for index2 in index..content.len() {
                    if content[index2] == '【' {
                        num += 1;
                    }
                    else if content[index2] == '】' {
                        num -= 1;
                    }
                    if num == 0 {
                        let s = content.get(index..index2).unwrap();
                        out_content.push_str(&String::from_iter(s.iter()));
                        index = index2 + 1;
                        break;
                    }
                }
                if num != 0 {
                    break;
                }
            } else {

            }
        }

    }

}

#[test]
fn test_structured_const_keeps_value_shape() {
    init_redlang_test_funs();
    let pkg_name = "test_structured_const_keeps_value_shape";
    crate::del_pkg_memory(pkg_name);

    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut rl = crate::redlang::RedLang::new();
        rl.pkg_name = pkg_name.to_string();
        let ret = rl.parse(
            "【定义常量@结构常量@【对象@列表@【数组@甲@乙】@对象@【对象@键@值】】】\
             【取类型@【取元素@【常量@结构常量】@列表】】|\
             【取元素@【取元素@【常量@结构常量】@列表】@1】|\
             【取元素@【取元素@【常量@结构常量】@对象】@键】",
        )?;
        match &*ret {
            crate::redlang::RedValue::Text(s) => Ok(s.to_string()),
            other => Err(format!("expected text result, got {}", other.get_type_name()).into()),
        }
    })();

    crate::del_pkg_memory(pkg_name);
    assert_eq!(result.unwrap(), "A|乙|值");
}

#[test]
fn test_structured_runtime_operations() {
    init_redlang_test_funs();
    let pkg_name = "test_structured_runtime_operations";
    crate::del_pkg_memory(pkg_name);

    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut rl = crate::redlang::RedLang::new();
        rl.pkg_name = pkg_name.to_string();
        let ret = rl.parse(
            "【取长度@【截取@【数组@1@2@3】@1@2】】|\
             【取元素@【取元素@【JSON解析@【对象@列表@【数组@甲@乙】】】@列表】@1】|\
             【计数@【数组@甲@乙@甲】@甲】|\
             【取元素@【翻转@【数组@甲@乙】】@0】",
        )?;
        match &*ret {
            crate::redlang::RedValue::Text(s) => Ok(s.to_string()),
            other => Err(format!("expected text result, got {}", other.get_type_name()).into()),
        }
    })();

    crate::del_pkg_memory(pkg_name);
    assert_eq!(result.unwrap(), "2|乙|2|乙");
}

// #[test]
// fn test_wav_to_pcm() {
//     let wav_info = crate::mytool::wav_to_pcm::WavFormat::decode("D:\\青雀语音\\71201001_cn.wav").unwrap();

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
