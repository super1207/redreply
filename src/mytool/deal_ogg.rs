use std::io::{BufReader, Read};

use super::all_to_silk::PCMStruct;

pub fn deal_ogg(mut bufr: BufReader<&[u8]>)  -> Result<PCMStruct, Box<dyn std::error::Error>> {
    let mut pcm = PCMStruct{
        channel_num:2,
        bits_per_sample:16,
        sample_rate:24000,
        data: Vec::new(),
    };
    let mut buf = vec![];
    bufr.read_to_end(&mut buf)?;
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(buf))?;
    pcm.channel_num = reader.ident_hdr.audio_channels as usize;
    pcm.sample_rate = reader.ident_hdr.audio_sample_rate as usize;
    while let Some(pck_samples) = reader.read_dec_packet_itl()? {
        for it in pck_samples {
            pcm.data.push(it as f64);
        }
    }
    return Ok(pcm);
}