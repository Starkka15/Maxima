use std::{cmp, sync::Arc};

use egui::{text::{LayoutJob, TextWrapping}, vec2, Color32, Id, Margin, Rounding, Sense, Stroke, TextFormat, Ui};
use egui_extras::StripBuilder;
use maxima::rtm::{client::BasicPresence};

use crate::{DemoEguiApp, bridge_thread, ui_image::UIImage, widgets::enum_dropdown::enum_dropdown};

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
const PFP_SIZE: f32 = 42.0;
const PFP_IMG_SIZE: f32 = PFP_SIZE - 4.0;

pub fn friends_view(app : &mut DemoEguiApp, ui: &mut Ui) {
  puffin::profile_function!();
  let context = ui.ctx().clone();
  // this is a fucking mistake.
  let sidebar_rect = ui.available_rect_before_wrap();
  let mistake = ui.allocate_rect(sidebar_rect, Sense::hover());
  //ui.painter().rect_filled(mistake.rect, Rounding::ZERO, Color32::from_black_alpha(255));
  let hovering_friends = context.animate_bool_with_time(egui::Id::new("FriendsListWidthAnimator"), mistake.hovered(), ui.style().animation_time*2.0);
  let hover_diff = 300.0 - PFP_SIZE;
  app.friends_width = PFP_SIZE + (hovering_friends * hover_diff);
  ui.allocate_space(vec2(0.0, -mistake.rect.height()));

  let top_bar = egui::Frame::default()
  //.fill(Color32::from_gray(255))
  //.outer_margin(Margin::same(-4.0))
  //.inner_margin(Margin::same(5.0))
  ;
  
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
      friends.sort_by(|a,b| {a.name.cmp(&b.name)});
      friends.sort_by(|a,b| {(b.online.clone() as u8).cmp(&(a.online.clone() as u8))});
      
      
      // scrollbar
      ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::WHITE;
      ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.active.rounding = Rounding::same(4.0);
      ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(4.0);

      egui::ScrollArea::new([false,mistake.hovered()])
      .id_source("FriendsListFriendListScrollArea") //hmm yes, the friends list is made of friends list
      .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
      .show(ui, |ui| {
        puffin::profile_scope!("friends");
        let mut marge = Margin::same(0.0);
        marge.bottom = 4.5;
        let scrollbar_width = ui.spacing().scroll_bar_width;
        let item_width = app.friends_width - (if mistake.hovered() { scrollbar_width + 3.0 } else { 0.0 });
        ui.label(format!("{} friends", friends.len()));
        StripBuilder::new(ui)
        .clip(true)
        .sizes(egui_extras::Size::initial(PFP_SIZE), friends.len()) 
        .vertical(|mut friends_ui| {
          for friend in friends {
            puffin::profile_scope!("friend");
            let buttons = app.friends_view_bar.friend_sel.eq(&friend.id) && mistake.hovered();
            let how_buttons = context.animate_bool(Id::new("friendlistbuttons_".to_owned()+&friend.id), buttons);
            let avatar: Option<&Arc<UIImage>> = match &friend.avatar {
              UIFriendImageWrapper::DoNotLoad => {
                None
              },
              UIFriendImageWrapper::Unloaded(url) => {
                let _ = app.backend.tx.send(bridge_thread::MaximaLibRequest::GetUserAvatarRequest(friend.id.clone(), url.to_string()));
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
            let friend_status = 
            match friend.online {
              
              BasicPresence::Unknown => "Unknown".to_string(),
              BasicPresence::Offline => "Offline".to_string(),
              BasicPresence::Dnd => "Do not Disturb".to_string(),
              BasicPresence::Away => "Away".to_string(),
              BasicPresence::Online => {
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
                }.to_string()
              },
            };



            friends_ui.cell(|friendo| {
              friendo.spacing_mut().item_spacing = vec2(0.0,0.0);
              let sensor = friendo.allocate_rect(friendo.available_rect_before_wrap(), Sense::click());
              let how_hover = context.animate_bool(Id::new("friendlistrect_".to_owned()+&friend.id), sensor.hovered() || buttons);
              let rect_bg_col = Color32::from_white_alpha((how_hover*u8::MAX as f32) as u8);
              let text_col = Color32::from_gray(((1.0-how_hover)*u8::MAX as f32) as u8);
              friendo.allocate_space(-sensor.rect.size());
              friendo.painter().rect_filled(sensor.rect, Rounding::same(4.0), rect_bg_col);
              friendo.horizontal(|friendo| {
                friendo.allocate_space(vec2(2.0,0.0));
                friendo.vertical(|friendo| {
                  friendo.allocate_space(vec2(0.0,2.0));
                  if let Some(pfp) = avatar {
                    friendo.image((pfp.renderable, vec2(PFP_IMG_SIZE,PFP_IMG_SIZE)));
                  } else {
                    friendo.image((app.user_pfp_renderable, vec2(PFP_IMG_SIZE,PFP_IMG_SIZE)));
                  }
                  let mut outline_rect = sensor.rect.clone();
                  outline_rect.min += vec2(1.0,1.0);
                  outline_rect.set_height(PFP_IMG_SIZE + 2.0);
                  outline_rect.set_width(PFP_IMG_SIZE + 2.0);
                  friendo.painter().rect_stroke(outline_rect, Rounding::same(4.0), Stroke::new(2.0, 
                    match friend.online {
                        BasicPresence::Unknown => Color32::DARK_RED,
                        BasicPresence::Offline => Color32::GRAY,
                        BasicPresence::Dnd => Color32::RED,
                        BasicPresence::Away => Color32::GOLD,
                        BasicPresence::Online => Color32::GREEN,
                    }));
                });
                //if friend.online {  } else {  }
                friendo.allocate_space(vec2(6.0,0.0));
                friendo.vertical(|muchotexto| {
                  muchotexto.allocate_space(vec2(0.0,2.0));
                  let friend_name = egui::Label::new(egui::RichText::new(&friend.name).size(15.0).color(text_col)).wrap(false);
                  muchotexto.add(friend_name);
                  muchotexto.allocate_space(vec2(0.0,3.0));
                  muchotexto.label(egui::RichText::new(friend_status).color(text_col).size(10.0));
                });

              });


            });
          }
        });
        
        //ui.allocate_space(vec2(item_width-2.0,ui.available_size_before_wrap().y));
      });
    });
  });
  ui.allocate_space(vec2(0.0,8.0));

  
}