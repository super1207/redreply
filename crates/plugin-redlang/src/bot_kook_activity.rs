use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

pub(super) fn badge(rl: &mut RedLang, params: &[Ast], target: &str) -> RedCommandResult {
    let style = match rl.get_param_text(params, 2)?.as_str() {
        "" | "名称" | "0" => 0,
        "在线人数" | "1" => 1,
        "在线及总人数" | "2" => 2,
        _ => return Err("徽章样式请填名称、在线人数或在线及总人数".into()),
    };
    let guild_id = guild(rl, target)?;
    let response = success(call_platform_api_value(
        rl,
        "kook",
        "kook_guild_badge",
        json!({"guild_id":guild_id,"style":style}),
    )?)?;
    let bytes = response["data"]["base64"]
        .as_str()
        .ok_or("KOOK 未返回徽章图片")?;
    Ok(Some(crate::core::rv_bin(STANDARD.decode(bytes)?)))
}

fn intimacy_fields(data: &Value) -> Value {
    let images: Vec<_> = data["img_list"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|image| json!({"形象ID":image["id"],"链接":image["url"]}))
        .collect();
    json!({"分数":data["score"],"社交信息":data["social_info"],"形象链接":data["img_url"],
           "形象列表":images,"上次查看时间":data["last_read"],"上次修改时间":data["last_modify"]})
}

fn intimacy(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let [operation, user, score, social, image] = text_params::<5>(rl, params, 0)?;
    let mut body = json!({"user_id":context(rl,&user,"发送者ID")?});
    match operation.as_str() {
        "" | "查询" => {
            let response = kook(rl, "GET", "intimacy/index", body)?;
            output(&intimacy_fields(&response["data"]))
        }
        "更新" => {
            if score.is_empty() && social.is_empty() && image.is_empty() {
                return Err("请至少填写分数、社交信息或形象 ID 中的一项".into());
            }
            if !score.is_empty() {
                body["score"] = bounded(&score, 0, 2200, "分数")?.into();
            }
            if social.chars().count() > 500 {
                return Err("社交信息最多 500 字".into());
            }
            insert_text(&mut body, "social_info", &social);
            insert_text(&mut body, "img_id", &image);
            kook(rl, "POST", "intimacy/update", body)?;
            empty()
        }
        "清空社交信息" => {
            body["social_info"] = "".into();
            kook(rl, "POST", "intimacy/update", body)?;
            empty()
        }
        _ => Err("亲密度操作请填查询、更新或清空社交信息".into()),
    }
}

fn game_fields(row: &Value) -> Value {
    let kind = match row["type"].as_i64() {
        Some(0) => json!("游戏"),
        Some(1) => json!("虚拟主播"),
        Some(2) => json!("进程"),
        _ => row["type"].clone(),
    };
    json!({"游戏ID":row["id"],"名称":row["name"],"类型":kind,"图标":row["icon"],
           "进程信息":row["options"],"进程名":row["process_name"],"产品名":row["product_name"],
           "管理员权限启动":row["kmhook_admin"]})
}

fn game(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let [operation, first, second, third] = text_params::<4>(rl, params, 0)?;
    match operation.as_str() {
        "" | "列表" => {
            let kind = match first.as_str() {
                "" | "全部" => "0",
                "用户创建" => "1",
                "系统创建" => "2",
                _ => return Err("游戏列表范围请填全部、用户创建或系统创建".into()),
            };
            let page = if second.is_empty() {
                1
            } else {
                positive_id(&second)?
            };
            let mut body = json!({"type":kind,"page":page});
            if !third.is_empty() {
                body["page_size"] = bounded(&third, 1, 50, "每页数量")?.into();
            }
            let response = kook(rl, "GET", "game", body)?;
            mapped_list(&response["data"]["items"], game_fields)
        }
        "创建" | "更新" => {
            let (path, body) = if operation == "创建" {
                let mut body = json!({"name":required(&first,"游戏名称")?});
                insert_text(&mut body, "icon", &second);
                ("game/create", body)
            } else {
                if second.is_empty() && third.is_empty() {
                    return Err("请填写游戏名称或图标链接".into());
                }
                let mut body = json!({"id":positive_id(&first)?});
                insert_text(&mut body, "name", &second);
                insert_text(&mut body, "icon", &third);
                ("game/update", body)
            };
            let response = kook(rl, "POST", path, body)?;
            output(&game_fields(&response["data"]))
        }
        "删除" => {
            kook(
                rl,
                "POST",
                "game/delete",
                json!({"id":positive_id(&first)?}),
            )?;
            empty()
        }
        _ => Err("游戏操作请填列表、创建、更新或删除".into()),
    }
}

fn activity(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let [operation, first, singer, software] = text_params::<4>(rl, params, 0)?;
    let (path, body) = match operation.as_str() {
        "游戏" => (
            "game/activity",
            json!({"data_type":1,"id":positive_id(&first)?}),
        ),
        "音乐" => {
            let software = match software.as_str() {
                "" | "网易云音乐" => "cloudmusic",
                "QQ音乐" => "qqmusic",
                "酷狗音乐" => "kugou",
                _ => return Err("音乐平台请填网易云音乐、QQ音乐或酷狗音乐".into()),
            };
            (
                "game/activity",
                json!({"data_type":2,"music_name":required(&first,"歌名")?,
                                    "singer":required(&singer,"歌手")?,"software":software}),
            )
        }
        "停止游戏" => ("game/delete-activity", json!({"data_type":1})),
        "停止音乐" => ("game/delete-activity", json!({"data_type":2})),
        _ => return Err("动态操作请填游戏、音乐、停止游戏或停止音乐".into()),
    };
    kook(rl, "POST", path, body)?;
    empty()
}

pub(super) fn register() {
    add_fun(vec!["KOOK亲密度"], intimacy);
    add_fun(vec!["KOOK游戏"], game);
    add_fun(vec!["KOOK动态"], activity);
}
