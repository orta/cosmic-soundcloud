// SPDX-License-Identifier: MPL-2.0

use crate::api::{SoundCloudClient, Track, User};
use crate::audio::{open_in_browser, AudioCommand, AudioEvent, AudioPlayer};
use crate::config::Config;
use crate::fl;
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
    StopPlayback,
    NextTrack,
    PreviousTrack,
    SetVolume(f32),

    // Artwork
    LoadArtwork(String),
    ArtworkLoaded(String, Vec<u8>),
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

        // Check if we have a saved token
        let (auth_state, api_client) = if config.has_token() {
            let token = config.oauth_token.clone().unwrap();
            (
                AuthState::Authenticating,
                Some(SoundCloudClient::new(token)),
            )
        } else {
            (AuthState::NotAuthenticated, None)
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
        };

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
            let user_info = widget::row::with_capacity(2)
                .push(
                    widget::icon::from_name("avatar-default-symbolic")
                        .size(24)
                        .apply(Element::from),
                )
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
        let main_content = if !self.config.has_token() {
            self.view_login()
        } else {
            match &self.auth_state {
                AuthState::Authenticated => self.view_main_layout(),
                AuthState::Authenticating => self.view_loading("Authenticating..."),
                AuthState::Failed(err) => self.view_error(err),
                AuthState::NotAuthenticated => self.view_login(),
            }
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

        // Audio player subscription - spawn once and keep running
        if self.audio_cmd_tx.is_none() {
            subscriptions.push(Subscription::run(|| {
                iced_futures::stream::channel(32, |mut emitter| async move {
                    let (cmd_tx, mut evt_rx) = AudioPlayer::spawn();

                    // Send the command channel back to the app
                    let _ = emitter.send(Message::AudioReady(cmd_tx)).await;

                    // Forward audio events
                    while let Some(event) = evt_rx.recv().await {
                        let _ = emitter.send(Message::AudioEvent(event)).await;
                    }
                })
            }));
        }

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
                if !token.is_empty() {
                    self.auth_state = AuthState::Authenticating;
                    self.api_client = Some(SoundCloudClient::new(token.clone()));

                    // Save token to config
                    self.config.oauth_token = Some(token);
                    if let Ok(config_context) =
                        cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                    {
                        let _ = self.config.write_entry(&config_context);
                    }

                    // Fetch user info
                    let client = self.api_client.clone().unwrap();
                    return cosmic::task::future(async move {
                        match client.get_me().await {
                            Ok(user) => Message::UserLoaded(Ok(user)),
                            Err(e) => Message::UserLoaded(Err(e.to_string())),
                        }
                    })
                    .map(cosmic::Action::App);
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

                // Clear saved token
                self.config.oauth_token = None;
                if let Ok(config_context) =
                    cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                {
                    let _ = self.config.write_entry(&config_context);
                }
            }

            Message::UserLoaded(result) => match result {
                Ok(user) => {
                    self.current_user = Some(user);
                    self.auth_state = AuthState::Authenticated;
                    return cosmic::task::message(cosmic::Action::App(Message::LoadLikes));
                }
                Err(err) => {
                    self.auth_state = AuthState::Failed(err);
                    self.api_client = None;
                }
            },

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
                if let Some(artwork_url) = &track.artwork_url {
                    if !self.artwork_cache.contains_key(artwork_url) && !self.artwork_loading.contains(artwork_url) {
                        tasks.push(cosmic::task::message(cosmic::Action::App(Message::LoadArtwork(artwork_url.clone()))));
                    }
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
                    return cosmic::task::message(cosmic::Action::App(Message::NextTrack));
                }
                AudioEvent::Error(err) => {
                    eprintln!("Audio error: {err}");
                    self.playback_status = PlaybackStatus::Stopped;
                }
                AudioEvent::DrmProtected { drm_type, track_url } => {
                    eprintln!("DRM-protected content ({drm_type}) - opening in browser");
                    self.playback_status = PlaybackStatus::Stopped;
                    if !track_url.is_empty() {
                        if let Err(e) = open_in_browser(&track_url) {
                            eprintln!("Failed to open browser: {e}");
                        }
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

            Message::StopPlayback => {
                if let Some(tx) = &self.audio_cmd_tx {
                    let _ = tx.blocking_send(AudioCommand::Stop);
                }
                self.playback_status = PlaybackStatus::Stopped;
                self.current_track = None;
            }

            Message::NextTrack => {
                if !self.current_playlist.is_empty() {
                    let next_index = (self.playlist_index + 1) % self.current_playlist.len();
                    if next_index != 0 || self.config.repeat_mode == crate::config::RepeatMode::All
                    {
                        let track = self.current_playlist[next_index].clone();
                        let playlist = self.current_playlist.clone();
                        return cosmic::task::message(cosmic::Action::App(
                            Message::PlayTrackInPlaylist(track, playlist, next_index),
                        ));
                    } else {
                        // End of playlist
                        self.playback_status = PlaybackStatus::Stopped;
                    }
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
        }
        Task::none()
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
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

    /// Main layout with library content and player bar at bottom
    fn view_main_layout(&self) -> Element<'_, Message> {
        widget::column::with_capacity(2)
            .push(
                widget::container(self.view_library())
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

        widget::container(
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
        .height(Length::Fixed(100.0))
        .into()
    }

    fn view_login(&self) -> Element<'_, Message> {
        let space_m = cosmic::theme::spacing().space_m;
        let space_l = cosmic::theme::spacing().space_l;

        let content = widget::column::with_capacity(5)
            .push(widget::text::title1("Welcome to Cosmic SoundCloud"))
            .push(widget::vertical_space().height(Length::Fixed(space_l as f32)))
            .push(widget::text::body(
                "Enter your SoundCloud OAuth token to get started.\n\
                 You can find this in your browser cookies after logging into SoundCloud.",
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

    fn view_library(&self) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;
        let space_m = cosmic::theme::spacing().space_m;

        // Tab bar
        let tabs =
            widget::segmented_button::horizontal(&self.tab_model).on_activate(Message::SwitchTab);

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

        let tracks: Vec<Element<_>> = self
            .likes
            .items
            .iter()
            .map(|track| self.view_track_item(track))
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

        widget::scrollable(padded_content).into()
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

        let tracks: Vec<Element<_>> = self
            .history
            .items
            .iter()
            .map(|track| self.view_track_item(track))
            .collect();

        let content = widget::column::with_children(tracks).spacing(space_s);

        // Add bottom padding for player bar clearance and right padding for scrollbar
        let padded_content = widget::container(content)
            .padding([0, space_m as u16, 120, 0]);

        widget::scrollable(padded_content).into()
    }

    fn view_track_item(&self, track: &Track) -> Element<'_, Message> {
        let space_s = cosmic::theme::spacing().space_s;

        // Highlight currently playing track
        let is_playing = self
            .current_track
            .as_ref()
            .map_or(false, |t| t.id == track.id);

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
        let username = track.user.username.clone();
        let duration_text = track.duration_formatted();
        let track_clone = track.clone();

        let info = widget::column::with_capacity(2)
            .push(widget::text::body(title))
            .push(widget::text::caption(username));

        let duration = widget::text::caption(duration_text);

        widget::button::custom(
            widget::row::with_capacity(4)
                .push(artwork)
                .push(info)
                .push(widget::horizontal_space())
                .push(duration)
                .spacing(space_s)
                .align_y(Alignment::Center),
        )
        .on_press(Message::PlayTrack(track_clone))
        .width(Length::Fill)
        .class(if is_playing {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Text
        })
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
}

/// The page to display in the application.
pub enum Page {
    Library,
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
