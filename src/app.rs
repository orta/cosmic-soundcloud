// SPDX-License-Identifier: MPL-2.0

use crate::api::{Album, Playlist, SoundCloudClient, Track, User};
use crate::audio::{open_in_browser, AudioCommand, AudioEvent, AudioPlayer};
use crate::config::{Config, RecentArtist};
use crate::fl;
use crate::keyring;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::widget::{self, about::About, icon, image, menu, nav_bar, segmented_button};
use cosmic::{iced_futures, prelude::*, Element};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/com.github.orta.cosmic-soundcloud.svg");

/// Authentication state
#[derive(Debug, Clone, Default)]
pub enum AuthState {
    #[default]
    NotAuthenticated,
    Authenticating,
    Authenticated,
    Failed(String),
}

/// Playback state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
    Buffering,
}

/// Library tab selection
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LibraryTab {
    #[default]
    Overview,
    Likes,
    Playlists,
    Albums,
    Stations,
    Following,
    History,
}

impl LibraryTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Likes => "Likes",
            Self::Playlists => "Playlists",
            Self::Albums => "Albums",
            Self::Stations => "Stations",
            Self::Following => "Following",
            Self::History => "History",
        }
    }

    pub fn all() -> &'static [LibraryTab] {
        &[
            Self::Overview,
            Self::Likes,
            Self::Playlists,
            Self::Albums,
            Self::Stations,
            Self::Following,
            Self::History,
        ]
    }
}

/// Navigation page
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Library,
    Artist(u64),
    Search,
    Recommendations,
}

/// Paginated data container
#[derive(Debug, Clone)]
pub struct PaginatedData<T> {
    pub items: Vec<T>,
    pub next_href: Option<String>,
    pub loading: bool,
}

impl<T> Default for PaginatedData<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_href: None,
            loading: false,
        }
    }
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    /// The about page for this app.
    about: About,
    /// Contains items assigned to the nav bar panel.
    nav: nav_bar::Model,
    /// Key bindings for the application's menu bar.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    /// Configuration data that persists between application runs.
    config: Config,

    // === Authentication ===
    auth_state: AuthState,
    login_token_input: String,

    // === User Data ===
    current_user: Option<User>,
    api_client: Option<SoundCloudClient>,

    // === Library State ===
    current_tab: LibraryTab,
    tab_model: segmented_button::SingleSelectModel,
    likes: PaginatedData<Track>,
    history: PaginatedData<Track>,

    // === Audio Player State ===
    audio_cmd_tx: Option<mpsc::Sender<AudioCommand>>,
    playback_status: PlaybackStatus,
    current_track: Option<Track>,
    current_playlist: Vec<Track>,
    playlist_index: usize,
    volume: f32,

    // === Artwork Cache ===
    artwork_cache: HashMap<String, image::Handle>,
    artwork_loading: HashSet<String>,

    // === Artist Page State ===
    current_page: Page,
    artist_user: Option<User>,
    artist_albums: Vec<Album>,
    artist_tracks: PaginatedData<Track>,

    // === Search Page State ===
    search_query: String,
    search_results: PaginatedData<User>,

    // === Recommendations Page State ===
    recommendations: Vec<Playlist>,
    recommendations_loading: bool,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    // System
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),

    // Authentication
    LoginTokenInput(String),
    SubmitToken,
    Logout,
    UserLoaded(Result<User, String>),

    // Library Navigation
    SwitchTab(segmented_button::Entity),

    // Likes
    LoadLikes,
    LoadMoreLikes,
    LikesLoaded(Result<(Vec<Track>, Option<String>), String>),
    LikesScrolled(cosmic::iced_widget::scrollable::Viewport),

    // History
    LoadHistory,
    HistoryLoaded(Result<(Vec<Track>, Option<String>), String>),

    // Track Actions
    PlayTrack(Track),
    PlayTrackInPlaylist(Track, Vec<Track>, usize),

    // Audio Player
    AudioReady(mpsc::Sender<AudioCommand>),
    AudioEvent(AudioEvent),
    StreamUrlLoaded(Result<String, String>),
    TogglePlayPause,
    NextTrack,
    PreviousTrack,
    SetVolume(f32),

    // Artwork
    LoadArtwork(String),
    ArtworkLoaded(String, Vec<u8>),

    // Artist Navigation
    NavigateToArtist(u64, String, Option<String>), // id, username, avatar_url
    NavigateToLibrary,
    ArtistLoaded(Result<User, String>),
    ArtistAlbumsLoaded(Result<Vec<Album>, String>),
    ArtistTracksLoaded(Result<(Vec<Track>, Option<String>), String>),
    LoadMoreArtistTracks,

    // Album Playback
    PlayAlbum(u64),                              // album_id - load tracks and play
    AlbumTracksLoaded(Result<Vec<Track>, String>),

    // Search
    SearchQueryInput(String),
    SubmitSearch,
    SearchResultsLoaded(Result<(Vec<User>, Option<String>), String>),
    LoadMoreSearchResults,
    NavigateToSearch,

    // Recommendations
    NavigateToRecommendations,
    LoadRecommendations,
    RecommendationsLoaded(Result<Vec<Playlist>, String>),
    PlayPlaylist(u64),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "com.github.orta.cosmic-soundcloud";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Create a nav bar with Library page
        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text(fl!("library"))
            .data::<Page>(Page::Library)
            .icon(icon::from_name("folder-music-symbolic"))
            .activate();

        // Create tab model for library tabs
        let mut tab_model = segmented_button::SingleSelectModel::default();
        for tab in LibraryTab::all() {
            tab_model.insert().text(tab.label()).data(*tab);
        }
        tab_model.activate_position(0);

        // Create the about widget
        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        // Load configuration
        let config: Config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let volume = config.volume;

        // Check if we have a saved token in keyring
        // Also migrate any token from old config storage to keyring
        let (auth_state, api_client) = {
            // Try to get token from keyring first
            eprintln!("[init] Checking keyring for existing token...");
            let keyring_result = keyring::get_token();
            eprintln!("[init] Keyring result: {keyring_result:?}");
            let keyring_token = keyring_result.ok().flatten();

            // Check if there's a token in config that should be migrated
            let config_token = config.oauth_token.clone().filter(|t| !t.trim().is_empty());
            eprintln!("[init] Config token present: {}", config_token.is_some());

            // Use keyring token, or migrate config token to keyring
            let token = if let Some(token) = keyring_token {
                eprintln!("[init] Using token from keyring");
                Some(token)
            } else if let Some(token) = config_token {
                // Migrate token from config to keyring
                eprintln!("[init] Migrating token from config to keyring...");
                match keyring::store_token(&token) {
                    Ok(()) => eprintln!("[init] Migration successful"),
                    Err(e) => eprintln!("[init] Migration failed: {e}"),
                }
                Some(token)
            } else {
                eprintln!("[init] No token found");
                None
            };

            if let Some(token) = token {
                eprintln!("[init] Token found, will authenticate (token length: {})", token.len());
                (
                    AuthState::Authenticating,
                    Some(SoundCloudClient::new(token)),
                )
            } else {
                eprintln!("[init] No token, showing login screen");
                (AuthState::NotAuthenticated, None)
            }
        };

        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            nav,
            key_binds: HashMap::new(),
            config,
            auth_state,
            login_token_input: String::new(),
            current_user: None,
            api_client,
            current_tab: LibraryTab::default(),
            tab_model,
            likes: PaginatedData::default(),
            history: PaginatedData::default(),
            audio_cmd_tx: None,
            playback_status: PlaybackStatus::Stopped,
            current_track: None,
            current_playlist: Vec::new(),
            playlist_index: 0,
            volume,
            artwork_cache: HashMap::new(),
            artwork_loading: HashSet::new(),
            // Artist page state
            current_page: Page::Library,
            artist_user: None,
            artist_albums: Vec::new(),
            artist_tracks: PaginatedData::default(),
            // Search page state
            search_query: String::new(),
            search_results: PaginatedData::default(),
            // Recommendations page state
            recommendations: Vec::new(),
            recommendations_loading: false,
        };

        // Rebuild nav to include recent artists from config
        app.rebuild_nav();

        // If we have a token, fetch user info
        let command = if app.api_client.is_some() {
            let client = app.api_client.clone().unwrap();
            cosmic::task::future(async move {
                match client.get_me().await {
                    Ok(user) => Message::UserLoaded(Ok(user)),
                    Err(e) => Message::UserLoaded(Err(e.to_string())),
                }
            })
            .map(cosmic::Action::App)
        } else {
            app.update_title()
        };

        (app, command)
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        let mut elements = Vec::new();

        if let Some(user) = &self.current_user {
            // Show user's avatar if loaded, otherwise default icon
            let avatar: Element<_> = if let Some(avatar_url) = &user.avatar_url {
                if let Some(handle) = self.artwork_cache.get(avatar_url) {
                    widget::image(handle.clone())
                        .width(Length::Fixed(24.0))
                        .height(Length::Fixed(24.0))
                        .content_fit(cosmic::iced::ContentFit::Cover)
                        .into()
                } else {
                    widget::icon::from_name("avatar-default-symbolic")
                        .size(24)
                        .apply(Element::from)
                }
            } else {
                widget::icon::from_name("avatar-default-symbolic")
                    .size(24)
                    .apply(Element::from)
            };

            let user_info = widget::row::with_capacity(2)
                .push(avatar)
                .push(widget::text::body(&user.username))
                .spacing(cosmic::theme::spacing().space_xs)
                .align_y(Alignment::Center);

            elements.push(user_info.into());
        }

        elements
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Use auth_state to decide what to show, not keyring
        // Keyring is only for persistence across restarts
        let main_content = match &self.auth_state {
            AuthState::Authenticated => self.view_main_layout(),
            AuthState::Authenticating => self.view_loading("Authenticating..."),
            AuthState::Failed(err) => self.view_error(err),
            AuthState::NotAuthenticated => self.view_login(),
        };

        widget::container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        // Audio player subscription - MUST be returned every time or iced will cancel it
        // The subscription identity is tracked internally by iced, so it only spawns once
        subscriptions.push(Subscription::run(|| {
            iced_futures::stream::channel(32, |mut emitter| async move {
                let (cmd_tx, mut evt_rx) = AudioPlayer::spawn();

                // Send the command channel back to the app
                let _ = emitter.send(Message::AudioReady(cmd_tx)).await;

                // Forward audio events forever
                while let Some(event) = evt_rx.recv().await {
                    let _ = emitter.send(Message::AudioEvent(event)).await;
                }
            })
        }));

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::LaunchUrl(url) => {
                if let Err(err) = open::that_detached(&url) {
                    eprintln!("failed to open {url:?}: {err}");
                }
            }

            // === Authentication ===
            Message::LoginTokenInput(input) => {
                self.login_token_input = input;
            }

            Message::SubmitToken => {
                let token = self.login_token_input.trim().to_string();
                eprintln!("[login] SubmitToken called, token length: {}", token.len());
                if !token.is_empty() {
                    eprintln!("[login] Token is not empty, proceeding with auth...");
                    self.auth_state = AuthState::Authenticating;
                    self.api_client = Some(SoundCloudClient::new(token.clone()));

                    // Try to save to keyring (may not work on all systems)
                    eprintln!("[login] Storing token in keyring...");
                    match keyring::store_token(&token) {
                        Ok(()) => eprintln!("[login] Token stored in keyring"),
                        Err(e) => eprintln!("[login] Keyring unavailable: {e}"),
                    }

                    // Also save to config as fallback
                    self.config.oauth_token = Some(token);
                    if let Ok(config_context) =
                        cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                    {
                        let _ = self.config.write_entry(&config_context);
                        eprintln!("[login] Token saved to config");
                    }

                    // Fetch user info
                    eprintln!("[login] Fetching user info from API...");
                    let client = self.api_client.clone().unwrap();
                    return cosmic::task::future(async move {
                        eprintln!("[login] Making API request to /me...");
                        match client.get_me().await {
                            Ok(user) => {
                                eprintln!("[login] API success: got user {}", user.username);
                                Message::UserLoaded(Ok(user))
                            }
                            Err(e) => {
                                eprintln!("[login] API error: {e}");
                                Message::UserLoaded(Err(e.to_string()))
                            }
                        }
                    })
                    .map(cosmic::Action::App);
                } else {
                    eprintln!("[login] Token is empty, ignoring submit");
                }
            }

            Message::Logout => {
                self.auth_state = AuthState::NotAuthenticated;
                self.api_client = None;
                self.current_user = None;
                self.login_token_input.clear();
                self.likes = PaginatedData::default();
                self.history = PaginatedData::default();

                // Stop playback
                if let Some(tx) = &self.audio_cmd_tx {
                    let _ = tx.blocking_send(AudioCommand::Stop);
                }
                self.playback_status = PlaybackStatus::Stopped;
                self.current_track = None;

                // Delete token from keyring (if available)
                let _ = keyring::delete_token();

                // Clear token from config
                self.config.oauth_token = None;
                if let Ok(config_context) =
                    cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                {
                    let _ = self.config.write_entry(&config_context);
                }
            }

            Message::UserLoaded(result) => {
                eprintln!("[login] UserLoaded message received");
                match result {
                    Ok(user) => {
                        eprintln!("[login] Authentication successful! User: {}", user.username);
                        // Load user's avatar if available
                        let mut tasks: Vec<Task<cosmic::Action<Message>>> =
                            vec![cosmic::task::message(cosmic::Action::App(Message::LoadLikes))];
                        if let Some(avatar_url) = &user.avatar_url
                            && !self.artwork_cache.contains_key(avatar_url)
                            && !self.artwork_loading.contains(avatar_url)
                        {
                            tasks.push(cosmic::task::message(cosmic::Action::App(
                                Message::LoadArtwork(avatar_url.clone()),
                            )));
                        }
                        self.current_user = Some(user);
                        self.auth_state = AuthState::Authenticated;
                        return cosmic::task::batch(tasks);
                    }
                    Err(err) => {
                        eprintln!("[login] Authentication failed: {err}");
                        self.auth_state = AuthState::Failed(err);
                        self.api_client = None;
                    }
                }
            }

            // === Library Navigation ===
            Message::SwitchTab(entity) => {
                self.tab_model.activate(entity);
                if let Some(tab) = self.tab_model.active_data::<LibraryTab>() {
                    self.current_tab = *tab;

                    match self.current_tab {
                        LibraryTab::Likes if self.likes.items.is_empty() && !self.likes.loading => {
                            return cosmic::task::message(cosmic::Action::App(Message::LoadLikes));
                        }
                        LibraryTab::History
                            if self.history.items.is_empty() && !self.history.loading =>
                        {
                            return cosmic::task::message(cosmic::Action::App(Message::LoadHistory));
                        }
                        _ => {}
                    }
                }
            }

            // === Likes ===
            Message::LoadLikes => {
                if let (Some(client), Some(user)) = (&self.api_client, &self.current_user) {
                    self.likes.loading = true;
                    let client = client.clone();
                    let user_id = user.id;
                    return cosmic::task::future(async move {
                        match client.get_user_likes(user_id, None).await {
                            Ok((tracks, next)) => Message::LikesLoaded(Ok((tracks, next))),
                            Err(e) => Message::LikesLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::LoadMoreLikes => {
                if let (Some(client), Some(user), Some(next_href)) =
                    (&self.api_client, &self.current_user, &self.likes.next_href)
                {
                    self.likes.loading = true;
                    let client = client.clone();
                    let next = next_href.clone();
                    let user_id = user.id;
                    return cosmic::task::future(async move {
                        match client.get_user_likes(user_id, Some(&next)).await {
                            Ok((tracks, next)) => Message::LikesLoaded(Ok((tracks, next))),
                            Err(e) => Message::LikesLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::LikesLoaded(result) => {
                self.likes.loading = false;
                match result {
                    Ok((tracks, next_href)) => {
                        // Queue artwork loading for new tracks
                        let artwork_urls: Vec<_> = tracks
                            .iter()
                            .filter_map(|t| t.artwork_url.clone())
                            .filter(|url| !self.artwork_cache.contains_key(url) && !self.artwork_loading.contains(url))
                            .collect();

                        self.likes.items.extend(tracks);
                        self.likes.next_href = next_href;

                        // Load artwork
                        if !artwork_urls.is_empty() {
                            let tasks: Vec<Task<cosmic::Action<Message>>> = artwork_urls
                                .into_iter()
                                .map(|url| cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(url))))
                                .collect();
                            return cosmic::task::batch(tasks);
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load likes: {err}");
                    }
                }
            }

            Message::LikesScrolled(viewport) => {
                // Auto-load more when scrolled near bottom (80% threshold)
                let scroll_percentage = viewport.relative_offset().y;
                if scroll_percentage > 0.8
                    && self.likes.next_href.is_some()
                    && !self.likes.loading
                {
                    return cosmic::task::message(cosmic::Action::App(Message::LoadMoreLikes));
                }
            }

            // === History ===
            Message::LoadHistory => {
                if let Some(client) = &self.api_client {
                    self.history.loading = true;
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.get_history(None).await {
                            Ok((tracks, next)) => Message::HistoryLoaded(Ok((tracks, next))),
                            Err(e) => Message::HistoryLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::HistoryLoaded(result) => {
                self.history.loading = false;
                match result {
                    Ok((tracks, next_href)) => {
                        // Queue artwork loading for new tracks
                        let artwork_urls: Vec<_> = tracks
                            .iter()
                            .filter_map(|t| t.artwork_url.clone())
                            .filter(|url| !self.artwork_cache.contains_key(url) && !self.artwork_loading.contains(url))
                            .collect();

                        self.history.items.extend(tracks);
                        self.history.next_href = next_href;

                        // Load artwork
                        if !artwork_urls.is_empty() {
                            let tasks: Vec<Task<cosmic::Action<Message>>> = artwork_urls
                                .into_iter()
                                .map(|url| cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(url))))
                                .collect();
                            return cosmic::task::batch(tasks);
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load history: {err}");
                    }
                }
            }

            // === Track Actions ===
            Message::PlayTrack(track) => {
                // Set playlist from current view
                let playlist = match self.current_tab {
                    LibraryTab::Likes => self.likes.items.clone(),
                    LibraryTab::History => self.history.items.clone(),
                    _ => vec![track.clone()],
                };
                let index = playlist.iter().position(|t| t.id == track.id).unwrap_or(0);

                return cosmic::task::message(cosmic::Action::App(Message::PlayTrackInPlaylist(
                    track, playlist, index,
                )));
            }

            Message::PlayTrackInPlaylist(track, playlist, index) => {
                self.current_track = Some(track.clone());
                self.current_playlist = playlist;
                self.playlist_index = index;
                self.playback_status = PlaybackStatus::Buffering;

                // Load artwork if not cached
                let mut tasks = Vec::new();
                if let Some(artwork_url) = &track.artwork_url
                    && !self.artwork_cache.contains_key(artwork_url)
                    && !self.artwork_loading.contains(artwork_url)
                {
                    tasks.push(cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(artwork_url.clone()))));
                }

                // Fetch stream URL and play
                if let Some(client) = &self.api_client {
                    let client = client.clone();
                    tasks.push(cosmic::task::future(async move {
                        match client.get_stream_url(&track).await {
                            Ok(url) => Message::StreamUrlLoaded(Ok(url)),
                            Err(e) => Message::StreamUrlLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App));
                    return cosmic::task::batch(tasks);
                }
            }

            Message::StreamUrlLoaded(result) => match result {
                Ok(url) => {
                    if let Some(tx) = &self.audio_cmd_tx {
                        let permalink_url = self
                            .current_track
                            .as_ref()
                            .and_then(|t| t.permalink_url.clone());
                        let _ = tx.blocking_send(AudioCommand::SetVolume(self.volume));
                        let _ = tx.blocking_send(AudioCommand::Play {
                            stream_url: url,
                            permalink_url,
                        });
                    }
                }
                Err(err) => {
                    eprintln!("Failed to get stream URL: {err}");
                    self.playback_status = PlaybackStatus::Stopped;
                }
            },

            // === Audio Player ===
            Message::AudioReady(tx) => {
                // Set initial volume
                let _ = tx.blocking_send(AudioCommand::SetVolume(self.volume));
                self.audio_cmd_tx = Some(tx);
            }

            Message::AudioEvent(event) => match event {
                AudioEvent::Playing => {
                    self.playback_status = PlaybackStatus::Playing;
                }
                AudioEvent::Paused => {
                    self.playback_status = PlaybackStatus::Paused;
                }
                AudioEvent::Stopped => {
                    self.playback_status = PlaybackStatus::Stopped;
                }
                AudioEvent::Buffering(buffering) => {
                    if buffering {
                        self.playback_status = PlaybackStatus::Buffering;
                    }
                }
                AudioEvent::Finished => {
                    // Auto-play next track
                    eprintln!("[auto-advance] AudioEvent::Finished received, dispatching NextTrack");
                    return cosmic::task::message(cosmic::Action::App(Message::NextTrack));
                }
                AudioEvent::Error(err) => {
                    eprintln!("Audio error: {err}");
                    self.playback_status = PlaybackStatus::Stopped;
                }
                AudioEvent::DrmProtected { drm_type, track_url } => {
                    eprintln!("DRM-protected content ({drm_type}) - opening in browser");
                    self.playback_status = PlaybackStatus::Stopped;
                    if !track_url.is_empty()
                        && let Err(e) = open_in_browser(&track_url)
                    {
                        eprintln!("Failed to open browser: {e}");
                    }
                }
                AudioEvent::Ready => {}
            },

            Message::TogglePlayPause => {
                if let Some(tx) = &self.audio_cmd_tx {
                    match self.playback_status {
                        PlaybackStatus::Playing => {
                            let _ = tx.blocking_send(AudioCommand::Pause);
                        }
                        PlaybackStatus::Paused => {
                            let _ = tx.blocking_send(AudioCommand::Resume);
                        }
                        PlaybackStatus::Stopped => {
                            // Restart current track if there is one
                            if let Some(track) = &self.current_track {
                                return cosmic::task::message(cosmic::Action::App(
                                    Message::PlayTrack(track.clone()),
                                ));
                            }
                        }
                        PlaybackStatus::Buffering => {}
                    }
                }
            }

            Message::NextTrack => {
                eprintln!("[auto-advance] NextTrack: playlist_len={}, playlist_index={}",
                    self.current_playlist.len(), self.playlist_index);
                if !self.current_playlist.is_empty() {
                    let next_index = (self.playlist_index + 1) % self.current_playlist.len();
                    eprintln!("[auto-advance] NextTrack: next_index={}, repeat_mode={:?}",
                        next_index, self.config.repeat_mode);
                    if next_index != 0 || self.config.repeat_mode == crate::config::RepeatMode::All
                    {
                        let track = self.current_playlist[next_index].clone();
                        eprintln!("[auto-advance] NextTrack: playing '{}'", track.title);
                        let playlist = self.current_playlist.clone();
                        return cosmic::task::message(cosmic::Action::App(
                            Message::PlayTrackInPlaylist(track, playlist, next_index),
                        ));
                    } else {
                        // End of playlist
                        eprintln!("[auto-advance] NextTrack: end of playlist, stopping");
                        self.playback_status = PlaybackStatus::Stopped;
                    }
                } else {
                    eprintln!("[auto-advance] NextTrack: playlist is empty!");
                }
            }

            Message::PreviousTrack => {
                if !self.current_playlist.is_empty() {
                    let prev_index = if self.playlist_index == 0 {
                        self.current_playlist.len() - 1
                    } else {
                        self.playlist_index - 1
                    };
                    let track = self.current_playlist[prev_index].clone();
                    let playlist = self.current_playlist.clone();
                    return cosmic::task::message(cosmic::Action::App(
                        Message::PlayTrackInPlaylist(track, playlist, prev_index),
                    ));
                }
            }

            Message::SetVolume(vol) => {
                self.volume = vol.clamp(0.0, 1.0);
                if let Some(tx) = &self.audio_cmd_tx {
                    let _ = tx.blocking_send(AudioCommand::SetVolume(self.volume));
                }
                // Save to config
                self.config.volume = self.volume;
                if let Ok(config_context) =
                    cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                {
                    let _ = self.config.write_entry(&config_context);
                }
            }

            // === Artwork ===
            Message::LoadArtwork(url) => {
                if !self.artwork_cache.contains_key(&url) && !self.artwork_loading.contains(&url) {
                    self.artwork_loading.insert(url.clone());
                    return cosmic::task::future(async move {
                        match reqwest::get(&url).await {
                            Ok(response) => match response.bytes().await {
                                Ok(bytes) => Message::ArtworkLoaded(url, bytes.to_vec()),
                                Err(_) => Message::ArtworkLoaded(url, Vec::new()),
                            },
                            Err(_) => Message::ArtworkLoaded(url, Vec::new()),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::ArtworkLoaded(url, data) => {
                self.artwork_loading.remove(&url);
                if !data.is_empty() {
                    self.artwork_cache
                        .insert(url, image::Handle::from_bytes(data));
                }
            }

            // === Artist Navigation ===
            Message::NavigateToArtist(user_id, username, avatar_url) => {
                // Update recent artists list
                let recent_artist = RecentArtist {
                    id: user_id,
                    username: username.clone(),
                    avatar_url: avatar_url.clone(),
                };

                // Remove if already exists, then add to front
                self.config
                    .recent_artists
                    .retain(|a| a.id != user_id);
                self.config.recent_artists.insert(0, recent_artist);

                // Keep only 10 most recent
                self.config.recent_artists.truncate(10);

                // Save to config
                if let Ok(config_context) =
                    cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                {
                    let _ = self.config.write_entry(&config_context);
                }

                // Switch to artist page
                self.current_page = Page::Artist(user_id);
                self.artist_user = None;
                self.artist_albums = Vec::new();
                self.artist_tracks = PaginatedData::default();

                // Rebuild nav with recent artists
                self.rebuild_nav();

                // Fetch artist data
                if let Some(client) = &self.api_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.get_user(user_id).await {
                            Ok(user) => Message::ArtistLoaded(Ok(user)),
                            Err(e) => Message::ArtistLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::NavigateToLibrary => {
                self.current_page = Page::Library;
                self.rebuild_nav();
            }

            Message::ArtistLoaded(result) => match result {
                Ok(user) => {
                    let user_id = user.id;

                    // Load avatar artwork if available
                    let avatar_task = if let Some(avatar_url) = &user.avatar_url {
                        if !self.artwork_cache.contains_key(avatar_url)
                            && !self.artwork_loading.contains(avatar_url)
                        {
                            Some(cosmic::task::message(cosmic::Action::App(
                                Message::LoadArtwork(avatar_url.clone()),
                            )))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    self.artist_user = Some(user);

                    // Load albums and tracks in parallel
                    if let Some(client) = &self.api_client {
                        let client1 = client.clone();
                        let client2 = client.clone();

                        let albums_task = cosmic::task::future(async move {
                            match client1.get_user_albums(user_id).await {
                                Ok(albums) => Message::ArtistAlbumsLoaded(Ok(albums)),
                                Err(e) => Message::ArtistAlbumsLoaded(Err(e.to_string())),
                            }
                        })
                        .map(cosmic::Action::App);

                        let tracks_task = cosmic::task::future(async move {
                            match client2.get_user_tracks(user_id, None).await {
                                Ok((tracks, next)) => Message::ArtistTracksLoaded(Ok((tracks, next))),
                                Err(e) => Message::ArtistTracksLoaded(Err(e.to_string())),
                            }
                        })
                        .map(cosmic::Action::App);

                        let mut tasks = vec![albums_task, tracks_task];
                        if let Some(avatar) = avatar_task {
                            tasks.push(avatar);
                        }
                        return Task::batch(tasks);
                    }
                }
                Err(err) => {
                    eprintln!("Failed to load artist: {err}");
                }
            },

            Message::ArtistAlbumsLoaded(result) => {
                if let Ok(albums) = result {
                    // Load album artwork
                    let artwork_tasks: Vec<_> = albums
                        .iter()
                        .filter_map(|album| {
                            album.artwork_url.as_ref().and_then(|url| {
                                if !self.artwork_cache.contains_key(url)
                                    && !self.artwork_loading.contains(url)
                                {
                                    Some(cosmic::task::message(cosmic::Action::App(
                                        Message::LoadArtwork(url.clone()),
                                    )))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    self.artist_albums = albums;

                    if !artwork_tasks.is_empty() {
                        return Task::batch(artwork_tasks);
                    }
                }
            }

            Message::ArtistTracksLoaded(result) => {
                if let Ok((tracks, next_href)) = result {
                    // Load track artwork
                    let artwork_tasks: Vec<_> = tracks
                        .iter()
                        .filter_map(|track| {
                            track.artwork_url.as_ref().and_then(|url| {
                                if !self.artwork_cache.contains_key(url)
                                    && !self.artwork_loading.contains(url)
                                {
                                    Some(cosmic::task::message(cosmic::Action::App(
                                        Message::LoadArtwork(url.clone()),
                                    )))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    self.artist_tracks.items.extend(tracks);
                    self.artist_tracks.next_href = next_href;
                    self.artist_tracks.loading = false;

                    if !artwork_tasks.is_empty() {
                        return Task::batch(artwork_tasks);
                    }
                }
            }

            Message::LoadMoreArtistTracks => {
                if let (Some(client), Page::Artist(user_id), Some(next_href)) = (
                    &self.api_client,
                    &self.current_page,
                    &self.artist_tracks.next_href,
                ) {
                    self.artist_tracks.loading = true;
                    let client = client.clone();
                    let next = next_href.clone();
                    let user_id = *user_id;
                    return cosmic::task::future(async move {
                        match client.get_user_tracks(user_id, Some(&next)).await {
                            Ok((tracks, next)) => Message::ArtistTracksLoaded(Ok((tracks, next))),
                            Err(e) => Message::ArtistTracksLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            // === Album Playback ===
            Message::PlayAlbum(album_id) => {
                if let Some(client) = &self.api_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.get_playlist_tracks(album_id).await {
                            Ok(tracks) => Message::AlbumTracksLoaded(Ok(tracks)),
                            Err(e) => Message::AlbumTracksLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::AlbumTracksLoaded(result) => {
                if let Ok(tracks) = result {
                    if !tracks.is_empty() {
                        // Set as playlist and play first track
                        let first_track = tracks[0].clone();
                        let playlist = tracks;
                        return cosmic::task::message(cosmic::Action::App(
                            Message::PlayTrackInPlaylist(first_track, playlist, 0),
                        ));
                    }
                } else if let Err(e) = result {
                    eprintln!("Failed to load album tracks: {e}");
                }
            }

            // === Search ===
            Message::NavigateToSearch => {
                self.current_page = Page::Search;
                self.rebuild_nav();
            }

            Message::SearchQueryInput(query) => {
                self.search_query = query;
            }

            Message::SubmitSearch => {
                let query = self.search_query.trim().to_string();
                if !query.is_empty() {
                    self.search_results = PaginatedData::default();
                    self.search_results.loading = true;

                    if let Some(client) = &self.api_client {
                        let client = client.clone();
                        return cosmic::task::future(async move {
                            match client.search_users(&query, None).await {
                                Ok((users, next)) => Message::SearchResultsLoaded(Ok((users, next))),
                                Err(e) => Message::SearchResultsLoaded(Err(e.to_string())),
                            }
                        })
                        .map(cosmic::Action::App);
                    }
                }
            }

            Message::SearchResultsLoaded(result) => {
                self.search_results.loading = false;
                match result {
                    Ok((users, next_href)) => {
                        // Queue artwork loading for new users
                        let artwork_urls: Vec<_> = users
                            .iter()
                            .filter_map(|u| u.avatar_url.clone())
                            .filter(|url| {
                                !self.artwork_cache.contains_key(url)
                                    && !self.artwork_loading.contains(url)
                            })
                            .collect();

                        self.search_results.items.extend(users);
                        self.search_results.next_href = next_href;

                        if !artwork_urls.is_empty() {
                            let tasks: Vec<Task<cosmic::Action<Message>>> = artwork_urls
                                .into_iter()
                                .map(|url| {
                                    cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(
                                        url,
                                    )))
                                })
                                .collect();
                            return cosmic::task::batch(tasks);
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to search users: {err}");
                    }
                }
            }

            Message::LoadMoreSearchResults => {
                if let (Some(client), Some(next_href)) =
                    (&self.api_client, &self.search_results.next_href)
                {
                    self.search_results.loading = true;
                    let client = client.clone();
                    let next = next_href.clone();
                    let query = self.search_query.clone();
                    return cosmic::task::future(async move {
                        match client.search_users(&query, Some(&next)).await {
                            Ok((users, next)) => Message::SearchResultsLoaded(Ok((users, next))),
                            Err(e) => Message::SearchResultsLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            // === Recommendations ===
            Message::NavigateToRecommendations => {
                self.current_page = Page::Recommendations;
                self.rebuild_nav();

                // Load recommendations if not already loaded
                if self.recommendations.is_empty() && !self.recommendations_loading {
                    return cosmic::task::message(cosmic::Action::App(Message::LoadRecommendations));
                }
            }

            Message::LoadRecommendations => {
                if let Some(client) = &self.api_client {
                    self.recommendations_loading = true;
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.get_recommendations().await {
                            Ok(playlists) => Message::RecommendationsLoaded(Ok(playlists)),
                            Err(e) => Message::RecommendationsLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
                }
            }

            Message::RecommendationsLoaded(result) => {
                self.recommendations_loading = false;
                match result {
                    Ok(playlists) => {
                        // Queue artwork loading for playlists
                        let artwork_urls: Vec<_> = playlists
                            .iter()
                            .filter_map(|p| p.artwork_url.clone())
                            .filter(|url| {
                                !self.artwork_cache.contains_key(url)
                                    && !self.artwork_loading.contains(url)
                            })
                            .collect();

                        self.recommendations = playlists;

                        if !artwork_urls.is_empty() {
                            let tasks: Vec<Task<cosmic::Action<Message>>> = artwork_urls
                                .into_iter()
                                .map(|url| {
                                    cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(
                                        url,
                                    )))
                                })
                                .collect();
                            return cosmic::task::batch(tasks);
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load recommendations: {err}");
                    }
                }
            }

            Message::PlayPlaylist(playlist_id) => {
                // Reuse the album/playlist loading logic
                return cosmic::task::message(cosmic::Action::App(Message::PlayAlbum(playlist_id)));
            }
        }
        Task::none()
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);

        // Handle page navigation based on nav data
        if let Some(page) = self.nav.data::<Page>(id) {
            match page {
                Page::Library => {
                    self.current_page = Page::Library;
                }
                Page::Artist(user_id) => {
                    // Navigate to artist page - find the artist info from recent_artists
                    if let Some(artist) = self
                        .config
                        .recent_artists
                        .iter()
                        .find(|a| a.id == *user_id)
                    {
                        let user_id = artist.id;
                        let username = artist.username.clone();
                        let avatar_url = artist.avatar_url.clone();
                        return cosmic::task::message(cosmic::Action::App(
                            Message::NavigateToArtist(user_id, username, avatar_url),
                        ));
                    }
                }
                Page::Search => {
                    return cosmic::task::message(cosmic::Action::App(Message::NavigateToSearch));
                }
                Page::Recommendations => {
                    return cosmic::task::message(cosmic::Action::App(
                        Message::NavigateToRecommendations,
                    ));
                }
            }
        }

        self.update_title()
    }
}

impl AppModel {
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    /// Rebuild the navigation model with Library, Search, Recommendations, and recent artists
    fn rebuild_nav(&mut self) {
        self.nav.clear();

        // Clone data we need to avoid borrow issues
        let recent_artists: Vec<_> = self
            .config
            .recent_artists
            .iter()
            .map(|a| (a.id, a.username.clone()))
            .collect();
        let current_page = self.current_page.clone();

        // Add Library entry
        let library_id = self
            .nav
            .insert()
            .text(fl!("library"))
            .icon(icon::from_name("folder-music-symbolic"))
            .data::<Page>(Page::Library)
            .id();

        // Add Search entry
        let search_id = self
            .nav
            .insert()
            .text(fl!("search"))
            .icon(icon::from_name("system-search-symbolic"))
            .data::<Page>(Page::Search)
            .id();

        // Add Recommendations entry
        let recommendations_id = self
            .nav
            .insert()
            .text(fl!("recommendations"))
            .icon(icon::from_name("starred-symbolic"))
            .data::<Page>(Page::Recommendations)
            .id();

        // Add recent artists section header and entries
        let mut artist_nav_id = None;
        if !recent_artists.is_empty() {
            // Add "Recent Artists" section header (no icon, no page data = non-navigable)
            let header_id = self
                .nav
                .insert()
                .text(fl!("recent-artists"))
                .id();
            self.nav.divider_above_set(header_id, true);
        }

        for (artist_id, username) in recent_artists {
            let nav_id = self
                .nav
                .insert()
                .text(username)
                .icon(icon::from_name("system-users-symbolic"))
                .data::<Page>(Page::Artist(artist_id))
                .id();

            if let Page::Artist(current_id) = &current_page
                && *current_id == artist_id
            {
                artist_nav_id = Some(nav_id);
            }
        }

        // Activate the appropriate nav item based on current page
        match current_page {
            Page::Library => {
                self.nav.activate(library_id);
            }
            Page::Artist(_) => {
                if let Some(nav_id) = artist_nav_id {
                    self.nav.activate(nav_id);
                }
            }
            Page::Search => {
                self.nav.activate(search_id);
            }
            Page::Recommendations => {
                self.nav.activate(recommendations_id);
            }
        }
    }

    /// Main layout with content based on current page and player bar at bottom
    fn view_main_layout(&self) -> Element<'_, Message> {
        let content: Element<_> = match &self.current_page {
            Page::Library => self.view_library(),
            Page::Artist(_) => self.view_artist(),
            Page::Search => self.view_search(),
            Page::Recommendations => self.view_recommendations(),
        };

        widget::column::with_capacity(2)
            .push(
                widget::container(content)
                    .width(Length::Fill)
                    .height(Length::FillPortion(4)),
            )
            .push(self.view_player_bar())
            .into()
    }

    /// Bottom player bar with transport controls
    fn view_player_bar(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Left: Track info
        let track_info: Element<_> = if let Some(track) = &self.current_track {
            let artwork: Element<_> = if let Some(artwork_url) = &track.artwork_url {
                if let Some(handle) = self.artwork_cache.get(artwork_url) {
                    widget::image(handle.clone())
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .content_fit(cosmic::iced::ContentFit::Cover)
                        .into()
                } else {
                    widget::icon::from_name("audio-x-generic-symbolic")
                        .size(64)
                        .apply(Element::from)
                }
            } else {
                widget::icon::from_name("audio-x-generic-symbolic")
                    .size(64)
                    .apply(Element::from)
            };

            widget::row::with_capacity(2)
                .push(artwork)
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text::body(&track.title))
                        .push(widget::text::caption(&track.user.username)),
                )
                .spacing(space_s)
                .align_y(Alignment::Center)
                .into()
        } else {
            widget::text::caption("No track playing").into()
        };

        // Center: Transport controls
        let play_icon = match self.playback_status {
            PlaybackStatus::Playing => "media-playback-pause-symbolic",
            PlaybackStatus::Buffering => "content-loading-symbolic",
            _ => "media-playback-start-symbolic",
        };

        let controls = widget::row::with_capacity(3)
            .push(
                widget::button::icon(widget::icon::from_name("media-skip-backward-symbolic"))
                    .on_press(Message::PreviousTrack),
            )
            .push(
                widget::button::icon(widget::icon::from_name(play_icon))
                    .on_press(Message::TogglePlayPause)
                    .class(cosmic::theme::Button::Suggested),
            )
            .push(
                widget::button::icon(widget::icon::from_name("media-skip-forward-symbolic"))
                    .on_press(Message::NextTrack),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);

        // Status text
        let status_text = match self.playback_status {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Buffering => "Buffering...",
            PlaybackStatus::Stopped => "Stopped",
        };

        let center = widget::column::with_capacity(2)
            .push(controls)
            .push(widget::text::caption(status_text))
            .align_x(Alignment::Center);

        // Right: Volume
        let volume_control = widget::row::with_capacity(2)
            .push(
                widget::icon::from_name("audio-volume-high-symbolic")
                    .size(16)
                    .apply(Element::from),
            )
            .push(
                widget::slider(0.0..=1.0, self.volume, Message::SetVolume).width(Length::Fixed(
                    100.0,
                )),
            )
            .spacing(space_s)
            .align_y(Alignment::Center);

        let player_bar = widget::container(
            widget::row::with_capacity(3)
                .push(
                    widget::container(track_info)
                        .width(Length::FillPortion(1))
                        .align_x(Horizontal::Left),
                )
                .push(
                    widget::container(center)
                        .width(Length::FillPortion(1))
                        .align_x(Horizontal::Center),
                )
                .push(
                    widget::container(volume_control)
                        .width(Length::FillPortion(1))
                        .align_x(Horizontal::Right),
                )
                .padding(space_m)
                .align_y(Alignment::Center),
        )
        .class(cosmic::theme::Container::Card)
        .width(Length::Fill)
        .height(Length::Fixed(100.0));

        widget::container(player_bar)
            .padding([0, 0, space_s / 2, 0])
            .width(Length::Fill)
            .into()
    }

    fn view_login(&self) -> Element<'_, Message> {
        let space_m = cosmic::theme::spacing().space_m;
        let space_l = cosmic::theme::spacing().space_l;

        let content = widget::column::with_capacity(5)
            .push(widget::text::title1("Welcome to COSMIC SoundCloud"))
            .push(widget::vertical_space().height(Length::Fixed(space_l as f32)))
            .push(widget::text::body(
                "Enter your SoundCloud OAuth token to get started.\n\
                 You can find this in your browser cookies after logging into SoundCloud. You need to look through networking requests to find 'authorization' headers.",
            ))
            .push(widget::vertical_space().height(Length::Fixed(space_m as f32)))
            .push(
                widget::text_input("OAuth token (e.g., 2-310174-...)", &self.login_token_input)
                    .on_input(Message::LoginTokenInput)
                    .on_submit(|_| Message::SubmitToken)
                    .password()
                    .width(Length::Fixed(400.0)),
            )
            .push(widget::vertical_space().height(Length::Fixed(space_m as f32)))
            .push(widget::button::suggested("Login").on_press(Message::SubmitToken))
            .align_x(Alignment::Center);

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }

    fn view_loading<'a>(&'a self, message: &'a str) -> Element<'a, Message> {
        let content = widget::column::with_capacity(2)
            .push(widget::text::title3(message))
            .push(widget::text::body("Please wait..."))
            .align_x(Alignment::Center);

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }

    fn view_error<'a>(&'a self, error: &'a str) -> Element<'a, Message> {
        let space_m = cosmic::theme::spacing().space_m;

        let content = widget::column::with_capacity(4)
            .push(widget::text::title3("Authentication Failed"))
            .push(widget::text::body(error))
            .push(widget::vertical_space().height(Length::Fixed(space_m as f32)))
            .push(widget::button::standard("Try Again").on_press(Message::Logout))
            .align_x(Alignment::Center);

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }

    /// View for artist page showing artist info, albums, and tracks
    fn view_artist(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;
        let space_l = cosmic::theme::spacing().space_l;

        // Back button to library
        let back_button = widget::button::icon(icon::from_name("go-previous-symbolic"))
            .on_press(Message::NavigateToLibrary)
            .class(cosmic::theme::Button::Text);

        // Loading state
        let Some(user) = &self.artist_user else {
            return widget::column::with_capacity(2)
                .push(back_button)
                .push(self.view_loading("Loading artist..."))
                .spacing(space_m)
                .into();
        };

        // Header with artist info
        let avatar: Element<_> = if let Some(avatar_url) = &user.avatar_url {
            if let Some(handle) = self.artwork_cache.get(avatar_url) {
                widget::image(handle.clone())
                    .width(Length::Fixed(80.0))
                    .height(Length::Fixed(80.0))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .into()
            } else {
                widget::icon::from_name("avatar-default-symbolic")
                    .size(80)
                    .apply(Element::from)
            }
        } else {
            widget::icon::from_name("avatar-default-symbolic")
                .size(80)
                .apply(Element::from)
        };

        let stats_text = format!(
            "{} tracks · {} followers",
            user.track_count, user.followers_count
        );

        let header = widget::row::with_capacity(3)
            .push(back_button)
            .push(avatar)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text::title1(&user.username))
                    .push(widget::text::body(stats_text))
                    .spacing(space_s),
            )
            .spacing(space_m)
            .align_y(Alignment::Center);

        let mut content = widget::column::with_capacity(4)
            .push(header)
            .spacing(space_l)
            .width(Length::Fill);

        // Albums section (if any) - horizontally scrollable for artists with many albums
        if !self.artist_albums.is_empty() {
            let album_cards: Vec<Element<_>> = self
                .artist_albums
                .iter()
                .map(|album| self.view_album_card(album))
                .collect();

            let albums_row = widget::row::with_children(album_cards).spacing(space_m);
            let albums_scrollable = widget::scrollable::horizontal(albums_row);

            let albums_section = widget::column::with_capacity(2)
                .push(widget::text::title3("Albums"))
                .push(albums_scrollable)
                .spacing(space_s);

            content = content.push(albums_section);
        }

        // Tracks section
        let tracks_section = if self.artist_tracks.items.is_empty() && self.artist_tracks.loading {
            widget::column::with_capacity(2)
                .push(widget::text::title3("Tracks"))
                .push(self.view_loading("Loading tracks..."))
                .spacing(space_s)
        } else if self.artist_tracks.items.is_empty() {
            widget::column::with_capacity(2)
                .push(widget::text::title3("Tracks"))
                .push(widget::text::body("No tracks found."))
                .spacing(space_s)
        } else {
            // Clone the full track list for playlist context
            let playlist = self.artist_tracks.items.clone();
            let track_items: Vec<Element<_>> = self
                .artist_tracks
                .items
                .iter()
                .enumerate()
                .map(|(idx, track)| self.view_track_item_in_playlist(track, playlist.clone(), idx))
                .collect();

            let mut tracks = widget::column::with_children(track_items).spacing(space_s);

            // Load more button
            if self.artist_tracks.next_href.is_some() {
                tracks = tracks.push(widget::vertical_space().height(Length::Fixed(8.0)));
                tracks = tracks.push(
                    widget::button::text(if self.artist_tracks.loading {
                        "Loading..."
                    } else {
                        "Load More"
                    })
                    .on_press_maybe(if self.artist_tracks.loading {
                        None
                    } else {
                        Some(Message::LoadMoreArtistTracks)
                    }),
                );
            }

            widget::column::with_capacity(2)
                .push(widget::text::title3("Tracks"))
                .push(tracks)
                .spacing(space_s)
        };

        content = content.push(tracks_section);

        // Add padding - right padding for scrollbar, bottom padding for player bar clearance
        let padded_content = widget::container(content)
            .padding([space_m as u16, space_m as u16, 120, space_m as u16]);

        widget::scrollable(padded_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// View for an album card - clicking plays the album
    fn view_album_card(&self, album: &Album) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let album_id = album.id;

        let artwork: Element<_> = if let Some(artwork_url) = &album.artwork_url {
            if let Some(handle) = self.artwork_cache.get(artwork_url) {
                widget::image(handle.clone())
                    .width(Length::Fixed(80.0))
                    .height(Length::Fixed(80.0))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .into()
            } else {
                widget::icon::from_name("folder-music-symbolic")
                    .size(80)
                    .apply(Element::from)
            }
        } else {
            widget::icon::from_name("folder-music-symbolic")
                .size(80)
                .apply(Element::from)
        };

        // Clone values to avoid lifetime issues
        let title_text = album.title.clone();
        let title = widget::text::body(title_text).width(Length::Fixed(80.0));

        let release_info: Element<_> = if let Some(release_date) = &album.release_date {
            // Extract year from date string (e.g., "2024-01-15" -> "2024")
            let year = release_date
                .split('-')
                .next()
                .unwrap_or(release_date)
                .to_string();
            widget::text::caption(year).into()
        } else {
            widget::text::caption(format!("{} tracks", album.track_count)).into()
        };

        let card_content = widget::column::with_capacity(3)
            .push(artwork)
            .push(title)
            .push(release_info)
            .spacing(space_s);

        // Wrap in a button to make clickable
        widget::button::custom(card_content)
            .on_press(Message::PlayAlbum(album_id))
            .class(cosmic::theme::Button::Text)
            .padding(space_s)
            .into()
    }

    fn view_library(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Tab bar (full width, evenly distributed, centered labels)
        let tabs = widget::segmented_button::horizontal(&self.tab_model)
            .on_activate(Message::SwitchTab)
            .spacing(space_s)
            .width(Length::Fill)
            .button_alignment(Alignment::Center);

        // Tab content
        let tab_content = match self.current_tab {
            LibraryTab::Overview => self.view_overview(),
            LibraryTab::Likes => self.view_likes(),
            LibraryTab::History => self.view_history(),
            _ => self.view_coming_soon(),
        };

        widget::column::with_capacity(2)
            .push(tabs)
            .push(
                widget::container(tab_content)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(space_m),
            )
            .spacing(space_s)
            .into()
    }

    fn view_overview(&self) -> Element<'_, Message> {
        let space_m = cosmic::theme::spacing().space_m;

        let mut content = widget::column::with_capacity(4);

        if let Some(user) = &self.current_user {
            let welcome_text = format!("Welcome, {}!", user.username);
            content = content
                .push(widget::text::title2(welcome_text))
                .push(widget::vertical_space().height(Length::Fixed(space_m as f32)));

            let stats = widget::row::with_capacity(4)
                .push(self.view_stat_owned("Tracks", user.track_count))
                .push(self.view_stat_owned("Playlists", user.playlist_count))
                .push(self.view_stat_owned("Followers", user.followers_count))
                .push(self.view_stat_owned("Following", user.followings_count))
                .spacing(space_m);

            content = content.push(stats);
        }

        content.into()
    }

    fn view_stat_owned(&self, label: &'static str, count: u32) -> Element<'_, Message> {
        widget::column::with_capacity(2)
            .push(widget::text::title3(count.to_string()))
            .push(widget::text::caption(label))
            .align_x(Alignment::Center)
            .into()
    }

    fn view_likes(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        if self.likes.loading && self.likes.items.is_empty() {
            return self.view_loading("Loading likes...");
        }

        if self.likes.items.is_empty() {
            return widget::text::body("No liked tracks yet.").into();
        }

        // Clone the full track list for playlist context
        let playlist = self.likes.items.clone();
        let tracks: Vec<Element<_>> = self
            .likes
            .items
            .iter()
            .enumerate()
            .map(|(idx, track)| self.view_track_item_in_playlist(track, playlist.clone(), idx))
            .collect();

        let mut content = widget::column::with_children(tracks).spacing(space_s);

        // Load more button
        if self.likes.next_href.is_some() {
            content = content.push(widget::vertical_space().height(Length::Fixed(8.0)));
            content = content.push(
                widget::button::text(if self.likes.loading {
                    "Loading..."
                } else {
                    "Load More"
                })
                .on_press_maybe(if self.likes.loading {
                    None
                } else {
                    Some(Message::LoadMoreLikes)
                }),
            );
        }

        // Add bottom padding for player bar clearance and right padding for scrollbar
        let padded_content = widget::container(content)
            .padding([0, space_m as u16, 120, 0]);

        widget::scrollable(padded_content)
            .on_scroll(Message::LikesScrolled)
            .into()
    }

    fn view_history(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        if self.history.loading && self.history.items.is_empty() {
            return self.view_loading("Loading history...");
        }

        if self.history.items.is_empty() {
            return widget::text::body("No listening history yet.").into();
        }

        // Clone the full track list for playlist context
        let playlist = self.history.items.clone();
        let tracks: Vec<Element<_>> = self
            .history
            .items
            .iter()
            .enumerate()
            .map(|(idx, track)| self.view_track_item_in_playlist(track, playlist.clone(), idx))
            .collect();

        let content = widget::column::with_children(tracks).spacing(space_s);

        // Add bottom padding for player bar clearance and right padding for scrollbar
        let padded_content = widget::container(content)
            .padding([0, space_m as u16, 120, 0]);

        widget::scrollable(padded_content).into()
    }

    /// Render a track item. If playlist_context is Some, clicking plays in playlist context.
    fn view_track_item_in_playlist(
        &self,
        track: &Track,
        playlist: Vec<Track>,
        index: usize,
    ) -> Element<'_, Message> {
        self.view_track_item_inner(track, Some((playlist, index)))
    }

    fn view_track_item_inner(
        &self,
        track: &Track,
        playlist_context: Option<(Vec<Track>, usize)>,
    ) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        // Highlight currently playing track
        let is_playing = self
            .current_track
            .as_ref()
            .is_some_and(|t| t.id == track.id);

        // Get artwork element
        let artwork: Element<_> = if let Some(artwork_url) = &track.artwork_url {
            if let Some(handle) = self.artwork_cache.get(artwork_url) {
                widget::image(handle.clone())
                    .width(Length::Fixed(48.0))
                    .height(Length::Fixed(48.0))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .into()
            } else {
                // Show placeholder while loading
                let icon_name = if is_playing && self.playback_status == PlaybackStatus::Playing {
                    "media-playback-start-symbolic"
                } else {
                    "audio-x-generic-symbolic"
                };
                widget::icon::from_name(icon_name)
                    .size(48)
                    .apply(Element::from)
            }
        } else {
            // No artwork URL - show icon
            let icon_name = if is_playing && self.playback_status == PlaybackStatus::Playing {
                "media-playback-start-symbolic"
            } else {
                "audio-x-generic-symbolic"
            };
            widget::icon::from_name(icon_name)
                .size(48)
                .apply(Element::from)
        };

        let title = track.title.clone();
        let duration_text = track.duration_formatted();
        let track_clone = track.clone();

        // Make artist name clickable
        let user_id = track.user.id;
        let username = track.user.username.clone();
        let avatar_url = track.user.avatar_url.clone();

        // Use contrasting text colors when track is playing (accent background)
        let (title_element, artist_element, duration_element): (Element<_>, Element<_>, Element<_>) =
            if is_playing {
                // Use on_accent color for text on accent background
                let on_accent_style = cosmic::style::Text::Custom(|theme| {
                    cosmic::iced_widget::text::Style {
                        color: Some(theme.cosmic().on_accent_color().into()),
                    }
                });

                let title_text = widget::text::body(title).class(on_accent_style);
                let artist_text =
                    widget::text::caption(username.clone()).class(on_accent_style);
                let duration_text_widget =
                    widget::text::caption(duration_text).class(on_accent_style);

                // Wrap artist in a button that looks like text
                let artist_btn = widget::button::custom(artist_text)
                    .on_press(Message::NavigateToArtist(user_id, username, avatar_url))
                    .class(cosmic::theme::Button::Text)
                    .padding(0);

                (title_text.into(), artist_btn.into(), duration_text_widget.into())
            } else {
                // Normal styling
                let title_text = widget::text::body(title);
                let artist_btn = widget::button::text(username.clone())
                    .on_press(Message::NavigateToArtist(user_id, username, avatar_url))
                    .class(cosmic::theme::Button::Link)
                    .padding(0);
                let duration_text_widget = widget::text::caption(duration_text);

                (title_text.into(), artist_btn.into(), duration_text_widget.into())
            };

        let info = widget::column::with_capacity(2)
            .push(title_element)
            .push(artist_element);

        let duration = duration_element;

        // Play button - if playlist context is provided, play in playlist mode
        let play_message = if let Some((playlist, idx)) = playlist_context {
            Message::PlayTrackInPlaylist(track_clone, playlist, idx)
        } else {
            Message::PlayTrack(track_clone)
        };

        let play_button = widget::button::custom(artwork)
            .on_press(play_message)
            .class(cosmic::theme::Button::Text)
            .padding(0);

        widget::container(
            widget::row::with_capacity(4)
                .push(play_button)
                .push(info)
                .push(widget::horizontal_space())
                .push(duration)
                .spacing(space_s)
                .align_y(Alignment::Center),
        )
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            cosmic::iced_widget::container::Style {
                background: if is_playing {
                    Some(cosmic::iced::Background::Color(
                        cosmic.accent_color().into(),
                    ))
                } else {
                    None
                },
                border: cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_s.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .padding(space_s)
        .width(Length::Fill)
        .into()
    }

    fn view_coming_soon(&self) -> Element<'_, Message> {
        let content = widget::column::with_capacity(2)
            .push(widget::text::title3("Coming Soon"))
            .push(widget::text::body("This feature is not yet implemented."))
            .align_x(Alignment::Center);

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }

    /// View for the search page
    fn view_search(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Search input
        let search_input = widget::text_input(fl!("search-artists"), &self.search_query)
            .on_input(Message::SearchQueryInput)
            .on_submit(|_| Message::SubmitSearch)
            .width(Length::Fill);

        let search_button = widget::button::suggested(fl!("search"))
            .on_press(Message::SubmitSearch);

        let search_bar = widget::row::with_capacity(2)
            .push(search_input)
            .push(search_button)
            .spacing(space_s)
            .align_y(Alignment::Center);

        // Results
        let results_content: Element<_> = if self.search_results.loading
            && self.search_results.items.is_empty()
        {
            self.view_loading("Loading...")
        } else if self.search_results.items.is_empty() && !self.search_query.is_empty() {
            widget::text::body(fl!("no-results")).into()
        } else if self.search_results.items.is_empty() {
            widget::text::body("Enter a search term to find artists.").into()
        } else {
            let user_items: Vec<Element<_>> = self
                .search_results
                .items
                .iter()
                .map(|user| self.view_user_search_result(user))
                .collect();

            let mut results = widget::column::with_children(user_items).spacing(space_s);

            // Load more button
            if self.search_results.next_href.is_some() {
                results = results.push(widget::vertical_space().height(Length::Fixed(8.0)));
                results = results.push(
                    widget::button::text(if self.search_results.loading {
                        "Loading..."
                    } else {
                        "Load More"
                    })
                    .on_press_maybe(if self.search_results.loading {
                        None
                    } else {
                        Some(Message::LoadMoreSearchResults)
                    }),
                );
            }

            // Add bottom padding for player bar clearance
            let padded_results =
                widget::container(results).padding([0, space_m as u16, 120, 0]);

            widget::scrollable(padded_results)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        widget::column::with_capacity(2)
            .push(search_bar)
            .push(results_content)
            .spacing(space_m)
            .padding(space_m)
            .into()
    }

    /// View for a user search result item
    fn view_user_search_result(&self, user: &User) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        let avatar: Element<_> = if let Some(avatar_url) = &user.avatar_url {
            if let Some(handle) = self.artwork_cache.get(avatar_url) {
                widget::image(handle.clone())
                    .width(Length::Fixed(48.0))
                    .height(Length::Fixed(48.0))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .into()
            } else {
                widget::icon::from_name("avatar-default-symbolic")
                    .size(48)
                    .apply(Element::from)
            }
        } else {
            widget::icon::from_name("avatar-default-symbolic")
                .size(48)
                .apply(Element::from)
        };

        let stats_text = format!(
            "{} tracks · {} followers",
            user.track_count, user.followers_count
        );

        // Clone user data to avoid lifetime issues
        let username_display = user.username.clone();
        let user_id = user.id;
        let username = user.username.clone();
        let avatar_url = user.avatar_url.clone();

        let info = widget::column::with_capacity(2)
            .push(widget::text::body(username_display))
            .push(widget::text::caption(stats_text));

        widget::button::custom(
            widget::row::with_capacity(2)
                .push(avatar)
                .push(info)
                .spacing(space_s)
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .on_press(Message::NavigateToArtist(user_id, username, avatar_url))
        .class(cosmic::theme::Button::Text)
        .padding(space_s)
        .width(Length::Fill)
        .into()
    }

    /// View for the recommendations page
    fn view_recommendations(&self) -> Element<'_, Message> {
        let space_m = cosmic::theme::spacing().space_m;

        let header = widget::text::title2(fl!("recommendations"));

        let content: Element<_> = if self.recommendations_loading && self.recommendations.is_empty()
        {
            self.view_loading("Loading...")
        } else if self.recommendations.is_empty() {
            widget::text::body("No recommendations available.").into()
        } else {
            // Grid of playlist cards - build rows of 4
            let mut rows: Vec<Element<_>> = Vec::new();
            let playlists: Vec<_> = self.recommendations.iter().collect();

            for chunk in playlists.chunks(4) {
                let mut row = widget::row::with_capacity(4).spacing(space_m);
                for playlist in chunk {
                    row = row.push(self.view_playlist_card(playlist));
                }
                // Fill remaining space if less than 4 items
                for _ in chunk.len()..4 {
                    row = row.push(widget::horizontal_space());
                }
                rows.push(row.into());
            }

            let grid = widget::column::with_children(rows).spacing(space_m);

            // Add bottom padding for player bar clearance
            let padded_grid = widget::container(grid).padding([0, space_m as u16, 120, 0]);

            widget::scrollable(padded_grid)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        widget::column::with_capacity(2)
            .push(header)
            .push(content)
            .spacing(space_m)
            .padding(space_m)
            .into()
    }

    /// View for a playlist card
    fn view_playlist_card(&self, playlist: &Playlist) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let playlist_id = playlist.id;

        let artwork: Element<_> = if let Some(artwork_url) = &playlist.artwork_url {
            if let Some(handle) = self.artwork_cache.get(artwork_url) {
                widget::image(handle.clone())
                    .width(Length::Fixed(120.0))
                    .height(Length::Fixed(120.0))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .into()
            } else {
                widget::icon::from_name("folder-music-symbolic")
                    .size(120)
                    .apply(Element::from)
            }
        } else {
            widget::icon::from_name("folder-music-symbolic")
                .size(120)
                .apply(Element::from)
        };

        // Clone data to avoid lifetime issues
        let title_text = playlist.title.clone();
        let track_count = playlist.track_count;

        let title = widget::text::body(title_text).width(Length::Fixed(120.0));
        let subtitle = widget::text::caption(format!("{track_count} tracks"));

        let card_content = widget::column::with_capacity(3)
            .push(artwork)
            .push(title)
            .push(subtitle)
            .spacing(space_s)
            .width(Length::Fixed(120.0));

        widget::button::custom(card_content)
            .on_press(Message::PlayPlaylist(playlist_id))
            .class(cosmic::theme::Button::Text)
            .padding(space_s)
            .into()
    }
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
