//! TTS 命令仅依赖共享服务端口，合成和人声信息由 TTS 插件提供。

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::core::astparser::Ast;
use crate::core::{
    add_fun, rv_array, rv_bin, rv_object, rv_text, RedCommandResult, RedLang, RedValue,
};
use redlang::plugin_ports::{TtsOptions, TtsService, TtsServiceHandle, TtsVoice, TtsVoiceSample};

fn service() -> Result<Arc<TtsServiceHandle>, Box<dyn std::error::Error>> {
    redlang::framework_services()
        .and_then(|services| services.get::<TtsServiceHandle>("tts"))
        .ok_or_else(|| RedLang::make_err("TTS 插件未启用"))
}

fn synthesize(rl: &mut RedLang, params: &[Ast], tts: &dyn TtsService) -> RedCommandResult {
    if params.len() > 7 {
        return Err(RedLang::make_err(
            "TTS 最多接受文本、音色、语速、音调、音量、总超时秒、重试次数七个参数",
        ));
    }
    let text = rl.get_param_text(params, 0)?;
    let player = rl.get_param_text(params, 1)?;
    let player = if player.is_empty() {
        "YunyangNeural"
    } else {
        &player
    };
    let options = TtsOptions::from_parameters(
        &rl.get_param_text(params, 2)?,
        &rl.get_param_text(params, 3)?,
        &rl.get_param_text(params, 4)?,
        &rl.get_param_text(params, 5)?,
        &rl.get_param_text(params, 6)?,
    )
    .map_err(|error| RedLang::make_err(&error))?;
    let audio = tts
        .synthesize_with_options(&text, player, &rl.get_coremap("代理"), &options)
        .map_err(|error| RedLang::make_err(&error))?;
    Ok(Some(rv_bin(audio)))
}

fn voice_value(voice: TtsVoice) -> Rc<RedValue> {
    rv_object(BTreeMap::from([
        ("id".into(), rv_text(voice.id)),
        ("name".into(), rv_text(voice.name)),
        ("locale".into(), rv_text(voice.locale)),
        ("gender".into(), rv_text(voice.gender)),
        (
            "aliases".into(),
            rv_array(voice.aliases.into_iter().map(rv_text).collect()),
        ),
    ]))
}

fn list_voices(rl: &mut RedLang, params: &[Ast], tts: &dyn TtsService) -> RedCommandResult {
    if params.len() > 3 {
        return Err(RedLang::make_err(
            "TTS人声 最多接受语言前缀、性别、总超时秒三个参数",
        ));
    }
    let locale = rl.get_param_text(params, 0)?.trim().to_ascii_lowercase();
    if !locale
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err(RedLang::make_err(
            "语言前缀应为 zh、zh-CN、en-US 等代码，省略或 all 表示全部",
        ));
    }
    let gender = rl.get_param_text(params, 1)?.trim().to_ascii_lowercase();
    let gender = match gender.as_str() {
        "" | "all" => "",
        "male" | "男" => "male",
        "female" | "女" => "female",
        _ => return Err(RedLang::make_err("性别应为 Male、Female、男、女，或省略")),
    };
    let options = TtsOptions::from_parameters("", "", "", &rl.get_param_text(params, 2)?, "")
        .map_err(|error| RedLang::make_err(&error))?;
    let mut voices = tts
        .voices(&rl.get_coremap("代理"), options.timeout_secs)
        .map_err(|error| RedLang::make_err(&error))?;
    voices.retain(|voice| {
        (locale.is_empty()
            || locale == "all"
            || voice.locale.to_ascii_lowercase().starts_with(&locale))
            && (gender.is_empty() || voice.gender.eq_ignore_ascii_case(gender))
    });
    voices.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Some(rv_array(
        voices.into_iter().map(voice_value).collect(),
    )))
}

fn sample_value(sample: TtsVoiceSample) -> Rc<RedValue> {
    rv_object(BTreeMap::from([
        ("id".into(), rv_text(sample.id)),
        ("name".into(), rv_text(sample.name)),
        ("category".into(), rv_text(sample.category)),
        ("gender".into(), rv_text(sample.gender)),
        ("sample_url".into(), rv_text(sample.sample_url)),
        ("source_url".into(), rv_text(sample.source_url)),
    ]))
}

pub(crate) fn register() {
    add_fun(vec!["TTS"], |rl, params| {
        synthesize(rl, params, service()?.0.as_ref())
    });
    add_fun(vec!["TTS人声"], |rl, params| {
        list_voices(rl, params, service()?.0.as_ref())
    });
    add_fun(vec!["TTS样本"], |_rl, _params| {
        Ok(Some(rv_array(
            service()?
                .0
                .samples()
                .into_iter()
                .map(sample_value)
                .collect(),
        )))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::astparser::str_to_ast;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct Call {
        text: String,
        voice: String,
        proxy: String,
        options: TtsOptions,
    }

    #[derive(Default)]
    struct FakeTts {
        calls: Mutex<Vec<Call>>,
    }

    impl TtsService for FakeTts {
        fn synthesize(&self, text: &str, voice: &str, proxy: &str) -> Result<Vec<u8>, String> {
            self.synthesize_with_options(text, voice, proxy, &TtsOptions::default())
        }

        fn synthesize_with_options(
            &self,
            text: &str,
            voice: &str,
            proxy: &str,
            options: &TtsOptions,
        ) -> Result<Vec<u8>, String> {
            self.calls.lock().unwrap().push(Call {
                text: text.into(),
                voice: voice.into(),
                proxy: proxy.into(),
                options: options.clone(),
            });
            Ok(b"audio fixture".to_vec())
        }

        fn voices(&self, _proxy: &str, _timeout_secs: u64) -> Result<Vec<TtsVoice>, String> {
            Ok(vec![
                TtsVoice {
                    id: "zh-CN-XiaoxiaoNeural".into(),
                    name: "Xiaoxiao".into(),
                    locale: "zh-CN".into(),
                    gender: "Female".into(),
                    aliases: vec!["晓晓".into()],
                },
                TtsVoice {
                    id: "zh-CN-YunxiNeural".into(),
                    name: "Yunxi".into(),
                    locale: "zh-CN".into(),
                    gender: "Male".into(),
                    aliases: vec!["云希".into()],
                },
                TtsVoice {
                    id: "fr-FR-DeniseNeural".into(),
                    name: "Denise".into(),
                    locale: "fr-FR".into(),
                    gender: "Female".into(),
                    aliases: vec![],
                },
            ])
        }
    }

    fn params(values: &[&str]) -> Vec<Ast> {
        values
            .iter()
            .map(|value| str_to_ast((*value).into()))
            .collect()
    }

    #[test]
    fn old_tts_call_keeps_default_voice_and_binary_result() {
        let tts = FakeTts::default();
        let result = synthesize(&mut RedLang::new(), &params(&["你好"]), &tts)
            .unwrap()
            .unwrap();
        assert!(
            matches!(result.as_ref(), RedValue::Bin(bytes) if bytes.as_slice() == b"audio fixture")
        );
        let calls = tts.calls.lock().unwrap();
        assert_eq!(calls[0].voice, "YunyangNeural");
        assert_eq!(calls[0].options, TtsOptions::default());
    }

    #[test]
    fn options_and_raw_text_reach_service_without_reparsing() {
        let tts = FakeTts::default();
        let mut rl = RedLang::new();
        rl.set_coremap("代理", "http://127.0.0.1:7890").unwrap();
        let text = "中文 & <hello>\n【@文字##";
        synthesize(
            &mut rl,
            &params(&[text, "云希", "-15%", "+5Hz", "-20", "20", "2"]),
            &tts,
        )
        .unwrap();
        let calls = tts.calls.lock().unwrap();
        assert_eq!(calls[0].text, text);
        assert_eq!(calls[0].voice, "云希");
        assert_eq!(calls[0].proxy, "http://127.0.0.1:7890");
        assert_eq!(calls[0].options.rate, -15);
        assert_eq!(calls[0].options.pitch, 5);
        assert_eq!(calls[0].options.volume, -20);
        assert_eq!(calls[0].options.timeout_secs, 20);
        assert_eq!(calls[0].options.retries, 2);
    }

    #[test]
    fn invalid_option_is_rejected_before_service_call() {
        let tts = FakeTts::default();
        let result = synthesize(
            &mut RedLang::new(),
            &params(&["你好", "晓晓", "fast"]),
            &tts,
        );
        assert!(result.is_err());
        assert!(tts.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn voice_listing_filters_locale_and_gender_and_preserves_structure() {
        let tts = FakeTts::default();
        let value = list_voices(&mut RedLang::new(), &params(&["ZH", "女"]), &tts)
            .unwrap()
            .unwrap();
        let RedValue::Array(voices) = value.as_ref() else {
            panic!("expected array")
        };
        assert_eq!(voices.len(), 1);
        let RedValue::Object(voice) = voices[0].as_ref() else {
            panic!("expected object")
        };
        assert_eq!(voice["id"].as_text(), Some("zh-CN-XiaoxiaoNeural"));
        assert!(matches!(voice["aliases"].as_ref(), RedValue::Array(_)));
        let all = list_voices(&mut RedLang::new(), &params(&["all"]), &tts)
            .unwrap()
            .unwrap();
        assert!(matches!(all.as_ref(), RedValue::Array(voices) if voices.len() == 3));
        assert!(list_voices(&mut RedLang::new(), &params(&["zh", "unknown"]), &tts).is_err());
    }
}
