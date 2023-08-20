use egui::{Ui, Color32, vec2, Margin, ScrollArea, Rect, Pos2, Mesh, Shape, Rounding, epaint::Shadow};
use egui_extras::{StripBuilder, Size};
use crate::{DemoEguiApp, GameInfo};

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const GAMELIST_BUTTON_NORMAL: Color32 = Color32::from_rgb(20, 20, 20);
const GAMELIST_BUTTON_HIGHLIGHT: Color32 = Color32::from_rgb(40, 40, 40);
const ACCENT_COLOR : Color32 = Color32::from_rgb(120, 0, 255);

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
  let game = &app.games[app.game_sel];
  //let's just load the logo now, the hero usually takes longer and it
  //just looks better if the logo is there first
  let _ = game.logo(&mut app.game_image_handler);
  StripBuilder::new(ui).size(Size::remainder()).vertical(|mut strip| {
    strip.cell(|ui| {
      let mut hero_rect = Rect::clone(&ui.available_rect_before_wrap());
      let hero_container_max_y = hero_rect.max.y;
      let style = ui.style_mut();
      style.visuals.clip_rect_margin = 0.0;
      style.spacing.item_spacing = vec2(0.0,0.0);
      hero_rect.max.x -= style.spacing.scroll_bar_width + style.spacing.scroll_bar_inner_margin;
      hero_rect.max.y = hero_rect.min.y + (hero_rect.size().x / ASPECT_RATIO);
      let mut hero_rect_2 = hero_rect.clone();
      if hero_rect_2.size().x > 650.0 {
        hero_rect.max.y = hero_rect.min.y + (650.0 / ASPECT_RATIO);
        hero_rect_2.max.x = hero_rect_2.min.x + 650.0;
        hero_rect_2.max.y = hero_rect_2.min.y + (650.0 / ASPECT_RATIO);
      }
      ui.push_id("GameViewPanel_ScrollerArea", |ui| {
        ui.vertical(|ui| {
          if let Ok(hero) = game.hero(&mut app.game_image_handler) {
            if let Some(gvbg) = &app.game_view_bg_renderer {
              gvbg.draw(ui, hero_rect, hero);
              ui.allocate_space(hero_rect.size());
            } else {
              ui.put(hero_rect, egui::Image::new(hero, hero_rect_2.size()));
            }
            ui.allocate_space(vec2(0.0,-hero_rect.size().y));
          } else {
            ui.painter().rect_filled(hero_rect, Rounding::same(0.0), Color32::TRANSPARENT);
          }
          
          
          let mut frac :f32 = 0.0;
          ScrollArea::vertical().show(ui, |ui| {
            StripBuilder::new(ui).size(Size::exact(900.0))
            .vertical(|mut strip| {
              strip.cell(|ui| {
                ui.allocate_space(vec2(0.0,hero_rect.size().y - 40.0));
                let mut fade_rect = Rect::clone(&ui.cursor());
                fade_rect.max.y = fade_rect.min.y + 40.0;
                frac = (fade_rect.max.y - hero_rect.min.y) / (hero_rect.max.y - hero_rect.min.y);
                frac = if frac < 0.0 { 1.0 } else { if frac > 1.0 { 0.0 } else { bezier_ease(1.0 -  frac) }}; //clamping
                let mut mesh = Mesh::default();
                mesh.colored_vertex(fade_rect.left_top(), Color32::TRANSPARENT);
                mesh.colored_vertex(fade_rect.right_top(), Color32::TRANSPARENT);
                mesh.colored_vertex(fade_rect.left_bottom(), Color32::BLACK);
                mesh.colored_vertex(fade_rect.right_bottom(), Color32::BLACK);
                mesh.colored_vertex(Pos2::new(fade_rect.min.x, hero_container_max_y), Color32::BLACK);
                mesh.colored_vertex(Pos2::new(fade_rect.max.x, hero_container_max_y), Color32::BLACK);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(1, 2, 3);
                mesh.add_triangle(2, 3, 4);
                mesh.add_triangle(3, 4, 5);
                ui.painter().add(Shape::mesh(mesh));
                ui.allocate_space(vec2(0.0,9.0));

                let play_bar_frame = egui::Frame::default()
                .fill(Color32::from_black_alpha(120))
                .rounding(Rounding::same(6.0))
                .inner_margin(Margin::same(4.0))
                .outer_margin(Margin::same(4.0));
                let _settings_frame = egui::Frame::default()
                  .fill(Color32::from_black_alpha(120))
                  .rounding(Rounding::same(6.0))
                  .inner_margin(Margin::same(4.0))
                  .outer_margin(Margin::same(4.0));
              ui.horizontal(|ui| {
                play_bar_frame.show(ui, |ui| {
                  ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing = vec2(15.0, 10.0);
                    let play_str = if cfg!(target_os = "linux") { "Play on " } else { &app.locale.localization.games_view.main.play };
                    //ui.set_enabled(!cfg!(target_os = "linux"));
                    if ui.add_sized(vec2(175.0,50.0), egui::Button::new(egui::RichText::new(play_str)
                      .size(26.0)
                      .color(Color32::WHITE))
                      .fill(if cfg!(target_os = "linux") { Color32::from_rgb(100, 100, 100) } else { Color32::from_rgb(120, 0, 255) })
                      .rounding(Rounding::same(3.0))
                    ).clicked() {
                      app.backend.tx.send(crate::interact_thread::MaximaLibRequest::StartGameRequest(game.offer.clone()));
                    }
                    ui.vertical(|ui| {
                      ui.strong(&app.locale.localization.games_view.main.playtime);
                      ui.label(format!("{:?} hours",app.games[app.game_sel].time as f32 / 10.0));
                    });
                    ui.vertical(|ui| {
                      ui.strong(&app.locale.localization.games_view.main.achievements);
                      ui.label(format!("{:?} / {:?}",app.games[app.game_sel].achievements_unlocked,app.games[app.game_sel].achievements_total));
                    });
                  })
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {

                  play_bar_frame.show(ui, |ui| {
                    ui.menu_button(egui::RichText::new("⛭").size(50.0), |cm| {
                      if cm.button(&app.locale.localization.games_view.main.uninstall).clicked() {
                        game.uninstall();
                        app.backend.tx.send(crate::interact_thread::MaximaLibRequest::BitchesRequest);
                      }
                    });
                    //ui.add_sized(vec2(50.0,50.0), egui::Button::new());
                    
                  });
                });
              });  
                ui.vertical(|ui| {
                  ui.strong("Frac");
                  ui.label(format!("{:?}",frac));
                  for _idx in 0..55 {
                    //ui.heading("test");
                  }
                });
              })
            }) // StripBuilder
          }); // ScrollArea
          let logo_size = vec2(egui::lerp(320.0..=160.0, frac), egui::lerp(160.0..=90.0, frac));
          let logo_rect = Rect::from_min_max(
            Pos2 { x: (egui::lerp(hero_rect.min.x..=hero_rect.max.x-180.0, frac)), y: (hero_rect.min.y) },
            Pos2 { x: (egui::lerp(hero_rect.max.x..=hero_rect.max.x-20.0, frac)), y: (egui::lerp(hero_rect.max.y..=hero_rect.min.y+80.0, frac)) }
          );
          if let Ok(logo) = game.logo(&mut app.game_image_handler) {
            ui.put(logo_rect, egui::Image::new(logo, logo_size));
          } else {
            ui.put(logo_rect, egui::Spinner::new().size(logo_rect.size().min_elem()));
            //ui.add_sized(logo_rect.size(), egui::Spinner::new());
            //ui.painter().rect_filled(logo_rect, Rounding::same(0.0), Color32::TRANSPARENT);
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

fn show_game_list_buttons(app : &mut DemoEguiApp, ui : &mut Ui) {
  let icon_size = vec2(10. * app.game_view_bar.game_size,10. * app.game_view_bar.game_size);

    //create a rect that takes up all the vertical space in the window, and prohibits anything from going beyond that without us knowing, so we can add a scroll bar
    //because apparently some dumb fucks (me) buy EA games and can overflow the list on the default window size
    let rect = ui.allocate_exact_size(vec2(260.0, ui.available_height()), egui::Sense::click());
    
    let mut what = ui.child_ui(rect.0, egui::Layout::default() );
  egui::ScrollArea::vertical()
  .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
  .max_width(260.0)
  .max_height(f32::INFINITY)
  .show(&mut what, |ui| {
    ui.vertical(|games_list| {
      games_list.allocate_space(vec2(150.0,0.0));
      for game_idx in 0..app.games.len() {
        let game = &app.games[game_idx];
        if let Ok(icon) = game.icon(&mut app.game_image_handler) {
          if games_list.add_sized(vec2(250.0, icon_size.y),
            egui::Button::image_and_text(icon, icon_size, &game.name)
            .fill(if app.game_sel == game_idx {  ACCENT_COLOR } else { Color32::TRANSPARENT })
            .rounding(Rounding::same(0.0)))
            .context_menu(|ui| { game_list_button_context_menu(game, ui) })
            .clicked() {
              app.game_sel = game_idx;
          }
        } else {
          if games_list.add_sized(vec2(250.0, icon_size.y+4.0), egui::Button::image_and_text(egui::TextureId::Managed(0), vec2(0.0, 0.0), &game.name)
              .fill(if app.game_sel == game_idx {  ACCENT_COLOR } else { Color32::TRANSPARENT })
              .rounding(Rounding::same(0.0)))
              .context_menu(|ui| { game_list_button_context_menu(game, ui) })
              .clicked() {
                app.game_sel = game_idx;
            }
        }
      }
    });
  });
          

}

pub fn games_view(app : &mut DemoEguiApp, ui: &mut Ui) {
  if app.games.len() < 1 {
    ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::RightToLeft), |ui| {
      ui.heading(&app.locale.localization.games_view.main.no_loaded_games);
    });
  } else {
    let mut filter_bar_frame = egui::Frame::default();
    filter_bar_frame.fill = Color32::from_rgb(15, 15, 15);
    filter_bar_frame.outer_margin.top = -4.0;
    filter_bar_frame.outer_margin.left = -4.0;
    filter_bar_frame.outer_margin.right = -4.0;
    filter_bar_frame.inner_margin = Margin::same(4.0);
    
    filter_bar_frame.show(ui, |bar| {
      bar.horizontal(|horizontal|{
        horizontal.add_sized([18.,18.], egui::Button::new("⟳"));
        horizontal.label(&app.locale.localization.games_view.toolbar.genre_filter);
        horizontal.push_id("GameTypeComboBox", |horizontal| {
          egui::ComboBox::from_label("")
          .selected_text(match app.game_view_bar.genre_filter {
            GameViewBarGenre::AllGames => &app.locale.localization.games_view.toolbar.genre_options.all,
            GameViewBarGenre::Shooters => &app.locale.localization.games_view.toolbar.genre_options.shooter,
            GameViewBarGenre::Simulation => &app.locale.localization.games_view.toolbar.genre_options.simulation,
          })
          .show_ui(horizontal, |combo| {
            combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::AllGames, &app.locale.localization.games_view.toolbar.genre_options.all);
            combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::Shooters, &app.locale.localization.games_view.toolbar.genre_options.shooter);
            combo.selectable_value(&mut app.game_view_bar.genre_filter, GameViewBarGenre::Simulation, &app.locale.localization.games_view.toolbar.genre_options.simulation);
            }
          );
        });
        
        horizontal.label(&app.locale.localization.games_view.toolbar.platform_filter);
        horizontal.push_id("PlatformComboBox", |horizontal| {
          egui::ComboBox::from_label("")
          .selected_text(match app.game_view_bar.platform_filter {
            GameViewBarPlatform::AllPlatforms => &app.locale.localization.games_view.toolbar.platform_options.all,
            GameViewBarPlatform::Windows => &app.locale.localization.games_view.toolbar.platform_options.windows,
            GameViewBarPlatform::Mac => &app.locale.localization.games_view.toolbar.platform_options.mac,
          })
          .show_ui(horizontal, |combo| {
            combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::AllPlatforms, &app.locale.localization.games_view.toolbar.platform_options.all);
            combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::Windows, &app.locale.localization.games_view.toolbar.platform_options.windows);
            combo.selectable_value(&mut app.game_view_bar.platform_filter, GameViewBarPlatform::Mac, &app.locale.localization.games_view.toolbar.platform_options.mac);
            }
          );
        });

        horizontal.with_layout(egui::Layout::right_to_left(egui::Align::Center), |rtl| {
          rtl.add_sized([150.,20.], egui::text_edit::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.game_view_bar.search_buffer), &app.locale.localization.games_view.toolbar.search_bar_hint));
          //different stuff for if i ever re-impl the grid view
          //rtl.add_sized([24.,24.], egui::Checkbox::new(&mut app.game_view_rows, ""));
          //rtl.add_sized([24.,24.], egui::Image::new(app.biggen_image.texture_id(ctx), [24.,24.]));
          //rtl.add(egui::Slider::new(&mut app.game_view_bar.game_size, 1.0..=5.0).show_value(false));
          //rtl.add_sized([24.,24.], egui::Image::new(app.smallen_image.texture_id(ctx), [24.,24.]));
        });
      });
    });
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