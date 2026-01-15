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

#[derive(Debug, Clone, CosmicConfigEntry, PartialEq)]
#[version = 2]
pub struct Config {
    /// OAuth token for SoundCloud API authentication
    pub oauth_token: Option<String>,
    /// Volume level (0.0 - 1.0)
    pub volume: f32,
    /// Shuffle mode enabled
    pub shuffle: bool,
    /// Repeat mode
    pub repeat_mode: RepeatMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oauth_token: None,
            volume: 0.8,
            shuffle: false,
            repeat_mode: RepeatMode::None,
        }
    }
}

impl Config {
    /// Check if we have a valid OAuth token
    pub fn has_token(&self) -> bool {
        self.oauth_token
            .as_ref()
            .map_or(false, |t| !t.trim().is_empty())
    }
}
