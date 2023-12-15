#![feature(slice_pattern)]
use clap::{arg, command, Parser};
//lmao?
//use winapi::um::winuser::{SetWindowLongA, GWL_STYLE, ShowWindow, SW_SHOW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, SetWindowTextA};
use std::{
    cmp::{self, min},
    path::Path,
    rc::Rc,
    sync::Arc,
    thread::sleep,
    time,
};

use eframe::egui_glow;
use eframe::{egui, Frame};
use egui::{
    egui_assert, pos2, style::WidgetVisuals, vec2, Button, Color32, ComboBox, Image, Margin, Mesh,
    Rect, Response, Rounding, Shape, Stroke, Style, TextureId, Ui, Vec2, Visuals,
};
use egui_extras::{Column, RetainedImage, TableBuilder};
use egui_glow::glow;

use game_info_image_handler::{GameImageHandler, GameImageType};
use interact_thread::MaximaThread;
use std::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;

use std::{fs::File, io, panic, path::PathBuf};

use fs::image_loader::{save_image_from_url, ImageLoader};
use game_view_bg_renderer::GameViewBgRenderer;
use translation_manager::TranslationManager;
use views::friends_view::{FriendsViewBar, FriendsViewBarPage, FriendsViewBarStatusFilter};

use maxima::util::log::init_logger;

use views::debug_view::debug_view;
use views::friends_view::friends_view;
use views::game_view::games_view;
use views::settings_view::settings_view;
use views::{
    game_view::GameViewBar, game_view::GameViewBarGenre, game_view::GameViewBarPlatform,
    undefinied_view::undefined_view,
};

mod fs;
mod views;

mod game_info_image_handler;
mod game_view_bg_renderer;
mod interact_thread;
mod translation_manager;

// WHAT THE FUCK IS THIS?????????
use maxima::{
    core::{
        self,
        ecommerce::request_offer_data,
        service_layer::{
            send_service_request, ServiceGetUserPlayerRequest, ServiceUser, ServiceUserGameProduct,
            SERVICE_REQUEST_GETUSERPLAYER,
        },
        Maxima, MaximaEvent,
    },
    ooa::{request_license, save_licenses},
    util::{
        self,
        native::{take_foreground_focus},
        log::LOGGER,
        registry::{check_registry_validity, bootstrap_path, launch_bootstrap, read_game_path},
    },
};

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
        ..Default::default()
    };
    eframe::run_native(
        "Maxima",
        native_options,
        Box::new(move |cc| {
            Box::new({
                
                let mut app = DemoEguiApp::new(cc, args);
                // Run initialization code that needs access to the UI here, but DO NOT run any long-runtime functions here,
                // as it's before the UI is shown
                if !args.no_login {

                    if let Err(err) = check_registry_validity() {
                        println!("{}, fixing...", err);
                        launch_bootstrap().expect("Failed to launch installer");
                    }
                }
                app
            })
        }),
    )
    .expect("Failed i guess?")
}

#[derive(Debug, PartialEq, Default)]
enum PageType {
    #[default]
    Games,
    Store,
    Friends,
    Settings,
    Debug,
}
#[derive(Debug, PartialEq)]
enum InProgressLoginType {
    Oauth,
    UsernamePass
}


//haha,
//fuck.
#[derive(Clone)]
pub struct GameImage {
    /// Holds the actual texture data
    retained: Option<Arc<RetainedImage>>,
    /// Pass to egui to render
    renderable: Option<TextureId>,
    /// Look for this on FS first
    fs_path: String,
    /// If it's not on fs, download it here
    url: String,
}

pub struct GameInfo {
    /// Origin slug of the game
    slug: String,
    /// Origin offer ID of the game
    offer: String,
    /// Display name of the game
    name: String,
    /// DO NOT USE THIS unless you KNOW it's not null.
    icon: Option<Arc<RetainedImage>>,
    /// May be deprecated or otherwise not used, EA doesn't provide them
    icon_renderable: Option<TextureId>,
    /// YOOOOO
    hero : Arc<GameImage>,
    /// The stylized logo of the game
    logo: Arc<GameImage>,
    /// Time (in hours/10) you have logged in the game
    time: u32, // hours/10 allows for better precision, i'm only using one decimal place
    /// Achievements you have unlocked
    achievements_unlocked: u16,
    /// Total achievements in the game
    achievements_total: u16,
    /// Is the game installed
    installed: bool,
    /// Path the game is installed to
    path: String
}

impl GameInfo {
    /// TEST FUNC FOR SHIT IDK LMAO
    pub fn uninstall(&self) {
        println!("Uninstall requested for \"{}\"", self.name);
    }
    /// TEST FUNC FOR SHIT IDK LMAO
    pub fn launch(&self) {
        println!("Launch requested for \"{}\"", self.name);
    }
}

pub struct DemoEguiApp {
    debug: bool,                          // general toggle for showing debug info
    game_view_bar: GameViewBar,           // stuff for the bar on the top of the Games view
    friends_view_bar: FriendsViewBar,     // stuff for the bar on the top of the Friends view
    user_name: String,                    // Logged in user's display name
    user_pfp: Rc<RetainedImage>,          // temp icon for the user's profile picture
    user_pfp_renderable: TextureId,       // actual renderable for the user's profile picture
    games: Vec<GameInfo>,                 // games
    game_sel: usize,                      // selected game
    game_view_rows: bool,                 // if the game view is in rows mode
    page_view: PageType,                  // what page you're on (games, friends, etc)
    needs_first_time_load: bool,          // Don't let this ship, please
    game_image_handler: GameImageHandler, // Game image loader, i will probably replace this with a more robust all images loader
    game_view_bg_renderer: Option<GameViewBgRenderer>, // Renderer for the blur effect in the game view
    locale: TranslationManager,
    critical_bg_thread_crashed: bool, // If a core thread has crashed and made the UI unstable
    backend: MaximaThread,
    logged_in: bool, // temp book to track login status
    in_progress_login: bool, // if the login flow is in progress
    in_progress_login_type: InProgressLoginType // what type of login we're using
}

fn load_games(app: &mut DemoEguiApp) {
    /* could use these with --no-login but i cba since i got login working on linux
    app.games
    .push(GameInfo::new("battlefield-5", "Battlefield V", true));
    app.games
    .push(GameInfo::new("titanfall-2", "Titanfall 2", true));
    app.games
    .push(GameInfo::new("battlefield-4", "Battlefield 4", false));
    */
}

impl DemoEguiApp {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        //might edit these later to make it less obiously egui.
        //i personally like egui's visuals, but that's not
        //particularlly a very professional stance on UI design.
        let vis: Visuals = { Visuals::dark() };
        //awful lot of lines to justify keeping one

        cc.egui_ctx.set_visuals(vis);
        cc.egui_ctx.set_debug_on_hover(args.debug);

        let user_pfp =
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
            user_pfp_renderable: (&user_pfp).texture_id(&cc.egui_ctx),
            user_pfp,
            user_name : "User".to_owned(),
            games: Vec::new(),
            game_sel: 0,
            game_view_rows: false,
            page_view: PageType::Games,
            needs_first_time_load: true,
            game_image_handler: GameImageHandler::new(),
            game_view_bg_renderer: GameViewBgRenderer::new(cc),
            locale: TranslationManager::new("./res/locale/en_us.json".to_owned())
                .expect("Could not load translation file"),
            critical_bg_thread_crashed: false,
            backend: MaximaThread::new(), //please don't fucking break
            logged_in: args.no_login, //temporary hack to just let me work on UI without needing to implement everything on unix lmao
            in_progress_login: false,
            in_progress_login_type: InProgressLoginType::Oauth,
        }
    }
}

// modified from https://github.com/emilk/egui/blob/master/examples/custom_window_frame/src/main.rs
// will be more modified to actually look good later

pub fn tab_bar_button(ui: &mut Ui, res: Response) {
    let mut res2 = Rect::clone(&res.rect);
    res2.min.y = res2.max.y - 4.;
    ui.painter().rect_filled(
        res2,
        Rounding::none(),
        if res.hovered() {
            Color32::from_rgb(92, 92, 92)
        } else {
            Color32::from_rgb(72, 72, 72)
        },
    );
}

fn custom_window_frame(
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
    title: &str,
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
        let title_bar_rect = {
            let mut rect = app_rect;
            rect.max.y = rect.min.y + title_bar_height;
            rect
        };
        #[cfg(target_os = "macos")] //eventually offer this on other platforms, but mac is the only functional one
        title_bar_ui(ui, frame, title_bar_rect, title);
        
        // Add the contents:
        #[cfg(target_os = "macos")]
        let content_rect = {
            let mut rect = app_rect;
            rect.min.y = title_bar_rect.max.y;
            rect
        };
        #[cfg(not(target_os = "macos"))]
        let content_rect = {app_rect};
        
        let mut content_ui = ui.child_ui(content_rect, *ui.layout());

        add_contents(&mut content_ui);
    });
}

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
}

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
}

impl eframe::App for DemoEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let rec = self.game_image_handler.rx.try_recv();
        if let Ok(rcv) = rec {
            for idx in 0..self.games.len() {
                if self.games[idx].slug.eq(&rcv.game_slug) {
                    println!("loading image for slug {}",rcv.game_slug);
                    match rcv.image_type {
                        GameImageType::Icon => {
                        }
                        GameImageType::Hero => {
                            let temp_name = rcv.image.to_owned();
                            let renderable = if temp_name.retained.is_some() { Some(temp_name.retained.clone().expect("what").texture_id(ctx)) } else { None };
                            
                            let assign = GameImage {
                                retained: temp_name.retained.clone(),
                                renderable,
                                fs_path: String::new(),
                                url: String::new(),
                            };
                            self.games[idx].hero = assign.into();
                            
                        }
                        GameImageType::Logo => {
                            let temp_name = rcv.image.to_owned();
                            let renderable = if temp_name.retained.is_some() { Some(temp_name.retained.clone().expect("what").texture_id(ctx)) } else { None };
                            
                            let assign = GameImage {
                                retained: temp_name.retained.clone(),
                                renderable,
                                fs_path: String::new(),
                                url: String::new(),
                            };
                            self.games[idx].logo = assign.into();
                        }
                    }
                }
            }
        } else {
            //println!("lol, lmao");
        }

        if let Ok(rcv) = self.backend.rx.try_recv() {
            match rcv {
                interact_thread::MaximaLibResponse::LoginResponse(res) => {
                    if let Some(name) = res.res {
                        self.logged_in = true;
                        println!("Logged in as {}!", name);
                        self.user_name = name.clone();
                    }
                }
                interact_thread::MaximaLibResponse::GameInfoResponse(res) => {
                    self.games.push(res.game);
                }
                _ => {
                    println!("Recieved something from backend!");
                }
            }

            let mut style: egui::Style = (*ctx.style()).clone();
            style.spacing.window_margin = Margin::same(0.0);
            style.spacing.menu_margin = Margin::same(0.0);
            let panel = egui::CentralPanel::default().frame(
                egui::Frame::window(&style)
                    .inner_margin(Margin::same(0.0))
                    .outer_margin(Margin::same(0.0))
                    .rounding(Rounding::none())
                    .stroke(Stroke::NONE),
            );
        }
        custom_window_frame(ctx, frame, "Maxima", |ui| {
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
                            ui.vertical_centered(|ui| {
                                ui.heading("Not Implemented.");
                            });
                        }
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.heading("You're not logged in.");
                        ui.horizontal(|ui| {
                            if ui.button("OAuth (Browser)").clicked() {
                                self.backend
                                .tx
                                .send(interact_thread::MaximaLibRequest::LoginRequestOauth).unwrap();
                                self.backend
                                .tx
                                .send(interact_thread::MaximaLibRequest::GetGamesRequest).unwrap();
                            }
                            ui.set_enabled(false);
                            if ui.button("Username & Password").clicked() {

                            }
                        })
                    });
                }
                
            } else {
                let mut top_nav_frame = egui::Frame::default();
                top_nav_frame.fill = Color32::from_rgb(19, 19, 19);
                top_nav_frame.outer_margin = Margin::same(0.0);
                top_nav_frame.inner_margin = Margin::same(0.0);
                top_nav_frame.outer_margin.bottom = -2.0;
                top_nav_frame.show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.x = 0.;
                    ui.style_mut().spacing.item_spacing.y = 0.;
                    ui.style_mut().spacing.button_padding.x += 8.;
                    ui.horizontal(|horizonal| {
                        //horizonal.image(texture_id, size)
                        horizonal.style_mut().visuals.button_frame = false;
                        horizonal.style_mut().spacing.item_spacing.x = -2.;
                        horizonal.style_mut().spacing.button_padding.x += 8.;
                        //THIS CODE FUCKING SUCKS
                        let gb_button0 = horizonal.add_sized(
                            [40., 40.],
                            egui::Button::new(&self.locale.localization.menubar.games),
                        );
                        if gb_button0.clicked() {
                            self.page_view = PageType::Games;
                        }
                        if self.page_view == PageType::Games {
                            tab_bar_button(horizonal, gb_button0);
                        }
                        horizonal.separator();
                        let gb_button1 = horizonal.add_sized(
                            [40., 40.],
                            egui::Button::new(&self.locale.localization.menubar.store),
                        );
                        if gb_button1.clicked() {
                            self.page_view = PageType::Store;
                        }
                        if self.page_view == PageType::Store {
                            tab_bar_button(horizonal, gb_button1);
                        }
                        horizonal.separator();
                        let gb_button2 = horizonal.add_sized(
                            [40., 40.],
                            egui::Button::new(&self.locale.localization.menubar.friends),
                        );
                        if gb_button2.clicked() {
                            self.page_view = PageType::Friends;
                        }
                        if self.page_view == PageType::Friends {
                            tab_bar_button(horizonal, gb_button2);
                        }
                        horizonal.separator();
                        let gb_button3 = horizonal.add_sized(
                            [40., 40.],
                            egui::Button::new(&self.locale.localization.menubar.settings),
                        );
                        if gb_button3.clicked() {
                            self.page_view = PageType::Settings;
                        }
                        if self.page_view == PageType::Settings {
                            tab_bar_button(horizonal, gb_button3);
                        }
                        if self.debug {
                            horizonal.separator();
                            let gb_button4 =
                                horizonal.add_sized([40., 40.], egui::Button::new("Debug"));
                            if gb_button4.clicked() {
                                self.page_view = PageType::Debug;
                            }
                            if self.page_view == PageType::Debug {
                                tab_bar_button(horizonal, gb_button4);
                            }
                        }

                        horizonal.separator();
                        horizonal.style_mut().spacing.item_spacing.x = 5.;
                        horizonal.style_mut().spacing.button_padding.x -= 8.;
                        horizonal.set_enabled(true);
                        horizonal.style_mut().visuals.button_frame = true;
                        horizonal.allocate_space(vec2(horizonal.available_width() - 300., 40.));

                        horizonal.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |rtl| {
                                rtl.style_mut().spacing.item_spacing = vec2(5., 5.);
                                rtl.style_mut().spacing.button_padding = vec2(0., 0.);
                                rtl.allocate_space(vec2(0., 0.));

                                rtl.menu_image_button(
                                    self.user_pfp_renderable,
                                    vec2(35., 35.),
                                    |ui| {
                                        if ui
                                            .button(
                                                &self.locale.localization.profile_menu.view_profile,
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
                                rtl.label(self.user_name.clone());
                            },
                        );
                    });
                });

                let mut content_frame = egui::Frame::default();
                content_frame.outer_margin = Margin::same(0.0);
                content_frame.inner_margin = Margin::same(4.0);

                content_frame.show(ui, |content| match self.page_view {
                    PageType::Games => games_view(self, content),
                    PageType::Friends => friends_view(self, content),
                    PageType::Settings => settings_view(self, content),
                    PageType::Debug => debug_view(self, content),
                    _ => undefined_view(self, content),
                });
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.game_image_handler.shutdown();
    }
}
