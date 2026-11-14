// SPDX-License-Identifier: LGPL-3.0-only
//! Sec-MS-GEC v2 protocol token, adapted from edge-tts 7.3.1's drm.py.
//!
//! Upstream: rany and the edge-tts contributors, https://github.com/rany2/edge-tts
//! This compact Rust port retains the protocol constants and uses ordinary
//! SHA-256 round loops instead of the Python implementation's unrolled loops.
//! See ../THIRD_PARTY_NOTICES.md and ../licenses/edge-tts-LICENSE.txt.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::TRUSTED_CLIENT_TOKEN;

const INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];
// State after the fixed protocol prefix's first fourteen SHA-256 rounds.
const PREFIX_STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4, 0x03EFFBD2,
];
const PREFIX_SCHEDULE: [u32; 14] = [
    0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA, 0xA622EE8C, 0xD1DD9491, 0xC0DD25A0, 0xF9E70AB6,
    0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33, 0xD258B060, 0x72766564,
];
// FIPS 180-4 SHA-256 round constants.
const K: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];
const PREFIX_ROUND_14: u32 = 0xC9DC06A7;
const CS64_COEFFICIENTS: [(u32, u32); 14] = [
    (0xE82E91B1, 0xB32700D0),
    (0x44DE24C6, 0x88DFF51D),
    (0x3BEB65BC, 0x29514038),
    (0xC89C5933, 0xB328CBDF),
    (0x8D1EC211, 0xD405B4D5),
    (0x314EF5D5, 0x26E55F3C),
    (0xE92491B1, 0x607801C6),
    (0xF0A5B9D5, 0x82B2C1FB),
    (0x06064CF3, 0xFA51A3DA),
    (0xDA79FA60, 0x207CC464),
    (0xBED20560, 0x3C3C74F3),
    (0x89F9C9F8, 0xD2D14B61),
    (0x35FA38C6, 0xBB04A2C1),
    (0xCB49E3C6, 0xC5626167),
];

fn sigma0(value: u32) -> u32 {
    value.rotate_right(7) ^ value.rotate_right(18) ^ (value >> 3)
}

fn sigma1(value: u32) -> u32 {
    value.rotate_right(17) ^ value.rotate_right(19) ^ (value >> 10)
}

fn expand(words: &mut [u32; 64], first: usize) {
    for index in first..64 {
        words[index] = sigma1(words[index - 2])
            .wrapping_add(words[index - 7])
            .wrapping_add(sigma0(words[index - 15]))
            .wrapping_add(words[index - 16]);
    }
}

fn rounds(state: [u32; 8], words: &[u32; 64], first: usize) -> [u32; 8] {
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
    for index in first..64 {
        let constant = if first == 14 && index == 14 {
            PREFIX_ROUND_14
        } else {
            K[index]
        };
        let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let choice = (e & f) ^ (!e & g);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t1 = h
            .wrapping_add(sum1)
            .wrapping_add(choice)
            .wrapping_add(constant)
            .wrapping_add(words[index]);
        let t2 = sum0.wrapping_add(majority);
        [a, b, c, d, e, f, g, h] = [t1.wrapping_add(t2), a, b, c, d.wrapping_add(t1), e, f, g];
    }
    [a, b, c, d, e, f, g, h]
}

fn first_block(data: &[u8]) -> [u32; 8] {
    let mut words = [0_u32; 64];
    words[14] = u32::from_be_bytes([0, data[0], data[1], data[2]]);
    words[15] = u32::from_be_bytes([data[3], data[4], data[5], data[6]]);
    for index in 16..30 {
        let mut value = PREFIX_SCHEDULE[index - 16];
        if index >= 16 {
            let adjustment = if index == 16 { 0x000B92C0 } else { 0 };
            value = value.wrapping_add(sigma1(words[index - 2]) ^ adjustment);
        }
        if index >= 21 {
            value = value.wrapping_add(words[index - 7]);
        }
        if index >= 29 {
            let adjustment = if index == 29 { 0x059C0B80 } else { 0 };
            value = value.wrapping_add(sigma0(words[index - 15]) ^ adjustment);
        }
        words[index] = value;
    }
    words[30] = sigma1(words[28])
        .wrapping_add(words[23])
        .wrapping_add(sigma0(words[15]))
        .wrapping_add(words[14])
        .wrapping_add(PREFIX_ROUND_14.wrapping_sub(K[14]));
    expand(&mut words, 31);
    let working = rounds(PREFIX_STATE, &words, 14);
    std::array::from_fn(|index| INITIAL[index].wrapping_add(working[index]))
}

fn protocol_sha256(data: &[u8]) -> [u8; 32] {
    // The protocol has a fixed 57-byte prefix; first_block consumes its block
    // together with the first seven data bytes. Token data always exceeds 7.
    let mut state = first_block(data);
    let bit_length = (57 + data.len()) as u64 * 8;
    let mut tail = data[7..].to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_length.to_be_bytes());
    for block in tail.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        expand(&mut words, 16);
        let working = rounds(state, &words, 0);
        state = std::array::from_fn(|index| state[index].wrapping_add(working[index]));
    }
    let mut digest = [0_u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn cs64(data: &[u8], digest: &[u8; 32]) -> [u32; 2] {
    let key_a = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1;
    let key_b = u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1;
    let factors = [
        key_a.wrapping_mul(0x82893933),
        key_b.wrapping_mul(0x82893933),
    ];
    let mut terms = Vec::new();
    let mut offsets = Vec::new();
    for (index, (coefficient, offset)) in CS64_COEFFICIENTS.iter().enumerate() {
        terms.push(coefficient.wrapping_mul(factors[index % 2]));
        offsets.push(*offset);
    }
    let mut padded = vec![0];
    padded.extend_from_slice(data);
    while !padded.len().is_multiple_of(8) {
        padded.push(0);
    }
    for (index, bytes) in padded.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let key = if index % 2 == 0 { key_a } else { key_b };
        let coefficient = if index % 2 == 0 {
            0xEF0569FB
        } else {
            0xC31713DB
        };
        let mut term = word.wrapping_mul(key);
        let mut offset = word.rotate_left(16).wrapping_mul(coefficient);
        if index == 0 {
            term = term.wrapping_add(0xF2F90B1A_u32.wrapping_mul(factors[0]));
            offset = offset.wrapping_add(0x0B1A0000);
        }
        terms.push(term);
        offsets.push(offset);
    }
    let (mut swapped_sum, mut swapped_last, mut state, mut sum) = (0_u32, 0_u32, 0_u32, 0_u32);
    for index in (0..terms.len()).step_by(2) {
        let (x, y) = (terms[index], terms[index + 1]);
        let q = offsets[index]
            .wrapping_add(x)
            .rotate_left(16)
            .wrapping_mul(0x76F50F34);
        swapped_sum =
            swapped_sum.wrapping_add(q.rotate_left(16).wrapping_mul(0xC3EFEA97).wrapping_add(q));
        let q = offsets[index + 1]
            .wrapping_add(y)
            .rotate_left(16)
            .wrapping_mul(0x1881CE3C);
        swapped_last = q.rotate_left(16).wrapping_mul(0x35BD1EC9).wrapping_add(q);
        swapped_sum = swapped_sum.wrapping_add(swapped_last);
        state = state.wrapping_mul(key_a).wrapping_add(x);
        for coefficient in [0xCF98B111, 0x87085B9F, 0x12CEB96D, 0x257E1D83] {
            state = state.rotate_left(16).wrapping_mul(coefficient);
        }
        sum = sum.wrapping_add(state);
        let mut next = state.wrapping_mul(key_b).wrapping_add(y);
        for coefficient in [0xA27416F5, 0xD38396FF, 0x7C932B89, 0xBFA49F69] {
            next = next.rotate_left(16).wrapping_mul(coefficient);
        }
        state = next;
        sum = sum.wrapping_add(state);
    }
    [swapped_last ^ state, swapped_sum ^ sum]
}

fn append_hex(target: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        target.push(HEX[(byte >> 4) as usize] as char);
        target.push(HEX[(byte & 15) as usize] as char);
    }
}

pub(crate) fn sec_ms_gec() -> String {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    sec_ms_gec_for_unix(unix_seconds)
}

pub(crate) fn sec_ms_gec_for_unix(unix_seconds: u64) -> String {
    // Integer arithmetic avoids platform-specific floating point rounding.
    let ticks = ((u128::from(unix_seconds) + 11_644_473_600) / 300) * 3_000_000_000;
    let data = format!("{ticks}{TRUSTED_CLIENT_TOKEN}");
    let digest = protocol_sha256(data.as_bytes());
    let checksum = cs64(data.as_bytes(), &digest);
    let mut token = String::with_capacity(81);
    for word in checksum {
        append_hex(&mut token, &word.to_le_bytes());
    }
    token.push('-');
    append_hex(&mut token, &digest);
    token
}

#[cfg(test)]
mod tests {
    use super::sec_ms_gec_for_unix;

    #[test]
    fn agrees_with_edge_tts_7_3_1_reference_vectors() {
        // Generated by edge_tts.drm.generate_sec_ms_gec_v2(fixed_unix_time).
        let vectors = [
            (
                0,
                "52BC7A17B50DFEC9-28C4394775C8751CD2F6859C030C06DB1489ACAB3B3A7651831FC26895177ED6",
            ),
            (
                300,
                "F7535A20A262E331-28675BF85C7106BBF20035A78FCB0B6890813EAD5C3B983EF3424931B720BABB",
            ),
            (
                1_700_000_000,
                "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2",
            ),
            (
                1_794_584_400,
                "E8CF48003EDEEBFA-393C5F57327927C787F0E018C47608FAE541C794747947DAFDDC2C3DFC4E7BC2",
            ),
            (
                2_147_483_647,
                "96AE288915A09C99-EB52B45DB68387A1383783E40D42BBBD69D4DCCDF2540629BD4E7FB2E3287822",
            ),
        ];
        for (timestamp, expected) in vectors {
            assert_eq!(sec_ms_gec_for_unix(timestamp), expected, "unix={timestamp}");
        }
    }

    #[test]
    fn five_minute_windows_have_stable_boundaries() {
        assert_eq!(sec_ms_gec_for_unix(0), sec_ms_gec_for_unix(299));
        assert_ne!(sec_ms_gec_for_unix(299), sec_ms_gec_for_unix(300));
        assert_eq!(sec_ms_gec_for_unix(300), sec_ms_gec_for_unix(599));
    }
}
