use std::io::BufReader;

use minimp3_fixed::{Decoder, Frame};

use super::all_to_silk::PCMStruct;


pub fn deal_mp3(bufr: BufReader<&[u8]>)  -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut pcm = PCMStruct{
        channel_num:1,
        bits_per_sample:16,
        sample_rate:24000,
        data: Vec::new(),
    };
    
    let mut decoder = Decoder::new(bufr);
    
    loop {
        match decoder.next_frame() {
            Ok(Frame { data, sample_rate, channels, .. }) => {
                pcm.sample_rate = sample_rate as usize;
                pcm.channel_num = channels;
                for it in data {
                    pcm.data.push(it as f64);
                }
            },
            Err(minimp3_fixed::Error::Eof) => break,
            Err(e) => {
                return Err(Box::new(e));
            },
        }
    }
    
    return Ok(pcm);
}