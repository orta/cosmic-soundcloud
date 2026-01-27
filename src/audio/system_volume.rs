// SPDX-License-Identifier: MPL-2.0

//! System volume control using PulseAudio/PipeWire via pactl

use std::process::Command;

/// Get the current system volume as a value between 0.0 and 1.0
pub fn get_volume() -> Option<f32> {
    let output = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "Volume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB"
    // We need to extract the percentage
    for part in stdout.split('/') {
        let trimmed = part.trim();
        if trimmed.ends_with('%') {
            if let Ok(percent) = trimmed.trim_end_matches('%').trim().parse::<u32>() {
                return Some((percent as f32 / 100.0).clamp(0.0, 1.0));
            }
        }
    }

    None
}

/// Set the system volume to a value between 0.0 and 1.0
pub fn set_volume(volume: f32) -> bool {
    let percent = (volume.clamp(0.0, 1.0) * 100.0).round() as u32;
    let volume_str = format!("{percent}%");

    Command::new("pactl")
        .args(["set-sink-volume", "@DEFAULT_SINK@", &volume_str])
        .status()
        .is_ok_and(|s| s.success())
}

