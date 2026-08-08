use std::io::{BufReader, Read};

use symphonia::core::audio::SampleBuffer;
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
    let mut sample_buf: Option<SampleBuffer<i16>> = None;

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
        if sample_buf.is_none() {
            let spec = *decoded.spec();
            if spec.rate == 0 || spec.channels.count() == 0 {
                return Err("invalid m4a stream format".into());
            }
            pcm.sample_rate = spec.rate as usize;
            pcm.channel_num = spec.channels.count();
            sample_buf = Some(SampleBuffer::<i16>::new(decoded.capacity() as u64, spec));
        }
        if let Some(buf) = &mut sample_buf {
            buf.copy_interleaved_ref(decoded);
            for sample in buf.samples() {
                pcm.data.push(*sample as f64);
            }
        }
    }

    if pcm.data.is_empty() {
        return Err("m4a contains no samples".into());
    }
    if pcm.data.len() % pcm.channel_num != 0 {
        return Err("m4a sample count is not channel-aligned".into());
    }
    Ok(pcm)
}
