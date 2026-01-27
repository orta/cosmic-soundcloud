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

    /// Fetch full track details for multiple track IDs in batches
    pub async fn get_tracks_by_ids(&self, ids: &[u64]) -> Result<Vec<Track>, ApiError> {
        let mut all_tracks = Vec::with_capacity(ids.len());

        for chunk in ids.chunks(50) {
            let ids_param: String = chunk
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = self.url_with_client_id(&format!("/tracks?ids={ids_param}"));

            let response = self
                .http
                .get(&url)
                .header("Authorization", self.auth_header())
                .send()
                .await?;

            if response.status() == 401 {
                return Err(ApiError::Unauthorized);
            }

            let tracks: Vec<Track> = response.json().await?;
            all_tracks.extend(tracks);
        }

        Ok(all_tracks)
    }

    /// Get preview track titles from an album/playlist (the ~5 complete tracks SoundCloud embeds)
    pub async fn get_album_preview_titles(&self, album_id: u64) -> Result<Vec<String>, ApiError> {
        let url = self.url_with_client_id(&format!("/playlists/{album_id}"));
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

        let playlist: super::types::PlaylistWithTracks = response
            .json()
            .await
            .map_err(|e| ApiError::Json(e.to_string()))?;

        let titles: Vec<String> = playlist
            .tracks
            .iter()
            .filter(|t| t.is_complete())
            .map(|t| t.title.to_lowercase())
            .collect();

        Ok(titles)
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

        let text = response.text().await?;
        let playlist: super::types::PlaylistWithTracks = serde_json::from_str(&text)
            .map_err(|e| {
                eprintln!("[api] JSON parse error: {e}");
                ApiError::Json(e.to_string())
            })?;

        // Separate complete and stub tracks, preserving original order
        let mut complete_by_id: std::collections::HashMap<u64, Track> = std::collections::HashMap::new();
        let mut stub_ids: Vec<u64> = Vec::new();
        let track_order: Vec<u64> = playlist.tracks.iter().map(|t| t.id).collect();

        for track in playlist.tracks {
            if track.is_complete() {
                complete_by_id.insert(track.id, track);
            } else {
                stub_ids.push(track.id);
            }
        }

        eprintln!(
            "[api] Playlist has {} complete, {} stub tracks (out of {} total)",
            complete_by_id.len(),
            stub_ids.len(),
            playlist.track_count
        );

        // Fetch full data for stub tracks
        if !stub_ids.is_empty() {
            match self.get_tracks_by_ids(&stub_ids).await {
                Ok(resolved) => {
                    for track in resolved {
                        complete_by_id.insert(track.id, track);
                    }
                }
                Err(e) => {
                    eprintln!("[api] Failed to resolve stub tracks: {e}");
                }
            }
        }

        // Reassemble in original order
        let tracks: Vec<Track> = track_order
            .into_iter()
            .filter_map(|id| complete_by_id.remove(&id))
            .collect();

        eprintln!("[api] Returning {} tracks", tracks.len());
        Ok(tracks)
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
