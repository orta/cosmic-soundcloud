// SPDX-License-Identifier: MPL-2.0

//! yt-dlp integration for extracting playable stream URLs
//!
//! SoundCloud provides unencrypted streams to certain clients, which yt-dlp can extract.
//! This bypasses the DRM-encrypted streams that the official API returns.

use std::process::Command;

/// Extract a playable audio URL using yt-dlp
pub fn extract_stream_url(track_url: &str) -> Result<String, String> {
    // Check if yt-dlp is available
    let output = Command::new("yt-dlp")
        .args([
            "-f", "bestaudio",
            "--get-url",
            "--no-warnings",
            track_url,
        ])
        .output()
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp failed: {stderr}"));
    }

    let url = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    if url.is_empty() {
        return Err("yt-dlp returned empty URL".into());
    }

    Ok(url)
}

/// Check if yt-dlp is available on the system
pub fn is_available() -> bool {
    Command::new("yt-dlp")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
