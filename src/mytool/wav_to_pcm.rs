//! Fast WAVE PCM file format encoder and decoder.
//!
//! WAVE PCM is a library for fast encoding and decoding of WAV PCM format files.
//! As the name suggests, the library only supports the PCM version of WAVE format specification.
//! from https://github.com/oniani/wavepcm
//! LICENSE MIT

#![warn(clippy::all, clippy::pedantic, missing_docs)]

use std::convert::TryInto;
use std::io::{prelude::Read, BufReader};

// Read 2 bytes from a reader.
//
// # Arguments
//
// * `reader` - A reader.
fn read2<T>(reader: &mut T) -> [u8; 2]
where
    T: Read,
{
    let mut buf = [0_u8; 2];
    let _nbytes = reader.read(&mut buf);
    buf
}

// Read 4 bytes from a reader.
//
// # Arguments
//
// * `reader` - A reader.
fn read4<T>(reader: &mut T) -> [u8; 4]
where
    T: Read,
{
    let mut buf = [0_u8; 4];
    let _nbytes = reader.read(&mut buf);
    buf
}

// Read arbitrary number of bytes from a reader.
//
// # Arguments
//
// * `reader` - A reader.
//
// # Errors
//
// If the value cannot fit when performing type conversion.
fn readn<T>(reader: T, nbytes: u32) -> Result<Vec<u8>, anyhow::Error>
where
    T: Read,
{
    let mut buf = Vec::with_capacity(nbytes.try_into()?);
    let mut chunk = reader.take(u64::from(nbytes));
    let _val = chunk.read_to_end(&mut buf);
    Ok(buf)
}

/// WAVE PCM file format.
pub struct WavFormat {
    /// RIFF tag ("RIFF").
    pub riff_tag: [u8; 4],
    /// Total size of a file in bytes.
    pub total_size: [u8; 4],
    /// WAVE tag ("WAVE").
    pub wave_tag: [u8; 4],
    /// WavFormat tag ("fmt ").
    pub fmt_chunk_tag: [u8; 4],
    /// WavFormat chunk size (16 for PCM).
    pub fmt_chunk_size: [u8; 4],
    /// WavFormat type (1 for PCM - uncompressed).
    pub fmt_code: [u8; 2],
    /// Number of channels in the audio data.
    pub num_channels: [u8; 2],
    /// Sampling rate in the audio data (blocks per second).
    pub sampling_rate: [u8; 4],
    /// Byte rate (sampling_rate * num_channels * bits_per_sample / 8).
    pub byte_rate: [u8; 4],
    /// Block alignment value (num_channels * bits_per_sample / 8).
    pub block_alignment: [u8; 2],
    /// Bits per sample in the audio data (8 - 8 bits, 16 - 16 bits, etc).
    pub bits_per_sample: [u8; 2],
    /// Data tag ("data").
    pub data_tag: [u8; 4],
    /// Size of the audio data (num_samples * num_channels * bits_per_sample / 8).
    pub data_size: [u8; 4],
    /// Raw audio data.
    pub data: Vec<u8>,
}

impl WavFormat {

    /// `decode` decodes WAVE PCM file.
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the WAV PCM file.
    ///
    /// # Errors
    ///
    /// This function will return an error if `path` does not already exist.
    /// Other errors may also be returned according to `OpenOptions::open`.
    ///
    /// # Example
    ///
    /// ```
    /// use wavepcm::WavFormat;
    ///
    /// fn main() -> Result<(), anyhow::Error> {
    ///     let decoding = WavFormat::decode("sample.wav")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn decode(mut bufr: BufReader<&[u8]>) -> Result<Self, anyhow::Error> {

        let riff_tag = read4(&mut bufr);
        //println!("RIFF: {}", String::from_utf8_lossy(&riff_tag));
        let total_size = read4(&mut bufr);
        //println!("total_size: {}", u32::from_le_bytes(total_size));
        let wave_tag = read4(&mut bufr);
        //println!("wave_tag: {}", String::from_utf8_lossy(&wave_tag));
        let fmt_chunk_tag = read4(&mut bufr);
        //println!("fmt_chunk_tag: {}", String::from_utf8_lossy(&fmt_chunk_tag));
        let fmt_chunk_size = read4(&mut bufr);
        //println!("fmt_chunk_size: {}", u32::from_le_bytes(fmt_chunk_size));
        let fmt_code = read2(&mut bufr);
        //println!("fmt_code: {}", u16::from_le_bytes(fmt_code)); // 压缩方式
        let num_channels = read2(&mut bufr);
        //println!("num_channels: {}", u16::from_le_bytes(num_channels)); 
        let sampling_rate = read4(&mut bufr);
        //println!("sampling_rate: {}", u32::from_le_bytes(sampling_rate));
        let byte_rate = read4(&mut bufr);
        //println!("byte_rate: {}", u32::from_le_bytes(byte_rate));
        let block_alignment = read2(&mut bufr);
        //println!("block_alignment: {}", u16::from_le_bytes(block_alignment));
        let bits_per_sample = read2(&mut bufr);
        //println!("bits_per_sample: {}", u16::from_le_bytes(bits_per_sample));
        let mut data_tag = read4(&mut bufr);
        //println!("data_tag: {}", String::from_utf8_lossy(&data_tag));
        let data_size;
        let data;
        if String::from_utf8_lossy(&data_tag) == "data" {
            data_size = read4(&mut bufr);
            data = readn(&mut bufr, u32::from_le_bytes(data_size))?;
        }else{
            let cover_info_len = u32::from_le_bytes(read4(&mut bufr));
            readn(&mut bufr, cover_info_len)?;
            data_tag = read4(&mut bufr);
            data_size = read4(&mut bufr);
            data = readn(&mut bufr, u32::from_le_bytes(data_size))?;
        }
        

        Ok(WavFormat {
            riff_tag,
            total_size,
            wave_tag,
            fmt_chunk_tag,
            fmt_chunk_size,
            fmt_code,
            num_channels,
            sampling_rate,
            byte_rate,
            block_alignment,
            bits_per_sample,
            data_tag,
            data_size,
            data,
        })
    }
}

