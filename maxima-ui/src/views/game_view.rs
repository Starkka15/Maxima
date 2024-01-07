use egui::{Ui, Color32, vec2, Margin, ScrollArea, Rect, Pos2, Mesh, Shape, Rounding, RichText, Stroke};
use egui_extras::{StripBuilder, Size};
use log::debug;
use crate::{DemoEguiApp, GameInfo, GameUIImagesWrapper, interact_thread, GameUIImages, GameDetails, GameDetailsWrapper};

#[derive(Debug, PartialEq, Default)]
pub enum GameViewBarGenre {
  #[default] AllGames,
  Shooters,
  Simulation
}

#[derive(Debug, PartialEq, Default)]
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

pub fn game_view_details_panel(app : &mut DemoEguiApp, ui: &mut Ui) {
  if app.games.len() < 1 { return }
  if app.game_sel > app.games.len() { return }
  let game = &mut app.games[app.game_sel];
  //let's just load the logo now, the hero usually takes longer and it
  //just looks better if the logo is there first
  let game_images: Option<&GameUIImages> = match &game.images {
    GameUIImagesWrapper::Unloaded => {
      debug!("Loading images for {:?}", game.name);
      app.backend.tx.send(interact_thread::MaximaLibRequest::GetGameImagesRequest(game.slug.clone())).unwrap();
      game.images = GameUIImagesWrapper::Loading;
      None
    },
    GameUIImagesWrapper::Loading => {
      None
    },
    GameUIImagesWrapper::Available(images) => {
      Some(images) },
  };

  let game_details: Option<&GameDetails> = match &game.details {
    GameDetailsWrapper::Unloaded => {
      debug!("Loading details for {:?}", game.name);
      app.backend.tx.send(interact_thread::MaximaLibRequest::GetGameDetailsRequest(game.slug.clone())).unwrap();
      game.details = GameDetailsWrapper::Loading;
      None
    },
    GameDetailsWrapper::Loading => {
      None
    },
    GameDetailsWrapper::Available(details) => {
      Some(details) },
};
  StripBuilder::new(ui).size(Size::remainder()).vertical(|mut strip| {
    strip.cell(|ui| {
      let mut hero_rect = Rect::clone(&ui.available_rect_before_wrap());
      let aspect_ratio: f32 = 
      if let Some(images) = game_images {
        images.hero.size.x / images.hero.size.y
      } else {
        16.0 / 9.0
      };
      let style = ui.style_mut();
      style.visuals.clip_rect_margin = 0.0;
      style.spacing.item_spacing = vec2(0.0,0.0);
      hero_rect.max.x -= style.spacing.scroll_bar_width + style.spacing.scroll_bar_inner_margin;
      hero_rect.max.y = hero_rect.min.y + (hero_rect.size().x / aspect_ratio);
      let mut hero_rect_2 = hero_rect.clone();
      if hero_rect_2.size().x > 650.0 {
        hero_rect.max.y = hero_rect.min.y + (650.0 / aspect_ratio);
        hero_rect_2.max.x = hero_rect_2.min.x + 650.0;
        hero_rect_2.max.y = hero_rect_2.min.y + (650.0 / aspect_ratio);
      }
      ui.push_id("GameViewPanel_ScrollerArea", |ui| {
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
        ui.vertical(|ui| {
          if let Some(images) = game_images {
            if let Some(gvbg) = &app.game_view_bg_renderer {
              gvbg.draw(ui, hero_rect, images.hero.size, images.hero.renderable, app.game_view_frac);
              ui.allocate_space(hero_rect.size());
            } else {
              ui.put(hero_rect, egui::Image::new(images.hero.renderable, hero_rect_2.size()));
            }
            ui.allocate_space(vec2(0.0,-hero_rect.size().y));
          } else {
            ui.painter().rect_filled(hero_rect, Rounding::same(0.0), Color32::TRANSPARENT);
          }
          
          
          
          ScrollArea::vertical().show(ui, |ui| {
            StripBuilder::new(ui).size(Size::exact(900.0))
            .vertical(|mut strip| {
              strip.cell(|ui| {
                ui.allocate_space(vec2(0.0,hero_rect.size().y));
                let mut fade_rect = Rect::clone(&ui.cursor());
                fade_rect.max.y = fade_rect.min.y + 40.0;
                app.game_view_frac = (fade_rect.max.y - hero_rect.min.y) / (hero_rect.max.y - hero_rect.min.y);
                app.game_view_frac = if app.game_view_frac < 0.0 { 1.0 } else { if app.game_view_frac > 1.0 { 0.0 } else { bezier_ease(1.0 -  app.game_view_frac) }}; //clamping
                let mut mesh = Mesh::default();

                let we_do_a_smidge_of_trolling_dont_fucking_ship_this = Color32::from_black_alpha(20);
                mesh.colored_vertex(hero_rect.left_bottom() - vec2(0.0, app.game_view_frac * hero_rect.height()), we_do_a_smidge_of_trolling_dont_fucking_ship_this);
                mesh.colored_vertex(hero_rect.right_bottom() - vec2(0.0, app.game_view_frac * hero_rect.height()), we_do_a_smidge_of_trolling_dont_fucking_ship_this);
                mesh.colored_vertex(hero_rect.right_top(), we_do_a_smidge_of_trolling_dont_fucking_ship_this);
                mesh.colored_vertex(hero_rect.left_top(), we_do_a_smidge_of_trolling_dont_fucking_ship_this);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);

                ui.painter().add(Shape::mesh(mesh));

                let mut bar_rounding = Rounding::same(3.0);
                bar_rounding.nw = 0.0;
                bar_rounding.ne = 0.0;
                let play_bar_frame = egui::Frame::default()
                //.fill(Color32::from_black_alpha(120))
                .rounding(Rounding::none());
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
                          stats.label("game details not loaded");
                        }

                        stats.allocate_space(vec2(stats.available_width(),0.0));
                      });
                    });
                    
                    let buttons_frame = egui::Frame::default()
                    .outer_margin(Margin::symmetric(0.0, 8.0))
                    .fill(Color32::TRANSPARENT);
                    buttons_frame.show(ui, |buttons| {
                      buttons.horizontal(|buttons| {
                        buttons.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                        buttons.style_mut().spacing.item_spacing.x = 8.0;

                        //disabling the platform lockout for now, looks better for UI showcases
                        let play_str = /*if cfg!(target_os = "linux") { "Play on " } else*/ { &app.locale.localization.games_view.main.play };
                        if buttons.add_sized(vec2(125.0,50.0), egui::Button::new(egui::RichText::new(play_str)
                          .size(26.0)
                          .color(Color32::WHITE))
                          //.fill(if cfg!(target_os = "linux") { ACCENT_COLOR } else { ACCENT_COLOR })
                          .rounding(Rounding::same(2.0))
                        ).clicked() {
                          let _ = app.backend.tx.send(crate::interact_thread::MaximaLibRequest::StartGameRequest(game.offer.clone()));
                        }

                        if buttons.add_sized(vec2(125.0,50.0), egui::Button::new(egui::RichText::new("Mods")
                          .size(26.0)
                          .color(Color32::WHITE))
                          .rounding(Rounding::same(2.0))
                        ).clicked() {
                          let _ = app.backend.tx.send(crate::interact_thread::MaximaLibRequest::BitchesRequest);
                        }
                      });
                    });

                  });
                });
                /*
                ui.horizontal(|ui| {
                  ui.style_mut().visuals.override_text_color = Some(Color32::WHITE);
                  play_bar_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                      ui.style_mut().spacing.item_spacing = vec2(15.0, 10.0);
                      ui.style_mut().visuals.widgets.hovered.weak_bg_fill = ACCENT_COLOR;
                      ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(5, 107, 153);
                      ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(6, 132, 190);
                      //disabling the platform lockout for now, looks better for UI showcases
                      let play_str = /*if cfg!(target_os = "linux") { "Play on " } else*/ { &app.locale.localization.games_view.main.play };
                      //ui.set_enabled(!cfg!(target_os = "linux"));
                      if ui.add_sized(vec2(175.0,50.0), egui::Button::new(egui::RichText::new(play_str)
                        .size(26.0)
                        .color(Color32::WHITE))
                        //.fill(if cfg!(target_os = "linux") { ACCENT_COLOR } else { ACCENT_COLOR })
                        .rounding(Rounding::same(0.0))
                      ).clicked() {
                        app.backend.tx.send(crate::interact_thread::MaximaLibRequest::StartGameRequest(game.offer.clone()));
                      }
                      
                      
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                      ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(3.0, Color32::WHITE);
                      ui.label(RichText::new(&app.locale.localization.games_view.main.playtime).size(15.0));
                      ui.label(RichText::new(format!("{:?} hours",app.games[app.game_sel].time as f32 / 10.0)).size(25.0));
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                      ui.style_mut().visuals.override_text_color = Some(Color32::WHITE);
                      ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
                      ui.label(RichText::new(&app.locale.localization.games_view.main.achievements).size(15.0));
                      ui.label(RichText::new(format!("{:?} / {:?}",app.games[app.game_sel].achievements_unlocked,app.games[app.game_sel].achievements_total)).size(25.0));
                    });
                    ui.separator();
                    ui.menu_button(egui::RichText::new("⛭").size(50.0), |cm| {
                      if cm.button(&app.locale.localization.games_view.main.uninstall).clicked() {
                        game.uninstall();
                        //shut the FUCK up rust
                        let _ = app.backend.tx.send(crate::interact_thread::MaximaLibRequest::BitchesRequest);
                      }
                    });
                    //ui.add_sized(vec2(50.0,50.0), egui::Button::new());
                    
                  });
                });*/
                ui.vertical(|ui| {
                  
                  ui.style_mut().spacing.item_spacing = vec2(5.0,5.0);

                  ui.strong("Frac");
                  ui.label(format!("{:?}",app.game_view_frac));

                  ui.allocate_space(vec2(0.0,16.0));

                  
                  let req_width = (ui.available_size_before_wrap().x - 5.0) / 2.0;
                  ui.horizontal(|sysreq| {
                    if let Some(details) = game_details {

                      sysreq.vertical(|min| {
                        min.set_min_width(req_width);
                        min.set_max_width(req_width);
                        min.heading(&app.locale.localization.games_view.details.min_system_req);
                        egui_demo_lib::easy_mark::easy_mark(min, &details.system_requirements_min);
                      });
                      sysreq.vertical(|rec| {
                        rec.set_min_width(req_width);
                        rec.set_max_width(req_width);
                        rec.heading(&app.locale.localization.games_view.details.rec_system_req);
                        egui_demo_lib::easy_mark::easy_mark(rec, &details.system_requirements_rec);
                      });
                    } else {
                      sysreq.heading("System requirements unloaded");
                    }
                  });
                  for _idx in 0..75 {
                    ui.heading("");
                  }
                });
              })
            }) // StripBuilder
          }); // ScrollArea
          if let Some(images) = game_images {
            if let Some(logo) = &images.logo {
              let logo_size_pre = if logo.size.x >= logo.size.y {
                // wider than it is tall, scale based on X as max
                let mult_frac = 320.0 / logo.size.x;
                logo.size.y * mult_frac
              } else {
                // taller than it is wide, scale based on Y
                // fringe edge case, here in case EA decides they want to pull something really fucking stupid
                0.0 // TODO:: CALCULATE IT
              };
              let frac2 = app.game_view_frac.clone();
              let logo_size = vec2(egui::lerp(320.0..=160.0, frac2), egui::lerp(logo_size_pre..=(logo_size_pre/2.0), frac2));
              let logo_rect = Rect::from_min_max(
                Pos2 { x: (egui::lerp(hero_rect.min.x..=hero_rect.max.x-180.0, frac2)), y: (hero_rect.min.y) },
                Pos2 { x: (egui::lerp(hero_rect.max.x..=hero_rect.max.x-20.0, frac2)), y: (egui::lerp(hero_rect.max.y..=hero_rect.min.y+80.0, frac2)) }
              );
              ui.put(logo_rect, egui::Image::new(logo.renderable, logo_size));
              }
          } else {
            //ui.put(hero_rect, egui::Label::new("NO LOGO"));
          }
        }) // Vertical
      }); // ID
    })
  }); // StripBuilder
}

fn game_list_button_context_menu(game : &GameInfo, ui : &mut Ui) {
  if ui.button("▶ Play").clicked() {
    game.launch();
    ui.close_menu();
  }
  ui.separator();
  if ui.button("UNINSTALL").clicked() {
    game.uninstall();
    ui.close_menu();
  }
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);
const DARK_GREY: Color32 = Color32::from_rgb(53, 53, 53);

fn show_game_list_buttons(app : &mut DemoEguiApp, ui : &mut Ui) {
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
      
        filter_chunk.add_sized([260.,20.], egui::text_edit::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.game_view_bar.search_buffer), &app.locale.localization.games_view.toolbar.search_bar_hint));
        filter_chunk.horizontal(|filters| {
          filters.push_id("GameTypeComboBox", |filters| {
            egui::ComboBox::from_label("")
            .selected_text(match app.game_view_bar.genre_filter {
              GameViewBarGenre::AllGames => &app.locale.localization.games_view.toolbar.genre_options.all,
              GameViewBarGenre::Shooters => &app.locale.localization.games_view.toolbar.genre_options.shooter,
              GameViewBarGenre::Simulation => &app.locale.localization.games_view.toolbar.genre_options.simulation,
            })
            .width((260.0 / 2.0) - 8.0)
            .show_ui(filters, |combo| {
              
              combo.visuals_mut().selection.bg_fill = F9B233;
              combo.visuals_mut().selection.stroke = Stroke::new(2.0, Color32::BLACK);
              combo.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(2.0, F9B233);

              let mut rounding = Rounding::same(2.0);
              rounding.se = 0.0; rounding.sw = 0.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::AllGames, &app.locale.localization.games_view.toolbar.genre_options.all);
              rounding.ne = 0.0; rounding.nw = 0.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::Shooters, &app.locale.localization.games_view.toolbar.genre_options.shooter);
              rounding.se = 2.0; rounding.sw = 2.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::Simulation, &app.locale.localization.games_view.toolbar.genre_options.simulation);
              }
            );
          });
          filters.push_id("PlatformComboBox", |horizontal| {
            egui::ComboBox::from_label("")
            .selected_text(match app.game_view_bar.platform_filter {
              GameViewBarPlatform::AllPlatforms => &app.locale.localization.games_view.toolbar.platform_options.all,
              GameViewBarPlatform::Windows => &app.locale.localization.games_view.toolbar.platform_options.windows,
              GameViewBarPlatform::Mac => &app.locale.localization.games_view.toolbar.platform_options.mac,
            })
            .width((260.0 / 2.0) - 8.0)
            .show_ui(horizontal, |combo| {
              combo.visuals_mut().selection.bg_fill = F9B233;
              combo.visuals_mut().selection.stroke = Stroke::new(2.0, Color32::BLACK);
              combo.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(2.0, F9B233);
              let mut rounding = Rounding::same(2.0);
              rounding.se = 0.0; rounding.sw = 0.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::AllPlatforms, &app.locale.localization.games_view.toolbar.platform_options.all);
              rounding.ne = 0.0; rounding.nw = 0.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::Windows, &app.locale.localization.games_view.toolbar.platform_options.windows);
              rounding.se = 2.0; rounding.sw = 2.0;
              combo.visuals_mut().widgets.inactive.rounding = rounding;
              combo.visuals_mut().widgets.active.rounding = rounding;
              combo.visuals_mut().widgets.hovered.rounding = rounding;
              combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::Mac, &app.locale.localization.games_view.toolbar.platform_options.mac);
              }
            );
          });
        });
      });

    let rect = ui.allocate_exact_size(vec2(260.0, ui.available_height()), egui::Sense::click());
    
    let mut what = ui.child_ui(rect.0, egui::Layout::default() );
  egui::ScrollArea::vertical()
  .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
  .max_width(260.0)
  .max_height(f32::INFINITY)
  .show(&mut what, |ui| {
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
    ui.vertical(|games_list| {
      games_list.allocate_space(vec2(150.0,0.0));
      let style = games_list.style_mut();
      //fuck outlines on buttons
      //i don't know when fg is used vs bg, fuck it, do all of em
      style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
      //style.visuals.widgets.inactive.fg_stroke = Stroke::NONE;
      style.visuals.widgets.active.bg_stroke = Stroke::NONE;
      //style.visuals.widgets.active.fg_stroke = egui::Stroke::NONE;
      style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
      //style.visuals.widgets.hovered.fg_stroke = egui::Stroke::NONE;
      
      style.visuals.widgets.hovered.weak_bg_fill = F9B233;
      style.visuals.widgets.inactive.bg_fill = Color32::WHITE;

      style.visuals.widgets.active.weak_bg_fill = F9B233.gamma_multiply(0.6);
      
      style.spacing.item_spacing = vec2(0.0,0.0);
      
      let filtered_games : Vec<&GameInfo> = app.games.iter().filter(|obj| 
        obj.name.to_lowercase().contains(&app.game_view_bar.search_buffer.to_lowercase())
      ).collect();

      for game_idx in 0..filtered_games.len() {
        let style = games_list.style_mut();
        if app.game_sel == game_idx {
          style.visuals.widgets.inactive.weak_bg_fill = F9B233.gamma_multiply(0.8);
          style.visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::BLACK);
        } else {
          style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
          style.visuals.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        }
        let game = filtered_games[game_idx];
        /*if let Ok(icon) = game.icon(&mut app.game_image_handler) {
          if games_list.add_sized(vec2(250.0, icon_size.y),
            egui::Button::image_and_text(icon, icon_size, RichText::new(&game.name).color(Color32::WHITE).strong())
            .rounding(Rounding::same(0.0)))
            .context_menu(|ui| { game_list_button_context_menu(game, ui) })
            .clicked() {
              app.game_sel = game_idx;
          }
        } else {*/
          if games_list.add_sized(vec2(250.0, icon_size.y+4.0), egui::Button::image_and_text(egui::TextureId::Managed(0), vec2(0.0, 0.0), &game.name)
              //.fill(if app.game_sel == game_idx {  ACCENT_COLOR } else { Color32::TRANSPARENT })
              .rounding(Rounding::same(0.0)))
              .context_menu(|ui| { game_list_button_context_menu(game, ui) })
              .clicked() {
                app.game_sel = game_idx;
            }
        //}
      }
      games_list.allocate_space(games_list.available_size_before_wrap());
    });
  });
  });
          

}

pub fn games_view(app : &mut DemoEguiApp, ui: &mut Ui) {
  if app.games.len() < 1 {
    ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::RightToLeft), |ui| {
      ui.heading(&app.locale.localization.games_view.main.no_loaded_games);
    });
  } else {
    let alloc_height = ui.available_height();
  
    ui.horizontal(|games| {
      games.allocate_space(vec2(-8.0,alloc_height));
      show_game_list_buttons(app, games);
      game_view_details_panel(app, games);
    });
      
    
  }
}

fn bezier_ease(t: f32) -> f32 {
  t * t * (3.0 - 2.0 * t)
}