#![feature(slice_pattern)]
use clap::{arg, command, Parser};

use egui::{pos2, ViewportBuilder};
use egui::style::{ScrollStyle, Spacing};
use egui::Style;
use log::{error, info, warn};
use maxima::core::library::OwnedOffer;
use views::undefinied_view::coming_soon_view;
use std::collections::HashMap;
use std::default::Default;
use std::{ops::RangeInclusive, rc::Rc, sync::Arc};
use ui_image::UIImage;
use views::friends_view::UIFriend;


use eframe::egui_glow;
use egui::{
    style::{WidgetVisuals, Widgets},
    vec2, Color32, FontData, FontDefinitions, FontFamily, Margin, Rect, Response, Rounding, Stroke,
    TextureId, Ui, Vec2, Visuals,
};
use egui_extras::{RetainedImage, Size, StripBuilder};
use egui_glow::glow;

use bridge_thread::BridgeThread;
use event_thread::EventThread;

use app_bg_renderer::AppBgRenderer;
use fs::image_loader::ImageLoader;
use game_view_bg_renderer::GameViewBgRenderer;
use renderers::app_bg_renderer;
use renderers::game_view_bg_renderer;
use translation_manager::TranslationManager;
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
mod fs;
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

use maxima::util::registry::check_registry_validity;

const ACCENT_COLOR: Color32 = Color32::from_rgb(8, 171, 244);
const APP_MARGIN: Vec2 = vec2(12.0, 12.0); //TODO: user setting

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
    remove_hardcoded_game_paths: bool,
}

#[tokio::main]
async fn main() {
    init_logger();
    let mut args = Args::parse();

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
        .with_min_inner_size([940.0, 480.0]),
        
        /* icon_data: {
            let res = IconData::try_from_png_bytes(include_bytes!("../../maxima-resources/assets/logo.png"));
            if let Ok(icon) = res {
                Some(icon)
            } else {
                None
            }
        },*/
        //min_window_size: Some(vec2()),
        ..Default::default()
    };
    eframe::run_native(
        "Maxima",
        native_options,
        Box::new(move |cc| {
            let app = DemoEguiApp::new(cc, args);
            // Run initialization code that needs access to the UI here, but DO NOT run any long-runtime functions here,
            // as it's before the UI is shown
            if args.no_login {
                return Ok(Box::new(app));
            }
            if let Err(err) = check_registry_validity() {
                warn!("{}, fixing...", err);
                // this is if let in case set_up_registry ever returns something useful, instead of bailing
                if let Err(_er) = set_up_registry() {
                    error!("Registry setup failed!");
                }
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

pub struct GameUIImages {
    /// YOOOOO
    hero: Arc<UIImage>,
    /// The stylized logo of the game, some games don't have this!
    logo: Option<Arc<UIImage>>,
}

pub enum GameUIImagesWrapper {
    Unloaded,
    Loading,
    Available(GameUIImages),
}

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

pub enum GameDetailsWrapper {
    Unloaded,
    Loading,
    Available(GameDetails),
}

pub struct GameInfo {
    /// Origin slug of the game
    slug: String,
    /// Origin offer ID of the game
    offer: String,
    /// Display name of the game
    name: String,
    /// Art Assets
    images: GameUIImagesWrapper,
    /// Game info
    details: GameDetailsWrapper,
    dlc: Vec<OwnedOffer>,
    installed: bool,
}

impl GameInfo {
    /// TEST FUNC FOR SHIT IDK LMAO
    pub fn uninstall(&self) {
        info!("Uninstall requested for \"{}\"", self.name);
    }
    /// TEST FUNC FOR SHIT IDK LMAO
    pub fn launch(&self) {
        info!("Launch requested for \"{}\"", self.name);
    }
}

pub struct DemoEguiApp {
    /// general toggle for showing debug info
    debug: bool,
    /// stuff for the bar on the top of the Games view
    game_view_bar: GameViewBar,
    /// stuff for the bar on the top of the Friends view
    friends_view_bar: FriendsViewBar,
    /// Logged in user's display name
    user_name: String,
    /// temp icon for the user's profile picture
    _user_pfp: Rc<RetainedImage>, 
    /// actual renderable for the user's profile picture //TODO
    user_pfp_renderable: TextureId,
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
    //game_view_rows: bool,                               // if the game view is in rows mode
    /// what page you're on (games, friends, etc)
    page_view: PageType,
    /// Renderer for the blur effect in the game view
    game_view_bg_renderer: Option<GameViewBgRenderer>,
    /// Renderer for the app's background
    app_bg_renderer: Option<AppBgRenderer>,
    /// Translations
    locale: TranslationManager, 
    /// If a core thread has crashed and made the UI unstable
    critical_bg_thread_crashed: bool, 
    /// pepega
    backend: BridgeThread, 
    /// pepega
    events: EventThread, 
    /// temp book to track login status
    logged_in: bool, 
    /// waiting for the bridge to check auth storage
    login_cache_waiting: bool, 
    /// if the login flow is in progress
    in_progress_login: bool,
    /// what type of login we're using
    in_progress_login_type: InProgressLoginType,
    /// Username buffer for logging in with a username/password
    in_progress_username: String,
    /// Password buffer for logging in with a username/password
    in_progress_password: String,
    /// Errors info etc for logging in with a username/password
    in_progress_credential_status: String,
    /// Currently waiting on the maxima thread to log us in with credentials
    credential_login_in_progress: bool, 
    /// Hardcodes game exe paths to stuff on my computer
    hardcode_game_paths: bool,
    /// Slug of the game currently running, may not be fully accurate but it's good enough to let the user know the button was clicked
    playing_game: Option<String>
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);

const WIDGET_HOVER: Color32 = Color32::from_rgb(255, 188, 61);

impl DemoEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let style: Style = Style {
            spacing: Spacing {
                scroll: ScrollStyle {
                    bar_width: 8.0,
                    floating_width: 8.0,
                    handle_min_length: 12.0,
                    bar_inner_margin: 4.0,
                    bar_outer_margin: 0.0,
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
                        expansion: 0.0,
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

        
        let _user_pfp =
            Rc::new(RetainedImage::from_image_bytes("Timothy Dean Sweeney", include_bytes!("../res/usericon_tmp.png")).expect("yeah"));

        Self {
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
            user_pfp_renderable: (&_user_pfp).texture_id(&cc.egui_ctx),
            _user_pfp,
            user_name: "User".to_owned(),
            games: HashMap::new(),
            game_sel: String::new(),
            friends: Vec::new(),
            friends_width: 300.0,
            force_friends: false,
            //game_view_rows: false,
            page_view: PageType::Games,
            game_view_bg_renderer: GameViewBgRenderer::new(cc),
            app_bg_renderer: AppBgRenderer::new(cc),
            locale: TranslationManager::new()
                .expect("Could not load translation file"),
            critical_bg_thread_crashed: false,
            backend: BridgeThread::new(&cc.egui_ctx), //please don't fucking break
            events: EventThread::new(&cc.egui_ctx),
            logged_in: args.no_login, // largely deprecated but i'm going to keep it here
            login_cache_waiting: true,
            in_progress_login: false,
            in_progress_login_type: InProgressLoginType::Oauth,
            in_progress_username: String::new(),
            in_progress_password: String::new(),
            in_progress_credential_status: String::new(),
            credential_login_in_progress: false,
            hardcode_game_paths: !args.remove_hardcoded_game_paths,
            playing_game: None,
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
        outer_margin: 0.0.into(), // this used to check if it was maximized but that got deprecated and we don't care
        ..Default::default()
    };

    CentralPanel::default()
    .frame(panel_frame).show(ctx, |ui| {
        let app_rect = ui.max_rect();

        let title_bar_height = 28.0; //height on a standard monitor on macOS monterey
        let _title_bar_rect = {
            let mut rect = app_rect;
            rect.max.y = rect.min.y + title_bar_height;
            rect
        };
        
        let content_rect = Rect {
            min: app_rect.min + APP_MARGIN,
            max: app_rect.max - APP_MARGIN,
        };

        let mut content_ui = ui.child_ui(content_rect, *ui.layout(), None);

        add_contents(&mut content_ui);
    });
}

/// Wrapper/helper for the tab buttons in the top left of the app
fn tab_button(ui: &mut Ui, edit_var: &mut PageType, page: PageType, label: String) {
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

impl eframe::App for DemoEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        puffin::profile_function!();
        bridge_processor::frontend_processor(self, ctx);
        event_processor::frontend_processor(self, ctx);

        custom_window_frame(ctx, frame, "Maxima", |ui| {
            if let Some(render) = &self.app_bg_renderer {
                let mut fullrect = ui.available_rect_before_wrap().clone();
                fullrect.min -= APP_MARGIN;
                fullrect.max += APP_MARGIN;
                let has_game_img = self.logged_in && self.games.len() > 0;
                let gaming = self.page_view == PageType::Games && has_game_img;
                let how_game: f32 = ctx.animate_bool(egui::Id::new("MainAppBackgroundGamePageFadeBool"), gaming);
                if has_game_img
                {
                    if self.game_sel.is_empty() && self.games.len() > 0 {
                        if let Some(key) = self.games.keys().next() {
                            self.game_sel = key.clone()
                        }
                    }
                    match &self.games[&self.game_sel].images {
                        GameUIImagesWrapper::Unloaded | GameUIImagesWrapper::Loading => {
                            render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1), how_game);
                        }
                        GameUIImagesWrapper::Available(images) => {
                            render.draw(ui, fullrect, images.hero.size, images.hero.renderable, how_game);
                        }
                    }
                } else {
                    render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1), how_game);
                }
            }
            if self.login_cache_waiting {
                ui.heading("hey, hi, hold on a bit");
            } else {
                if !self.logged_in {
                    if self.in_progress_login {
                        match self.in_progress_login_type {
                            InProgressLoginType::Oauth => {
                                ui.vertical_centered(|ui| {
                                    ui.add_sized([400.0, 400.0], egui::Spinner::new().size(400.0));
                                    ui.heading("Logging in...");
                                });
                            }
                            InProgressLoginType::UsernamePass => {
                                ui.set_enabled(!self.credential_login_in_progress);
                                ui.vertical_centered(|ui| {
                                    ui.add_sized(
                                        [260., 30.],
                                        egui::text_edit::TextEdit::hint_text(
                                            egui::text_edit::TextEdit::singleline(
                                                &mut self.in_progress_username,
                                            ),
                                            &self.locale.localization.login.username_box_hint,
                                        ),
                                    );
                                    ui.add_sized(
                                        [260., 30.],
                                        egui::text_edit::TextEdit::hint_text(
                                            egui::text_edit::TextEdit::singleline(
                                                &mut self.in_progress_password,
                                            )
                                            .password(true),
                                            &self.locale.localization.login.password_box_hint,
                                        ),
                                    );
                                    ui.heading(
                                        egui::RichText::new(&self.in_progress_credential_status)
                                            .color(Color32::RED),
                                    );
                                    if ui
                                        .add_sized(
                                            [260., 30.],
                                            egui::Button::new(
                                                egui::RichText::new(
                                                    &self.locale.localization.login.credential_confirm,
                                                )
                                                .size(25.0),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.backend
                                            .tx
                                            .send(
                                                bridge_thread::MaximaLibRequest::LoginRequestUserPass(
                                                    self.in_progress_username.clone(),
                                                    self.in_progress_password.clone(),
                                                ),
                                            )
                                            .unwrap();
                                        self.credential_login_in_progress = true;
                                        self.in_progress_credential_status =
                                            self.locale.localization.login.credential_waiting.clone();
                                    }
                                });
                            }
                        }
                    } else {
                        ui.allocate_exact_size(
                            vec2(0.0, (ui.available_size_before_wrap().y / 2.0) - 120.0),
                            egui::Sense::click(),
                        );
                        ui.vertical_centered_justified(|ui| {
                            ui.heading("You're not logged in.");
                            ui.horizontal(|ui| {
                                ui.allocate_exact_size(
                                    vec2(
                                        (ui.available_width()
                                            - (160.0 + ui.style().spacing.item_spacing.x))
                                            / 2.0,
                                        0.0,
                                    ),
                                    egui::Sense::click(),
                                );

                                if ui
                                    .add_sized(
                                        [160.0, 60.0],
                                        egui::Button::new(
                                            &self.locale.localization.login.oauth_option,
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.in_progress_login_type = InProgressLoginType::Oauth;
                                    self.in_progress_login = true;
                                    self.backend
                                        .tx
                                        .send(bridge_thread::MaximaLibRequest::LoginRequestOauth)
                                        .unwrap();
                                }
                                /*if ui
                                    .add_sized(
                                        [160.0, 60.0],
                                        egui::Button::new(
                                            &self.locale.localization.login.credentials_option,
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.in_progress_login_type = InProgressLoginType::UsernamePass;
                                    self.in_progress_login = true;
                                }*/
                            })
                        });
                    }
                } else {
                    let main_width = ui.available_width() - (300.0 + ui.spacing().item_spacing.x);
                    let size_width = 300.0;
                    let outside_spacing = ui.spacing().item_spacing.x.clone();
                    if self.critical_bg_thread_crashed {
                        let mut warning_margin = Margin::same(0.0 - APP_MARGIN.x);
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
                    }
                    ui.spacing_mut().item_spacing.y = outside_spacing;
                    StripBuilder::new(ui)
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
                                                    self.locale
                                                        .localization
                                                        .menubar
                                                        .games
                                                        .clone(),
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Store,
                                                    self.locale
                                                        .localization
                                                        .menubar
                                                        .store
                                                        .clone(),
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Settings,
                                                    self.locale
                                                        .localization
                                                        .menubar
                                                        .settings
                                                        .clone(),
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Downloads,
                                                    "Downloads".to_string(),
                                                );
                                                #[cfg(debug_assertions)]
                                                if self.debug {
                                                    tab_button(
                                                        ui,
                                                        &mut self.page_view,
                                                        PageType::Debug,
                                                        "Debug".to_owned(),
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
                                            egui::Layout::right_to_left(
                                                egui::Align::Center,
                                            ),
                                            |rtl| {
                                                rtl.visuals_mut()
                                                    .widgets
                                                    .inactive
                                                    .bg_stroke =
                                                    Stroke::new(2.0, Color32::GREEN);
                                                rtl.menu_image_button(
                                                    (self.user_pfp_renderable, vec2(36.0, 36.0)),
                                                    |ui| {
                                                        if ui
                                                            .button(
                                                                &self
                                                                    .locale
                                                                    .localization
                                                                    .profile_menu
                                                                    .view_profile,
                                                            )
                                                            .clicked()
                                                        {
                                                            ui.close_menu();
                                                        }
                                                        if ui
                                                            .button(
                                                                &self
                                                                    .locale
                                                                    .localization
                                                                    .profile_menu
                                                                    .view_wishlist,
                                                            )
                                                            .clicked()
                                                        {
                                                            ui.close_menu();
                                                        }
                                                    },
                                                );
                                                rtl.label(
                                                    egui::RichText::new(
                                                        self.user_name.clone(),
                                                    )
                                                    .size(15.0)
                                                    .color(Color32::WHITE),
                                                );
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
                                        _ => undefined_view(self, bigmain),
                                    }
                                });
                                main.allocate_ui_at_rect(friends_rect, |friends| {
                                    friends_view(self, friends);
                                });
                            });
                        });

                    /*StripBuilder::new(ui)
                        .size(Size::initial(main_width))
                        .size(Size::remainder())
                        .horizontal(|mut strip| {
                            strip.cell(|ui| {
                                
                                let avail_height = ui.available_height() - (32.0 + outside_spacing);
                                StripBuilder::new(ui)
                                    .size(Size::exact(32.0))
                                    .size(Size::exact(avail_height))
                                    .vertical(|mut strip| {
                                        
                                        strip.cell(|body| {
                                           
                                        })
                                    });
                            });
                            strip.cell(|ui| {
                                puffin::profile_scope!("right panel");
                                ui.spacing_mut().item_spacing.y = outside_spacing;
                                let avail_height = ui.available_height() - (40.0);
                                StripBuilder::new(ui)
                                    .size(Size::exact(32.0))
                                    .size(Size::exact(avail_height))
                                    .vertical(|mut strip| {
                                        strip.cell(|header| {
                                            
                                            //header.painter().rect_filled(header.available_rect_before_wrap(), Rounding::ZERO, Color32::from_white_alpha(20));
                                            let navbar = egui::Frame::default()
                                                .stroke(Stroke::new(2.0, Color32::WHITE))
                                                //.fill(Color32::BLACK)
                                                .outer_margin(Margin::same(1.0))
                                                .inner_margin(Margin::same(-2.0))
                                                .rounding(Rounding::same(4.0));
                                            navbar.show(header, |ui| {
                                                
                                            });
                                        });
                                        strip.cell(|body| {
                                            //
                                        })
                                    });
                            });
                        });*/
                }
            }
        });
        puffin::GlobalProfiler::lock().new_frame();
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.backend
            .tx
            .send(bridge_thread::MaximaLibRequest::ShutdownRequest)
            .unwrap();
    }
}
