use std::io::{BufReader, Read};

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::all_to_silk::PCMStruct;

/// Decode MP4/M4A (AAC-LC) to interleaved 16-bit PCM. MP4 channel order is
/// already WAVE/FLAC order (FL, FR, ...), so the front pair stays first for
/// the headphone-first downmix policy.
pub fn deal_m4a(mut bufr: BufReader<&[u8]>) -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut buf = vec![];
    bufr.read_to_end(&mut buf)?;

    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(buf)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or("m4a contains no audio track")?;
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut pcm = PCMStruct {
        channel_num: 1,
        bits_per_sample: 16,
        sample_rate: 24000,
        data: Vec::new(),
    };
    let mut init_spec: Option<SignalSpec> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(Box::new(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        match init_spec {
            None => {
                if spec.rate == 0 || spec.channels.count() == 0 {
                    return Err("invalid m4a stream format".into());
                }
                pcm.sample_rate = spec.rate as usize;
                pcm.channel_num = spec.channels.count();
                init_spec = Some(spec);
            }
            Some(s) => {
                // 中途变规格会导致样本率/通道数与已解码数据不一致，
                // 显式报错而不是触发 SampleBuffer 的容量断言 panic
                if s != spec {
                    return Err("m4a stream format changed mid-stream".into());
                }
            }
        }
        // 每个包按自身容量分配缓冲，避免中途出现更大帧时触发断言
        let mut sample_buf = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        for sample in sample_buf.samples() {
            pcm.data.push(*sample as f64);
        }
        super::all_to_silk::ensure_sample_limit(pcm.data.len())?;
    }

    if pcm.data.is_empty() {
        return Err("m4a contains no samples".into());
    }
    if !pcm.data.len().is_multiple_of(pcm.channel_num) {
        return Err("m4a sample count is not channel-aligned".into());
    }
    Ok(pcm)
}
