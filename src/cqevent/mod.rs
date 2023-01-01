pub(crate) mod do_group_msg;
mod do_private_msg;
mod do_guild_msg;
mod do_other_evt;
extern crate sciter;


use sciter::{dispatch_script_call};
use crate::{cqapi::*, save_config, read_config, redlang::{RedLang}, mytool::read_json_str, PAGING_UUID};

// 处理1207号事件
pub fn do_1207_event(onebot_json_str: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let root:serde_json::Value = serde_json::from_str(onebot_json_str)?;
    if let Some(message_type) = root.get("message_type") {
        if message_type == "group" {
            do_group_msg::do_group_msg(&root)?;
        }else if message_type == "private"{
            do_private_msg::do_private_msg(&root)?;
        }else if message_type == "guild"{
            do_guild_msg::do_guild_msg(&root)?;
        }
    }
    do_other_evt::do_other_evt(&root)?;
    Ok(0)
}

pub fn do_paging(outstr:&str) -> Result<Vec<&str>, Box<dyn std::error::Error>> {
    let out = outstr.split(PAGING_UUID.as_str());
    let outvec = out.collect::<Vec<&str>>();
    return Ok(outvec);
}

pub fn get_msg_type(rl:& RedLang) -> &'static str {
    let user_id_str = rl.get_exmap("发送者ID").to_string();
    let group_id_str = rl.get_exmap("群ID").to_string();
    let guild_id_str = rl.get_exmap("频道ID").to_string();
    let channel_id_str = rl.get_exmap("子频道ID").to_string();
    let msg_type:&str;
    if group_id_str != "" {
        msg_type = "group";
    }else if channel_id_str != "" && guild_id_str != ""{
        msg_type = "channel";
    }else if user_id_str  != "" {
        msg_type = "private";
    }else{
        msg_type = "";
    }
    return msg_type;
}

pub fn do_script(rl:&mut RedLang,code:&str) -> Result<(), Box<dyn std::error::Error>>{
    let out_str_t = rl.parse(code)?;
    let out_str_vec = do_paging(&out_str_t)?;
    for out_str in out_str_vec {
        crate::redlang::cqexfun::send_one_msg(rl, out_str)?;
    }
    Ok(())
}

struct Handler;

impl Handler {
    pub fn calc_sum(&self, a: i32, b: i32) -> i32 {
      a + b
    }
    pub fn print_log(&self, s: String){
        cq_add_log(&s).unwrap();
    }
    pub fn save_code(&self, contents: String) -> bool{
        if let Err(err) = save_config(&contents){
            cq_add_log_w(&format!("can't save_config:{}",err)).unwrap();
            return false;
        }
        if let Err(err) = crate::initevent::do_init_event(){
            cq_add_log_w(&format!("can't call init evt:{}",err)).unwrap();
        }
        return true;
    }
    pub fn read_code(&self) -> String {
        let cfg = read_config();
        return match cfg {
            Ok(s) => s.to_string(),
            Err(err) => {
                cq_add_log_w(&format!("can't read_config:{}",err)).unwrap();
                "".to_string()
            }
        }
    }
  }
  
impl sciter::EventHandler for Handler {
    dispatch_script_call! {
        fn calc_sum(i32, i32);
        fn print_log(String);
        fn save_code(String);
        fn read_code();
    }
}

pub fn do_menu_event() -> Result<i32, Box<dyn std::error::Error>> {
    let mut frame = sciter::Window::new();
    frame.load_file(&(cq_get_app_directory().unwrap() + "minimal.htm"));
    frame.event_handler(Handler {});
    frame.run_app();
    Ok(0)
}

fn set_normal_evt_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    rl.set_exmap("机器人ID", &read_json_str(root,"self_id"))?;
    rl.set_exmap("发送者ID", &read_json_str(root,"user_id"))?;
    rl.set_exmap("群ID", &read_json_str(root,"group_id"))?;
    rl.set_exmap("机器人名字", "露娜sama")?;
    rl.set_exmap("原始事件", &root.to_string())?;
    rl.set_exmap("频道ID", &read_json_str(root,"guild_id"))?;
    rl.set_exmap("子频道ID", &read_json_str(root,"channel_id"))?;
    rl.set_exmap("机器人频道ID", &read_json_str(root,"self_tiny_id"))?;
    Ok(())
}

fn set_normal_message_info(rl:&mut RedLang,root:&serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    rl.set_exmap("消息ID", &read_json_str(root,"message_id"))?;
    {
        let sender = root.get("sender").ok_or("sender not exists")?;
        if let Some(js_v) = sender.get("nickname") {
            rl.set_exmap("发送者昵称", js_v.as_str().unwrap_or(""))?;
        }
    }
    set_normal_evt_info(rl,root)?;
    Ok(())
}


fn is_key_match(rl:&mut RedLang,ppfs:&str,keyword:&str,msg:&str) -> Result<bool, Box<dyn std::error::Error>>{
    let mut is_match = false;
    if ppfs == "完全匹配"{
        if keyword == msg {
            is_match = true;
        }
    }else if ppfs == "模糊匹配"{
        if let Some(_pos)  = msg.find(keyword) {
            is_match = true;
        }
    }else if ppfs == "前缀匹配"{
        if msg.starts_with(keyword){
            is_match = true;
            rl.set_exmap("子关键词", msg.get(keyword.len()..).ok_or("前缀匹配失败")?)?;
        }
    }else if ppfs == "正则匹配"{
        let re = fancy_regex::Regex::new(keyword)?;
        let mut sub_key_vec = String::new();
        sub_key_vec.push_str(&rl.type_uuid);
        sub_key_vec.push('A');
        for cap_iter in re.captures_iter(&msg) {
            let cap = cap_iter?;
            is_match = true;
            let len = cap.len();
            let mut temp_vec = String::new();
            temp_vec.push_str(&rl.type_uuid);
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
        rl.set_exmap("子关键词", &sub_key_vec)?;
    }
    Ok(is_match)
}

fn get_script_info<'a>(script_json:&'a serde_json::Value) -> Result<(&'a str,&'a str,&'a str,&'a str), Box<dyn std::error::Error>>{
    let node = script_json.get("content").ok_or("script.json文件缺少content字段")?;
    let keyword = node.get("关键词").ok_or("脚本中无关键词")?.as_str().ok_or("脚本中关键词不是str")?;
    let cffs = node.get("触发方式").ok_or("脚本中无触发方式")?.as_str().ok_or("脚本中触发方式不是str")?;
    let code = node.get("code").ok_or("脚本中无code")?.as_str().ok_or("脚本中code不是str")?;
    let ppfs = node.get("匹配方式").ok_or("脚本中无匹配方式")?.as_str().ok_or("脚本中匹配方式不是str")?;
    return Ok((keyword,cffs,code,ppfs));
}