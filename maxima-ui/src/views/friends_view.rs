use egui::{Ui, Color32, Margin, Vec2, vec2, Rounding, TextBuffer, Layout};

use crate::DemoEguiApp;

#[derive(Debug, PartialEq, Default)]
pub enum FriendsViewBarStatusFilter {
  #[default] Name,
  Game,
}

#[derive(Debug, PartialEq, Default)]
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
}

struct Friend {
  name : String,
  online : bool,
  game : Option<String>,
  game_presence : Option<String>
}


pub fn friends_view(app : &mut DemoEguiApp, ui: &mut Ui) {
  let friends_raw : Vec<Friend> = Vec::from(
    [
      Friend {
        name : "AMoistEggroll".to_owned(),
        online : false,
        game: None,
        game_presence: None,
      },
      Friend {
        name : "BattleDash".to_owned(),
        online : true,
        game: Some("Battlefield 2042".to_owned()),
        game_presence: None,
      },
      Friend {
        name : "GEN_Burnout".to_owned(),
        online : true,
        game: None,
        game_presence: None,
      },
      Friend {
        name : "KursedKrabbo".to_owned(),
        online : true,
        game: Some("Titanfall 2".to_owned()),
        game_presence: Some("Pilots vs Pilots on Glitch".to_owned()),
      }
    ]
  );

  let top_bar = egui::Frame::default()
  //.fill(Color32::from_gray(255))
  .outer_margin(Margin::same(-4.0))
  .inner_margin(Margin::same(5.0));
  
  top_bar.show(ui, |ui| {
    ui.style_mut().spacing.item_spacing = vec2(5.0,5.0);
    
    ui.horizontal(|ui| {
      let button_width = (ui.available_width() - 20.0) / 5.0;
      if ui.add_sized([button_width, 20.0],egui::Button::new(&app.locale.localization.friends_view.toolbar.online).fill(if app.friends_view_bar.page == FriendsViewBarPage::Online {Color32::from_white_alpha(20)} else { Color32::TRANSPARENT })).clicked() {
        app.friends_view_bar.page = FriendsViewBarPage::Online;
      }
      if ui.add_sized([button_width, 20.0],egui::Button::new(&app.locale.localization.friends_view.toolbar.all).fill(if app.friends_view_bar.page == FriendsViewBarPage::All {Color32::from_white_alpha(20)} else { Color32::TRANSPARENT })).clicked() {
        app.friends_view_bar.page = FriendsViewBarPage::All;
      }
      if ui.add_sized([button_width, 20.0],egui::Button::new(&app.locale.localization.friends_view.toolbar.pending).fill(if app.friends_view_bar.page == FriendsViewBarPage::Pending {Color32::from_white_alpha(20)} else { Color32::TRANSPARENT })).clicked() {
        app.friends_view_bar.page = FriendsViewBarPage::Pending;
      }
      if ui.add_sized([button_width, 20.0],egui::Button::new(&app.locale.localization.friends_view.toolbar.blocked).fill(if app.friends_view_bar.page == FriendsViewBarPage::Blocked {Color32::from_white_alpha(20)} else { Color32::TRANSPARENT })).clicked() {
        app.friends_view_bar.page = FriendsViewBarPage::Blocked;
      }
      if ui.add_sized([button_width, 20.0], egui::Button::new(&app.locale.localization.friends_view.toolbar.add_friend)).clicked() {

      }
    });
    ui.horizontal(|ui| {
      ui.style_mut().spacing.item_spacing.x = 5.0;
      ui.set_min_size(vec2(160.0, 20.0));
      ui.push_id("FriendsListStatusFilterComboBox", |horizontal| {
        egui::ComboBox::from_label("")
        .selected_text(match app.friends_view_bar.status_filter {
          FriendsViewBarStatusFilter::Name => &app.locale.localization.friends_view.toolbar.filter_options.name,
          FriendsViewBarStatusFilter::Game => &app.locale.localization.friends_view.toolbar.filter_options.game
        })
        .show_ui(horizontal, |combo| {
          combo.selectable_value(&mut app.friends_view_bar.status_filter, FriendsViewBarStatusFilter::Name, &app.locale.localization.friends_view.toolbar.filter_options.name);
          combo.selectable_value(&mut app.friends_view_bar.status_filter, FriendsViewBarStatusFilter::Game, &app.locale.localization.friends_view.toolbar.filter_options.game);
          }
        );
      });
      
      ui.add_sized([ui.available_width(), 20.0], egui::TextEdit::hint_text(egui::text_edit::TextEdit::singleline(&mut app.friends_view_bar.search_buffer), "Search friends list"));
    });
    
  });
  ui.allocate_space(vec2(0.0,8.0));

  let friends : Vec<Friend> = friends_raw.into_iter().filter(|obj| 
    match app.friends_view_bar.status_filter {
        FriendsViewBarStatusFilter::Name => obj.name.to_ascii_lowercase().contains((&app.friends_view_bar.search_buffer)),
        FriendsViewBarStatusFilter::Game => if let Some(game) = &obj.game {
          game.to_ascii_lowercase().contains((&app.friends_view_bar.search_buffer))
        } else {
          false
        }
    }
    &&
    match app.friends_view_bar.page {
        FriendsViewBarPage::Online => obj.online,
        FriendsViewBarPage::All => true,
        FriendsViewBarPage::Pending => todo!(),
        FriendsViewBarPage::Blocked => todo!(),
    }
  ).collect();

  let columns = if ui.available_width() >  (270.0 * 4.0) { 4 } else { ui.available_width() as u32 / 270};
  let rows = (friends.len() as f32  / columns as f32).ceil() as usize;

  ui.vertical(|ui| {
    ui.style_mut().spacing.item_spacing = vec2(0.0,0.0);
    for row_idx in 0..rows {
      let row_offset = row_idx as u32 * columns;
      ui.horizontal(|ui| {
        ui.allocate_space(vec2((ui.available_width() - (270.0 * columns as f32)) / 2.0, 0.0)); // AHAHAHAHAHAHAHAHAHAHA FUCK YOU EGUI! (later headass here, what this does is offset it from the left, centering didn't work)
        for col_idx in 0..columns {
          let idx = row_offset + col_idx;
          if idx < friends.len() as u32{
            let friend = &friends[idx as usize];
            let mut friend_str : String = "".to_owned();
            friend_str += &friend.name;

            if friend.online {
              if let Some(friend_game) = &friend.game {
                friend_str += &format!("\n{}",friend_game);
                if let Some(friend_presence) = &friend.game_presence {
                  friend_str += &format!("\n{}",friend_presence);
                }
              } else {
                friend_str += "\n";
                friend_str += &app.locale.localization.friends_view.status_online;
              }
            } else {
              friend_str += "\n";
              friend_str += &app.locale.localization.friends_view.status_offline;
            }

            let friend_frame = egui::Frame::default()
            .shadow(egui::epaint::Shadow { extrusion: 5.0, color: Color32::BLACK })
            .outer_margin(Margin::same(10.0))
            .inner_margin(Margin::same(2.0))
            .rounding(Rounding::same(5.0))
            .fill(Color32::BLACK);
            friend_frame.show(ui, |ui| {
              ui.add_sized(vec2(240.0,50.0),
              egui::Button::image_and_text(egui::TextureId::Managed(0),
              vec2(42.0,42.0),
              egui::RichText::new(friend_str)).rounding(Rounding::same(4.0)));
            });
          }
        }
      });
    }
  });
}