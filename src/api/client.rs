// SPDX-License-Identifier: MPL-2.0

use reqwest::Client;
use std::fmt;

use super::types::{Album, AlbumsResponse, LikesResponse, Playlist, StreamUrlResponse, Track, TracksResponse, User, UsersSearchResponse};

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

    /// Get any user's profile by ID
    pub async fn get_user(&self, user_id: u64) -> Result<User, ApiError> {
        let url = self.url_with_client_id(&format!("/users/{user_id}"));
        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }
        if response.status() == 404 {
            return Err(ApiError::NotFound);
        }

        Ok(response.json().await?)
    }

    /// Get a user's albums
    pub async fn get_user_albums(&self, user_id: u64) -> Result<Vec<Album>, ApiError> {
        let url = self.url_with_client_id(&format!(
            "/users/{user_id}/albums?limit=50&linked_partitioning=1"
        ));

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }

        let albums: AlbumsResponse = response.json().await?;
        Ok(albums.collection)
    }

    /// Get a user's uploaded tracks
    pub async fn get_user_tracks(
        &self,
        user_id: u64,
        next_href: Option<&str>,
    ) -> Result<(Vec<Track>, Option<String>), ApiError> {
        let url = match next_href {
            Some(href) => href.to_string(),
            None => self.url_with_client_id(&format!(
                "/users/{user_id}/tracks?limit=24&linked_partitioning=1"
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

        let tracks: TracksResponse = response.json().await?;
        Ok((tracks.collection, tracks.next_href))
    }

    /// Get tracks from a playlist/album
    pub async fn get_playlist_tracks(&self, playlist_id: u64) -> Result<Vec<Track>, ApiError> {
        let url = self.url_with_client_id(&format!("/playlists/{playlist_id}"));
        eprintln!("[api] Fetching playlist tracks from: {url}");

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }
        if response.status() == 404 {
            return Err(ApiError::NotFound);
        }

        // Get raw text to debug
        let text = response.text().await?;
        eprintln!("[api] Playlist response (first 500 chars): {}", &text[..text.len().min(500)]);

        let playlist: super::types::PlaylistWithTracks = serde_json::from_str(&text)
            .map_err(|e| {
                eprintln!("[api] JSON parse error: {e}");
                ApiError::Json(e.to_string())
            })?;

        // Filter out stub tracks (those missing title/user data)
        let complete_tracks: Vec<Track> = playlist
            .tracks
            .into_iter()
            .filter(|t| t.is_complete())
            .collect();

        eprintln!(
            "[api] Playlist has {} complete tracks (out of {} total)",
            complete_tracks.len(),
            playlist.track_count
        );

        Ok(complete_tracks)
    }

    /// Search for users/artists
    pub async fn search_users(
        &self,
        query: &str,
        next_href: Option<&str>,
    ) -> Result<(Vec<User>, Option<String>), ApiError> {
        let url = match next_href {
            Some(href) => href.to_string(),
            None => {
                let encoded_query = urlencoding::encode(query);
                self.url_with_client_id(&format!(
                    "/search/users?q={encoded_query}&limit=24&linked_partitioning=1"
                ))
            }
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

        let results: UsersSearchResponse = response.json().await?;
        Ok((results.collection, results.next_href))
    }

    /// Get recommended/featured playlists (uses the mixed selections endpoint)
    pub async fn get_recommendations(&self) -> Result<Vec<Playlist>, ApiError> {
        // Use the discover/sets endpoint which returns curated playlists
        let url = self.url_with_client_id("/mixed-selections?limit=10");

        let response = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ApiError::Unauthorized);
        }

        // The mixed-selections endpoint returns a different structure
        // with "collection" containing selection items that have playlists
        let text = response.text().await?;

        // Parse the mixed selections response
        #[derive(serde::Deserialize)]
        struct MixedSelectionsResponse {
            collection: Vec<MixedSelection>,
        }

        #[derive(serde::Deserialize)]
        struct MixedSelection {
            items: Option<MixedItems>,
        }

        #[derive(serde::Deserialize)]
        struct MixedItems {
            collection: Vec<MixedItem>,
        }

        #[derive(serde::Deserialize)]
        #[serde(tag = "kind")]
        enum MixedItem {
            #[serde(rename = "playlist")]
            Playlist(Playlist),
            #[serde(other)]
            Other,
        }

        let selections: MixedSelectionsResponse = serde_json::from_str(&text)
            .map_err(|e| ApiError::Json(e.to_string()))?;

        // Extract playlists from selections
        let playlists: Vec<Playlist> = selections
            .collection
            .into_iter()
            .filter_map(|s| s.items)
            .flat_map(|items| items.collection)
            .filter_map(|item| match item {
                MixedItem::Playlist(p) => Some(p),
                MixedItem::Other => None,
            })
            .take(20)
            .collect();

        Ok(playlists)
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
