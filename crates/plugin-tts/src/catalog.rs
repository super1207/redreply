//! Reference names and existing sample links, not an availability allow-list.
//! Facts from novel2voice, commit 4df8583e37268fadd80759bd3c0c0f19e8977e3a.

pub const SOURCE_URL: &str =
    "https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/index.html";
pub const SAMPLE_BASE: &str = "https://jangviktor-web.github.io/novel2voice/scripts/voice_samples/";

pub struct ReferenceVoice {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub gender: &'static str,
    pub sample_file: &'static str,
    pub aliases: &'static [&'static str],
}

pub const VOICES: &[ReferenceVoice] = &[
    ReferenceVoice {
        id: "zh-CN-XiaoxiaoNeural",
        name: "晓晓",
        category: "female",
        gender: "Female",
        sample_file: "XiaoxiaoNeural.mp3",
        aliases: &["晓晓"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoyiNeural",
        name: "晓依",
        category: "female",
        gender: "Female",
        sample_file: "XiaoyiNeural.mp3",
        aliases: &["晓依", "晓伊"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaohanNeural",
        name: "晓涵",
        category: "female",
        gender: "Female",
        sample_file: "XiaohanNeural.mp3",
        aliases: &["晓涵"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaomoNeural",
        name: "晓墨",
        category: "female",
        gender: "Female",
        sample_file: "XiaomoNeural.mp3",
        aliases: &["晓墨"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoruiNeural",
        name: "晓睿",
        category: "female",
        gender: "Female",
        sample_file: "XiaoruiNeural.mp3",
        aliases: &["晓睿"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaoshuangNeural",
        name: "晓双",
        category: "female",
        gender: "Female",
        sample_file: "XiaoshuangNeural.mp3",
        aliases: &["晓双"],
    },
    ReferenceVoice {
        id: "zh-CN-XiaomengNeural",
        name: "晓梦",
        category: "female",
        gender: "Female",
        sample_file: "XiaomengNeural.mp3",
        aliases: &["晓梦"],
    },
    ReferenceVoice {
        id: "zh-CN-YunxiNeural",
        name: "云希",
        category: "male",
        gender: "Male",
        sample_file: "YunxiNeural.mp3",
        aliases: &["云希"],
    },
    ReferenceVoice {
        id: "zh-CN-YunyangNeural",
        name: "云扬",
        category: "male",
        gender: "Male",
        sample_file: "YunyangNeural.mp3",
        aliases: &["云扬"],
    },
    ReferenceVoice {
        id: "zh-CN-YunjianNeural",
        name: "云健",
        category: "male",
        gender: "Male",
        sample_file: "YunjianNeural.mp3",
        aliases: &["云健"],
    },
    ReferenceVoice {
        id: "zh-CN-YunxiaNeural",
        name: "云夏",
        category: "male",
        gender: "Male",
        sample_file: "YunxiaNeural.mp3",
        aliases: &["云夏"],
    },
    ReferenceVoice {
        id: "zh-CN-YunzeNeural",
        name: "云泽",
        category: "male",
        gender: "Male",
        sample_file: "YunzeNeural.mp3",
        aliases: &["云泽"],
    },
    ReferenceVoice {
        id: "zh-CN-YunfengNeural",
        name: "云枫",
        category: "male",
        gender: "Male",
        sample_file: "YunfengNeural.mp3",
        aliases: &["云枫"],
    },
    ReferenceVoice {
        id: "zh-CN-liaoning-YunbiaoNeural",
        name: "云彪（东北）",
        category: "dialect",
        gender: "Male",
        sample_file: "liaoning-Yunbiao.mp3",
        aliases: &["云彪（东北）", "云彪", "东北云彪", "云彪（辽宁）"],
    },
    ReferenceVoice {
        id: "zh-CN-liaoning-XiaobeiNeural",
        name: "晓北（东北）",
        category: "dialect",
        gender: "Female",
        sample_file: "liaoning-Xiaobei.mp3",
        aliases: &["晓北（东北）", "晓北", "东北晓北", "晓北（辽宁）"],
    },
    ReferenceVoice {
        id: "zh-CN-henan-YundengNeural",
        name: "云登（河南）",
        category: "dialect",
        gender: "Male",
        sample_file: "henan-Yundeng.mp3",
        aliases: &["云登（河南）", "云登", "河南云登"],
    },
    ReferenceVoice {
        id: "zh-CN-shandong-YunxiangNeural",
        name: "云翔（山东）",
        category: "dialect",
        gender: "Male",
        sample_file: "shandong-Yunxiang.mp3",
        aliases: &["云翔（山东）", "云翔", "山东云翔"],
    },
    ReferenceVoice {
        id: "zh-CN-sichuan-YunxiNeural",
        name: "云熙（四川）",
        category: "dialect",
        gender: "Male",
        sample_file: "sichuan-Yunxi.mp3",
        aliases: &["云熙（四川）", "云熙", "四川云希", "云希（四川）"],
    },
    ReferenceVoice {
        id: "zh-CN-guangxi-YunqiNeural",
        name: "云奇（广西）",
        category: "dialect",
        gender: "Male",
        sample_file: "guangxi-Yunqi.mp3",
        aliases: &["云奇（广西）", "云奇", "广西云奇"],
    },
    ReferenceVoice {
        id: "zh-CN-shaanxi-XiaoniNeural",
        name: "晓妮（陕西）",
        category: "dialect",
        gender: "Female",
        sample_file: "shaanxi-Xiaoni.mp3",
        aliases: &["晓妮（陕西）", "晓妮", "陕西晓妮"],
    },
    ReferenceVoice {
        id: "wuu-CN-YunzheNeural",
        name: "云哲（吴语）",
        category: "dialect",
        gender: "Male",
        sample_file: "wuu-Yunzhe.mp3",
        aliases: &["云哲（吴语）", "吴语云哲"],
    },
    ReferenceVoice {
        id: "wuu-CN-XiaotongNeural",
        name: "晓彤（吴语）",
        category: "dialect",
        gender: "Female",
        sample_file: "wuu-Xiaotong.mp3",
        aliases: &["晓彤（吴语）", "晓彤", "吴语晓彤"],
    },
    ReferenceVoice {
        id: "zh-HK-HiuGaaiNeural",
        name: "晓佳",
        category: "hk",
        gender: "Female",
        sample_file: "HK-HiuGaai.mp3",
        aliases: &["晓佳"],
    },
    ReferenceVoice {
        id: "zh-HK-HiuMaanNeural",
        name: "晓蔓",
        category: "hk",
        gender: "Female",
        sample_file: "HK-HiuMaan.mp3",
        aliases: &["晓蔓", "晓曼"],
    },
    ReferenceVoice {
        id: "zh-HK-WanLungNeural",
        name: "云龙",
        category: "hk",
        gender: "Male",
        sample_file: "HK-WanLung.mp3",
        aliases: &["云龙"],
    },
    ReferenceVoice {
        id: "zh-TW-HsiaoChenNeural",
        name: "晓晨",
        category: "tw",
        gender: "Female",
        sample_file: "TW-HsiaoChen.mp3",
        aliases: &["晓晨", "晓臻"],
    },
    ReferenceVoice {
        id: "zh-TW-HsiaoYuNeural",
        name: "晓雨",
        category: "tw",
        gender: "Female",
        sample_file: "TW-HsiaoYu.mp3",
        aliases: &["晓雨"],
    },
    ReferenceVoice {
        id: "zh-TW-YunJheNeural",
        name: "云哲",
        category: "tw",
        gender: "Male",
        sample_file: "TW-YunJhe.mp3",
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
