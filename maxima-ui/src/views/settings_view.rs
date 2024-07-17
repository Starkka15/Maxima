use egui::{vec2, Ui};

use crate::MaximaEguiApp;

#[derive(Debug, PartialEq)]
enum SettingsViewDemoTheme {
    System,
    Dark,
    Light,
}

pub fn settings_view(app: &mut MaximaEguiApp, ui: &mut Ui) {
    ui.style_mut().spacing.interact_size.y = 30.0;
    ui.heading("Game Installation");
    ui.separator();
    ui.label("Default installation folder:");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(ui.available_width() - (100.0 + ui.spacing().item_spacing.x), 30.0), egui::TextEdit::singleline(&mut app.settings.default_install_folder).vertical_align(egui::Align::Center));
        if ui.add_sized(vec2(100.0, 30.0), egui::Button::new("BROWSE")).clicked() {

        }
    });
    ui.heading("");
    ui.heading("Visuals");
    ui.separator();
    let mut val = SettingsViewDemoTheme::System;
    ui.push_id("Settings_VisualsComboBox", |horizontal| {
        egui::ComboBox::from_label("Colors")
            .selected_text("System Default")
            .show_ui(horizontal, |combo| {
                combo.selectable_value(&mut val, SettingsViewDemoTheme::System, "System Default");
                combo.selectable_value(&mut val, SettingsViewDemoTheme::Dark, "Dark");
                combo.selectable_value(&mut val, SettingsViewDemoTheme::Light, "Light");
            });
    });
    let mut val = 14.0;
    ui.add(egui::Slider::new(&mut val, 1.0..=15.0).suffix("px"));
}
