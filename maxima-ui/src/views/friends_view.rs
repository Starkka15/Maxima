use std::sync::Arc;

use egui::{Ui, Color32, Margin, vec2, Rounding, Stroke, Sense, Id};

use crate::{DemoEguiApp, interact_thread, ui_image::UIImage, widgets::enum_dropdown::enum_dropdown};

use strum_macros::EnumIter;

#[derive(Debug, PartialEq, Default, EnumIter)]
pub enum FriendsViewBarStatusFilter {
  #[default] Name,
  Game,
}

#[derive(Debug, Eq, PartialEq, Default, EnumIter)]
pub enum FriendsViewBarPage {
  #[default] Online,
  All,
  Pending,
  Blocked
}

pub struct FriendsViewBar {
  /// What page the user is currently on
  pub page : FriendsViewBarPage,
  /// The value of the criteria ComboBox
  pub status_filter : FriendsViewBarStatusFilter,
  //search_category : FriendViewBarSearchCategory,
  /// The buffer for the search box
  pub search_buffer : String,
  /// ID of the friend with buttons below
  pub friend_sel : String,
}

pub enum UIFriendImageWrapper {
  /// The user doesn't have an avatar or otherwise the app doesn't want it
  DoNotLoad,
  /// Avatar exists but is not loaded
  Unloaded(String),
  /// Avatar is being loaded
  Loading,
  /// Avatar can be rendered
  Available(Arc<UIImage>)
}

pub struct UIFriend {
  pub name : String,
  pub id : String,
  pub online : bool,
  pub game : Option<String>,
  pub game_presence : Option<String>,
  pub avatar: UIFriendImageWrapper,
}

impl UIFriend {
  pub fn set_avatar_loading_flag(&mut self) {
    self.avatar = UIFriendImageWrapper::Loading;
  }
}

const F9B233: Color32 = Color32::from_rgb(249, 178, 51);
const DARK_GREY: Color32 = Color32::from_rgb(64, 64, 64);


pub fn friends_view(app : &mut DemoEguiApp, ui: &mut Ui) {
  puffin::profile_function!();
  let context = ui.ctx().clone();
  let _friends_raw : Vec<UIFriend> = Vec::from(
    [
      UIFriend {
        name : "AMoistEggroll".to_owned(),
        id: "".to_owned(),
        online : false,
        game: None,
        game_presence: None,
        avatar: UIFriendImageWrapper::DoNotLoad,
      },
      UIFriend {
        name : "BattleDash".to_owned(),
        id: "".to_owned(),
        online : true,
        game: Some("Battlefield 2042".to_owned()),
        game_presence: None,
        avatar: UIFriendImageWrapper::DoNotLoad,
      },
      UIFriend {
        name : "GEN_Burnout".to_owned(),
        id: "".to_owned(),
        online : true,
        game: None,
        game_presence: None,
        avatar: UIFriendImageWrapper::DoNotLoad,
      },
      UIFriend {
        name : "KursedKrabbo".to_owned(),
        id: "".to_owned(),
        online : true,
        game: Some("Titanfall 2".to_owned()),
        game_presence: Some("Pilots vs Pilots on Glitch".to_owned()),
        avatar: UIFriendImageWrapper::DoNotLoad,
      }
    ]
  );

  let top_bar = egui::Frame::default()
  //.fill(Color32::from_gray(255))
  .outer_margin(Margin::same(-4.0))
  .inner_margin(Margin::same(5.0));
  
  top_bar.show(ui, |ui| {
    ui.style_mut().spacing.item_spacing = vec2(5.0,5.0);
    ui.vertical(|ui| {

      ui.vertical(|ui| { //separating this out for styling reasons
        puffin::profile_scope!("filters");
        ui.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;

        ui.visuals_mut().widgets.inactive.expansion = 0.0;
        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(2.0, DARK_GREY);
        ui.visuals_mut().widgets.inactive.rounding = Rounding::same(2.0);

        ui.visuals_mut().widgets.active.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.active.weak_bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.visuals_mut().widgets.active.bg_stroke = Stroke::new(2.0, DARK_GREY);
        ui.visuals_mut().widgets.active.rounding = Rounding::same(2.0);

        ui.visuals_mut().widgets.hovered.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(2.0, F9B233);
        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(2.0, F9B233);
        ui.visuals_mut().widgets.hovered.rounding = Rounding::same(2.0);

        ui.visuals_mut().widgets.open.bg_fill = DARK_GREY;
        ui.visuals_mut().widgets.open.weak_bg_fill = DARK_GREY;
        ui.visuals_mut().widgets.open.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        ui.visuals_mut().widgets.open.bg_stroke = Stroke::new(2.0, DARK_GREY);
        ui.visuals_mut().widgets.open.rounding = Rounding::same(2.0);

        ui.add_sized([ui.available_width(), 20.0], egui::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.friends_view_bar.search_buffer), "Search friends list"));
        let combo_width = (ui.available_width() / 2.0) - ui.spacing().item_spacing.x; //a lot of accounting for shit when i'm just gonna make it a fixed width anyway
        ui.horizontal(|ui| {
          enum_dropdown(ui, "FriendsListStatusFilterComboBox".to_owned(), &mut app.friends_view_bar.page, combo_width, &app.locale);
          enum_dropdown(ui, "FriendsListFilterTypeComboBox".to_owned(), &mut app.friends_view_bar.status_filter, combo_width, &app.locale);
        });
      });

      

      let friends : Vec<&mut UIFriend> = app.friends.iter_mut().filter(|obj| 
        match app.friends_view_bar.status_filter {
            FriendsViewBarStatusFilter::Name => obj.name.to_ascii_lowercase().contains(&app.friends_view_bar.search_buffer),
            FriendsViewBarStatusFilter::Game => if let Some(game) = &obj.game {
              game.to_ascii_lowercase().contains(&app.friends_view_bar.search_buffer)
            } else {
              false
            }
        }
        &&
        match app.friends_view_bar.page {
            FriendsViewBarPage::Online => obj.online,
            FriendsViewBarPage::All => true,
            FriendsViewBarPage::Pending => false,
            FriendsViewBarPage::Blocked => false,
        }
      ).collect();
      
      // scrollbar
      ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
      ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.active.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(4.0);

      egui::ScrollArea::new([false,true])
      .id_source("FriendsListFriendListScrollArea") //hmm yes, the friends list is made of friends list
      .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
      .show(ui, |ui| {
        puffin::profile_scope!("friends");
        let mut marge = Margin::same(0.0);
        marge.bottom = 4.5;
        let item_width = ui.available_rect_before_wrap().width() - (ui.spacing().scroll_bar_width);
        for friend in friends {
          puffin::profile_scope!("friend");
          let buttons = app.friends_view_bar.friend_sel.eq(&friend.id);
          let how_buttons = context.animate_bool(Id::new("friendlistbuttons_".to_owned()+&friend.id), buttons);
          ui.allocate_ui(vec2(item_width, 42.0 + ( 25.0 * how_buttons )), |ui| {
            //ui.painter().rect_filled(ui.available_rect_before_wrap(), Rounding::none(), Color32::LIGHT_RED);
            ui.spacing_mut().item_spacing.y = 0.0;
            if buttons || how_buttons > 0.0 {
              let button_width = ( ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0;
              ui.allocate_space(vec2(0.0,22.0 + ( 25.0 * how_buttons )));
              ui.horizontal(|ui| {
                ui.add(egui::Button::new("KILL").min_size(vec2(button_width,20.0)));
                ui.add(egui::Button::new("INVITE").min_size(vec2(button_width,20.0)));
                ui.add(egui::Button::new("CHAT").min_size(vec2(button_width,20.0)));
              });
              //let button_kill = ui.allocate_response(vec2(30.0, 20.0), Sense::click());
              //ui.painter().rect_stroke(button_kill.rect, Rounding::same(2.0), Stroke::new(2.0, Color32::WHITE));
              ui.allocate_space(vec2(0.0,- ( 42.0 + ( 25.0 * how_buttons ) )));
            }
            ui.horizontal(|container| {
              
              
              
              let avatar: Option<&Arc<UIImage>> = match &friend.avatar {
                  UIFriendImageWrapper::DoNotLoad => {
                    None
                  },
                  UIFriendImageWrapper::Unloaded(url) => {
                    let _ = app.backend.tx.send(interact_thread::MaximaLibRequest::GetUserAvatarRequest(friend.id.clone(), url.to_string()));
                    friend.set_avatar_loading_flag();
                    None
                  },
                  UIFriendImageWrapper::Loading => {
                    None
                  },
                  UIFriendImageWrapper::Available(img) => {
                    Some(img)
                  },
              };
              
              container.spacing_mut().item_spacing.x = 0.0;

              egui::Frame::default()
              //.stroke(Stroke { width: 2.0, color: Color32::WHITE })
              .inner_margin(Margin::same(1.0))
              .outer_margin(Margin::same(0.0))
              .show(container, |container| {
                container.spacing_mut().item_spacing.y = 2.0;
                let click_sensor = container.allocate_exact_size(vec2(item_width,42.0), Sense::click());

                let how_hover = context.animate_bool(Id::new("friendlistrect_".to_owned()+&friend.id), click_sensor.1.hovered() || buttons);
                let rect_bg = Color32::from_white_alpha((how_hover*u8::MAX as f32) as u8);
                let text = Color32::from_gray(((1.0-how_hover)*u8::MAX as f32) as u8);
                container.painter().rect_filled(click_sensor.0, Rounding::same(4.0), rect_bg);
                container.visuals_mut().override_text_color = Some(text);
                container.allocate_space(vec2((-item_width) + 2.0,-42.0));
                if let Some(pfp) = avatar {
                  container.image((pfp.renderable, vec2(38.0,38.0)));
                } else {
                  container.image((app.user_pfp_renderable, vec2(38.0,38.0)));
                }
                
                container.allocate_space(vec2(12.0,0.0));
                container.vertical(|text| {
                  text.label(egui::RichText::new(&friend.name).size(15.0));
                  let game_hack: String;
                  text.label(egui::RichText::new(
                    if friend.online {
                      if let Some(game) = &friend.game  {
                        if app.locale.localization.friends_view.prepend {
                          if let Some(presence) = &friend.game_presence {
                            game_hack = format!("{} {}: {}", &game, &app.locale.localization.friends_view.status_playing, &presence);
                          } else {
                            game_hack = format!("{} {}", &game, &app.locale.localization.friends_view.status_playing);
                          }
                        } else {
                          if let Some(presence) = &friend.game_presence {
                            game_hack = format!("{} {}: {}", &app.locale.localization.friends_view.status_playing, &game, &presence);
                          } else {
                            game_hack = format!("{} {}", &app.locale.localization.friends_view.status_playing, &game);
                          }
                        }
                        &game_hack
                        
                      } else {
                        &app.locale.localization.friends_view.status_online
                      }
                    } else {
                      &app.locale.localization.friends_view.status_offline
                    }
                  ).size(10.0));
                });
                let mut outline_rect_fucking_jank_ass_bitch_dont_ship_it_idiot_lmao = click_sensor.0;
                outline_rect_fucking_jank_ass_bitch_dont_ship_it_idiot_lmao.max = outline_rect_fucking_jank_ass_bitch_dont_ship_it_idiot_lmao.min + vec2(41.0, 41.0);
                outline_rect_fucking_jank_ass_bitch_dont_ship_it_idiot_lmao.min += vec2(1.0, 1.0);
                container.painter().rect_stroke(outline_rect_fucking_jank_ass_bitch_dont_ship_it_idiot_lmao, Rounding::same(4.0), Stroke::new(2.0, if friend.online { Color32::GREEN } else { Color32::GRAY }));
                if click_sensor.1.clicked() {
                  if app.friends_view_bar.friend_sel.eq(&friend.id){
                    app.friends_view_bar.friend_sel = ("").to_string();
                  } else {
                    app.friends_view_bar.friend_sel = friend.id.clone();
                  }
                }
              });

            });
            ui.allocate_space(ui.available_size_before_wrap());
          });
        }
        ui.allocate_space(vec2(item_width-2.0,ui.available_size_before_wrap().y));
      })
    });
  });
  ui.allocate_space(vec2(0.0,8.0));

  
}