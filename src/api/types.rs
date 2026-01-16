// SPDX-License-Identifier: MPL-2.0

use serde::{Deserialize, Serialize};

/// SoundCloud user profile
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub followers_count: u32,
    #[serde(default)]
    pub followings_count: u32,
    #[serde(default)]
    pub track_count: u32,
    #[serde(default)]
    pub playlist_count: u32,
    pub permalink_url: Option<String>,
}

/// Simplified user info embedded in tracks
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TrackUser {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub username: String,
    pub avatar_url: Option<String>,
}

/// Audio transcoding format
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscodingFormat {
    pub protocol: String,
    pub mime_type: String,
}

/// Audio transcoding option
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Transcoding {
    pub url: String,
    pub format: TranscodingFormat,
    #[serde(default)]
    pub quality: Option<String>,
}

/// Media container with transcoding options
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Media {
    pub transcodings: Vec<Transcoding>,
}

/// SoundCloud track
/// Note: Playlists may return "stub" tracks with only id - use is_complete() to check
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Track {
    pub id: u64,
    /// Title may be missing for stub tracks in playlists
    #[serde(default)]
    pub title: String,
    /// User may be missing for stub tracks
    #[serde(default)]
    pub user: TrackUser,
    pub artwork_url: Option<String>,
    /// Duration in milliseconds
    #[serde(default)]
    pub duration: u64,
    pub media: Option<Media>,
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub playback_count: u64,
    #[serde(default)]
    pub likes_count: u64,
    /// JWT token for authorizing stream access
    pub track_authorization: Option<String>,
}

impl Track {
    /// Format duration as MM:SS
    pub fn duration_formatted(&self) -> String {
        let total_secs = self.duration / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}:{secs:02}")
    }

    /// Check if this is a complete track (not a stub from playlist response)
    /// Stub tracks only have id and need to be fetched separately
    pub fn is_complete(&self) -> bool {
        !self.title.is_empty() && !self.user.username.is_empty()
    }

    /// Find progressive (direct) stream transcoding, preferring MP3 over MP4
    pub fn progressive_transcoding(&self) -> Option<&Transcoding> {
        let transcodings = &self.media.as_ref()?.transcodings;
        // Prefer MP3 (audio/mpeg) as it's simpler to decode
        transcodings
            .iter()
            .find(|t| t.format.protocol == "progressive" && t.format.mime_type.contains("mpeg"))
            .or_else(|| {
                transcodings
                    .iter()
                    .find(|t| t.format.protocol == "progressive")
            })
    }

    /// Find HLS stream transcoding (plain, non-encrypted), preferring MP3 over MP4/fMP4
    pub fn hls_transcoding(&self) -> Option<&Transcoding> {
        let transcodings = &self.media.as_ref()?.transcodings;
        // Prefer MP3 (audio/mpeg) as fMP4 (audio/mp4) has decoding issues
        transcodings
            .iter()
            .find(|t| t.format.protocol == "hls" && t.format.mime_type.contains("mpeg"))
            .or_else(|| {
                transcodings
                    .iter()
                    .find(|t| t.format.protocol == "hls" && !t.url.contains("encrypted"))
            })
    }

    /// Find encrypted HLS stream (ctr-encrypted-hls or cbc-encrypted-hls)
    /// These are the only working streams as of 2026
    pub fn encrypted_hls_transcoding(&self) -> Option<&Transcoding> {
        self.media
            .as_ref()?
            .transcodings
            .iter()
            .find(|t| t.url.contains("ctr-encrypted-hls") || t.url.contains("cbc-encrypted-hls"))
    }

    /// Get best available transcoding
    /// Prefers: hls (pre-buffered) > progressive > encrypted hls
    pub fn best_transcoding(&self) -> Option<&Transcoding> {
        // Try plain HLS first (downloads segments then plays from memory - most reliable)
        if let Some(t) = self.hls_transcoding() {
            return Some(t);
        }
        // Then progressive (direct streaming - can have buffering issues)
        if let Some(t) = self.progressive_transcoding() {
            return Some(t);
        }
        // Fall back to encrypted HLS (requires yt-dlp fallback)
        self.encrypted_hls_transcoding()
    }
}

/// A liked track item from the API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LikeItem {
    pub track: Track,
    pub created_at: String,
}

/// Paginated response for likes
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LikesResponse {
    pub collection: Vec<LikeItem>,
    pub next_href: Option<String>,
}

/// Paginated response for tracks (e.g., history)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TracksResponse {
    pub collection: Vec<Track>,
    pub next_href: Option<String>,
}

/// Playlist/album response with embedded tracks
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaylistWithTracks {
    pub id: u64,
    pub title: String,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub track_count: u32,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

/// SoundCloud album (a type of playlist)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Album {
    pub id: u64,
    pub title: String,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub track_count: u32,
    pub release_date: Option<String>,
    pub user: TrackUser,
    pub permalink_url: Option<String>,
}

/// Paginated response for albums
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlbumsResponse {
    pub collection: Vec<Album>,
    pub next_href: Option<String>,
}

/// Stream URL response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamUrlResponse {
    pub url: String,
}
