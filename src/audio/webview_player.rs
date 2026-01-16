// SPDX-License-Identifier: MPL-2.0

//! Browser fallback for DRM-protected content
//!
//! SoundCloud uses Microsoft PlayReady DRM which requires a CDM (Content Decryption Module)
//! that's only available in commercial browsers like Chrome, Edge, and Firefox.
//! When DRM content is detected, we offer to open the track in the user's default browser.

/// Open a SoundCloud track URL in the default browser
pub fn open_in_browser(track_url: &str) -> Result<(), String> {
    open::that(track_url).map_err(|e| format!("Failed to open browser: {e}"))
}
