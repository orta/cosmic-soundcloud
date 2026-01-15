// SPDX-License-Identifier: MPL-2.0

use reqwest::Client;
use std::fmt;

use super::types::{LikesResponse, StreamUrlResponse, Track, TracksResponse, User};

const SOUNDCLOUD_API_V2: &str = "https://api-v2.soundcloud.com";
const DEFAULT_CLIENT_ID: &str = "FPh1fGfGpygQyivIKoNCi4d6d490BOvt";

/// SoundCloud API error
#[derive(Debug)]
pub enum ApiError {
    Http(reqwest::Error),
    Json(String),
    NoStreamUrl,
    Unauthorized,
    NotFound,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Json(e) => write!(f, "JSON parse error: {e}"),
            Self::NoStreamUrl => write!(f, "No stream URL available"),
            Self::Unauthorized => write!(f, "Unauthorized - invalid or expired token"),
            Self::NotFound => write!(f, "Resource not found"),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

/// SoundCloud API client
#[derive(Clone)]
pub struct SoundCloudClient {
    http: Client,
    oauth_token: String,
    client_id: String,
}

impl SoundCloudClient {
    /// Create a new client with OAuth token
    pub fn new(oauth_token: impl Into<String>) -> Self {
        let token = oauth_token.into();
        // Strip "OAuth " prefix if present
        let clean_token = token
            .strip_prefix("OAuth ")
            .unwrap_or(&token)
            .to_string();

        Self {
            http: Client::new(),
            oauth_token: clean_token,
            client_id: DEFAULT_CLIENT_ID.to_string(),
        }
    }

    /// Build authorization header value
    fn auth_header(&self) -> String {
        format!("OAuth {}", self.oauth_token)
    }

    /// Build URL with client_id parameter
    fn url_with_client_id(&self, endpoint: &str) -> String {
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        format!(
            "{SOUNDCLOUD_API_V2}{endpoint}{separator}client_id={}",
            self.client_id
        )
    }

    /// Get authenticated user profile
    pub async fn get_me(&self) -> Result<User, ApiError> {
        let url = self.url_with_client_id("/me");
        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }

        Ok(response.json().await?)
    }

    /// Get user's liked tracks
    pub async fn get_user_likes(
        &self,
        user_id: u64,
        next_href: Option<&str>,
    ) -> Result<(Vec<Track>, Option<String>), ApiError> {
        let url = match next_href {
            Some(href) => href.to_string(),
            None => self.url_with_client_id(&format!(
                "/users/{user_id}/track_likes?limit=24&linked_partitioning=1"
            )),
        };

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }

        let likes: LikesResponse = response.json().await?;
        let tracks = likes.collection.into_iter().map(|item| item.track).collect();
        Ok((tracks, likes.next_href))
    }

    /// Get user's listening history
    pub async fn get_history(
        &self,
        next_href: Option<&str>,
    ) -> Result<(Vec<Track>, Option<String>), ApiError> {
        let url = match next_href {
            Some(href) => href.to_string(),
            None => self.url_with_client_id("/me/play-history/tracks?limit=25&linked_partitioning=1"),
        };

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }

        let history: TracksResponse = response.json().await?;
        Ok((history.collection, history.next_href))
    }

    /// Get the actual stream URL for a track
    pub async fn get_stream_url(&self, track: &Track) -> Result<String, ApiError> {
        // Debug: print all available transcodings
        if let Some(media) = &track.media {
            eprintln!("Available transcodings for '{}':", track.title);
            for t in &media.transcodings {
                eprintln!("  - protocol: {}, mime: {}, url: {}...",
                    t.format.protocol,
                    t.format.mime_type,
                    &t.url[..t.url.len().min(80)]);
            }
        }

        // Use encrypted HLS (only working option since Dec 2025)
        let transcoding = track
            .best_transcoding()
            .ok_or(ApiError::NoStreamUrl)?;

        eprintln!("Selected transcoding: {}", &transcoding.url[..transcoding.url.len().min(100)]);

        // Get track authorization token
        let track_auth = track
            .track_authorization
            .as_ref()
            .ok_or(ApiError::NoStreamUrl)?;

        // The transcoding URL returns a redirect to the actual stream
        let url = format!(
            "{}?client_id={}&track_authorization={}",
            transcoding.url, self.client_id, track_auth
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        let text = response.text().await?;
        let stream_response: StreamUrlResponse = serde_json::from_str(&text)
            .map_err(|e| ApiError::Json(e.to_string()))?;
        Ok(stream_response.url)
    }
}
