use std::io::{BufReader, Read};

use super::all_to_silk::PCMStruct;

fn readn<T>(reader: T, nbytes: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>>
where
    T: Read,
{
    // 头字段完全不可信：with_capacity 会按声明大小立即分配(最大约4GiB)，
    // 分配失败会直接 abort 整个进程。改为随读随长，并限制总量上限。
    const MAX_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
    if u64::from(nbytes) > MAX_AUDIO_BYTES {
        return Err(format!("音频块大小超出限制: {nbytes}").into());
    }
    let mut buf = Vec::new();
    let mut chunk = reader.take(u64::from(nbytes));
    // 硬性 IO 错误必须传播；短读（RIFF 头声明的长度大于实际剩余）容忍，
    // 后续对齐检查会拦截明显异常的数据。
    chunk.read_to_end(&mut buf)?;
    Ok(buf)
}

fn read2<T>(reader: &mut T) -> Result<[u8; 2], Box<dyn std::error::Error>>
where
    T: Read,
{
    let mut buf = [0_u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

fn read4<T>(reader: &mut T) -> Result<[u8; 4], Box<dyn std::error::Error>>
where
    T: Read,
{
    let mut buf = [0_u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn deal_wav(mut bufr: BufReader<&[u8]>) -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut pcm = PCMStruct {
        channel_num: 1,
        bits_per_sample: 16,
        sample_rate: 24000,
        data: Vec::new(),
    };
    let riff_tag = readn(&mut bufr, 4)?;
    if String::from_utf8_lossy(&riff_tag) != "RIFF" {
        return Err("not a wav file".into());
    }
    let _total_size = readn(&mut bufr, 4)?;
    let wave_tag = readn(&mut bufr, 4)?;
    if String::from_utf8_lossy(&wave_tag) != "WAVE" {
        return Err("not a wav file".into());
    }
    let fmt_chunk_tag = readn(&mut bufr, 4)?;
    if String::from_utf8_lossy(&fmt_chunk_tag) != "fmt " {
        return Err("not a wav file".into());
    }
    let fmt_chunk_size = u32::from_le_bytes(read4(&mut bufr)?);
    let fmt_code = read2(&mut bufr);
    let fmt_code_num = u16::from_le_bytes(fmt_code?);
    // 1 = 整数 PCM，3 = IEEE float；其余格式（如压缩 WAV）不支持
    if fmt_code_num != 1 && fmt_code_num != 3 {
        return Err(format!("not a support wav file,fmt_code={fmt_code_num}").into());
    }
    let num_channels = read2(&mut bufr);
    pcm.channel_num = u16::from_le_bytes(num_channels?) as usize;
    let sampling_rate = read4(&mut bufr);
    pcm.sample_rate = u32::from_le_bytes(sampling_rate?) as usize;
    let _byte_rate = readn(&mut bufr, 4)?;
    let _block_alignment = readn(&mut bufr, 2)?;
    let bits_per_sample = read2(&mut bufr);
    pcm.bits_per_sample = u16::from_le_bytes(bits_per_sample?) as usize;
    // fmt 固定字段共 16 字节：fmt_code(2)+channels(2)+sample_rate(4)+byte_rate(4)+block_align(2)+bits(2)
    let consumed_fmt_body: u32 = 2 + 2 + 4 + 4 + 2 + 2;
    if fmt_chunk_size < consumed_fmt_body {
        // 声明的 fmt 长度连固定字段都放不下，说明文件损坏；
        // 继续解析会把后面块的内容当成 fmt 扩展字段，导致块扫描错位
        return Err("not a wav file".into());
    }
    // fmt 块可能带扩展字段（fmt_chunk_size > 16），跳过后再找 data 块
    if fmt_chunk_size > consumed_fmt_body {
        readn(&mut bufr, fmt_chunk_size - consumed_fmt_body)?;
    }
    // 跳过 fmt 与 data 之间的任意中间块（LIST/fact/bext 等），直到找到 data 块
    let data;
    loop {
        let chunk_tag = String::from_utf8_lossy(&readn(&mut bufr, 4)?).into_owned();
        let chunk_size = u32::from_le_bytes(read4(&mut bufr)?);
        if chunk_tag == "data" {
            data = readn(&mut bufr, chunk_size)?;
            break;
        }
        // 非数据块按声明长度跳过（readn 内部有总量上限保护）
        readn(&mut bufr, chunk_size)?;
    }
    // check
    let mod_num = pcm.bits_per_sample / 8 * pcm.channel_num;
    if mod_num == 0 {
        return Err("not a wav file".into());
    }
    if data.len() % (pcm.bits_per_sample / 8 * pcm.channel_num) != 0 {
        return Err("not a wav file".into());
    }
    let mut index: usize = 0;
    let per_sample_bytes = pcm.bits_per_sample / 8;
    while index < data.len() {
        if per_sample_bytes == 1 {
            // WAV 的 8-bit PCM 规范为无符号，128 是静音中心，按有符号解析会整体偏移 -128
            let d = (data[index] as i16 - 128) as f64;
            pcm.data.push(d);
            index += 1;
        } else if per_sample_bytes == 2 {
            if index + 1 >= data.len() {
                return Err("not a wav file".into());
            }
            let d = i16::from_le_bytes([data[index], data[index + 1]]) as f64;
            pcm.data.push(d);
            index += 2;
        } else if per_sample_bytes == 3 {
            if index + 2 >= data.len() {
                return Err("not a wav file".into());
            }
            // 24 位样本需按最高字节的符号位扩展到 32 位，
            // 补 0x00 会让负样本变成巨大正值，波形完全失真
            let sign_byte = if data[index + 2] & 0x80 != 0 { 0xFF } else { 0x00 };
            let d = i32::from_le_bytes([
                data[index],
                data[index + 1],
                data[index + 2],
                sign_byte,
            ]) as f64;
            pcm.data.push(d);
            index += 3;
        } else if per_sample_bytes == 4 {
            if index + 3 >= data.len() {
                return Err("not a wav file".into());
            }
            let byte_arr = [
                data[index],
                data[index + 1],
                data[index + 2],
                data[index + 3],
            ];
            // fmt_code=1 是 32 位整数 PCM，fmt_code=3 才是 IEEE float；
            // 整数样本误按 float 解码会得到无意义的杂音数据。
            // float 样本域是 ±1.0，需换算到 32 位整数域（±2^31），
            // 下游统一按 2^(16-bits) 缩放到 16 位时才是正常音量
            let d = if fmt_code_num == 3 {
                f32::from_le_bytes(byte_arr) as f64 * 2_147_483_648.0
            } else {
                i32::from_le_bytes(byte_arr) as f64
            };
            pcm.data.push(d);
            index += 4;
        } else {
            return Err("not a wav file".into());
        }
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::deal_wav;
    use std::io::BufReader;

    /// 构造最小 WAV：可选 fmt 扩展字段与 data 前的中间块
    fn build_wav(fmt_code: u16, bits: u16, fmt_extra: &[u8], pre_data_chunks: &[u8], data: &[u8]) -> Vec<u8> {
        let fmt_body_len = 16 + fmt_extra.len();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        let riff_size = 4 + 8 + fmt_body_len + pre_data_chunks.len() + 8 + data.len();
        wav.extend_from_slice(&(riff_size as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(fmt_body_len as u32).to_le_bytes());
        wav.extend_from_slice(&fmt_code.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // 单声道
        wav.extend_from_slice(&24000u32.to_le_bytes()); // 采样率
        wav.extend_from_slice(&48000u32.to_le_bytes()); // byte_rate
        wav.extend_from_slice(&(bits / 8).to_le_bytes()); // block_align
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(fmt_extra);
        wav.extend_from_slice(pre_data_chunks);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(data);
        wav
    }

    fn parse(wav: &[u8]) -> crate::mytool::all_to_silk::PCMStruct {
        deal_wav(BufReader::new(wav)).expect("deal_wav 解析失败")
    }

    #[test]
    fn standard_16_byte_fmt_wav_parses() {
        // 回归：旧代码把 fmt 固定字段算成 12 字节，标准 fmt=16 的 WAV
        // 会多跳 4 字节吞掉 data 标签导致解析失败
        let samples = 7i16.to_le_bytes();
        let pcm = parse(&build_wav(1, 16, &[], &[], &samples));
        assert_eq!(pcm.bits_per_sample, 16);
        assert_eq!(pcm.sample_rate, 24000);
        assert_eq!(pcm.data, vec![7.0]);
    }

    #[test]
    fn fmt_with_extension_fields_parses() {
        // fmt=18（cbSize=0）：扩展字段应被精确跳过，不多不少
        let samples = (-3i16).to_le_bytes();
        let pcm = parse(&build_wav(1, 16, &[0, 0], &[], &samples));
        assert_eq!(pcm.data, vec![-3.0]);
    }

    #[test]
    fn intermediate_chunks_are_skipped() {
        // fmt 与 data 之间的 LIST 块（含 4 字节 "LIST" + 4 字节长度 + 4 字节内容）
        let mut list = Vec::new();
        list.extend_from_slice(b"LIST");
        list.extend_from_slice(&4u32.to_le_bytes());
        list.extend_from_slice(b"INFO");
        let samples = (5i16).to_le_bytes();
        let pcm = parse(&build_wav(1, 16, &[], &list, &samples));
        assert_eq!(pcm.data, vec![5.0]);
    }

    #[test]
    fn int24_negative_samples_sign_extend() {
        // -1 的 24 位小端表示是 FF FF FF，符号扩展后应为 -1.0；
        // 旧实现补 0x00 会把它解析成 16777215
        let pcm = parse(&build_wav(1, 24, &[], &[], &[0xFF, 0xFF, 0xFF]));
        assert_eq!(pcm.data, vec![-1.0]);
    }

    #[test]
    fn int24_positive_samples_unchanged() {
        // 0x7FFFFE = 8388606，最高位为 0 时不做符号扩展
        let pcm = parse(&build_wav(1, 24, &[], &[], &[0xFE, 0xFF, 0x7F]));
        assert_eq!(pcm.data, vec![8388606.0]);
    }

    #[test]
    fn uint8_samples_centered_on_128() {
        // 8-bit PCM 规范为无符号：128 是静音中心，0 是最小值
        let pcm = parse(&build_wav(1, 8, &[], &[], &[128, 0, 255]));
        assert_eq!(pcm.data, vec![0.0, -128.0, 127.0]);
    }

    #[test]
    fn float32_full_scale_maps_to_int32_domain() {
        // float WAV 样本域是 ±1.0，换算到 32 位整数域后
        // 下游 2^(16-32) 缩放才能得到正常音量（旧实现输出近乎静音）
        let pcm = parse(&build_wav(3, 32, &[], &[], &1.0f32.to_le_bytes()));
        assert_eq!(pcm.data, vec![2_147_483_648.0]);
        let pcm = parse(&build_wav(3, 32, &[], &[], &(-1.0f32).to_le_bytes()));
        assert_eq!(pcm.data, vec![-2_147_483_648.0]);
    }

    #[test]
    fn int32_samples_not_treated_as_float() {
        let samples = 100000i32.to_le_bytes();
        let pcm = parse(&build_wav(1, 32, &[], &[], &samples));
        assert_eq!(pcm.data, vec![100000.0]);
    }

    #[test]
    fn truncated_fmt_size_is_rejected() {
        // fmt_chunk_size < 16 应直接报错而不是错位解析
        let mut wav = build_wav(1, 16, &[], &[], &[0, 0]);
        wav[16..20].copy_from_slice(&12u32.to_le_bytes());
        assert!(deal_wav(BufReader::new(&wav[..])).is_err());
    }
}
