//! Chinese voice aliases accepted by RedLang's TTS commands.

pub struct ReferenceVoice {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
}

pub const VOICES: &[ReferenceVoice] = &[
    ReferenceVoice {
        id: "zh-CN-XiaoxiaoNeural",
        aliases: &["晓晓"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoyiNeural",
        aliases: &["晓依", "晓伊"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaohanNeural",
        aliases: &["晓涵"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaomoNeural",
        aliases: &["晓墨"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoruiNeural",
        aliases: &["晓睿"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoshuangNeural",
        aliases: &["晓双"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaomengNeural",
        aliases: &["晓梦"],
    },
    ReferenceVoice {
        id: "zh-CN-YunxiNeural",
        aliases: &["云希"],
    },
    ReferenceVoice {
        id: "zh-CN-YunyangNeural",
        aliases: &["云扬"],
    },
    ReferenceVoice {
        id: "zh-CN-YunjianNeural",
        aliases: &["云健"],
    },
    ReferenceVoice {
        id: "zh-CN-YunxiaNeural",
        aliases: &["云夏"],
    },
    ReferenceVoice {
        id: "zh-CN-YunzeNeural",
        aliases: &["云泽"],
    },
    ReferenceVoice {
        id: "zh-CN-YunfengNeural",
        aliases: &["云枫"],
    },
    ReferenceVoice {
        id: "zh-CN-liaoning-YunbiaoNeural",
        aliases: &["云彪（东北）", "云彪", "东北云彪", "云彪（辽宁）"],
    },
    ReferenceVoice {
        id: "zh-CN-liaoning-XiaobeiNeural",
        aliases: &["晓北（东北）", "晓北", "东北晓北", "晓北（辽宁）"],
    },
    ReferenceVoice {
        id: "zh-CN-henan-YundengNeural",
        aliases: &["云登（河南）", "云登", "河南云登"],
    },
    ReferenceVoice {
        id: "zh-CN-shandong-YunxiangNeural",
        aliases: &["云翔（山东）", "云翔", "山东云翔"],
    },
    ReferenceVoice {
        id: "zh-CN-sichuan-YunxiNeural",
        aliases: &["云熙（四川）", "云熙", "四川云希", "云希（四川）"],
    },
    ReferenceVoice {
        id: "zh-CN-guangxi-YunqiNeural",
        aliases: &["云奇（广西）", "云奇", "广西云奇"],
    },
    ReferenceVoice {
        id: "zh-CN-shaanxi-XiaoniNeural",
        aliases: &["晓妮（陕西）", "晓妮", "陕西晓妮"],
    },
    ReferenceVoice {
        id: "wuu-CN-YunzheNeural",
        aliases: &["云哲（吴语）", "吴语云哲"],
    },
    ReferenceVoice {
        id: "wuu-CN-XiaotongNeural",
        aliases: &["晓彤（吴语）", "晓彤", "吴语晓彤"],
    },
    ReferenceVoice {
        id: "zh-HK-HiuGaaiNeural",
        aliases: &["晓佳"],
    },
    ReferenceVoice {
        id: "zh-HK-HiuMaanNeural",
        aliases: &["晓蔓", "晓曼"],
    },
    ReferenceVoice {
        id: "zh-HK-WanLungNeural",
        aliases: &["云龙"],
    },
    ReferenceVoice {
        id: "zh-TW-HsiaoChenNeural",
        aliases: &["晓晨", "晓臻"],
    },
    ReferenceVoice {
        id: "zh-TW-HsiaoYuNeural",
        aliases: &["晓雨"],
    },
    ReferenceVoice {
        id: "zh-TW-YunJheNeural",
        aliases: &["云哲", "台湾云哲", "云哲（台湾）"],
    },
];

pub fn reference_for_id(id: &str) -> Option<&'static ReferenceVoice> {
    VOICES
        .iter()
        .find(|voice| voice.id.eq_ignore_ascii_case(id))
}

pub fn id_for_alias(alias: &str) -> Option<&'static str> {
    let normalized = alias.trim().replace('(', "（").replace(')', "）");
    VOICES
        .iter()
        .find(|voice| {
            voice
                .aliases
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&normalized))
        })
        .map(|voice| voice.id)
}
