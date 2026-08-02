use super::all_to_silk::PCMStruct;

const SILK_SAMPLE_RATE: usize = 24_000;
const SILK_FRAME_SAMPLES: usize = SILK_SAMPLE_RATE / 50; // 20ms per frame
// QQ-compatible profile: 24 kHz wideband PCM encoded at 24 kbps.
const SILK_BITRATE: i32 = 24_000;

// 97 taps total. Keeps the passband flat while giving enough transition width
// to suppress content above the target Nyquist.
const RESAMPLE_TAPS: i64 = 48;
const RESAMPLE_CUTOFF_GUARD: f64 = 0.96;

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    }
}

/// Four-term Blackman-Harris window centered at x=0, x in [-1, 1].
fn blackman_harris(x: f64) -> f64 {
    const A0: f64 = 0.35875;
    const A1: f64 = 0.48829;
    const A2: f64 = 0.14128;
    const A3: f64 = 0.01168;
    A0 + A1 * (std::f64::consts::PI * x).cos()
        + A2 * (2.0 * std::f64::consts::PI * x).cos()
        + A3 * (3.0 * std::f64::consts::PI * x).cos()
}

/// Band-limited windowed-sinc resampling into a fixed-size output buffer.
pub fn resample(in_pcm: &[f64], out_pcm: &mut [f64]) {
    let in_count = in_pcm.len();
    let out_count = out_pcm.len();
    if in_count == 0 || out_count == 0 {
        return;
    }
    if in_count == out_count {
        out_pcm.copy_from_slice(in_pcm);
        return;
    }

    let step = in_count as f64 / out_count as f64;
    let cutoff = (1.0 / step).min(1.0) * RESAMPLE_CUTOFF_GUARD;
    let taps = RESAMPLE_TAPS as f64;
    for (i, sample) in out_pcm.iter_mut().enumerate() {
        let pos = (i as f64 + 0.5) * step - 0.5;
        let center = pos.floor() as i64;
        let frac = pos - center as f64;
        let mut acc = 0.0;
        let mut weight_sum = 0.0;
        for k in -RESAMPLE_TAPS..=RESAMPLE_TAPS {
            let index = center + k;
            if index < 0 || index >= in_count as i64 {
                continue;
            }
            let distance = k as f64 - frac;
            let weight = sinc(distance * cutoff) * blackman_harris(distance / taps);
            acc += in_pcm[index as usize] * weight;
            weight_sum += weight;
        }
        *sample = if weight_sum.abs() > 1e-12 {
            acc / weight_sum
        } else {
            in_pcm[pos.round().clamp(0.0, (in_count - 1) as f64) as usize]
        };
    }
}

/// Headphone-first downmix coefficients for standard WAVE/FLAC channel order.
/// Only FL/FR contribute; center, LFE, and surround are deliberately discarded.
pub fn downmix_coeffs(channel_num: usize) -> Vec<f64> {
    match channel_num {
        0 => Vec::new(),
        1 => vec![1.0],
        n => {
            let mut weights = vec![0.0; n];
            weights[0] = 0.5; // FL
            weights[1] = 0.5; // FR
            weights
        }
    }
}

/// Convert interleaved native-depth PCM to 16-bit-scale mono.
pub fn downmix_to_mono(pcm: &PCMStruct) -> Result<Vec<f64>, String> {
    if pcm.channel_num == 0 {
        return Err("audio channel count is zero".into());
    }
    if pcm.sample_rate == 0 {
        return Err("audio sample rate is zero".into());
    }
    if pcm.bits_per_sample == 0 || pcm.bits_per_sample > 32 {
        return Err(format!("unsupported PCM depth: {}", pcm.bits_per_sample));
    }
    if pcm.data.is_empty() {
        return Err("audio contains no samples".into());
    }
    if pcm.data.len() % pcm.channel_num != 0 {
        return Err("PCM sample count is not channel-aligned".into());
    }

    let channels = pcm.channel_num;
    let bit_scale = f64::powi(2.0, 16 - pcm.bits_per_sample as i32);
    let frames = pcm.data.len() / channels;
    let weights = downmix_coeffs(channels);

    let mut mono = Vec::with_capacity(frames);
    for frame in pcm.data.chunks_exact(channels) {
        let sample = frame
            .iter()
            .zip(&weights)
            .map(|(value, weight)| value * bit_scale * weight)
            .sum();
        mono.push(sample);
    }
    Ok(mono)
}

/// Clamp samples to i16 range with minimal whole-stream attenuation.
pub fn fit_i16_range(samples: &mut [f64]) {
    let max = samples
        .iter()
        .fold(f64::NEG_INFINITY, |current, value| current.max(*value));
    let min = samples
        .iter()
        .fold(f64::INFINITY, |current, value| current.min(*value));
    let mut gain = 1.0f64;
    if max > 32767.0 {
        gain = gain.min(32767.0 / max);
    }
    if min < -32768.0 {
        gain = gain.min(-32768.0 / min);
    }
    if gain < 1.0 {
        for value in samples {
            *value *= gain;
        }
    }
}

/// Encode arbitrary PCM as Tencent SILK (24 kHz, 24 kbps, mono).
pub fn to_qq_silk(pcm: &PCMStruct) -> Result<Vec<u8>, String> {
    let mono = downmix_to_mono(pcm)?;
    let mut mono_24k = if pcm.sample_rate == SILK_SAMPLE_RATE {
        mono
    } else {
        let output_len_u128 = (mono.len() as u128)
            .saturating_mul(SILK_SAMPLE_RATE as u128)
            .saturating_add((pcm.sample_rate / 2) as u128)
            / pcm.sample_rate as u128;
        let output_len = usize::try_from(output_len_u128)
            .map_err(|_| "resampled audio is too large".to_string())?;
        if output_len == 0 {
            return Err("audio is too short after resampling".into());
        }
        let mut resampled = vec![0.0; output_len];
        resample(&mono, &mut resampled);
        resampled
    };
    fit_i16_range(&mut mono_24k);
    // Pad to SILK frame boundary (20ms = 480 samples at 24kHz)
    let remainder = mono_24k.len() % SILK_FRAME_SAMPLES;
    if remainder != 0 {
        let padded_len = mono_24k
            .len()
            .checked_add(SILK_FRAME_SAMPLES - remainder)
            .ok_or_else(|| "resampled audio is too large".to_string())?;
        mono_24k.resize(padded_len, 0.0);
    }

    let mut pcm16 = Vec::with_capacity(mono_24k.len().saturating_mul(2));
    for sample in mono_24k {
        pcm16.extend_from_slice(&(sample.round() as i16).to_le_bytes());
    }
    let silk = silk_rs_no_llvm::encode_silk(pcm16, SILK_SAMPLE_RATE as i32, SILK_BITRATE, true)
        .map_err(|error| format!("silk encode failed: {error}"))?;
    Ok(silk)
}