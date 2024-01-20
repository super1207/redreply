use std::io::{BufReader, Read};
use crate::mytool::PCMStruct;

fn readn<T>(reader: T, nbytes: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>>
where
    T: Read,
{
    let mut buf = Vec::with_capacity(nbytes.try_into()?);
    let mut chunk = reader.take(u64::from(nbytes));
    let _val = chunk.read_to_end(&mut buf);
    Ok(buf)
}

fn read2<T>(reader: &mut T) -> [u8; 2]
where
    T: Read,
{
    let mut buf = [0_u8; 2];
    let _nbytes = reader.read(&mut buf);
    buf
}

fn read4<T>(reader: &mut T) -> [u8; 4]
where
    T: Read,
{
    let mut buf = [0_u8; 4];
    let _nbytes = reader.read(&mut buf);
    buf
}



pub fn deal_wav(mut bufr: BufReader<&[u8]>)  -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut pcm = PCMStruct{
        channel_num:1,
        bits_per_sample:16,
        sample_rate:24000,
        data: Vec::new(),
    };
    let riff_tag = readn(&mut bufr,4)?;
    if String::from_utf8_lossy(&riff_tag) != "RIFF" {
        return Err("not a wav file".into());
    }
    let _total_size = readn(&mut bufr,4)?;
    let wave_tag = readn(&mut bufr,4)?;
    if String::from_utf8_lossy(&wave_tag) != "WAVE" {
        return Err("not a wav file".into());
    }
    let fmt_chunk_tag = readn(&mut bufr,4)?;
    if String::from_utf8_lossy(&fmt_chunk_tag) != "fmt " {
        return Err("not a wav file".into());
    }
    let _fmt_chunk_size = readn(&mut bufr,4)?;
    let fmt_code = read2(&mut bufr);
    let fmt_code_num = u16::from_le_bytes(fmt_code);
    if fmt_code_num != 1 {
        return Err(format!("not a support wav file,fmt_code={fmt_code_num}").into());
    }
    let num_channels = read2(&mut bufr);
    pcm.channel_num = u16::from_le_bytes(num_channels) as usize;
    let sampling_rate = read4(&mut bufr);
    pcm.sample_rate = u32::from_le_bytes(sampling_rate) as usize;
    let _byte_rate = readn(&mut bufr,4)?;
    let _block_alignment = readn(&mut bufr,2)?;
    let bits_per_sample = read2(&mut bufr);
    pcm.bits_per_sample = u16::from_le_bytes(bits_per_sample) as usize;
    let data_tag = readn(&mut bufr,4)?;
    let data_tag_str = String::from_utf8_lossy(&data_tag);
    let data;
    if data_tag_str == "data" {
        let data_size = read4(&mut bufr);
        data = readn(&mut bufr, u32::from_le_bytes(data_size))?;
    }else if data_tag_str == "LIST"{
        let cover_info_len = u32::from_le_bytes(read4(&mut bufr));
        readn(&mut bufr, cover_info_len)?;
        let data_tag = readn(&mut bufr,4)?;
        let data_tag_str = String::from_utf8_lossy(&data_tag);
        if data_tag_str != "data" {
            return Err("not a wav file".into());
        }
        let data_size = read4(&mut bufr);
        data = readn(&mut bufr, u32::from_le_bytes(data_size))?;
    }else {
        return Err("not a wav file".into());
    }
    // check
    let mod_num = pcm.bits_per_sample / 8 * pcm.channel_num;
    if mod_num == 0 {
        return Err("not a wav file".into());
    }
    if data.len() % (pcm.bits_per_sample / 8 * pcm.channel_num) != 0 {
        return Err("not a wav file".into());
    }
    // println!("pcm:{:#?}",pcm);
    let mut index:usize = 0;
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
            // println!("pcm.data[i]:{}",pcm.data[index]);
            let d = i16::from_le_bytes([data[index],data[index+1]]) as f64;
            pcm.data.push(d);
            index += 2;
        } else if per_sample_bytes == 3 {
            if index + 2 >= data.len() {
                return Err("not a wav file".into());
            }
            let d = i32::from_le_bytes([data[index],data[index+1],data[index+2],0]) as f64;
            pcm.data.push(d);
            index += 3;
        } else if per_sample_bytes == 4 {
            if index + 3 >= data.len() {
                return Err("not a wav file".into());
            }
            let d = f32::from_le_bytes([data[index],data[index+1],data[index+2],data[index+3]]) as f64;
            pcm.data.push(d);
            index += 4;
        }else {
            return Err("not a wav file".into());
        }
    }
    return Ok(pcm);
}
