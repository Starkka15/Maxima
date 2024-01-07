use egui::Ui;

use crate::DemoEguiApp;

#[derive(Debug, PartialEq)]
enum SettingsViewDemoTheme {
    System,
    Dark,
    Light,
}

pub fn settings_view(_app: &mut DemoEguiApp, ui: &mut Ui) {
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
