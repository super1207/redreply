use super::all_to_silk::PCMStruct;

// 线性插值
pub fn linear_resample(in_pcm: &Vec<f64>, out_pcm: &mut Vec<f64>) {
    let in_sample_count: usize = in_pcm.len() as usize;
    let out_sample_count: usize = out_pcm.len() as usize;
    for i in 0..out_sample_count {
        let pos_inpcm = i as f64 / out_sample_count as f64 * in_sample_count as f64;
        let pos_low = pos_inpcm as usize;
        let pos_high = (pos_inpcm + 1.0) as usize;
        let q_high = pos_inpcm - pos_low as f64;
        let q_low = pos_high as f64 - pos_inpcm;
        if pos_high >= in_sample_count || pos_low >= in_sample_count{
            out_pcm[i] = in_pcm[in_sample_count - 1];
        }else {
            out_pcm[i] = in_pcm[pos_high] * q_high+in_pcm[pos_low] * q_low;
        }
        
    }
}

pub fn to_qq_silk(pcm: &PCMStruct) -> Vec<u8> {

    // 腾讯极有可能只支持24000
    let out_sample_rate = 24000;

    // 分解通道
    let mut datas = vec![];
    for _i in 0..pcm.channel_num {
        datas.push(vec![]);
    }
    if pcm.data.len() % pcm.channel_num != 0 { // 防止下面数组越界
        return vec![];
    }
    let mut index = 0usize;
    loop {
        if index + pcm.channel_num > pcm.data.len() {
            break;
        }
        for i in 0..pcm.channel_num {
            let mut dat = pcm.data[index + i];
            // 采样深度缩放为16位（此处可以决定音量），silk只支持16位
            dat *= f64::powi(2.0,  16 - pcm.bits_per_sample as i32); 
            datas[i].push(dat);
        }
        index += pcm.channel_num;
    }

    // 转为单通道(自适应混音加权)
    let coloum = datas[0].len(); // 最终合成的长度
    let mut f = 1.0; // 衰减因子
    let mut single_channel_data = vec![];
    let max = 32767.0;
    let min = -32768.0;
    let mut mix_val;
    for i in 0..coloum {
        let mut sum = 0.0;
        for it in 0..pcm.channel_num {
            sum += datas[it][i];
        }
        mix_val = sum * f;
        if mix_val > max {
            f = max / mix_val;
            mix_val = max;
        }
        if mix_val < min {
            f = min / mix_val;
            mix_val = min;
        }
        if f < 1.0 {
            f += (1.0 - f) / 16.0;
        }
        single_channel_data.push(mix_val);
    }

    // 采样率转换24000（使用线性插值）
    let new_24000_data_len = ((single_channel_data.len() as f64 * pcm.channel_num as f64/ pcm.sample_rate as f64) * out_sample_rate as f64).round() as usize;
    let mut new_24000_data = vec![0f64; new_24000_data_len];
    linear_resample(&single_channel_data, &mut new_24000_data);

    // 转为单通道i16
    let mut u16_data = vec![];
    let mut index = 0usize;
    while index < new_24000_data_len{
        let dat_avg = new_24000_data[index];
        let dat_avg_u16 = dat_avg as i16;
        let bits = dat_avg_u16.to_le_bytes();
        u16_data.push(bits[0]);
        u16_data.push(bits[1]);
        index += pcm.channel_num;
    }
    // bit_rate也最好是24000，不然可能在NTQQ上无法播放
    if let Ok(out) = silk_rs::encode_silk(u16_data, out_sample_rate, out_sample_rate, true){
        return out;
    }else{
        return vec![];
    }
}