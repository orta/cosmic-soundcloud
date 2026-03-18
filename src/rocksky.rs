// SPDX-License-Identifier: MPL-2.0

//! Rocksky scrobbling integration.
//!
//! Rocksky is a Last.fm-compatible scrobbling service.
//! Docs: https://docs.rocksky.app/migrating-to-rocksky-scrobble-api-957839m0

use reqwest::Client;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const BASE_URL: &str = "https://audioscrobbler.rocksky.app/2.0";

// === Minimal MD5 implementation ===
// Per RFC 1321. No external crate needed.

#[allow(clippy::many_single_char_names, clippy::unreadable_literal)]
fn md5(input: &[u8]) -> [u8; 16] {
    // Per-round shift amounts
    const S: [u32; 64] = [
        7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,
        5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,
        4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,
        6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,
    ];
    // Precomputed table: floor(2^32 * |sin(i+1)|)
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: pad message
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // Process each 512-bit (64-byte) chunk
    for chunk in msg.chunks(64) {
        // Break chunk into sixteen 32-bit words
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            let b = i * 4;
            *w = u32::from_le_bytes([chunk[b], chunk[b+1], chunk[b+2], chunk[b+3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0u32..64 {
            let (f, g) = match i {
                0..=15  => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _       => (c ^ (b | !d), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(K[i as usize]).wrapping_add(m[g as usize]))
                    .rotate_left(S[i as usize])
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());
    digest
}

fn md5_hex(input: &[u8]) -> String {
    let digest = md5(input);
    digest.iter().fold(String::with_capacity(32), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

// === Rocksky API ===

/// Compute the API signature.
/// Sort all params by key, concatenate key+value pairs, append shared secret, MD5 hash.
fn compute_signature(params: &BTreeMap<&str, String>, shared_secret: &str) -> String {
    let mut sig_str = String::new();
    for (key, value) in params {
        sig_str.push_str(key);
        sig_str.push_str(value);
    }
    sig_str.push_str(shared_secret);
    md5_hex(sig_str.as_bytes())
}

/// Scrobble a track to Rocksky.
pub async fn scrobble(
    client: &Client,
    api_key: &str,
    shared_secret: &str,
    session_key: &str,
    artist: &str,
    track: &str,
    timestamp: u64,
) -> Result<(), String> {
    let mut params: BTreeMap<&str, String> = BTreeMap::new();
    params.insert("api_key", api_key.to_string());
    params.insert("artist[0]", artist.to_string());
    params.insert("method", "track.scrobble".to_string());
    params.insert("sk", session_key.to_string());
    params.insert("timestamp[0]", timestamp.to_string());
    params.insert("track[0]", track.to_string());

    let sig = compute_signature(&params, shared_secret);

    let mut form_params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    form_params.push(("api_sig".to_string(), sig));
    form_params.push(("format".to_string(), "json".to_string()));

    let response = client
        .post(BASE_URL)
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("Rocksky request failed: {e}"))?;

    if response.status().is_success() {
        eprintln!("[rocksky] Scrobbled: {artist} - {track}");
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Rocksky error {status}: {body}"))
    }
}

/// Get current Unix timestamp in seconds.
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
