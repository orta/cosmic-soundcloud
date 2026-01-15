// SPDX-License-Identifier: MPL-2.0

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// Repeat mode for audio playback
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    None,
    One,
    All,
}

/// A recently viewed artist for quick navigation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentArtist {
    pub id: u64,
    pub username: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, CosmicConfigEntry, PartialEq)]
#[version = 2]
pub struct Config {
    /// OAuth token for SoundCloud API authentication
    /// DEPRECATED: Token is now stored in system keyring for security.
    /// This field is kept for migration from older versions.
    pub oauth_token: Option<String>,
    /// Volume level (0.0 - 1.0)
    pub volume: f32,
    /// Shuffle mode enabled
    pub shuffle: bool,
    /// Repeat mode
    pub repeat_mode: RepeatMode,
    /// Recently viewed artists (max 10)
    pub recent_artists: Vec<RecentArtist>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oauth_token: None,
            volume: 0.8,
            shuffle: false,
            repeat_mode: RepeatMode::None,
            recent_artists: Vec::new(),
        }
    }
}

