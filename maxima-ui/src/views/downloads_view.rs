use egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Rect, Rounding, Shape, Stroke, Ui, Widget};
use humansize::DECIMAL;

use crate::{bridge_thread, MaximaEguiApp, APP_MARGIN};

#[derive(Clone)]
pub struct QueuedDownload {
    pub slug: String,
    pub offer: String,
    pub downloaded_bytes: usize,
    pub total_bytes: usize,
    // maybe add a thing here for updates? idk there's no real api to hook this up to yet
}

fn render_queued(app: &mut MaximaEguiApp, ui: &mut Ui, game: &QueuedDownload, is_current: bool) {
    if is_current { ui.ctx().request_repaint(); }
    ui.spacing_mut().item_spacing.y = APP_MARGIN.y;
    let container_size = vec2(ui.available_width(), 160.0);
    ui.allocate_ui(container_size, |ui| {
        let game_dl = game;
        let game = app.games.get_mut(&game.slug).unwrap();
        let (hero, logo) = {
            (
              app.img_cache.get(crate::ui_image::UIImageType::Hero(game.slug.clone())),
              app.img_cache.get(crate::ui_image::UIImageType::Logo(game.slug.clone()))
            )
          };
        let container_rect = Rect {
            min: ui.cursor().min,
            max: ui.cursor().min + container_size
        };

        let corner_radius = 8.0;

        ui.painter().rect_filled(container_rect, Rounding::same(corner_radius), Color32::BLACK);

        let img_rounding = Rounding { nw: corner_radius, ne: 0.0, sw: corner_radius, se: 0.0 };
        let img_response = if let Some(img) = hero {
            let img_size = vec2((160.0 / img.size_vec2().y) * img.size_vec2().x, 160.0);
            ui.add(egui::Image::new((img.id(), img_size)).rounding(img_rounding).max_size(img_size))
        } else {
            let response = ui.allocate_response(vec2(284.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(response.rect, img_rounding, Color32::GRAY);
            response
        };

        let top_left =        pos2(img_response.rect.max.x - 80.0, img_response.rect.min.y);
        let top_right =       pos2(img_response.rect.max.x - 00.0, img_response.rect.min.y);
        let top_righter =     pos2(img_response.rect.max.x + 10.0, img_response.rect.min.y);
        let bottom_left =     pos2(img_response.rect.max.x - 80.0, img_response.rect.max.y);
        let bottom_right =    pos2(img_response.rect.max.x - 00.0, img_response.rect.max.y);
        let bottom_righter =  pos2(img_response.rect.max.x + 10.0, img_response.rect.max.y);

        let mut mesh = Mesh::default();
        mesh.colored_vertex(top_left, Color32::TRANSPARENT);
        mesh.colored_vertex(bottom_left, Color32::TRANSPARENT);
        mesh.colored_vertex(top_right, Color32::BLACK);
        mesh.colored_vertex(bottom_right, Color32::BLACK);
        mesh.colored_vertex(top_righter, Color32::BLACK);
        mesh.colored_vertex(bottom_righter, Color32::BLACK);

        mesh.add_triangle(1, 3, 2);
        mesh.add_triangle(1, 2, 0);
        mesh.add_triangle(3, 5, 4);
        mesh.add_triangle(3, 4, 2);
        ui.painter().add(Shape::mesh(mesh));

        if let Some(handle) = &logo {
            let logo_rect = img_response.rect.clone().shrink(26.0);
            ui.put(logo_rect, egui::Image::new(handle).maintain_aspect_ratio(true).fit_to_exact_size(logo_rect.size()));
            //ui.painter().image(logo.renderable, logo_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }

        ui.painter().rect(container_rect, Rounding::same(corner_radius), Color32::TRANSPARENT, Stroke::new(2.0, Color32::WHITE));

        ui.painter().text(pos2(img_response.rect.max.x + 10.0, img_response.rect.min.y + 4.0), Align2::LEFT_TOP, &game.name, FontId::proportional(32.0), Color32::WHITE);

        let button_size = 40.0;
        let right_button_rect = Rect {
            min: pos2(container_rect.max.x - ((corner_radius - 1.0) + button_size), container_rect.min.y + (corner_radius - 1.0)),
            max: pos2(container_rect.max.x - (corner_radius - 1.0), container_rect.min.y + (corner_radius - 1.0) + button_size)
        };
        let left_button_rect = right_button_rect.translate(vec2(0.0, button_size + corner_radius));

        if is_current {
            let progress_bar_rect = Rect {
                min: img_response.rect.max + vec2(0.0, -18.0),
                max: container_rect.max - vec2(corner_radius, corner_radius)
            };
        
            let progress_bar_progress = Rect {
                min: progress_bar_rect.min,
                max: pos2(progress_bar_rect.min.x + (progress_bar_rect.width() * (game_dl.downloaded_bytes as f32 / game_dl.total_bytes as f32)), progress_bar_rect.max.y)
            };


            ui.painter().rect_filled(progress_bar_rect, Rounding::same(0.0), Color32::from_white_alpha(60));

            ui.painter().rect_filled(progress_bar_progress, Rounding::same(0.0), Color32::WHITE);

            
            ui.painter().text(progress_bar_rect.min - vec2(0.0, 8.0), Align2::LEFT_BOTTOM, format!("{}", humansize::SizeFormatter::new(game_dl.downloaded_bytes, DECIMAL)), FontId::proportional(12.0), Color32::WHITE);
            ui.painter().text(progress_bar_rect.max - vec2(0.0, progress_bar_rect.height() + 8.0), Align2::RIGHT_BOTTOM, format!("{}", humansize::SizeFormatter::new(game_dl.total_bytes, DECIMAL)), FontId::proportional(12.0), Color32::WHITE);

            if ui.put(left_button_rect, egui::Button::new("🗙")).clicked() {
                //TODO: Remove
            }
            if ui.put(right_button_rect, egui::Button::new("⏸")).clicked() {
                //TODO: Pause
            }
        } else {
            ui.painter().text(img_response.rect.max + vec2(18.0, -corner_radius), Align2::LEFT_BOTTOM, "Queued", FontId::proportional(22.0), Color32::WHITE);

            if ui.put(left_button_rect, egui::Button::new("🗙")).clicked() {
                //TODO: Remove
            }
            if ui.put(right_button_rect, egui::Button::new("⮉")).clicked() {
                //TODO: Move to top
            }
        }
        
    });
}

pub fn downloads_view(app : &mut MaximaEguiApp, ui: &mut Ui) {
    if let Some(now) = app.installing_now.clone() {
        render_queued(app, ui, &now, true);
        ui.separator();
    }
    for (_offer, game) in app.install_queue.clone() {
        if !_offer.eq(&game.offer) {
            egui::Label::new(egui::RichText::new(format!("offer mismatch! {} vs {}", _offer, game.offer))).selectable(false).ui(ui);
        } else {
            render_queued(app, ui, &game, false);
        }
    }
}