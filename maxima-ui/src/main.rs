#![feature(slice_pattern)]
use anyhow::bail;
use clap::{arg, command, Parser};

use desktop::check_desktop_icon;
use egui::{pos2, Align2, FontId, IconData, Layout, ViewportBuilder, Widget};
use egui::style::{ScrollStyle, Spacing};
use egui::Style;
use log::{error, warn};
use maxima::core::library::OwnedOffer;
use strum_macros::EnumIter;
use views::downloads_view::{downloads_view, QueuedDownload};
use views::undefinied_view::coming_soon_view;
use std::collections::HashMap;
use std::default::Default;
use std::path::PathBuf;
use std::{ops::RangeInclusive, rc::Rc, sync::Arc};
use ui_image::UIImageCache;
use views::friends_view::UIFriend;

use eframe::egui_glow;
use egui::{
    style::{WidgetVisuals, Widgets},
    vec2, Color32, FontData, FontDefinitions, FontFamily, Margin, Rect, Response, Rounding, Stroke,
    TextureId, Ui, Vec2, Visuals,
};
use egui_extras::{Size, StripBuilder};
use egui_glow::glow;

use bridge_thread::{BridgeThread, InteractThreadLocateGameResponse};

use app_bg_renderer::AppBgRenderer;
use game_view_bg_renderer::GameViewBgRenderer;
use renderers::app_bg_renderer;
use renderers::game_view_bg_renderer;
use translation_manager::{positional_replace, TranslationManager};
use views::friends_view::{FriendsViewBar, FriendsViewBarPage, FriendsViewBarStatusFilter};

use maxima::util::{log::init_logger, registry::set_up_registry};

use views::debug_view::debug_view;
use views::friends_view::friends_view;
use views::game_view::games_view;
use views::settings_view::settings_view;
use views::{
    game_view::GameViewBar, game_view::GameViewBarGenre, game_view::GameViewBarPlatform,
    undefinied_view::undefined_view,
};

pub mod bridge;
pub mod util;
mod views;
pub mod widgets;

mod bridge_processor;
mod event_processor;
mod bridge_thread;
mod event_thread;
mod renderers;
mod translation_manager;
mod enum_locale_map;
mod ui_image;
mod desktop;

const ACCENT_COLOR: Color32 = Color32::from_rgb(8, 171, 244);
const APP_MARGIN: Vec2 = vec2(12.0, 12.0); //TODO: user setting
const FRIEND_INGAME_COLOR: Color32 = Color32::from_rgb(39, 106, 252);// temp

#[derive(Parser, Debug, Copy, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    debug: bool,
    #[arg(short, long)]
    profile: bool,
    #[arg(short, long)]
    no_login: bool,
    #[arg(short, long)]
    allow_bf3: bool,
}

#[tokio::main]
async fn main() {
    init_logger();
    let mut args = Args::parse();

    if !std::env::var("MAXIMA_PACKAGED").is_ok_and(|var| var == "1") {
        if let Err(err) = check_desktop_icon() {
            error!("Failed to register desktop icon! {}", err);
        }
    }

    if !cfg!(debug_assertions) {
        args.debug = false;
    }

    if args.profile {
        puffin::set_scopes_on(true);

        match puffin_http::Server::new("127.0.0.1:8585") {
            Ok(puffin_server) => {
                std::process::Command::new("~/.cargo/bin/puffin_viewer")
                    .arg("--url")
                    .arg("127.0.0.1:8585")
                    .spawn()
                    .ok();

                // We can store the server if we want, but in this case we just want
                // it to keep running. Dropping it closes the server, so let's not drop it!
                #[allow(clippy::mem_forget)]
                std::mem::forget(puffin_server);
            }
            Err(err) => {
                eprintln!("Failed to start puffin server: {err}");
            }
        };
    }

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
        .with_inner_size([1280.0, 720.0])
        .with_min_inner_size([940.0, 480.0])
        .with_app_id("io.github.ArmchairDevelopers.Maxima")
        .with_icon(eframe::icon_data::from_png_bytes(&include_bytes!("../../maxima-resources/assets/logo.png")[..]).unwrap()),
        ..Default::default()
    };
    eframe::run_native(
        "Maxima",
        native_options,
        Box::new(move |cc| {
            let app = MaximaEguiApp::new(cc, args);
            // Run initialization code that needs access to the UI here, but DO NOT run any long-runtime functions here,
            // as it's before the UI is shown
            if args.no_login {
                return Ok(Box::new(app));
            }

            Ok(Box::new(app))
        }),
    )
    .expect("Failed i guess?")
}

#[derive(Debug, PartialEq, Default, Clone)]
enum PageType {
    #[default]
    Games,
    Store,
    Settings,
    Downloads,
    Debug,
}

#[derive(Debug, PartialEq, Clone)]
enum PopupModal { // ATM Machine
    GameSettings(String),
    GameInstall(String),
}

#[derive(Debug, PartialEq)]
enum InProgressLoginType {
    Oauth,
    /// Broken
    UsernamePass,
}

/// Which tab is selected in the game list info panel
#[derive(PartialEq, Default)]
pub enum GameInfoTab {
    #[default]
    Achievements,
    Dlc,
    Mods,
}

/// TBD
pub struct GameInstalledModsInfo {}

#[derive(PartialEq, Clone)]
pub struct GameDetails {
    /// Time (in hours/10) you have logged in the game
    time: u32, // hours/10 allows for better precision, i'm only using one decimal place
    /// Achievements you have unlocked
    achievements_unlocked: u16,
    /// Total achievements in the game
    achievements_total: u16,
    /// Path the game is installed to
    path: String,
    /// Minimum specs to run the game, in EasyMark spec
    system_requirements_min: String,
    /// Recommended specs to run the game, in EasyMark spec
    system_requirements_rec: String,
}

#[derive(Clone)]
pub enum GameDetailsWrapper {
    Unloaded,
    Loading,
    Available(GameDetails),
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GameSettings {
    cloud_saves: bool,
    launch_args: String,
    exe_override: String,
}

impl GameSettings {
    pub fn new() -> Self {
        Self {
            cloud_saves: true,
            launch_args: String::new(),
            exe_override: String::new()
        }
    }
}

#[derive(Clone)]
pub struct GameInfo {
    /// Origin slug of the game
    slug: String,
    /// Origin offer ID of the game
    offer: String,
    /// Display name of the game
    name: String,
    /// Game info
    details: GameDetailsWrapper,
    dlc: Vec<OwnedOffer>,
    installed: bool,
    has_cloud_saves: bool,
}

#[derive(PartialEq, Eq)]
pub enum BackendStallState {
    Starting,
    UserNeedsToInstallService,
    UserNeedsToLogIn,
    LoggingIn,
    BingChilling
}

pub struct InstallModalState {
    locate_path: String,
    install_folder: String,
    locating: bool,
    locate_response: Option<InteractThreadLocateGameResponse>,
    should_close: bool,
}

impl InstallModalState {
    pub fn new(settings: &FrontendSettings) -> Self {
        Self {
            locate_path: String::new(),
            install_folder: settings.default_install_folder.clone(),
            locating: false,
            locate_response: None,
            should_close: false,
        }
    }
}

pub struct MaximaEguiApp {
    /// extra not-already-handled commandline args
    args: Args,
    /// general toggle for showing debug info
    debug: bool,
    /// stuff for the bar on the top of the Games view
    game_view_bar: GameViewBar,
    /// stuff for the bar on the top of the Friends view
    friends_view_bar: FriendsViewBar,
    /// Logged in user's display name
    user_name: String,
    /// Logged in user's ID
    user_id: String,
    /// games
    games: HashMap<String, GameInfo>,
    /// selected game
    game_sel: String,
    /// friends
    friends: Vec<UIFriend>, 
    /// width of the friends sidebar
    friends_width: f32,
    /// force visibility of friends sidebar
    force_friends: bool,
    /// what page you're on (games, friends, etc)
    page_view: PageType,
    /// Modal
    modal: Option<PopupModal>,
    /// Renderer for the blur effect in the game view
    game_view_bg_renderer: Option<GameViewBgRenderer>,
    /// Renderer for the app's background
    app_bg_renderer: Option<AppBgRenderer>,
    /// Image cache
    img_cache: UIImageCache,
    /// Translations
    locale: TranslationManager, 
    /// If a core thread has crashed and made the UI unstable
    critical_bg_thread_crashed: bool, 
    /// pepega
    backend: BridgeThread,
    /// what the backend doin?
    backend_state: BackendStallState,
    /// what type of login we're using
    /// Slug of the game currently running, may not be fully accurate but it's good enough to let the user know the button was clicked
    playing_game: Option<String>,
    /// Currently downloading game
    installing_now: Option<QueuedDownload>,
    /// Queue of game installs, indexed by offer ID
    install_queue: HashMap<String, QueuedDownload>,
    /// State for installer modal
    installer_state: InstallModalState,
    /// User Settings for the frontend
    settings: FrontendSettings,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, EnumIter)]
pub enum FrontendLanguage {
    SystemDefault,
    EnUS,
}

#[derive(serde::Serialize, serde::Deserialize, Copy, Clone)]
pub struct FrontendPerformanceSettings {
    disable_blur: bool
}

impl FrontendPerformanceSettings {
    pub fn new() -> Self {
        Self {
            disable_blur: false,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FrontendSettings {
    default_install_folder: String,
    language: FrontendLanguage,
    game_settings: HashMap<String, GameSettings>,
    performance_settings: FrontendPerformanceSettings,
}

impl FrontendSettings {
    pub fn new() -> Self {
        Self {
            default_install_folder: String::new(),
            language: FrontendLanguage::SystemDefault,
            game_settings: HashMap::new(),
            performance_settings: FrontendPerformanceSettings::new(),
        }
    }
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);

const WIDGET_HOVER: Color32 = Color32::from_rgb(255, 188, 61);

impl MaximaEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let style: Style = Style {
            spacing: Spacing {
                scroll: ScrollStyle {
                    bar_width: 8.0,
                    floating_width: 8.0,
                    handle_min_length: 12.0,
                    bar_inner_margin: 4.0,
                    bar_outer_margin: 0.0,
                    dormant_background_opacity: 0.4,
                    dormant_handle_opacity: 0.6,
                    foreground_color: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            visuals: Visuals {
                faint_bg_color: Color32::from_rgb(15, 20, 34),
                extreme_bg_color: Color32::from_rgb(20, 20, 20),
                window_fill: Color32::BLACK,
                //override_text_color: Some(Color32::WHITE),
                hyperlink_color: F9B233,
                widgets: Widgets {
                    hovered: WidgetVisuals {
                        weak_bg_fill: F9B233,
                        bg_fill: F9B233,
                        bg_stroke: Stroke::NONE,
                        fg_stroke: Stroke::new(1.0, Color32::BLACK),
                        rounding: Rounding::ZERO,
                        expansion: -1.0,
                    },
                    inactive: WidgetVisuals {
                        weak_bg_fill: Color32::TRANSPARENT,
                        bg_fill: Color32::BLACK,
                        bg_stroke: Stroke::new(2.0, Color32::WHITE),
                        fg_stroke: Stroke::new(1.5, Color32::WHITE),
                        rounding: Rounding::same(2.0),
                        expansion: -2.0,
                    },
                    active: WidgetVisuals {
                        weak_bg_fill: WIDGET_HOVER.linear_multiply(0.6),
                        bg_fill: WIDGET_HOVER.linear_multiply(0.6),
                        bg_stroke: Stroke::NONE,
                        fg_stroke: Stroke::new(2.0, WIDGET_HOVER.linear_multiply(0.6)),
                        rounding: Rounding::ZERO,
                        expansion: 0.0,
                    },
                    open: WidgetVisuals {
                        weak_bg_fill: WIDGET_HOVER.linear_multiply(0.0),
                        bg_fill: WIDGET_HOVER.linear_multiply(0.0),
                        bg_stroke: Stroke::NONE,
                        fg_stroke: Stroke::new(2.0, WIDGET_HOVER.linear_multiply(0.0)),
                        rounding: Rounding::ZERO,
                        expansion: 0.0,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let mut fonts = FontDefinitions::default();

        fonts.font_data.insert(
            "comic_sans".to_owned(),
            FontData::from_static(include_bytes!("../fonts/IBMPlexSans-Regular.ttf")),
        );

        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "comic_sans".to_owned());

        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .push("comic_sans".to_owned());

        cc.egui_ctx.set_style(style);
        cc.egui_ctx.set_fonts(fonts);

        #[cfg(debug_assertions)]
        cc.egui_ctx.set_debug_on_hover(args.debug);

        let settings: FrontendSettings = if let Some(storage) = cc.storage {
            eframe::get_value(storage, "settings").unwrap_or(FrontendSettings::new())
        } else { FrontendSettings::new() };

        let (img_cache, remote_provider_channel) = UIImageCache::new(cc.egui_ctx.clone());

        Self {
            args,
            debug: args.debug,
            game_view_bar: GameViewBar {
                genre_filter: GameViewBarGenre::AllGames,
                platform_filter: GameViewBarPlatform::AllPlatforms,
                game_size: 2.0,
                search_buffer: String::new(),
            },
            friends_view_bar: FriendsViewBar {
                page: FriendsViewBarPage::All,
                status_filter: FriendsViewBarStatusFilter::Name,
                search_buffer: String::new(),
                friend_sel : String::new(),
            },
            user_name: "User".to_owned(),
            user_id: String::new(),
            games: HashMap::new(),
            game_sel: String::new(),
            friends: Vec::new(),
            friends_width: 300.0,
            force_friends: false,
            //game_view_rows: false,
            page_view: PageType::Games,
            modal: None,
            game_view_bg_renderer: GameViewBgRenderer::new(cc),
            app_bg_renderer: AppBgRenderer::new(cc),
            img_cache,
            locale: TranslationManager::new(&settings.language),
            critical_bg_thread_crashed: false,
            backend: BridgeThread::new(&cc.egui_ctx, remote_provider_channel), //please don't fucking break
            backend_state: BackendStallState::Starting,
            playing_game: None,
            installing_now: None,
            install_queue: HashMap::new(),
            installer_state: InstallModalState::new(&settings),
            settings,
        }
    }
}

// modified from https://github.com/emilk/egui/blob/master/examples/custom_window_frame/src/main.rs

pub fn tab_bar_button(ui: &mut Ui, res: Response) {
    puffin::profile_function!();
    let mut res2 = Rect::clone(&res.rect);
    res2.min.y = res2.max.y - 4.;
    ui.painter().vline(
        res2.min.x + 2.0,
        RangeInclusive::new(res2.min.y, res2.max.y),
        Stroke::new(2.0, ACCENT_COLOR),
    );
    ui.painter().rect_filled(
        res2,
        Rounding::ZERO,
        if res.hovered() {
            ACCENT_COLOR
        } else {
            ACCENT_COLOR.linear_multiply(0.9)
        },
    );
}

/// We used to have a semi-functional implementation that only worked on Mac, but, as i would say, we do not care 🗣️🗣️🗣️
fn custom_window_frame(
    enabled: bool, // disables the entire app, used for if the bg thread crashes
    crash_text: String,
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
    _title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    puffin::profile_function!();
    use egui::*;

    let panel_frame = egui::Frame {
        fill: Color32::RED,
        rounding: 0.0.into(),
        stroke: Stroke::NONE,
        outer_margin: Margin {
            left: APP_MARGIN.x,
            right: APP_MARGIN.x,
            top: APP_MARGIN.y + if !enabled { APP_MARGIN.y * 3.0 } else { 0.0 },
            bottom: APP_MARGIN.y,
        },
        ..Default::default()
    };

    /*if !enabled {
        let mut warning_margin = Margin::same(0.0 - );
        warning_margin.bottom = APP_MARGIN.y;
        egui::Frame::default()
            .fill(Color32::RED)
            .outer_margin(warning_margin)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(
                        egui::RichText::new(
                            &self
                                .locale
                                .localization
                                .errors
                                .critical_thread_crashed,
                        )
                        .color(Color32::BLACK)
                        .size(16.0),
                    );
                });
            });
    }*/

    CentralPanel::default()
    .frame(panel_frame).show(ctx, |ui| {
        let warning_rect = Rect { min: pos2(0.0,0.0), max: pos2(ui.available_width() + APP_MARGIN.x * 2.0, APP_MARGIN.y * 3.0) };
        ui.painter().rect_filled(warning_rect, Rounding::same(0.0), Color32::RED);
        ui.painter().text(warning_rect.center(), Align2::CENTER_CENTER, crash_text, FontId::proportional(16.0), Color32::BLACK);
        ui.add_enabled_ui(enabled, add_contents);
    });
}

/// Wrapper/helper for the tab buttons in the top left of the app
fn tab_button(ui: &mut Ui, edit_var: &mut PageType, page: PageType, label: &str) {
    puffin::profile_function!();
    ui.style_mut().visuals.widgets.inactive.rounding = Rounding::ZERO;
    ui.style_mut().visuals.widgets.active.rounding = Rounding::ZERO;
    ui.style_mut().visuals.widgets.hovered.rounding = Rounding::ZERO;
    ui.style_mut().visuals.widgets.inactive.expansion = -1.0;
    ui.style_mut().visuals.widgets.active.expansion = -1.0;
    ui.style_mut().visuals.widgets.hovered.expansion = -1.0;

    if edit_var == &page {
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.active.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.active.bg_stroke = Stroke::NONE;
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    } else {
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.active.fg_stroke = Stroke::new(2.0, F9B233);
        ui.style_mut().visuals.widgets.active.bg_stroke = Stroke::NONE;
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.fg_stroke = Stroke::new(2.0, F9B233);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    }
    let text = egui::RichText::new(label.to_uppercase()).size(16.0);

    let test = ui.add_sized([120.0, 28.0], egui::Button::new(text));
    if test.clicked() {
        *edit_var = page.clone();
    }
}

// god-awful macro to do something incredibly simple because apparently wrapping it in a function has rustc fucking implode
// say what you want about C++ footguns but rust is the polar fucking opposite, shooting you in the head for doing literally anything
macro_rules! set_app_modal {
    ($arg1:expr, $arg2:expr) => {
        if let Some(modal) = $arg2 {
            match modal {
                PopupModal::GameSettings(slug) => {
                    if $arg1.settings.game_settings.get(&slug).is_none() {
                        $arg1.settings.game_settings.insert(slug.clone(), crate::GameSettings::new());
                    }
                },
                PopupModal::GameInstall(_) => {
                    $arg1.installer_state = InstallModalState::new(&$arg1.settings);
                },
            }
            $arg1.modal = $arg2;
        } else {
            $arg1.modal = None;
        }
    };
}

pub(crate) use set_app_modal;

impl eframe::App for MaximaEguiApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        puffin::profile_function!();
        bridge_processor::frontend_processor(self, ctx);
        event_processor::frontend_processor(self, ctx);
        
        custom_window_frame(!self.critical_bg_thread_crashed,
            self.locale.localization.errors.critical_thread_crashed.clone(),
            ctx, frame, "Maxima", |ui| {
            if let Some(render) = &self.app_bg_renderer {
                let mut fullrect = ui.available_rect_before_wrap().clone();
                fullrect.min -= APP_MARGIN;
                fullrect.max += APP_MARGIN;
                let has_game_img = self.backend_state == BackendStallState::BingChilling && self.games.len() > 0;
                let gaming = self.page_view == PageType::Games && has_game_img;
                let how_game: f32 = ctx.animate_bool(egui::Id::new("MainAppBackgroundGamePageFadeBool"), gaming);
                if has_game_img
                {
                    if self.game_sel.is_empty() && self.games.len() > 0 {
                        if let Some(key) = self.games.keys().next() {
                            self.game_sel = key.clone()
                        }
                    }
                    //TODO: background
                    match &self.img_cache.get(ui_image::UIImageType::Hero(self.games[&self.game_sel].slug.clone())) {
                        Some(tex) => render.draw(ui, fullrect, tex.size_vec2(), tex.id(), how_game, self.settings.performance_settings),
                        None => { render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1), 0.0, self.settings.performance_settings); },
                    }
                } else {
                    render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1), 0.0, self.settings.performance_settings);
                }
            }
            let app_rect = ui.available_rect_before_wrap().clone();
            match self.backend_state {
                BackendStallState::Starting => {
                    ui.painter().text(
                        app_rect.center(),
                        Align2::CENTER_CENTER, &self.locale.localization.startup_flow.starting,
                        FontId::proportional(30.0), Color32::WHITE
                    );
                    ui.put(app_rect, egui::Spinner::new().size(300.0));
                },
                BackendStallState::UserNeedsToInstallService => {
                    let main_block_rect = ui.painter().text(app_rect.center(), Align2::CENTER_BOTTOM,
                    &self.locale.localization.startup_flow.service_installer_description, egui::FontId::proportional(20.0), Color32::GRAY);
                    ui.painter().text(main_block_rect.center_top() - vec2(0.0, 4.0), Align2::CENTER_BOTTOM,
                    &self.locale.localization.startup_flow.service_installer_header, egui::FontId::proportional(30.0), Color32::WHITE);
                    let button_rect = Rect {
                        min: main_block_rect.center_bottom() + vec2(-60.0,  4.0),
                        max: main_block_rect.center_bottom() + vec2( 60.0, 34.0)
                    };
                    if ui.put(button_rect, egui::Button::new(&self.locale.localization.startup_flow.service_installer_button.to_ascii_uppercase())).clicked() {
                        self.backend.backend_commander
                                .send(bridge_thread::MaximaLibRequest::StartService)
                                .unwrap();
                        self.backend_state = BackendStallState::Starting;
                    }
                },
                BackendStallState::UserNeedsToLogIn => {
                    let main_block_rect = ui.painter().text(app_rect.center(), Align2::CENTER_BOTTOM,
                    &self.locale.localization.startup_flow.login_header, egui::FontId::proportional(30.0), Color32::WHITE);
                    let button_rect = Rect {
                        min: main_block_rect.center_bottom() + vec2(-60.0,  4.0),
                        max: main_block_rect.center_bottom() + vec2( 60.0, 34.0)
                    };
                    if ui.put(button_rect, egui::Button::new(&self.locale.localization.startup_flow.login_button.to_ascii_uppercase())).clicked() {
                        self.backend.backend_commander
                        .send(bridge_thread::MaximaLibRequest::LoginRequestOauth)
                        .unwrap();
                        self.backend_state = BackendStallState::LoggingIn;
                    }
                },
                BackendStallState::LoggingIn => {
                    ui.painter().text(
                        app_rect.center(),
                        Align2::CENTER_CENTER, &self.locale.localization.startup_flow.logging_in,
                        FontId::proportional(30.0), Color32::WHITE
                    );
                    ui.put(app_rect, egui::Spinner::new().size(300.0));
                },
                BackendStallState::BingChilling => {
                    let outside_spacing = ui.spacing().item_spacing.x.clone();
                    ui.add_enabled_ui(self.modal.is_none(), |non_modal| {
                        non_modal.spacing_mut().item_spacing.y = outside_spacing;
                        StripBuilder::new(non_modal)
                        .size(Size::exact(38.0))
                        .size(Size::remainder())
                        .vertical(|mut strip| {
                            strip.cell(|ui| {
                                puffin::profile_scope!("top bar");
                                StripBuilder::new(ui)
                                .size(Size::remainder())
                                .size(Size::exact(300.0))
                                .horizontal(|mut strip| {
                                    strip.cell(|header| {
                                        puffin::profile_scope!("tab bar");
                                        //header.painter().rect_filled(header.available_rect_before_wrap(), Rounding::ZERO, Color32::from_white_alpha(20));
                                        let navbar = egui::Frame::default()
                                            .stroke(Stroke::new(2.0, Color32::WHITE))
                                            .inner_margin(Margin::same(0.0))
                                            .outer_margin(Margin::same(2.0))
                                            .rounding(Rounding::same(4.0));
                                        navbar.show(header, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 0.0;
                                                ui.style_mut()
                                                    .visuals
                                                    .widgets
                                                    .inactive
                                                    .rounding = Rounding::ZERO;
                                                // BEGIN TAB BUTTONS
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Games,
                                                    &self.locale.localization.menubar.games,
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Store,
                                                    &self.locale.localization.menubar.store,
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Settings,
                                                    &self.locale.localization.menubar.settings,
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Downloads,
                                                    &self.locale.localization.menubar.downloads,
                                                );
                                                #[cfg(debug_assertions)]
                                                if self.debug {
                                                    tab_button(
                                                        ui,
                                                        &mut self.page_view,
                                                        PageType::Debug,
                                                        "Debug",
                                                    );
                                                }
                                                //END TAB BUTTONS
                                            });
                                        });
                                    });
                                    strip.cell(|profile| {
                                        puffin::profile_scope!("you");
                                        //profile.painter().rect_filled(profile.available_rect_before_wrap(), Rounding::ZERO, Color32::from_white_alpha(20));
                                        profile.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center), |rtl| {
                                                rtl.style_mut().spacing.item_spacing.x = 0.0;
                                                rtl.allocate_space(vec2(2.0, 2.0));
                                                rtl.style_mut().spacing.item_spacing.x = APP_MARGIN.x;
                                                
                                                let img_response = if let Some(av) = self.img_cache.get(ui_image::UIImageType::Avatar(self.user_id.clone())) {
                                                    rtl.image((av.id(), vec2(36.0, 36.0)))
                                                } else {
                                                    rtl.image((self.img_cache.placeholder_avatar.id(), vec2(36.0, 36.0)))
                                                };
                                                let stroke = Stroke::new(2.0, {
                                                    if self.playing_game.is_some() {
                                                        FRIEND_INGAME_COLOR
                                                    } else {
                                                        Color32::GREEN
                                                    }
                                                });
                                                rtl.painter().rect(img_response.rect.expand(1.0), Rounding::same(4.0), Color32::TRANSPARENT, stroke);
                                                let point = img_response.rect.left_center() + vec2(-rtl.spacing().item_spacing.x, 2.0);

                                                if let Some(game_slug) = &self.playing_game {
                                                    if let Some(game) = &self.games.get(game_slug) {
                                                        let offset = vec2(0.0, 0.5);
                                                        rtl.painter().text(point-offset, Align2::RIGHT_BOTTOM, &self.user_name, FontId::proportional(15.0), Color32::WHITE);
                                                        rtl.painter().text(point+offset, Align2::RIGHT_TOP, positional_replace!(&self.locale.localization.friends_view.status.presence_basic, "game", &game.name), FontId::proportional(10.0), Color32::WHITE);
                                                    }
                                                } else {
                                                    rtl.painter().text(point, Align2::RIGHT_CENTER, &self.user_name, FontId::proportional(15.0), Color32::WHITE);
                                                }
                                            },
                                        );
                                    });
                                });
                            });

                            strip.cell(|main| {
                                puffin::profile_scope!("main content");

                                let bigmain_rect = Rect {
                                    min: main.available_rect_before_wrap().min,
                                    max: main.available_rect_before_wrap().max - vec2(self.friends_width + main.style().spacing.item_spacing.x, 0.0)
                                };
                                let friends_rect = Rect {
                                    min: pos2(main.available_rect_before_wrap().max.x - self.friends_width, main.available_rect_before_wrap().min.y),
                                    max: main.available_rect_before_wrap().max
                                };

                                main.allocate_ui_at_rect(bigmain_rect, |bigmain| {
                                    puffin::profile_scope!("main view");
                                    match self.page_view {
                                        PageType::Games => games_view(self, bigmain),
                                        PageType::Settings => settings_view(self, bigmain),
                                        PageType::Debug => debug_view(self, bigmain),
                                        PageType::Store => coming_soon_view(self, bigmain),
                                        PageType::Downloads => downloads_view(self, bigmain),
                                        _ => undefined_view(self, bigmain),
                                    }
                                });
                                main.allocate_ui_at_rect(friends_rect, |friends| {
                                    friends_view(self, friends);
                                });
                            });
                        });
                    });
                    let mut clear = false;
                    if let Some(modal) = &self.modal {
                        ui.allocate_ui_at_rect(app_rect, |contents| {

                        
                            egui::Frame::default()
                            .fill(Color32::from_black_alpha(200))
                            .outer_margin(Margin::symmetric((app_rect.width() - 600.0) / 2.0, (app_rect.height() - 400.0) / 2.0))
                            .inner_margin(Margin::same(12.0))
                            .rounding(Rounding::same(8.0))
                            .stroke(Stroke::new(4.0, Color32::WHITE))
                            .show(contents, |ui| {
                                ui.style_mut().spacing.interact_size = vec2(100.0, 30.0);
                                ui.spacing_mut().icon_width = 30.0;
                                match modal {
                                    PopupModal::GameSettings(slug) => 'outer: {
                                        let game = if let Some(game) = self.games.get_mut(slug) { game } else { break 'outer; };
                                        //let game_settings = game.settings.borrow_mut();
                                        ui.horizontal(|header| {
                                            header.heading(positional_replace!(self.locale.localization.modals.game_settings.header, "game", &game.name));
                                            header.with_layout(Layout::right_to_left(egui::Align::Center), |close_button| {
                                                if close_button.add_sized(vec2(80.0, 30.0), egui::Button::new(&self.locale.localization.modals.close.to_ascii_uppercase())).clicked() {
                                                    clear = true
                                                }
                                            });
                                        });
                                        ui.separator();
                                        if game.installed {
                                            if let Some(settings) = self.settings.game_settings.get_mut(&game.slug) {
                                                ui.add_enabled(/* game.has_cloud_saves */ false, egui::Checkbox::new(&mut settings.cloud_saves, &self.locale.localization.modals.game_settings.cloud_saves));
                                                
                                                ui.label(&self.locale.localization.modals.game_settings.launch_arguments);
                                                ui.add_sized(vec2(ui.available_width(), ui.style().spacing.interact_size.y), egui::TextEdit::singleline(&mut settings.launch_args).vertical_align(egui::Align::Center));

                                                ui.separator();


                                                let button_size = vec2(100.0, 30.0);

                                                ui.label(&self.locale.localization.modals.game_settings.executable_override);
                                                ui.horizontal(|ui| {
                                                    let size = vec2(500.0 - (24.0 + ui.style().spacing.item_spacing.x), 30.0);
                                                    ui.add_sized(size, egui::TextEdit::singleline(&mut settings.exe_override).vertical_align(egui::Align::Center));
                                                    ui.add_sized(button_size, egui::Button::new("BROWSE"));
                                                });

                                                ui.separator();
                                            }

                                            ui.add_enabled(false, egui::Button::new(format!("  {}  ", &self.locale.localization.modals.game_settings.uninstall.to_ascii_uppercase())));
                                        } else {
                                            ui.label(&self.locale.localization.modals.game_settings.not_installed);
                                        }
                                    },
                                    PopupModal::GameInstall(slug) => 'outer: {
                                        let game = if let Some(game) = self.games.get_mut(slug) { game } else { break 'outer; };
                                        ui.horizontal(|header| {
                                            header.heading(positional_replace!(self.locale.localization.modals.game_install.header, "game", &game.name));
                                            header.with_layout(Layout::right_to_left(egui::Align::Center), |close_button| {
                                                close_button.add_enabled_ui(!self.installer_state.locating, |close_button| {
                                                    if close_button.add_sized(vec2(80.0, 30.0), egui::Button::new(&self.locale.localization.modals.close.to_ascii_uppercase())).clicked() {
                                                        clear = true
                                                    }
                                                });
                                            });
                                        });

                                        ui.separator();

                                        if !self.args.allow_bf3 && (slug.eq("battlefield-3") || slug.eq("battlefield-4")) {
                                            ui.heading("Battlefield 3 and 4 are currently unsupported due to how battlelog complicates game launching. This is on our radar, but isn't a huge priority at the moment.");
                                            break 'outer;
                                        }

                                        let button_size = vec2(100.0, 30.0);

                                        ui.label(&self.locale.localization.modals.game_install.locate_installed);
                                        if let Some(resp) = &self.installer_state.locate_response {
                                            match resp {
                                                InteractThreadLocateGameResponse::Success => {
                                                    self.installer_state.should_close = true;
                                                    game.installed = true;
                                                },
                                                InteractThreadLocateGameResponse::Error(err) => {
                                                    ui.spacing_mut().item_spacing.x = 0.0;
                                                    
                                                    egui::Label::new(egui::RichText::new(&self.locale.localization.modals.game_install.locate_failed).color(Color32::RED)).ui(ui);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label("Please report this on ");
                                                        ui.hyperlink_to("GitHub Issues", "https://github.com/ArmchairDevelopers/Maxima/issues/new");
                                                        ui.label(".");
                                                    });
                                                    ui.label("Make sure to specify:");
                                                    ui.horizontal_wrapped(|ui| {
                                                        egui::Label::new("• You were locating ").selectable(false).ui(ui);
                                                        egui::Label::new(egui::RichText::new(format!("{}", game.name)).color(Color32::WHITE)).ui(ui);
                                                    });
                                                    ui.horizontal_wrapped(|ui| {
                                                        egui::Label::new("• ").selectable(false).ui(ui);
                                                        egui::Label::new(egui::RichText::new(format!("{}", err.reason)).color(Color32::WHITE)).ui(ui);
                                                    });
                                                    ui.horizontal_wrapped(|ui| {
                                                        egui::Label::new("And attach ").selectable(false).ui(ui);
                                                        egui::Label::new(egui::RichText::new(&err.xml_path).color(Color32::WHITE)).ui(ui);
                                                    });
                                                    
                                                }
                                            }
                                        } else if self.installer_state.locating {
                                            ui.heading(&self.locale.localization.modals.game_install.locate_in_progress);
                                        } else {
                                            ui.horizontal(|ui| {
                                                let size = vec2(400.0 - (24.0 + ui.style().spacing.item_spacing.x*2.0), 30.0);
                                                ui.add_sized(size, egui::TextEdit::singleline(&mut self.installer_state.locate_path).vertical_align(egui::Align::Center));
                                                ui.add_sized(button_size, egui::Button::new("BROWSE"));
                                                ui.add_enabled_ui(PathBuf::from(&self.installer_state.locate_path).exists(), |ui| {

                                                    if ui.add_sized(button_size, egui::Button::new(&self.locale.localization.modals.game_install.locate_action.to_ascii_uppercase())).clicked() {
                                                        self.backend.backend_commander.send(bridge_thread::MaximaLibRequest::LocateGameRequest(slug.clone(), self.installer_state.locate_path.clone())).unwrap();
                                                        self.installer_state.locating = true;
                                                    }
                                                });
                                            });
                                        }
                                        ui.label("");
                                        ui.label(&self.locale.localization.modals.game_install.fresh_download);
                                        ui.add_enabled_ui(!self.installer_state.locating, |ui| {
                                            let size = vec2(500.0 - (24.0 + ui.style().spacing.item_spacing.x*2.0), 30.0);
                                            ui.horizontal(|ui| {
                                                ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, F9B233);
                                                ui.style_mut().visuals.widgets.hovered.expansion = -2.0;
                                                ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(2.0);
                                                ui.add_sized(size, egui::TextEdit::singleline(&mut self.installer_state.install_folder).vertical_align(egui::Align::Center));
                                            });
                                            let path = PathBuf::from(self.installer_state.install_folder.clone());
                                            let valid = path.exists();
                                            ui.add_enabled_ui(valid, |ui| {
                                                if ui.add_sized(button_size, egui::Button::new(&self.locale.localization.modals.game_install.fresh_action)).clicked() {
                                                    if self.installing_now.is_none() {
                                                        self.installing_now = Some(QueuedDownload { slug: game.slug.clone(), offer: game.offer.clone(), downloaded_bytes: 0, total_bytes: 0 });
                                                    } else {
                                                        self.install_queue.insert(game.offer.clone(),QueuedDownload { slug: game.slug.clone(), offer: game.offer.clone(), downloaded_bytes: 0, total_bytes: 0 });
                                                    }
                                                    self.backend.backend_commander.send(bridge_thread::MaximaLibRequest::InstallGameRequest(game.offer.clone(), path.join(slug))).unwrap();

                                                    clear = true;
                                                }
                                            });
                                            if !self.installer_state.install_folder.is_empty() {
                                                ui.horizontal_wrapped(|folder_hint| {
                                                    egui::Label::new(&self.locale.localization.modals.game_install.fresh_path_confirmation).selectable(false).ui(folder_hint);
                                                    egui::Label::new(egui::RichText::new(format!("{}",
                                                        path.join(slug).display())).color(Color32::WHITE)).selectable(false).ui(folder_hint);
                                                });
                                                if !valid {
                                                    egui::Label::new(egui::RichText::new(&self.locale.localization.modals.game_install.fresh_path_invalid).color(Color32::RED)).ui(ui);
                                                }
                                            }
                                        });
                                        

                                        if self.installer_state.should_close { clear = true; }
                                    }
                                }
                                ui.allocate_space(ui.available_size_before_wrap());
                            });
                        });
                    }
                    if clear {
                        self.modal = None;
                    }
                },
            };
        });
        puffin::GlobalProfiler::lock().new_frame();
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.backend
            .backend_commander
            .send(bridge_thread::MaximaLibRequest::ShutdownRequest)
            .unwrap();
    }
}
