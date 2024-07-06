use std::io::BufReader;

use super::all_to_silk::PCMStruct;

pub fn deal_flac(bufr: BufReader<&[u8]>)  -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut pcm = PCMStruct{
        channel_num:1,
        bits_per_sample:32,
        sample_rate:24000,
        data: Vec::new(),
    };
    
    let mut reader = claxon::FlacReader::new(bufr)?;
    pcm.channel_num = reader.streaminfo().channels as usize;
    pcm.sample_rate = reader.streaminfo().sample_rate as usize;
    pcm.bits_per_sample = reader.streaminfo().bits_per_sample as usize;
    for sample_t in reader.samples() {
        let sample = sample_t?;
        pcm.data.push(sample as f64);
    }
    return Ok(pcm);
}