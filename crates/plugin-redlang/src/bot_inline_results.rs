use super::*;

pub(super) fn article_options(result: &mut Value, options: &[Rc<RedValue>]) -> Result<()> {
    extra_fields(
        result,
        options,
        &[
            ("thumbnail_url", 'u'),
            ("thumbnail_width", 'n'),
            ("thumbnail_height", 'n'),
        ],
    )
}

fn extra_fields(spec: &mut Value, row: &[Rc<RedValue>], fields: &[(&str, char)]) -> Result<()> {
    if row.len() > fields.len() {
        return Err("行内结果附加项过多".into());
    }
    for (i, (name, kind)) in fields.iter().enumerate() {
        let value = row_text(row, i)?;
        if value.is_empty() {
            if matches!(kind, 'U' | 'T') {
                return Err("缺少必填的行内结果附加项".into());
            }
            continue;
        }
        spec[*name] = match kind {
            'u' | 'U' => {
                super::https_url(&value)?;
                value.into()
            }
            'n' => value.parse::<u32>()?.into(),
            'b' => boolean(&value, false)?.into(),
            _ => value.into(),
        };
    }
    Ok(())
}

pub(super) fn variant(
    kind: &str,
    title: &str,
    source: &str,
    description: &str,
    content: Option<&Rc<RedValue>>,
    options: &[Rc<RedValue>],
) -> Result<Value> {
    let text = || -> Result<String> {
        Ok(content
            .map(|v| v.expect_text_value())
            .transpose()?
            .unwrap_or_default())
    };
    let mut result = json!({});
    let mut caption = false;
    match kind {
        "位置" | "场所" => {
            let coordinates = array(content.ok_or("请填写坐标数组")?)?;
            let lon: f64 = row_text(coordinates, 0)?.parse()?;
            let lat: f64 = row_text(coordinates, 1)?.parse()?;
            if coordinates.len() != 2
                || !(-180.0..=180.0).contains(&lon)
                || !(-90.0..=90.0).contains(&lat)
            {
                return Err("请填【数组@经度@纬度】，坐标超出范围".into());
            }
            result = json!({"type":if kind == "位置" {"location"} else {"venue"},"latitude":lat,"longitude":lon,"title":super::required(title,"标题")?});
            let thumbs = [
                ("thumbnail_url", 'u'),
                ("thumbnail_width", 'n'),
                ("thumbnail_height", 'n'),
            ];
            if kind == "场所" {
                result["address"] = super::required(source, "地址")?.into();
                let fields = [
                    ("foursquare_id", 't'),
                    ("foursquare_type", 't'),
                    ("google_place_id", 't'),
                    ("google_place_type", 't'),
                ];
                extra_fields(
                    &mut result,
                    options,
                    &[fields.as_slice(), thumbs.as_slice()].concat(),
                )?;
            } else {
                extra_fields(
                    &mut result,
                    options,
                    &[
                        &[
                            ("horizontal_accuracy", 't'),
                            ("live_period", 'n'),
                            ("heading", 'n'),
                            ("proximity_alert_radius", 'n'),
                        ][..],
                        thumbs.as_slice(),
                    ]
                    .concat(),
                )?;
                if let Some(value) = result["horizontal_accuracy"].as_str() {
                    let value: f64 = value.parse()?;
                    if !(0.0..=1500.0).contains(&value) {
                        return Err("水平精度请填 0 至 1500 米".into());
                    }
                    result["horizontal_accuracy"] = value.into();
                }
            }
        }
        "联系人" => {
            result = json!({"type":"contact","first_name":super::required(title,"名字")?,"phone_number":super::required(source,"电话号码")?});
            if !description.is_empty() {
                result["last_name"] = description.into();
            }
            extra_fields(
                &mut result,
                options,
                &[
                    ("vcard", 't'),
                    ("thumbnail_url", 'u'),
                    ("thumbnail_width", 'n'),
                    ("thumbnail_height", 'n'),
                ],
            )?;
        }
        "游戏" => {
            result = json!({"type":"game","game_short_name":super::required(source,"游戏简称")?});
            if !options.is_empty() {
                return Err("游戏没有附加项".into());
            }
        }
        "贴纸" => {
            result =
                json!({"type":"sticker","sticker_file_id":super::required(source,"贴纸文件ID")?});
            if source.contains("://") || !options.is_empty() {
                return Err("贴纸只接受文件ID".into());
            }
        }
        _ => {
            let (wire, key) = match kind {
                "图片" => ("photo", "photo"),
                "视频" => ("video", "video"),
                "文件" => ("document", "document"),
                "音频" => ("audio", "audio"),
                "语音" | "语音便笺" => ("voice", "voice"),
                "动图" | "GIF" => ("gif", "gif"),
                "视频动图" => ("mpeg4_gif", "mpeg4"),
                _ => return Err("未知的行内结果类型".into()),
            };
            if source.contains("://") {
                super::https_url(source)?;
                result[format!("{key}_url")] = source.into();
                let fields: &[(&str, char)] = match wire {
                    "photo" => &[
                        ("thumbnail_url", 'U'),
                        ("photo_width", 'n'),
                        ("photo_height", 'n'),
                        ("show_caption_above_media", 'b'),
                    ],
                    "video" => &[
                        ("thumbnail_url", 'U'),
                        ("mime_type", 'T'),
                        ("video_width", 'n'),
                        ("video_height", 'n'),
                        ("video_duration", 'n'),
                    ],
                    "document" => &[
                        ("mime_type", 'T'),
                        ("thumbnail_url", 'u'),
                        ("thumbnail_width", 'n'),
                        ("thumbnail_height", 'n'),
                    ],
                    "audio" => &[("performer", 't'), ("audio_duration", 'n')],
                    "voice" => &[("voice_duration", 'n')],
                    "gif" => &[
                        ("thumbnail_url", 'U'),
                        ("thumbnail_mime_type", 't'),
                        ("gif_width", 'n'),
                        ("gif_height", 'n'),
                        ("gif_duration", 'n'),
                        ("show_caption_above_media", 'b'),
                    ],
                    _ => &[
                        ("thumbnail_url", 'U'),
                        ("thumbnail_mime_type", 't'),
                        ("mpeg4_width", 'n'),
                        ("mpeg4_height", 'n'),
                        ("mpeg4_duration", 'n'),
                        ("show_caption_above_media", 'b'),
                    ],
                };
                extra_fields(&mut result, options, fields)?;
                if wire == "video"
                    && !matches!(
                        result["mime_type"].as_str(),
                        Some("video/mp4" | "text/html")
                    )
                    || wire == "document"
                        && !matches!(
                            result["mime_type"].as_str(),
                            Some("application/pdf" | "application/zip")
                        )
                    || matches!(wire, "gif" | "mpeg4_gif")
                        && result.get("thumbnail_mime_type").is_some()
                        && !matches!(
                            result["thumbnail_mime_type"].as_str(),
                            Some("image/jpeg" | "image/gif" | "video/mp4")
                        )
                {
                    return Err("行内媒体或缩略图格式不受支持".into());
                }
                if matches!(wire, "audio" | "voice" | "video" | "document") {
                    super::required(title, "标题")?;
                }
            } else {
                result[format!("{key}_file_id")] =
                    super::required(source, "Telegram 文件ID")?.into();
                if matches!(wire, "video" | "document" | "voice") {
                    super::required(title, "标题")?;
                }
                if matches!(wire, "photo" | "gif" | "mpeg4_gif") {
                    extra_fields(&mut result, options, &[("show_caption_above_media", 'b')])?;
                } else if !options.is_empty() {
                    return Err("该类已上传媒体没有附加项".into());
                }
            }
            result["type"] = wire.into();
            if wire != "audio" || result.get("audio_file_id").is_none() {
                if !title.is_empty() {
                    result["title"] = title.into();
                }
            }
            if matches!(wire, "photo" | "video" | "document") && !description.is_empty() {
                result["description"] = description.into();
            }
            caption = true;
        }
    }
    if caption && content.is_some() {
        let value = text()?;
        if !value.is_empty() {
            result["caption"] = value.into();
        }
    }
    Ok(result)
}

pub(super) fn apply_alternate(result: &mut Value, content: &RedValue) -> Result<()> {
    let value = content.expect_text_value()?;
    if value.is_empty() {
        return Ok(());
    }
    if result["type"] == "game" {
        return Err("游戏结果不支持替换发送内容".into());
    }
    result["input_message_content"] = if value.starts_with("[CQ:telegram_rich,") {
        super::super::rich::inline(content)?
    } else {
        json!({"message_text":value})
    };
    Ok(())
}
