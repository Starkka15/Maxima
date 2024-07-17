use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, FontId, Id, Margin, Rect, Rounding, Stroke, Ui};
use maxima::rtm::client::BasicPresence;

use crate::{bridge_thread, ui_image::UIImage, widgets::enum_dropdown::enum_dropdown, MaximaEguiApp, FRIEND_INGAME_COLOR};

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
  pub online : BasicPresence,
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
const PFP_SIZE: f32 = 36.0;
const PFP_CORNER_RADIUS: f32 = 2.0;
const PFP_ELEMENT_SIZE: f32 = (PFP_SIZE + PFP_CORNER_RADIUS * 2.0);
const FRIEND_HIGHLIGHT_ROUNDING: Rounding = Rounding { nw: 6.0, ne: 4.0, sw: 6.0, se: 4.0 }; // the status border is flawed somehow, this "fixes" it slightly more than if i didn't

pub fn friends_view(app : &mut MaximaEguiApp, ui: &mut Ui) {
  puffin::profile_function!();
  ui.style_mut().spacing.item_spacing = vec2(5.0,0.0);
  let context = ui.ctx().clone();
  // this is a fucking mistake.
  let sidebar_rect = ui.available_rect_before_wrap();

  let mut hittest_rect = sidebar_rect.clone().expand2(vec2(12.0,12.0));
  hittest_rect.min.x += 8.0; // fix overlapping the scrollbar of the left view
  
  let friend_rect_hovered = if let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos()) {
    hittest_rect.contains(pos) && ui.is_enabled()
  } else {
    false
  } || app.force_friends;
  app.force_friends = false; // reset, it won't go away without this
  
  let hovering_friends = context.animate_bool_with_time(egui::Id::new("FriendsListWidthAnimator"), friend_rect_hovered, ui.style().animation_time*2.0);
  let hover_diff = 300.0 - PFP_ELEMENT_SIZE;
  app.friends_width = PFP_ELEMENT_SIZE + (hovering_friends * hover_diff);

  let top_bar = egui::Frame::default()
  //.fill(Color32::from_gray(255))
  //.outer_margin(Margin::same(-4.0))
  //.inner_margin(Margin::same(5.0))
  ;
  
  top_bar.show(ui, |ui| {
    ui.style_mut().spacing.item_spacing = vec2(5.0,5.0);
    ui.vertical(|ui| {
      if friend_rect_hovered { //TODO : smooth transition
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

        
          if ui.add_sized([ui.available_width(), 20.0], egui::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.friends_view_bar.search_buffer).vertical_align(egui::Align::Center), "Search friends list")).has_focus() {
            app.force_friends = true;
          }
          let combo_width = (ui.available_width() / 2.0) - ui.spacing().item_spacing.x; //a lot of accounting for shit when i'm just gonna make it a fixed width anyway
          ui.horizontal(|ui| {
            let dropdown0 = enum_dropdown(ui, "FriendsListStatusFilterComboBox".to_owned(), &mut app.friends_view_bar.page, combo_width, &app.locale).inner.is_some();
            let dropdown1 = enum_dropdown(ui, "FriendsListFilterTypeComboBox".to_owned(), &mut app.friends_view_bar.status_filter, combo_width, &app.locale).inner.is_some();
            if dropdown0 || dropdown1 {
              app.force_friends = true;
            }
          });
        });
      }
      
      

      let mut friends : Vec<&mut UIFriend> = app.friends.iter_mut().filter(|obj| 
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
            FriendsViewBarPage::All => true,
            FriendsViewBarPage::Online => {
              match obj.online {
                BasicPresence::Unknown => false,
                BasicPresence::Offline => false,
                BasicPresence::Dnd => true,
                BasicPresence::Away => true,
                BasicPresence::Online => true,
              }
            },
            FriendsViewBarPage::Pending => false,
            FriendsViewBarPage::Blocked => false,
        }
      ).collect();
      friends.sort_by(|a,b| {a.name.cmp(&b.name)});                                     // Alphabetically sort
      friends.sort_by(|a,b| {b.online.cmp(&a.online)}); // Put online ones first
      friends.sort_by(|a, b| {b.game.is_some().cmp(&a.game.is_some())});                // Put online and in-game ones first
      
      
      // scrollbar
      ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
      ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.active.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(4.0);

      let clip_rect = ui.available_rect_before_wrap();
      egui::ScrollArea::new([false,friend_rect_hovered])
      .id_source("FriendsListFriendListScrollArea") //hmm yes, the friends list is made of friends list
      .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
      .show(ui, |ui| {
        ui.set_clip_rect(clip_rect);
        puffin::profile_scope!("friends");
        let mut marge = Margin::same(0.0);
        marge.bottom = 4.5;

        let button_height = PFP_ELEMENT_SIZE * 0.6;
        let button_gap = PFP_ELEMENT_SIZE * 0.2;
        let width = ui.available_width() - if friend_rect_hovered { ui.style().spacing.scroll.bar_inner_margin + ui.style().spacing.scroll.bar_width } else { 0.0 };
        for friend in friends {
          puffin::profile_scope!("friend");
          let buttons = app.friends_view_bar.friend_sel.eq(&friend.id) && friend_rect_hovered;
          if buttons { app.force_friends = true; }
          let how_buttons = context.animate_bool(Id::new("friendlistbuttons_".to_owned()+&friend.id), buttons);
          let avatar: Option<&Arc<UIImage>> = match &friend.avatar {
            UIFriendImageWrapper::DoNotLoad => {
              None
            },
            UIFriendImageWrapper::Unloaded(url) => {
              let _ = app.backend.backend_commander.send(bridge_thread::MaximaLibRequest::GetUserAvatarRequest(friend.id.clone(), url.to_string()));
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
          let game_hack: String;
          let (friend_status, friend_color) = 
          match friend.online {
            BasicPresence::Unknown => (&app.locale.localization.friends_view.status.unknown as &String, Color32::DARK_RED),
            BasicPresence::Offline => (&app.locale.localization.friends_view.status.offline, Color32::GRAY),
            BasicPresence::Dnd => (&app.locale.localization.friends_view.status.do_not_disturb, Color32::RED),
            BasicPresence::Away => (&app.locale.localization.friends_view.status.away, Color32::GOLD),
            BasicPresence::Online => {
              
              if let Some(game) = &friend.game  {
                if app.locale.localization.friends_view.status.prepend {
                  if let Some(presence) = &friend.game_presence {
                    game_hack = format!("{} {}: {}", &game, &app.locale.localization.friends_view.status.playing, &presence);
                  } else {
                    game_hack = format!("{} {}", &game, &app.locale.localization.friends_view.status.playing);
                  }
                } else {
                  if let Some(presence) = &friend.game_presence {
                    game_hack = format!("{} {}: {}", &app.locale.localization.friends_view.status.playing, &game, &presence);
                  } else {
                    game_hack = format!("{} {}", &app.locale.localization.friends_view.status.playing, &game);
                  }
                }
                (&game_hack, FRIEND_INGAME_COLOR)
                
              } else {
                (&app.locale.localization.friends_view.status.online, Color32::GREEN)
              }
            },
          };

          let (f_res, f_painter) = ui.allocate_painter(vec2(width, 2.0 + PFP_ELEMENT_SIZE + ((button_height + button_gap) * how_buttons)), egui::Sense::click());
          let mut highlight_rect = f_res.rect.clone();
          highlight_rect.set_height(2.0 + PFP_ELEMENT_SIZE);
          if f_res.clicked() {
            if buttons {
              app.friends_view_bar.friend_sel = String::new();
            } else {
              app.friends_view_bar.friend_sel = friend.id.clone();
            }
          }

          if how_buttons > 0.0 {
            let size = vec2((width - (ui.style().spacing.item_spacing.x * 2.0)) / 3.0, PFP_ELEMENT_SIZE * 0.6);

            let rect_0 = Rect {
              min: pos2(f_res.rect.min.x, f_res.rect.max.y - size.y),
              max: pos2(f_res.rect.min.x + size.x, f_res.rect.max.y)
            };

            let rect_1 = Rect {
              min: pos2(rect_0.max.x + ui.spacing().item_spacing.x, rect_0.min.y),
              max: pos2(rect_0.max.x + size.x + ui.spacing().item_spacing.x, rect_0.max.y)
            };

            let rect_2 = Rect {
              min: pos2(rect_1.max.x + ui.spacing().item_spacing.x, rect_1.min.y),
              max: pos2(rect_1.max.x + size.x + ui.spacing().item_spacing.x, rect_1.max.y)
            };

            ui.add_enabled_ui(false, |buttons| {
              if buttons.put(rect_0, egui::Button::new(app.locale.localization.friends_view.friend_actions.profile.to_ascii_uppercase())).clicked()
              || buttons.put(rect_1, egui::Button::new(app.locale.localization.friends_view.friend_actions.chat.to_ascii_uppercase())).clicked()
              || buttons.put(rect_2, egui::Button::new(app.locale.localization.friends_view.friend_actions.unfriend.to_ascii_uppercase())).clicked() {
                app.friends_view_bar.friend_sel = String::new();
            }
          });
          }

          if f_res.hovered() || buttons {
            f_painter.rect_filled(highlight_rect, FRIEND_HIGHLIGHT_ROUNDING, Color32::WHITE);
          }

          let pfp_rect = Rect {
            min: f_res.rect.min + vec2(2.0, 2.0),
            max: f_res.rect.min + vec2(2.0, 2.0) + vec2(PFP_SIZE, PFP_SIZE)
          };

          let outline_rect = Rect {
            min: pfp_rect.min - vec2(1.0, 1.0),
            max: pfp_rect.max + vec2(1.0, 1.0)
          };

          if let Some(pfp) = avatar {
            f_painter.image(pfp.renderable, pfp_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
          } else {
            f_painter.image(app.user_pfp_renderable, pfp_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
          }

          f_painter.rect(outline_rect, Rounding::same(4.0), Color32::TRANSPARENT, Stroke::new(2.0, friend_color));

          let text_col = if f_res.hovered() || buttons {
            Color32::BLACK
          } else {
            Color32::WHITE
          };

          f_painter.text(pfp_rect.center() + vec2(PFP_SIZE/1.5,  2.0), Align2::LEFT_BOTTOM, &friend.name, FontId::proportional(15.0), text_col);
          f_painter.text(pfp_rect.center() + vec2(PFP_SIZE/1.5,  2.0), Align2::LEFT_TOP, friend_status, FontId::proportional(10.0), text_col);
        }
      });
    });
  });
  ui.allocate_space(vec2(0.0,8.0));

  
}