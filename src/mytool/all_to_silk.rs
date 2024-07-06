use std::io::BufReader;

use super::{deal_flac, deal_silk, mp3_deal, wav_deal};


#[derive(Debug)]
pub struct PCMStruct{
    pub channel_num:usize, // 通道数目
    pub bits_per_sample:usize, // 采样bit节大小
    pub sample_rate:usize, // 采样率
    pub data: Vec<f64>,
}

pub fn get_media_type(input:&Vec<u8>) -> &str{
    if input.starts_with(&[82,73,70,70]) {
        return "wav";
    }else if input.starts_with(&[73,68,51]) || input.starts_with(&[0xFF]){
        return "mp3";
    }else if input.starts_with(&[0x66,0x41,0x61,0x43]) {
        return "flac";
    }else if input.starts_with("#!SILK".as_bytes()){
        return "silk";
    }else{
        return "";
    }
}



pub fn all_to_silk(input:&Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error>>{
    let tp = get_media_type(input);
    let pcm;
    if tp == "wav"{
        pcm = wav_deal::deal_wav(BufReader::new(&input[..]))?;
    }else if tp == "mp3" {
        pcm = mp3_deal::deal_mp3(BufReader::new(&input[..]))?;
    }else if tp == "flac" {
        pcm = deal_flac::deal_flac(BufReader::new(&input[..]))?;
    }else if tp == "silk" {
        return Ok(input.to_owned());
    }else {
        return Err("not support".into());
    }
    let silk = deal_silk::to_qq_silk(&pcm);
    return Ok(silk);
}


