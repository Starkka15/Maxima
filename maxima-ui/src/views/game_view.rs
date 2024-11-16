use eframe::glow::OBJECT_TYPE;
use egui::{pos2, vec2, Color32, Margin, Mesh, Pos2, Rect, RichText, Rounding, ScrollArea, Shape, Stroke, Ui};
use log::debug;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use crate::{bridge_thread, set_app_modal, widgets::enum_dropdown::enum_dropdown, GameDetails, GameDetailsWrapper, GameInfo, InstallModalState, MaximaEguiApp, PageType, PopupModal};

use strum_macros::EnumIter;

#[derive(Debug, PartialEq, Default, EnumIter)]
pub enum GameViewBarGenre {
  #[default] AllGames,
  Shooters,
  Simulation
}

#[derive(Debug, PartialEq, Default, EnumIter)]
pub enum GameViewBarPlatform {
  #[default] AllPlatforms,
  Windows,
  Mac
}

pub struct GameViewBar {
  pub genre_filter : GameViewBarGenre,        // game type filter on the game sort bar
  pub platform_filter : GameViewBarPlatform,  // platform filter on the game sort bar
  pub game_size : f32,                        // game icon/art size slider on the game sort bar
  pub search_buffer : String,                 // search text on the game sort bar
}

const SKELETON_TEXT_COLOR: Color32 = Color32::from_rgba_premultiplied(53, 53, 53, 128);
const SKELETON_INFO_COLOR: Color32 = Color32::from_rgba_premultiplied(127, 90, 26, 128);

fn skeleton_text_block(ui: &mut egui::Ui, width: f32, height: f32) {
  let mut skeleton_rect = ui.available_rect_before_wrap();
  skeleton_rect.set_width(width);
  skeleton_rect.set_height(height);
  ui.painter().rect_filled(skeleton_rect, Rounding::same(2.0), SKELETON_TEXT_COLOR);
  ui.allocate_space(vec2(width,height));
}


fn skeleton_text_block1(ui: &mut egui::Ui, width: f32, width1: f32, height: f32) {
  let mut skeleton_rect = ui.available_rect_before_wrap();
  skeleton_rect.set_width(width);
  skeleton_rect.set_height(height);
  ui.painter().rect_filled(skeleton_rect, Rounding::same(2.0), SKELETON_INFO_COLOR);
  skeleton_rect.min.x = skeleton_rect.max.x + ui.spacing().item_spacing.x;
  skeleton_rect.set_width(width1); 
  ui.painter().rect_filled(skeleton_rect, Rounding::same(2.0), SKELETON_TEXT_COLOR);
  ui.allocate_space(vec2(width + width1 + ui.spacing().item_spacing.x,height));
}

pub fn game_view_details_panel(app : &mut MaximaEguiApp, ui: &mut Ui) {
    puffin::profile_function!();
    if app.games.len() < 1 { return }
    let game: &mut GameInfo = if let Some(game) = app.games.get_mut(&app.game_sel) { game } else { return };
    let (hero, logo) = {
      (
        app.img_cache.get(crate::ui_image::UIImageType::Hero(game.slug.clone())),
        app.img_cache.get(crate::ui_image::UIImageType::Logo(game.slug.clone()))
      )
    };

    let game_details: Option<&GameDetails> = match &game.details {
        GameDetailsWrapper::Unloaded => {
            debug!("Loading details for {:?}", game.name);
            app.backend.backend_commander.send(bridge_thread::MaximaLibRequest::GetGameDetailsRequest(game.slug.clone())).unwrap();
            game.details = GameDetailsWrapper::Loading;
            None
        },
        GameDetailsWrapper::Loading => {
            None
        },
        GameDetailsWrapper::Available(details) => {
            Some(details) },
    };

    let game = game.clone();

    let mut hero_rect = Rect::clone(&ui.available_rect_before_wrap());
    let aspect_ratio: f32 = 
    if let Some(handle) = &hero {
        handle.aspect_ratio()
    } else {
        16.0 / 9.0
    };
    let style = ui.style_mut();
    style.visuals.clip_rect_margin = 0.0;
    style.spacing.item_spacing = vec2(0.0,0.0);
    hero_rect.max.x -= style.spacing.scroll.bar_width + style.spacing.scroll.bar_inner_margin;
    hero_rect.max.y = hero_rect.min.y + (hero_rect.size().x / aspect_ratio);
    if hero_rect.size().x > 650.0 {
        hero_rect.max.y = hero_rect.min.y + (650.0 / aspect_ratio);
    }

    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
    ui.vertical(|ui| {
    
    // scrollbar
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
    ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(4.0);
    ui.style_mut().visuals.widgets.active.rounding = Rounding::same(4.0);
    ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(4.0);
    
    let mut logo_transition_frac = 0.0;
    ScrollArea::vertical()
    .auto_shrink(false)
    .id_source("GameViewPanel_ScrollerArea")
    .show(ui, |ui| {
        puffin::profile_scope!("details");
        let hero_height_capped = hero_rect.size().y.max(0.0);
        ui.allocate_space(vec2(0.0, hero_height_capped));
        let content_region_start = &ui.cursor().min.y;
        let mut hero_vis_frac = (content_region_start - hero_rect.min.y) / (hero_rect.max.y - hero_rect.min.y); // how much of the hero image is visible
        hero_vis_frac = if hero_vis_frac > 1.0 { 0.0 } else if hero_vis_frac < 0.0 { 1.0 } else { 1.0 - hero_vis_frac }; // clamping/inverting
        logo_transition_frac = bezier_ease(hero_vis_frac);

        { puffin::profile_scope!("hero image");
        if let Some(handle) = hero {
            if let Some(gvbg) = &app.game_view_bg_renderer {
            gvbg.draw(ui, hero_rect, handle.size_vec2(), handle.id(), hero_vis_frac, app.settings.performance_settings);
            //TODO: negative allocation fix
            //ui.allocate_space(hero_rect.size().max(vec2(0.0, 0.0)));
            }
            //ui.allocate_space(vec2(0.0,-hero_height_capped));
        } else {
            ui.painter().rect_filled(hero_rect, Rounding::same(0.0), Color32::TRANSPARENT);
        }
        }

        if hero_vis_frac < 1.0 /* && hero.is_some() */ {
        // TODO: find a better solution
        let mut mesh = Mesh::default();
        
        let hero_tint = if ui.is_enabled() { Color32::from_black_alpha(20) } else { Color32::from_black_alpha(174) };
        mesh.colored_vertex(hero_rect.left_bottom() - vec2(0.0, hero_rect.height() * hero_vis_frac), hero_tint);
        mesh.colored_vertex(hero_rect.right_bottom() - vec2(0.0, hero_rect.height() * hero_vis_frac), hero_tint);
        mesh.colored_vertex(hero_rect.right_top(), hero_tint);
        mesh.colored_vertex(hero_rect.left_top(), hero_tint);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3); 
        ui.painter().add(Shape::mesh(mesh));
        }

        let avoid_scrollbar_margin = Margin {
        left: 0.0,
        right: ui.style().spacing.scroll.bar_width + ui.style().spacing.scroll.bar_inner_margin,
        top: 0.0,
        bottom: 0.0,
        };

        let mut bar_rounding = Rounding::same(3.0);
        bar_rounding.nw = 0.0;
        bar_rounding.ne = 0.0;
        let play_bar_frame = egui::Frame::default()
        //.fill(Color32::from_black_alpha(120))
        .outer_margin(avoid_scrollbar_margin)
        .rounding(Rounding::ZERO);
        //.inner_margin(Margin::same(4.0));
        //.outer_margin(Margin::same(4.0));
        play_bar_frame.show(ui, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let stats_frame = egui::Frame::default()
            .fill(Color32::WHITE)
            .rounding(bar_rounding)
            .inner_margin(Margin::same(4.0));
            stats_frame.show(ui, |stats| {
            puffin::profile_scope!("stats");
            stats.horizontal(|stats| {
                stats.style_mut().spacing.item_spacing.x = 4.0;
                if let Some(details) = game_details {
                stats.label(
                    RichText::new(&app.locale.localization.games_view.main.playtime)
                    .color(Color32::BLACK)
                    .strong()
                );
                stats.label(
                    RichText::new(format!(": {:?} hours", details.time as f32 / 10.0))
                    .color(Color32::BLACK)
                );
                stats.separator();
                stats.label(
                    RichText::new(&app.locale.localization.games_view.main.achievements)
                    .color(Color32::BLACK)
                    .strong()
                );
                stats.label(
                    RichText::new(format!(": {:?} / {:?}", details.achievements_unlocked, details.achievements_total))
                    .color(Color32::BLACK)
                );
                } else {
                let mut skeleton_rect = stats.available_rect_before_wrap();
                skeleton_rect.set_width(126.0);
                stats.painter().rect_filled(skeleton_rect, Rounding::same(2.0), SKELETON_TEXT_COLOR);
                stats.allocate_space(vec2(126.0,0.0));
                stats.separator();
                skeleton_rect = stats.available_rect_before_wrap();
                skeleton_rect.set_width(126.0);
                stats.painter().rect_filled(skeleton_rect, Rounding::same(2.0), SKELETON_TEXT_COLOR);
                }

                stats.allocate_space(vec2(stats.available_width(),0.0));
            });
            });
            
            let buttons_frame = egui::Frame::default()
            .outer_margin(Margin::symmetric(0.0, 8.0))
            .fill(Color32::TRANSPARENT);
            buttons_frame.show(ui, |buttons| {
            puffin::profile_scope!("action buttons");
            buttons.horizontal(|buttons| {
                buttons.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                buttons.style_mut().spacing.item_spacing.x = 8.0;

                if if let Some(slug) = &app.playing_game { slug.eq(&game.slug) } else { false } {
                let play_str = { "  ".to_string() + &app.locale.localization.games_view.main.stop.to_uppercase() + "  " };
                    if buttons.add(egui::Button::new(egui::RichText::new(play_str)
                    .size(20.0)
                    .color(Color32::WHITE))
                    .rounding(Rounding::same(2.0))
                    .min_size(vec2(50.0,40.0))
                    ).clicked() {
                    //TODO
                    }
                } else {
                if game.installed {
                    let play_str = { "  ".to_string() + &app.locale.localization.games_view.main.play.to_uppercase() + "  " };
                    if buttons.add_enabled(app.playing_game.is_none(),egui::Button::new(egui::RichText::new(play_str)
                    .size(20.0)
                    .color(Color32::WHITE))
                    .rounding(Rounding::same(2.0))
                    .min_size(vec2(50.0,40.0))
                    ).clicked() {
                    app.playing_game = Some(game.slug.clone());
                    let settings = app.settings.game_settings.get(&game.slug);
                    let settings = if let Some(settings) = settings {
                      Some(settings.to_owned())
                    } else { None };
                    let _ = app.backend.backend_commander.send(crate::bridge_thread::MaximaLibRequest::StartGameRequest(game.clone(), settings));
                    }
                } else if app.install_queue.contains_key(&game.offer) || app.installing_now.as_ref().is_some_and(|n| n.offer.eq(&game.offer)) {
                    let install_str = { "  ".to_string() + &app.locale.localization.games_view.main.resume.to_uppercase() + "  " };
                    if buttons.add(egui::Button::new(egui::RichText::new(install_str)
                    .size(20.0)
                    .color(Color32::WHITE))
                    .rounding(Rounding::same(2.0))
                    .min_size(vec2(50.0,40.0))
                    ).clicked() {
                    app.page_view = PageType::Downloads;
                    }
                } else {
                    let install_str = { "  ".to_string() + &app.locale.localization.games_view.main.install.to_uppercase() + "  " };
                    if buttons.add(egui::Button::new(egui::RichText::new(install_str)
                    .size(20.0)
                    .color(Color32::WHITE))
                    .rounding(Rounding::same(2.0))
                    .min_size(vec2(50.0,40.0))
                    ).clicked() {
                        set_app_modal!(app, Some(PopupModal::GameInstall(game.slug.clone())));
                    //app.modal = Some(PopupModal::GameInstall(game.slug.clone()));
                    //app.installer_state = InstallModalState::new(&app.settings);
                    }
                }
                }
                
                /* buttons.set_enabled(false);
                if buttons.add(egui::Button::new(egui::RichText::new("  ⮋ Download  ")
                .size(26.0)
                .color(Color32::WHITE))
                .rounding(Rounding::same(2.0))
                .min_size(vec2(50.0,50.0))
                ).clicked() {
                } */
                let settings_str = { "  ".to_string() + &app.locale.localization.games_view.main.settings.to_uppercase() + "  " };
                if buttons.add(egui::Button::new(egui::RichText::new(settings_str)
                .size(20.0)
                .color(Color32::WHITE))
                .rounding(Rounding::same(2.0))
                .min_size(vec2(50.0,40.0))
                ).clicked() {
                    set_app_modal!(app, Some(PopupModal::GameSettings(game.slug.clone())));
                }
            });
            });

        });
        });
        ui.vertical(|ui| {
        puffin::profile_scope!("description");
        
        ui.style_mut().spacing.item_spacing = vec2(5.0,5.0);
        
        let req_width = ((ui.available_size_before_wrap().x - avoid_scrollbar_margin.right) - 5.0) / 2.0;
        ui.horizontal(|sysreq| {
            puffin::profile_scope!("system requirements");
            if let Some(details) = game_details {

            sysreq.vertical(|min| {
                puffin::profile_scope!("minimum");
                min.set_min_width(req_width);
                min.set_max_width(req_width);
                min.heading(&app.locale.localization.games_view.details.min_system_req);
                egui_demo_lib::easy_mark::easy_mark(min, &details.system_requirements_min);
            });
            sysreq.vertical(|rec| {
                puffin::profile_scope!("recommended");
                rec.set_min_width(req_width);
                rec.set_max_width(req_width);
                rec.heading(&app.locale.localization.games_view.details.rec_system_req);
                egui_demo_lib::easy_mark::easy_mark(rec, &details.system_requirements_rec);
            });
            } else {

            sysreq.vertical(|min| {
                puffin::profile_scope!("minimum skeleton");
                min.set_min_width(req_width);
                min.set_max_width(req_width);
                
                skeleton_text_block(min, 248.0, 24.0);
                skeleton_text_block1(min, 20.0,70.0, 13.0);
                skeleton_text_block1(min, 25.0, 199.0, 13.0);
                skeleton_text_block1(min, 27.0, 135.0, 13.0);
                skeleton_text_block1(min, 69.0, 100.0, 13.0);
                skeleton_text_block1(min, 27.0, 257.0, 13.0);
                skeleton_text_block1(min, 28.0, 188.0, 13.0);
                skeleton_text_block1(min, 22.0, 62.0, 13.0);
            });
            sysreq.vertical(|rec| {
                puffin::profile_scope!("recommended skeleton");
                rec.set_min_width(req_width);
                rec.set_max_width(req_width);

                skeleton_text_block(rec, 296.0, 24.0);
                skeleton_text_block1(rec,20.0, 70.0, 13.0);
                skeleton_text_block1(rec,25.0, 290.0, 13.0);
                skeleton_text_block1(rec,27.0, 139.0, 13.0);
                skeleton_text_block1(rec,69.0, 149.0, 13.0);
                skeleton_text_block1(rec,27.0, 185.0, 13.0);
                skeleton_text_block1(rec,28.0, 196.0, 13.0);
                skeleton_text_block1(rec,22.0, 64.0, 13.0);
            });
            }
        });

        ui.horizontal_wrapped(|dlc| {
            puffin::profile_scope!("dlc");
            for package in game.dlc {
                
                let (rect, _) = dlc.allocate_exact_size(vec2(400.0, 200.0), egui::Sense::click());
                dlc.painter().rect_filled(rect, Rounding::same(4.0), Color32::from_black_alpha(128));
                dlc.allocate_ui_at_rect(rect, |dlc| {
                    let _ = match package.product().status() {
                        maxima::core::service_layer::ServiceOwnershipStatus::Active => true,
                        maxima::core::service_layer::ServiceOwnershipStatus::Disabled => false,
                    };

                    dlc.heading(format!("{}", package.offer().display_name()));
                });
            }
        });

        {
            puffin::profile_scope!("filler");
            for _idx in 0..75 {
            ui.heading("");
            }
        }
        });

    }); // ScrollArea
      if let Some(handle) = logo {
        let logo_size_pre = if handle.size_vec2().x >= handle.size_vec2().y {
            // wider than it is tall, scale based on X as max
            let mult_frac = 320.0 / handle.size_vec2().x;
            handle.size_vec2().y * mult_frac
        } else {
            // taller than it is wide, scale based on Y
            // fringe edge case, here in case EA decides they want to pull something really fucking stupid
            0.0 // TODO:: CALCULATE IT
        };
        let frac2 = logo_transition_frac.clone();
        let logo_size = vec2(egui::lerp(320.0..=160.0, frac2), egui::lerp(logo_size_pre..=(logo_size_pre/2.0), frac2));
        let logo_rect = Rect::from_min_max(
            Pos2 { x: (egui::lerp(hero_rect.min.x..=hero_rect.max.x-180.0, frac2)), y: (hero_rect.min.y) },
            Pos2 { x: (egui::lerp(hero_rect.max.x..=hero_rect.max.x-20.0, frac2)), y: (egui::lerp(hero_rect.max.y..=hero_rect.min.y+80.0, frac2)) }
        );
        ui.put(logo_rect, egui::Image::new((handle.id(), logo_size)));
      }
    }); // Vertical
    
}

fn game_list_button_context_menu(app : &MaximaEguiApp, game : &GameInfo, ui : &mut Ui) {
  ui.add_enabled_ui(app.playing_game.is_none(), |play_button| {
    if play_button.button("▶ Play").clicked() {
      let settings = app.settings.game_settings.get(&game.slug);
      let settings = if let Some(settings) = settings {
        Some(settings.to_owned())
      } else { None };
      let _ = app.backend.backend_commander.send(crate::bridge_thread::MaximaLibRequest::StartGameRequest(game.clone(), settings));
      play_button.close_menu();
    }
  });
  ui.separator();
  if ui.button("UNINSTALL").clicked() {
    ui.close_menu();
  }
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);
const DARK_GREY: Color32 = Color32::from_rgb(53, 53, 53);

fn show_game_list_buttons(app : &mut MaximaEguiApp, ui : &mut Ui) {
  puffin::profile_function!();
  let matcher = SkimMatcherV2::default();
  let icon_size = vec2(10. * app.game_view_bar.game_size,10. * app.game_view_bar.game_size);
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE; //scroll bar
    //create a rect that takes up all the vertical space in the window, and prohibits anything from going beyond that without us knowing, so we can add a scroll bar
    //because apparently some dumb fucks (me) buy EA games and can overflow the list on the default window size
    ui.vertical(|ui| {
      ui.vertical(|filter_chunk| {
        filter_chunk.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;

        filter_chunk.visuals_mut().widgets.inactive.expansion = 0.0;
        filter_chunk.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        filter_chunk.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(2.0, DARK_GREY);
        filter_chunk.visuals_mut().widgets.inactive.rounding = Rounding::same(2.0);

        filter_chunk.visuals_mut().widgets.active.bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.active.weak_bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        filter_chunk.visuals_mut().widgets.active.bg_stroke = Stroke::new(2.0, DARK_GREY);
        filter_chunk.visuals_mut().widgets.active.rounding = Rounding::same(2.0);

        filter_chunk.visuals_mut().widgets.hovered.bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        filter_chunk.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(2.0, F9B233);
        filter_chunk.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(2.0, F9B233);
        filter_chunk.visuals_mut().widgets.hovered.rounding = Rounding::same(2.0);

        filter_chunk.visuals_mut().widgets.open.bg_fill = DARK_GREY;
        filter_chunk.visuals_mut().widgets.open.weak_bg_fill = DARK_GREY;
        filter_chunk.visuals_mut().widgets.open.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        filter_chunk.visuals_mut().widgets.open.bg_stroke = Stroke::new(2.0, DARK_GREY);
        filter_chunk.visuals_mut().widgets.open.rounding = Rounding::same(2.0);

        filter_chunk.spacing_mut().item_spacing = egui::vec2(4.0,4.0);

        {
          puffin::profile_scope!("game list filters");
          filter_chunk.add_sized([260.,20.], egui::text_edit::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.game_view_bar.search_buffer).vertical_align(egui::Align::Center), &app.locale.localization.games_view.toolbar.search_bar_hint));
          filter_chunk.horizontal(|filters| {
            let combo_width = 130.0 - filters.spacing().item_spacing.x;
            enum_dropdown(filters, "GameTypeComboBox".to_owned(), &mut app.game_view_bar.genre_filter, combo_width, "", &app.locale).response;
            enum_dropdown(filters, "PlatformComboBox".to_owned(), &mut app.game_view_bar.platform_filter, combo_width, "", &app.locale).response;
          });
        }
      });

    
    // scrollbar
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
    ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(4.0);
    ui.style_mut().visuals.widgets.active.rounding = Rounding::same(4.0);
    ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(4.0);

    egui::ScrollArea::vertical()
    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
    .max_width(260.0)
    .max_height(f32::INFINITY)
    .auto_shrink(false)
    .show(ui, |ui| {
      puffin::profile_scope!("game list games");
      ui.vertical(|games_list| {
        games_list.allocate_space(vec2(150.0,0.0));
        let style = games_list.style_mut();
        style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        style.visuals.widgets.inactive.expansion = 0.0;
        style.visuals.widgets.active.bg_stroke = Stroke::NONE;
        style.visuals.widgets.active.expansion = 0.0;
        style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        style.visuals.widgets.hovered.expansion = 0.0;
        
        style.visuals.widgets.hovered.weak_bg_fill = F9B233;
        style.visuals.widgets.inactive.bg_fill = Color32::WHITE;

        style.visuals.widgets.active.weak_bg_fill = F9B233.gamma_multiply(0.6);
        
        style.spacing.item_spacing = vec2(0.0,0.0);
        
        let /*ps3 has no*/games = app.games.iter();
        
        let mut games: Vec<(&String, &GameInfo)> = games.filter(|obj| 
          matcher.fuzzy_match(&obj.1.name, &app.game_view_bar.search_buffer).is_some()
        ).collect();
        games.sort_by(|(_, a_game),(_, b_game)| {
          a_game.name.cmp(&b_game.name)
        });
        
        for (slug, game) in games {
          puffin::profile_scope!("game list game");
          let style = games_list.style_mut();

          if app.game_sel.eq(slug) {
            style.visuals.widgets.inactive.weak_bg_fill = F9B233.gamma_multiply(0.8);
            style.visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::BLACK);
          } else {
            style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.inactive.fg_stroke = {
              if game.installed {
                Stroke::new(1.0, Color32::WHITE)
              } else {
                Stroke::new(1.0, Color32::GRAY)
              }
            };
          }

          let x = if let Some(running_slug) = &app.playing_game { running_slug.eq(slug) } else { false };
          let name = if x { &format!("{} - {}", &game.name, &app.locale.localization.games_view.toolbar.running_suffix) } else { &game.name };
          let list_response = games_list.add_sized(vec2(250.0, icon_size.y+4.0), egui::Button::image_and_text((egui::TextureId::Managed(0), vec2(0.0, 0.0)), name).rounding(Rounding::same(0.0)));
          list_response.context_menu(|ui| { game_list_button_context_menu(app, game, ui) });
          if list_response.clicked() {
            app.game_sel = slug.clone();
          }
        }
        games_list.allocate_space(games_list.available_size_before_wrap());
      });
    });
  });
          

}

pub fn games_view(app : &mut MaximaEguiApp, ui: &mut Ui) {
  puffin::profile_function!();
  if app.games.len() < 1 {
    ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::RightToLeft), |ui| {
      ui.heading(&app.locale.localization.games_view.main.no_loaded_games);
    });
  } else {
    let list_width: f32 = 260.0;
    let list_rect = Rect {
      min: ui.available_rect_before_wrap().min,
      max: pos2(ui.available_rect_before_wrap().min.x + list_width, ui.available_rect_before_wrap().max.y)
    };
    let game_rect = Rect {
      min: ui.available_rect_before_wrap().min + vec2(list_width + ui.spacing().item_spacing.x, 0.0),
      max: ui.available_rect_before_wrap().max
    };

    ui.allocate_ui_at_rect(list_rect, |list| {
      show_game_list_buttons(app, list);
    });
    ui.allocate_ui_at_rect(game_rect, |games| {
      game_view_details_panel(app, games);
    });
    
    
      
    
  }
}

fn bezier_ease(t: f32) -> f32 {
  t * t * (3.0 - 2.0 * t)
}