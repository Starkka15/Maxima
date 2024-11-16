// Handles different languages for the UI

use log::error;
use serde::Deserialize;
use serde_json;
use std::fs;
use sys_locale;

use crate::FrontendLanguage;

#[derive(Deserialize)]
pub struct LocalizedStrings {
    /// Generic app-wide errors
    pub errors: LocalizedGenericErrors,
    /// Messages and buttons shown before the app gets to the main interface/library
    pub startup_flow: LocalizedStartupFlow,
    /// Tabs for the page switcher
    pub menubar: LocalizedMenubar,
    pub profile_menu: LocalizedProfileMenu,
    /// Library page
    pub games_view: LocalizedGamesView,
    /// Friends sidebar
    pub friends_view: LocalizedFriendsView,
    /// Settings page
    pub settings_view: LocalizedSettingsView,
    /// Names of languages
    pub locale: LocalizedLocaleInfo,
    pub modals: LocalizedModals,
}

#[derive(Deserialize)]
pub struct LocalizedModals {
    /// The close button in the top right, makes the modal go away
    pub close: String,
    /// The modal shown when installing a game
    pub game_install: LocalizedGameInstallModal,
    /// The modal shown when changing settings for a game
    pub game_settings: LocalizedGameSettingsModal,
}

#[derive(Deserialize)]
pub struct LocalizedGameInstallModal {
    pub header: String,
    /// Label for the box to enter the path of an existing game install
    pub locate_installed: String,
    /// Button that initiates locating
    pub locate_action: String,
    /// Text informing the user that maxima is locating the game
    pub locate_in_progress: String,
    /// Text informing the user that the locate failed, english-only instructions always accompany it
    pub locate_failed: String,
    /// Label for the box to enter the path to install to
    pub fresh_download: String,
    /// Confirms the path the game is to be installed to
    pub fresh_path_confirmation: String,
    /// Informs the user the path they're trying to locate a game at is invalid
    pub fresh_path_invalid: String,
    /// Button that initiates the download
    pub fresh_action: String,
    
}

#[derive(Deserialize)]
pub struct LocalizedGameSettingsModal {
    pub header: String,
    /// Warning that the game is not installed, it will be the only thing in the modal if shown
    pub not_installed: String,
    /// Checkbox to enable/disable cloud saves
    pub cloud_saves: String,
    /// Label for a text box to enter command-line arguments
    pub launch_arguments: String,
    /// Label for a text box to contain the full path to the EXE to run instead
    pub executable_override: String,
    /// Button that initates uninstallation
    pub uninstall: String,
}

#[derive(Deserialize)]
pub struct LocalizedLocaleInfo {
    /// Let the system choose
    pub default: String,
    /// English (US)
    pub en_us: String, // I will deny all PRs that attempt to add british english.
}

#[derive(Deserialize)]
pub struct LocalizedStartupFlow {
    /// Shown alongside a throbber, keep the user at bay while maxima sets itself up
    pub starting: String,
    /// Shown alongside a throbber, let the user maxima is logging in
    pub logging_in: String,
    /// Warning the user they're not logged in
    pub login_header: String,
    /// Button that initiates login flow (through the browser)
    pub login_button: String,
    /// Warning the user the windows service is not installed
    pub service_installer_header: String,
    /// Describes what the windows service does, and that it's needed for maxima to work
    pub service_installer_description: String,
    /// Button that initiates windows service installation
    pub service_installer_button: String
}

#[derive(Deserialize)]
pub struct LocalizedGenericErrors {
    /// Informs the user that the page does not exist, the catch-all for unimplemented pages, could be a bug but more than likely just WIP
    pub view_not_impl: String,
    /// Informs the user that the page is planned to be implemented, but is currently not
    pub view_coming_soon: String,
    /// Warns the user that the backend crashed, if this is shown, the frontend is disabled and can do nothing but be closed
    pub critical_thread_crashed: String,
}

#[derive(Deserialize)]
pub struct LocalizedMenubar {
    pub games: String,
    pub store: String,
    pub friends: String,
    pub settings: String,
    pub downloads: String,
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
    /// Extra details about the game (DLC, system requirements, etc/TBC)
    pub details: LocalizedGamesViewDetails,
}

#[derive(Deserialize)]
pub struct LocalizedSettingsView {
    /// Label for a section of settings pertaining to the appearance or functionality of the UI
    pub interface: LocalizedInterfaceSettings,
    /// Label for a section of settings pertaining to the installation of games
    pub game_installation: LocalizedGameInstallationSettings,
    /// Label for a section of settings pertaining to performance of the launcher
    pub performance: LocalizedPerformanceSettings,
}

#[derive(Deserialize)]
pub struct LocalizedInterfaceSettings {
    pub header: String,
    /// Label for a combo box to select the frontend's language
    pub language: String,
}

#[derive(Deserialize)]
pub struct LocalizedGameInstallationSettings {
    pub header: String,
    /// Label for a text box for a default path to install games
    pub default_folder: String,
}

#[derive(Deserialize)]
pub struct LocalizedPerformanceSettings {
    pub header: String,
    /// Label for a checkbox to disable blur effects
    pub disable_blur: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewToolbar {
    /// Legacy, Label for the genre combo box
    pub genre_filter: String,
    /// Options for a genre combo box
    pub genre_options: LocalizedGamesViewToolbarGenreOptions,
    /// Legacy, Label for the platform combo box
    pub platform_filter: String,
    /// Options for a platform combo box
    pub platform_options: LocalizedGamesViewToolbarPlatformOptions,
    /// Displayed in the search bar when empty
    pub search_bar_hint: String,
    /// Appended after the title of a running game
    pub running_suffix: String,
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
    /// Button to start the game if it's installed
    pub play: String,
    /// Button to stop the game if it's running
    pub stop: String,
    /// Button to install the game
    pub install: String,
    /// Legacy/TBC? Button to uninstall the game if the user cannot play it anymore (lapsed subscription, delisted, etc)
    pub uninstall: String,
    /// Button to Pause the download
    pub pause: String,
    /// Button to Resume the download
    pub resume: String,
    /// Button that opens the settings modal
    pub settings: String,
    /// Label succeeded by the amount of hours/minutes the user has played the game
    pub playtime: String,
    /// Label succeeded by the amount of achievements the user has, "unlocked/total"
    pub achievements: String,
    /// Informs the user that they either have no games, or the backend is loading them. There is currently no way to differentiate those
    pub no_loaded_games: String,
}

#[derive(Deserialize)]
pub struct LocalizedGamesViewDetails {
    /// Minimum system requirements to run the game
    pub min_system_req: String,
    /// Recommended specs for a good experience
    pub rec_system_req: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsView {
    pub toolbar: LocalizedFriendsViewToolbar,
    /// Buttons underneath a friend in the list
    pub friend_actions: LocalizedFriendsViewFriendActions,
    /// Text below the username describing what they're doing
    pub status: LocalizedFriendsViewStatus,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewToolbar {
    /// Friends that are online
    pub online: String,
    /// All friends
    pub all: String,
    /// Pending friend requests (TBC)
    pub pending: String,
    /// Blocked users (TBC)
    pub blocked: String,
    pub add_friend: String,
    pub filter_options: LocalizedFriendsViewToolbarSearchFilterOptions,
    pub search_hint: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewFriendActions {
    /// Button to view the user's profile
    pub profile: String,
    /// Button to open a chat with the user
    pub chat: String,
    /// Remove the user from your friends list
    pub unfriend: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewStatus {
    pub unknown: String,
    pub do_not_disturb: String,
    pub away: String,
    pub online: String,
    pub offline: String,
    /// The user is playing a game
    pub presence_basic: String,
    /// The user is playing a game that has extra presence information
    pub presence_rich: String,
}

#[derive(Deserialize)]
pub struct LocalizedFriendsViewToolbarSearchFilterOptions {
    /// Filter/search by their name
    pub name: String,
    /// Filter/search by the game they're playing
    pub game: String,
}

pub struct TranslationManager {
    pub localization: LocalizedStrings,
}

macro_rules! language_include_matcher {
    (
        $match_var:expr, $fallback_var:expr;
        $($name:expr => $file:expr),* $(,)?
    ) => {
        match $match_var {
            $(
                $name => include_str!(concat!("../res/locale/", $file, ".json")),
            )*
            _ => $fallback_var,
        }
    };
}

/// shorthand for .replace(), sean thinks it's cleaner and won't shut up about it
macro_rules! positional_replace {
    ($key:expr, $($name:expr, $value:expr),*) => {
        {
            let mut temp_string = $key.to_string();
            $(
                temp_string = temp_string.replace(concat!("{", $name, "}"), &$value.to_string());
            )*
            temp_string
        }
    };
}

pub(crate) use positional_replace;

impl TranslationManager {
    /// Gets an instance of LocalizedStrings for the specified locale code
    pub fn get_for_locale(code: &str) -> LocalizedStrings {
        let english = include_str!("../res/locale/en_us.json").to_owned();
        let locale_json: &str = language_include_matcher!(code, &english;
            "en-US" => "en_us",
            //"de-bug" => "de_bug"
        );

        serde_json::from_str(locale_json).unwrap()
    }
    
    pub fn new(lang: &FrontendLanguage) -> Self {
        let locale = match lang {
            FrontendLanguage::SystemDefault => {
                let locale: Option<String> = sys_locale::get_locale();
                if let Some(code) = locale { code } else { "en-US".to_owned() }
            },
            FrontendLanguage::EnUS => "en-US".to_owned(),
        };
        
        let s = Self::get_for_locale(locale.as_str());
        Self { localization: s }
    }

    pub fn new_with(code: &str) -> Self {
        Self { localization: Self::get_for_locale(code) }
    }
}