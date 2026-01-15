// SPDX-License-Identifier: MPL-2.0

use super::{hls, ytdlp};
use reqwest::Client;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::Cursor;
use stream_download::storage::temp::TempStorageProvider;
use stream_download::{Settings, StreamDownload};
use tokio::sync::mpsc;

/// Commands sent to the audio player thread
#[derive(Debug, Clone)]
pub enum AudioCommand {
    /// Play audio from a stream URL, with optional permalink URL for browser fallback
    Play { stream_url: String, permalink_url: Option<String> },
    /// Pause playback
    Pause,
    /// Resume playback
    Resume,
    /// Stop playback completely
    Stop,
    /// Set volume (0.0 to 1.0)
    SetVolume(f32),
}

/// Events emitted by the audio player
#[derive(Debug, Clone)]
pub enum AudioEvent {
    /// Player is ready
    Ready,
    /// Started playing
    Playing,
    /// Playback paused
    Paused,
    /// Playback stopped
    Stopped,
    /// Track finished playing
    Finished,
    /// Buffering state changed
    Buffering(bool),
    /// Error occurred
    Error(String),
    /// DRM-protected content detected - includes track URL for browser fallback
    DrmProtected { drm_type: String, track_url: String },
}

/// Audio player that runs in a background thread
pub struct AudioPlayer {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Option<Sink>,
    volume: f32,
    event_tx: mpsc::Sender<AudioEvent>,
    http_client: Client,
}

impl AudioPlayer {
    /// Spawn the audio player in a background thread
    /// Returns channels for sending commands and receiving events
    pub fn spawn() -> (mpsc::Sender<AudioCommand>, mpsc::Receiver<AudioEvent>) {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<AudioCommand>(32);
        let (evt_tx, evt_rx) = mpsc::channel::<AudioEvent>(32);

        std::thread::spawn(move || {
            // Set up panic handler for this thread
            let evt_tx_panic = evt_tx.clone();
            let orig_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                eprintln!("Audio thread panic: {info}");
                let _ = evt_tx_panic.blocking_send(AudioEvent::Error(
                    format!("Audio thread panic: {info}")
                ));
                orig_hook(info);
            }));

            // Create the audio output stream - must be kept alive
            let (stream, stream_handle) = match OutputStream::try_default() {
                Ok(s) => s,
                Err(e) => {
                    let _ = evt_tx.blocking_send(AudioEvent::Error(format!(
                        "Failed to create audio output: {e}"
                    )));
                    return;
                }
            };

            let mut player = AudioPlayer {
                _stream: stream,
                stream_handle,
                sink: None,
                volume: 0.8,
                event_tx: evt_tx.clone(),
                http_client: Client::new(),
            };

            // Signal ready
            let _ = evt_tx.blocking_send(AudioEvent::Ready);

            // Create a tokio runtime for this thread
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            // Process commands and monitor playback completion
            rt.block_on(async {
                let mut check_interval = tokio::time::interval(std::time::Duration::from_millis(500));
                let mut was_playing = false;

                loop {
                    tokio::select! {
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(AudioCommand::Play { stream_url, permalink_url }) => {
                                    player.play_url(&stream_url, permalink_url.as_deref()).await;
                                    was_playing = true;
                                }
                                Some(AudioCommand::Pause) => {
                                    player.pause().await;
                                }
                                Some(AudioCommand::Resume) => {
                                    player.resume().await;
                                }
                                Some(AudioCommand::Stop) => {
                                    player.stop().await;
                                    was_playing = false;
                                }
                                Some(AudioCommand::SetVolume(vol)) => {
                                    player.set_volume(vol);
                                }
                                None => break, // Channel closed
                            }
                        }
                        _ = check_interval.tick() => {
                            // Check if playback finished
                            if was_playing {
                                if let Some(sink) = &player.sink {
                                    if sink.empty() {
                                        eprintln!("Track finished playing");
                                        was_playing = false;
                                        let _ = player.event_tx.send(AudioEvent::Finished).await;
                                    }
                                }
                            }
                        }
                    }
                }
            });
        });

        (cmd_tx, evt_rx)
    }

    async fn play_url(&mut self, url: &str, permalink_url: Option<&str>) {
        // Stop any existing playback
        self.stop().await;

        eprintln!("play_url: {}...", &url[..url.len().min(80)]);

        let _ = self.event_tx.send(AudioEvent::Buffering(true)).await;

        // Check if this is an HLS stream (m3u8)
        if url.contains(".m3u8") {
            eprintln!("  -> HLS stream detected");
            self.play_hls(url, permalink_url, false).await;
            return;
        }

        eprintln!("  -> Progressive stream, downloading...");

        // Regular progressive stream
        let url = match url.parse::<reqwest::Url>() {
            Ok(u) => u,
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Invalid URL: {e}")))
                    .await;
                return;
            }
        };

        // Create streaming download
        let stream = match StreamDownload::new_http(
            url,
            TempStorageProvider::default(),
            Settings::default(),
        )
        .await
        {
            Ok(s) => {
                eprintln!("  -> Stream download started");
                s
            }
            Err(e) => {
                eprintln!("  -> Stream download FAILED: {e}");
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to stream: {e}")))
                    .await;
                let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                return;
            }
        };

        let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;

        // Decode audio
        eprintln!("  -> Decoding audio...");
        let source = match Decoder::new(stream) {
            Ok(s) => {
                eprintln!("  -> Decoder created successfully");
                s
            }
            Err(e) => {
                eprintln!("  -> Decode FAILED: {e}");
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to decode: {e}")))
                    .await;
                return;
            }
        };

        // Create sink and play
        match Sink::try_new(&self.stream_handle) {
            Ok(sink) => {
                eprintln!("  -> Playing!");
                sink.set_volume(self.volume);
                sink.append(source);
                self.sink = Some(sink);
                let _ = self.event_tx.send(AudioEvent::Playing).await;
            }
            Err(e) => {
                eprintln!("  -> Sink creation FAILED: {e}");
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to create sink: {e}")))
                    .await;
            }
        }
    }

    async fn pause(&mut self) {
        if let Some(sink) = &self.sink {
            sink.pause();
            let _ = self.event_tx.send(AudioEvent::Paused).await;
        }
    }

    async fn resume(&mut self) {
        if let Some(sink) = &self.sink {
            sink.play();
            let _ = self.event_tx.send(AudioEvent::Playing).await;
        }
    }

    async fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
            let _ = self.event_tx.send(AudioEvent::Stopped).await;
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(sink) = &self.sink {
            sink.set_volume(self.volume);
        }
    }

    /// Play an HLS stream by downloading and concatenating segments
    /// `from_ytdlp` indicates this URL came from yt-dlp fallback (prevents recursion)
    async fn play_hls(&mut self, url: &str, permalink_url: Option<&str>, from_ytdlp: bool) {
        // Fetch and parse the m3u8 playlist
        let playlist = match hls::fetch_playlist(&self.http_client, url).await {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to parse HLS: {e}")))
                    .await;
                let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                return;
            }
        };

        // Check encryption - SoundCloud uses CENC/PlayReady which requires commercial DRM
        if let Some(enc) = &playlist.encryption {
            eprintln!("HLS encryption: method={}, keyformat={:?}, key_uri={:?}",
                enc.method, enc.keyformat, enc.uri);

            // Check for commercial DRM that requires a license server
            // SAMPLE-AES alone just means encryption - it's the keyformat that determines DRM
            let is_commercial_drm = enc.keyformat.as_ref().map_or(false, |k| {
                k.contains("playready")     // Microsoft PlayReady
                || k.contains("widevine")   // Google Widevine
                || k.contains("fairplay")   // Apple FairPlay
                || k.contains("urn:uuid")   // Generic CENC DRM
            });

            if is_commercial_drm || (enc.method.contains("AES") && enc.uri.is_some()) {
                let drm_type = enc.keyformat.as_deref().unwrap_or("encrypted").to_string();

                // If we're already from yt-dlp, don't try again (prevents infinite recursion)
                if from_ytdlp {
                    eprintln!("yt-dlp stream is also encrypted - giving up");
                    let track_url = permalink_url.unwrap_or("").to_string();
                    let _ = self
                        .event_tx
                        .send(AudioEvent::DrmProtected { drm_type, track_url })
                        .await;
                    let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                    return;
                }

                eprintln!("Encrypted stream detected ({}), trying yt-dlp fallback...", drm_type);

                // Try yt-dlp to get an unencrypted stream
                if let Some(track_url) = permalink_url {
                    if !track_url.is_empty() {
                        match ytdlp::extract_stream_url(track_url) {
                            Ok(ytdlp_url) => {
                                eprintln!("yt-dlp extracted URL: {}...", &ytdlp_url[..ytdlp_url.len().min(80)]);
                                // Play the yt-dlp URL using play_hls_stream directly to avoid recursion
                                self.play_hls_stream(&ytdlp_url).await;
                                return;
                            }
                            Err(e) => {
                                eprintln!("yt-dlp failed: {e}");
                            }
                        }
                    }
                }

                // yt-dlp failed, fall back to browser
                let track_url = permalink_url.unwrap_or("").to_string();
                let _ = self
                    .event_tx
                    .send(AudioEvent::DrmProtected { drm_type, track_url })
                    .await;
                let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                return;
            }
        }

        // Stream the playlist segments
        self.stream_hls_playlist(&playlist).await;
    }

    /// Stream an HLS playlist (no DRM check - use after verifying stream is playable)
    async fn play_hls_stream(&mut self, url: &str) {
        // Fetch and parse the m3u8 playlist
        let playlist = match hls::fetch_playlist(&self.http_client, url).await {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to parse HLS: {e}")))
                    .await;
                let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                return;
            }
        };

        self.stream_hls_playlist(&playlist).await;
    }

    /// Download and play HLS segments from a parsed playlist
    async fn stream_hls_playlist(&mut self, playlist: &hls::HlsStream) {
        if playlist.segments.is_empty() {
            let _ = self
                .event_tx
                .send(AudioEvent::Error("No segments in playlist".into()))
                .await;
            let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
            return;
        }

        // Download init segment first if present (for fMP4 streams)
        let mut audio_data = Vec::new();
        if let Some(init_url) = &playlist.init_segment_url {
            eprintln!("Downloading init segment: {}...", &init_url[..init_url.len().min(60)]);
            match hls::download_segment(&self.http_client, init_url).await {
                Ok(data) => {
                    audio_data.extend(data);
                }
                Err(e) => {
                    let _ = self
                        .event_tx
                        .send(AudioEvent::Error(format!("Failed to download init segment: {e}")))
                        .await;
                    let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                    return;
                }
            }
        }

        // Download all segments
        let total_segments = playlist.segments.len();
        eprintln!("Downloading {} segments...", total_segments);

        for (i, segment) in playlist.segments.iter().enumerate() {
            eprintln!("Downloading segment {}/{}: {}...", i + 1, total_segments, &segment.uri[..segment.uri.len().min(60)]);

            match hls::download_segment(&self.http_client, &segment.uri).await {
                Ok(data) => {
                    audio_data.extend(data);
                }
                Err(e) => {
                    let _ = self
                        .event_tx
                        .send(AudioEvent::Error(format!("Failed to download segment: {e}")))
                        .await;
                    let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;
                    return;
                }
            }
        }

        let _ = self.event_tx.send(AudioEvent::Buffering(false)).await;

        // Try to decode the concatenated segments
        let cursor = Cursor::new(audio_data);
        let source = match Decoder::new(cursor) {
            Ok(s) => s,
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to decode HLS segments: {e}")))
                    .await;
                return;
            }
        };

        // Create sink and play
        match Sink::try_new(&self.stream_handle) {
            Ok(sink) => {
                sink.set_volume(self.volume);
                sink.append(source);
                self.sink = Some(sink);
                let _ = self.event_tx.send(AudioEvent::Playing).await;
            }
            Err(e) => {
                let _ = self
                    .event_tx
                    .send(AudioEvent::Error(format!("Failed to create sink: {e}")))
                    .await;
            }
        }
    }
}
