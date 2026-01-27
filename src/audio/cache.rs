// SPDX-License-Identifier: MPL-2.0

//! Disk-based audio cache for preloaded tracks.
//!
//! Stores downloaded audio data in `~/.cache/cosmic-soundcloud/audio/`
//! using the track ID as the filename. This allows preloaded next-track
//! data to persist briefly without consuming application memory.

use std::path::PathBuf;

/// Return the audio cache directory (`~/.cache/cosmic-soundcloud/audio/`).
fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("cosmic-soundcloud").join("audio"))
}

/// Return the cache file path for a given track ID.
fn cache_path(track_id: u64) -> Option<PathBuf> {
    cache_dir().map(|d| d.join(format!("{track_id}.audio")))
}

/// Check whether audio data is cached for the given track.
pub fn has_cached(track_id: u64) -> bool {
    cache_path(track_id).is_some_and(|p| p.exists())
}

/// Read cached audio data for a track. Returns `None` if not cached.
pub fn read_cached(track_id: u64) -> Option<Vec<u8>> {
    let path = cache_path(track_id)?;
    std::fs::read(path).ok()
}

/// Write audio data to the cache for a track.
pub fn write_cached(track_id: u64, data: &[u8]) -> Result<(), String> {
    let dir = cache_dir().ok_or("No cache directory available")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create cache dir: {e}"))?;
    let path = dir.join(format!("{track_id}.audio"));
    std::fs::write(path, data).map_err(|e| format!("Failed to write cache file: {e}"))
}

/// Remove a single track from the cache.
pub fn remove_cached(track_id: u64) {
    if let Some(path) = cache_path(track_id) {
        let _ = std::fs::remove_file(path);
    }
}

/// Remove all cached audio files.
pub fn clear_cache() {
    if let Some(dir) = cache_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}
