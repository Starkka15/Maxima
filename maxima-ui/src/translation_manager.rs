// Handles different languages for the UI

use log::error;
use serde::Deserialize;
use serde_json;
use std::fs;

#[derive(Deserialize)]
pub struct LocalizedStrings {
    pub errors: LocalizedGenericErrors,
    pub menubar: LocalizedMenubar,
    pub login: LocalizedLoginView,
    pub profile_menu: LocalizedProfileMenu,
    pub games_view: LocalizedGamesView,
    pub friends_view: LocalizedFriendsView,
}

#[derive(Deserialize)]
pub struct LocalizedGenericErrors {
    pub view_not_impl: String,
    pub critical_thread_crashed: String,
}

#[derive(Deserialize)]
pub struct LocalizedMenubar {
    pub games: String,
    pub store: String,
    pub friends: String,
    pub settings: String,
}

#[derive(Deserialize)]
pub struct LocalizedProfileMenu {
    pub view_profile: String,
    pub view_wishlist: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesView {
    pub toolbar: LocalizedGamesViewToolbar,
    pub main: LocalizedGamesViewMain,
    pub details: LocalizedGamesViewDetails,
}

#[derive(Deserialize)]
pub struct LocalizedLoginView {
    pub oauth_option: String,
    pub credentials_option: String,
    pub username_box_hint: String,
    pub password_box_hint: String,
    pub credential_confirm: String,
    pub credential_waiting: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewToolbar {
    pub genre_filter: String,
    pub genre_options: LocalizedGamesViewToolbarGenreOptions,
    pub platform_filter: String,
    pub platform_options: LocalizedGamesViewToolbarPlatformOptions,
    pub search_bar_hint: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewToolbarGenreOptions {
    pub all: String,
    pub shooter: String,
    pub simulation: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewToolbarPlatformOptions {
    pub all: String,
    pub windows: String,
    pub mac: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewMain {
    pub play: String,
    pub uninstall: String,
    pub settings: String,
    pub playtime: String,
    pub achievements: String,
    pub no_loaded_games: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewDetails {
    pub min_system_req: String,
    pub rec_system_req: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsView {
    pub toolbar: LocalizedFriendsViewToolbar,
    pub status_online: String,
    pub status_offline: String,
    pub prepend: bool,
    pub status_playing: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewToolbar {
    pub online: String,
    pub all: String,
    pub pending: String,
    pub blocked: String,
    pub add_friend: String,
    pub filter_options: LocalizedFriendsViewToolbarSearchFilterOptions,
    pub search_hint: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewToolbarSearchFilterOptions {
    pub name: String,
    pub game: String,
}

#[derive(Deserialize)]
pub struct Lang {
    pub en_us: LocalizedStrings,
}
pub struct TranslationManager {
    pub localization: LocalizedStrings,
}

impl TranslationManager {
    pub fn new(path: String) -> Option<Self> {
        if let Ok(file) = fs::read_to_string(path) {
            let s: LocalizedStrings = serde_json::from_str(&file).unwrap();
            Some(Self { localization: s })
        } else {
            error!("Couldn't read locale file!");
            None
        }
    }
}
