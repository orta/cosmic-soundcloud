// SPDX-License-Identifier: MPL-2.0

//! HLS streaming support for SoundCloud's encrypted streams

use m3u8_rs::{MediaPlaylist, Playlist};
use reqwest::Client;

/// HLS stream information
#[derive(Debug, Clone)]
pub struct HlsStream {
    pub segments: Vec<HlsSegment>,
    pub target_duration: u64,
    pub encryption: Option<HlsEncryption>,
    /// fMP4 init segment URL (from #EXT-X-MAP)
    pub init_segment_url: Option<String>,
}

/// A single HLS segment
#[derive(Debug, Clone)]
pub struct HlsSegment {
    pub uri: String,
    pub duration: f32,
    pub byte_range: Option<(u64, u64)>,
}

/// HLS encryption information
#[derive(Debug, Clone)]
pub struct HlsEncryption {
    pub method: String,
    pub uri: Option<String>,
    pub iv: Option<String>,
    pub keyformat: Option<String>,
}

/// Fetch and parse an HLS m3u8 playlist
pub async fn fetch_playlist(client: &Client, url: &str) -> Result<HlsStream, String> {
    // Fetch the playlist
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch playlist: {e}"))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read playlist: {e}"))?;

    // Parse the playlist
    let parsed = m3u8_rs::parse_playlist(&bytes);

    match parsed {
        Ok((_, Playlist::MediaPlaylist(playlist))) => {
            Ok(parse_media_playlist(&playlist, url))
        }
        Ok((_, Playlist::MasterPlaylist(_))) => {
            Err("Master playlists not yet supported".into())
        }
        Err(e) => Err(format!("Failed to parse playlist: {e:?}")),
    }
}

fn parse_media_playlist(playlist: &MediaPlaylist, base_url: &str) -> HlsStream {
    // Extract base URL for relative segment paths
    let base = base_url.rsplit_once('/').map(|(b, _)| b).unwrap_or(base_url);

    // Get encryption info from the first key tag if present
    let encryption = playlist.segments.iter()
        .find_map(|s| s.key.as_ref())
        .map(|key| HlsEncryption {
            method: format!("{:?}", key.method),
            uri: key.uri.clone(),
            iv: key.iv.clone(),
            keyformat: key.keyformat.clone(),
        });

    // Get init segment URL from EXT-X-MAP (for fMP4 streams)
    let init_segment_url = playlist.segments.iter()
        .find_map(|seg| seg.map.as_ref())
        .map(|map| {
            if map.uri.starts_with("http") {
                map.uri.clone()
            } else {
                format!("{}/{}", base, map.uri)
            }
        });

    // Parse segments
    let segments = playlist.segments.iter()
        .filter_map(|seg| {
            let uri = if seg.uri.starts_with("http") {
                seg.uri.clone()
            } else {
                format!("{}/{}", base, seg.uri)
            };

            Some(HlsSegment {
                uri,
                duration: seg.duration,
                byte_range: seg.byte_range.as_ref().map(|br| {
                    (br.length, br.offset.unwrap_or(0))
                }),
            })
        })
        .collect();

    HlsStream {
        segments,
        target_duration: playlist.target_duration,
        encryption,
        init_segment_url,
    }
}

/// Download a segment
pub async fn download_segment(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch segment: {e}"))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read segment: {e}"))?;

    Ok(bytes.to_vec())
}
