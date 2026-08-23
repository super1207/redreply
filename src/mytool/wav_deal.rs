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
    // fmt 块可能带扩展字段（fmt_chunk_size > 16），跳过后再找 data 块
    let consumed_fmt_body = 2 + 2 + 4 + 2 + 2; // 已读的 fmt 固定字段
    if fmt_chunk_size > consumed_fmt_body as u32 {
        readn(
            &mut bufr,
            (fmt_chunk_size as usize - consumed_fmt_body) as u32,
        )?;
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
            let d = data[index] as i8 as f64;
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
            let d = i32::from_le_bytes([data[index], data[index + 1], data[index + 2], 0]) as f64;
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
            // 整数样本误按 float 解码会得到无意义的杂音数据
            let d = if fmt_code_num == 3 {
                f32::from_le_bytes(byte_arr) as f64
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
