#![feature(slice_pattern)]
use clap::{arg, command, Parser};

//lmao?
//use winapi::um::winuser::{SetWindowLongA, GWL_STYLE, ShowWindow, SW_SHOW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, SetWindowTextA};
use log::{error, info, warn};
use ui_image::UIImage;
use std::{ops::RangeInclusive, rc::Rc, sync::Arc};

use eframe::egui;
use eframe::egui_glow;
use egui::{
    style::{WidgetVisuals, Widgets},
    vec2, Color32, FontData, FontDefinitions, FontFamily, Margin, Rect, Response, Rounding, Stroke,
    TextureId, Ui, Vec2, Visuals,
};
use egui_extras::{RetainedImage, Size, StripBuilder};
use egui_glow::glow;

use interact_thread::MaximaThread;

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

mod ui_image;
mod interact_thread;
mod renderers;
mod translation_manager;
mod frontend_processor;

use maxima::util::registry::check_registry_validity;

const ACCENT_COLOR: Color32 = Color32::from_rgb(8, 171, 244);
const APP_MARGIN: Vec2 = vec2(12.0, 12.0); //TODO: user setting

#[derive(Parser, Debug, Copy, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    debug: bool,
    #[arg(short, long)]
    no_login: bool,
}

#[tokio::main]
async fn main() {
    init_logger();
    let args = Args::parse();

    let native_options = eframe::NativeOptions {
        transparent: true,
        #[cfg(target_os = "macos")]
        fullsize_content: true,
        initial_window_size: Some(vec2(1280.0, 720.0)),
        min_window_size: Some(vec2(940.0, 480.0)),
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
                return Box::new(app);
            }
            if let Err(err) = check_registry_validity() {
                warn!("{}, fixing...", err);
                // this is if let in case set_up_registry ever returns something useful, instead of bailing
                if let Err(_er) = set_up_registry() {
                    error!("Registry setup failed!");
                }
            }
            Box::new(app)
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
    Debug,
}
#[derive(Debug, PartialEq)]
enum InProgressLoginType {
    Oauth,
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
    debug: bool,                      // general toggle for showing debug info
    game_view_bar: GameViewBar,       // stuff for the bar on the top of the Games view
    friends_view_bar: FriendsViewBar, // stuff for the bar on the top of the Friends view
    user_name: String,                // Logged in user's display name
    _user_pfp: Rc<RetainedImage>,     // temp icon for the user's profile picture
    user_pfp_renderable: TextureId,   // actual renderable for the user's profile picture //TODO
    games: Vec<GameInfo>,             // games
    game_sel: usize,                  // selected game
    //game_view_rows: bool,                               // if the game view is in rows mode
    page_view: PageType, // what page you're on (games, friends, etc)
    game_view_bg_renderer: Option<GameViewBgRenderer>, // Renderer for the blur effect in the game view
    game_view_frac: f32, // Multi-purpose fraction, how far along the bottom edge of the initial bottom edge of the hero image has scrolled up
    app_bg_renderer: Option<AppBgRenderer>, // Renderer for the app's background
    locale: TranslationManager, // Translations
    critical_bg_thread_crashed: bool, // If a core thread has crashed and made the UI unstable
    backend: MaximaThread, // pepega
    logged_in: bool,     // temp book to track login status
    in_progress_login: bool, // if the login flow is in progress
    in_progress_login_type: InProgressLoginType, // what type of login we're using
    in_progress_username: String, // Username buffer for logging in with a username/password
    in_progress_password: String, // Password buffer for logging in with a username/password
    in_progress_credential_status: String, // Errors info etc for logging in with a username/password
    credential_login_in_progress: bool, // Currently waiting on the maxima thread to log us in with credentials
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);

const WIDGET_HOVER: Color32 = Color32::from_rgb(255, 188, 61);

impl DemoEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let vis: Visuals = Visuals {
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
                    rounding: Rounding::none(),
                    expansion: 0.0,
                },
                inactive: WidgetVisuals {
                    weak_bg_fill: Color32::BLACK,
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
                    rounding: Rounding::none(),
                    expansion: 0.0,
                },
                open: WidgetVisuals {
                    weak_bg_fill: WIDGET_HOVER.linear_multiply(0.0),
                    bg_fill: WIDGET_HOVER.linear_multiply(0.0),
                    bg_stroke: Stroke::NONE,
                    fg_stroke: Stroke::new(2.0, WIDGET_HOVER.linear_multiply(0.0)),
                    rounding: Rounding::none(),
                    expansion: 0.0,
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

        cc.egui_ctx.set_visuals(vis);
        cc.egui_ctx.set_fonts(fonts);

        cc.egui_ctx.set_debug_on_hover(args.debug);

        let _user_pfp =
            Rc::new(ImageLoader::load_from_fs("./res/usericon_tmp.png").expect("fuck, i guess?"));

        Self {
            debug: args.debug,
            game_view_bar: GameViewBar {
                genre_filter: GameViewBarGenre::AllGames,
                platform_filter: GameViewBarPlatform::AllPlatforms,
                game_size: 2.0,
                search_buffer: String::new(),
            },
            friends_view_bar: FriendsViewBar {
                page: FriendsViewBarPage::Online,
                status_filter: FriendsViewBarStatusFilter::Name,
                search_buffer: String::new(),
            },
            user_pfp_renderable: (&_user_pfp).texture_id(&cc.egui_ctx),
            _user_pfp,
            user_name: "User".to_owned(),
            games: Vec::new(),
            game_sel: 0,
            //game_view_rows: false,
            page_view: PageType::Games,
            game_view_bg_renderer: GameViewBgRenderer::new(cc),
            game_view_frac: 0.0,
            app_bg_renderer: AppBgRenderer::new(cc),
            locale: TranslationManager::new("./res/locale/en_us.json".to_owned())
                .expect("Could not load translation file"),
            critical_bg_thread_crashed: false,
            backend: MaximaThread::new(&cc.egui_ctx), //please don't fucking break
            logged_in: args.no_login, // largely deprecated but i'm going to keep it here
            in_progress_login: false,
            in_progress_login_type: InProgressLoginType::Oauth,
            in_progress_username: String::new(),
            in_progress_password: String::new(),
            in_progress_credential_status: String::new(),
            credential_login_in_progress: false,
        }
    }
}

// modified from https://github.com/emilk/egui/blob/master/examples/custom_window_frame/src/main.rs

pub fn tab_bar_button(ui: &mut Ui, res: Response) {
    let mut res2 = Rect::clone(&res.rect);
    res2.min.y = res2.max.y - 4.;
    ui.painter().vline(
        res2.min.x + 2.0,
        RangeInclusive::new(res2.min.y, res2.max.y),
        Stroke::new(2.0, ACCENT_COLOR),
    );
    ui.painter().rect_filled(
        res2,
        Rounding::none(),
        if res.hovered() {
            ACCENT_COLOR
        } else {
            ACCENT_COLOR.linear_multiply(0.9)
        },
    );
}

fn custom_window_frame(
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
    _title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    use egui::*;

    let panel_frame = egui::Frame {
        fill: ctx.style().visuals.window_fill(),
        rounding: 0.0.into(),
        stroke: Stroke::NONE,
        outer_margin: if frame.info().window_info.maximized {
            0.0.into()
        } else {
            0.0.into()
        },
        ..Default::default()
    };

    CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
        let app_rect = ui.max_rect();

        let title_bar_height = 28.0; //height on a standard monitor on macOS monterey
        let _title_bar_rect = {
            let mut rect = app_rect;
            rect.max.y = rect.min.y + title_bar_height;
            rect
        };
        #[cfg(target_os = "macos")]
        //eventually offer this on other platforms, but mac is the only functional one
        title_bar_ui(ui, frame, _title_bar_rect, title);

        // Add the contents:
        #[cfg(target_os = "macos")]
        let content_rect = {
            let mut rect = app_rect;
            rect.min.y = _title_bar_rect.max.y;
            rect
        };
        #[cfg(not(target_os = "macos"))]
        let content_rect = Rect {
            min: app_rect.min + APP_MARGIN,
            max: app_rect.max - APP_MARGIN,
        };

        let mut content_ui = ui.child_ui(content_rect, *ui.layout());

        add_contents(&mut content_ui);
    });
}

/* for custom window title bar thing, mostly aesthetic reasons
fn title_bar_ui(
    ui: &mut egui::Ui,
    frame: &mut eframe::Frame,
    title_bar_rect: eframe::epaint::Rect,
    title: &str,
) {
    use egui::*;

    let painter = ui.painter();

    let title_bar_response = ui.interact(title_bar_rect, Id::new("title_bar"), Sense::click());

    // Paint the title:
    painter.text(
        title_bar_rect.center(),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(20.0),
        ui.style().visuals.text_color(),
    );

    // Paint the line under the title:
    painter.line_segment(
        [
            title_bar_rect.left_bottom() + vec2(1.0, 0.0),
            title_bar_rect.right_bottom() + vec2(-1.0, 0.0),
        ],
        ui.visuals().widgets.noninteractive.bg_stroke,
    );

    // Interact with the title bar (drag to move window):
    if title_bar_response.double_clicked() {
        frame.set_maximized(!frame.info().window_info.maximized);
    } else if title_bar_response.is_pointer_button_down_on() {
        frame.drag_window();
    }

    ui.allocate_ui_at_rect(title_bar_rect, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.visuals_mut().button_frame = false;
            #[cfg(not(target_os = "macos"))]
            close_maximize_minimize(ui, frame);
        });
    });
} */
/* wrapper/help func to avoid nesting hell in custom window decorations
/// Show some close/maximize/minimize buttons for the native window.
fn close_maximize_minimize(ui: &mut egui::Ui, frame: &mut eframe::Frame) {
    use egui::{Button, RichText};

    let button_height = 12.0;
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::LIGHT_RED;
    ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::RED;

    let close_response = ui.add_sized(
        vec2(42.0, 32.0),
        Button::new(RichText::new("❌"))
            .rounding(Rounding::none())
            .stroke(Stroke::NONE),
    );
    if close_response.clicked() {
        frame.close();
    }

    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::from_black_alpha(50);
    ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_black_alpha(70);

    if frame.info().window_info.maximized {
        let maximized_response = ui.add_sized(
            vec2(42.0, 32.0),
            Button::new(RichText::new("🗗"))
                .rounding(Rounding::none())
                .stroke(Stroke::NONE),
        );
        if maximized_response.clicked() {
            frame.set_maximized(false);
        }
    } else {
        let maximized_response = ui.add_sized(
            vec2(42.0, 32.0),
            Button::new(RichText::new("🗗"))
                .rounding(Rounding::none())
                .stroke(Stroke::NONE),
        );
        if maximized_response.clicked() {
            frame.set_maximized(true);
        }
    }

    let minimized_response = ui.add_sized(
        vec2(42.0, 32.0),
        Button::new(RichText::new("🗕"))
            .rounding(Rounding::none())
            .stroke(Stroke::NONE),
    );
    if minimized_response.clicked() {
        frame.set_minimized(true);
    }
} */

/// Wrapper/helper for the tab buttons in the top left of the app
fn tab_button(ui: &mut Ui, edit_var: &mut PageType, page: PageType, label: String) {
    ui.style_mut().visuals.widgets.inactive.rounding = Rounding::none();
    ui.style_mut().visuals.widgets.active.rounding = Rounding::none();
    ui.style_mut().visuals.widgets.hovered.rounding = Rounding::none();

    if edit_var == &page {
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.active.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.active.bg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::WHITE;
        ui.style_mut().visuals.widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE; //this one behaves differently, idk why.
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

    let test = ui.add_sized([120.0, 30.0], egui::Button::new(text));
    if test.clicked() {
        *edit_var = page.clone();
    }
}

impl eframe::App for DemoEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        frontend_processor::frontend_processor(self, ctx);

        custom_window_frame(ctx, frame, "Maxima", |ui| {
            if let Some(render) = &self.app_bg_renderer {
                let mut fullrect = ui.available_rect_before_wrap().clone();
                fullrect.min -= APP_MARGIN;
                fullrect.max += APP_MARGIN;
                if self.page_view == PageType::Games
                    && self.logged_in
                    && self.games.len() > self.game_sel
                {
                    match &self.games[self.game_sel].images {
                        GameUIImagesWrapper::Unloaded | GameUIImagesWrapper::Loading => {
                            render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1));
                        }
                        GameUIImagesWrapper::Available(images) => {
                            render.draw(ui, fullrect, images.hero.size, images.hero.renderable);
                        }
                    }
                } else {
                    render.draw(ui, fullrect, fullrect.size(), TextureId::Managed(1));
                }
            }
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
                                            interact_thread::MaximaLibRequest::LoginRequestUserPass(
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
                                        - (330.0 + ui.style().spacing.item_spacing.x))
                                        / 2.0,
                                    0.0,
                                ),
                                egui::Sense::click(),
                            );

                            if ui
                                .add_sized(
                                    [160.0, 60.0],
                                    egui::Button::new(&self.locale.localization.login.oauth_option),
                                )
                                .clicked()
                            {
                                self.in_progress_login_type = InProgressLoginType::Oauth;
                                self.in_progress_login = true;
                                self.backend
                                    .tx
                                    .send(interact_thread::MaximaLibRequest::LoginRequestOauth)
                                    .unwrap();
                            }
                            if ui
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
                            }
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
                                        &self.locale.localization.errors.critical_thread_crashed,
                                    )
                                    .color(Color32::BLACK)
                                    .size(16.0),
                                );
                            });
                        });
                }
                StripBuilder::new(ui)
                    .size(Size::exact(main_width))
                    .size(Size::exact(size_width))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.spacing_mut().item_spacing.y = outside_spacing;
                            let avail_height = ui.available_height() - (32.0 + outside_spacing);
                            StripBuilder::new(ui)
                                .size(Size::exact(32.0))
                                .size(Size::exact(avail_height))
                                .vertical(|mut strip| {
                                    strip.cell(|header| {
                                        //header.painter().rect_filled(header.available_rect_before_wrap(), Rounding::none(), Color32::from_white_alpha(20));
                                        let navbar = egui::Frame::default()
                                            .stroke(Stroke::new(2.0, Color32::WHITE))
                                            //.fill(Color32::TRANSPARENT)
                                            .outer_margin(Margin::same(1.0))
                                            .rounding(Rounding::same(4.0));
                                        navbar.show(header, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 0.0;
                                                ui.style_mut().visuals.widgets.inactive.rounding =
                                                    Rounding::none();
                                                // BEGIN TAB BUTTONS
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Games,
                                                    self.locale.localization.menubar.games.clone(),
                                                );
                                                tab_button(
                                                    ui,
                                                    &mut self.page_view,
                                                    PageType::Store,
                                                    self.locale.localization.menubar.store.clone(),
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
                                    strip.cell(|body| {
                                        //body.painter().rect_filled(body.available_rect_before_wrap(), Rounding::none(), Color32::DARK_GREEN);
                                        match self.page_view {
                                            PageType::Games => games_view(self, body),
                                            PageType::Settings => settings_view(self, body),
                                            PageType::Debug => debug_view(self, body),
                                            _ => undefined_view(self, body),
                                        }
                                    })
                                });
                        });
                        strip.cell(|ui| {
                            ui.spacing_mut().item_spacing.y = outside_spacing;
                            let avail_height = ui.available_height() - (40.0);
                            StripBuilder::new(ui)
                                .size(Size::exact(32.0))
                                .size(Size::exact(avail_height))
                                .vertical(|mut strip| {
                                    strip.cell(|header| {
                                        //header.painter().rect_filled(header.available_rect_before_wrap(), Rounding::none(), Color32::from_white_alpha(20));
                                        let navbar = egui::Frame::default()
                                            .stroke(Stroke::new(2.0, Color32::WHITE))
                                            //.fill(Color32::BLACK)
                                            .outer_margin(Margin::same(1.0))
                                            .inner_margin(Margin::same(-2.0))
                                            .rounding(Rounding::same(4.0));
                                        navbar.show(header, |ui| {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |rtl| {
                                                    rtl.visuals_mut().widgets.inactive.bg_stroke =
                                                        Stroke::new(2.0, Color32::GREEN);
                                                    rtl.menu_image_button(
                                                        self.user_pfp_renderable,
                                                        vec2(30.0, 30.0),
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
                                                        egui::RichText::new(self.user_name.clone())
                                                            .size(15.0)
                                                            .color(Color32::WHITE),
                                                    );
                                                },
                                            );
                                        });
                                    });
                                    strip.cell(|body| {
                                        //body.painter().rect_filled(body.available_rect_before_wrap(), Rounding::none(), Color32::DARK_BLUE);
                                        friends_view(self, body);
                                    })
                                });
                        });
                    });
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.backend
            .tx
            .send(interact_thread::MaximaLibRequest::ShutdownRequest)
            .unwrap();
    }
}
