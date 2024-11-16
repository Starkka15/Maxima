use egui::{vec2, Ui};

use crate::{widgets::enum_dropdown::enum_dropdown, FrontendLanguage, MaximaEguiApp};

#[derive(Debug, PartialEq)]
enum SettingsViewDemoTheme {
    System,
    Dark,
    Light,
}

pub fn settings_view(app: &mut MaximaEguiApp, ui: &mut Ui) {
    ui.style_mut().spacing.interact_size.y = 30.0;
    ui.style_mut().spacing.icon_width = 30.0;
    ui.heading(&app.locale.localization.settings_view.interface.header);
    ui.separator();
    ui.horizontal(|ui| {
        enum_dropdown(ui, "Settings_LanguageComboBox".to_owned(), &mut app.settings.language, 150.0, &app.locale.localization.settings_view.interface.language, &app.locale);
    });
        
    ui.heading("");
    ui.heading(&app.locale.localization.settings_view.game_installation.header);
    ui.separator();
    ui.label(&app.locale.localization.settings_view.game_installation.default_folder);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(ui.available_width() - (100.0 + ui.spacing().item_spacing.x), 30.0), egui::TextEdit::singleline(&mut app.settings.default_install_folder).vertical_align(egui::Align::Center));
        if ui.add_sized(vec2(100.0, 30.0), egui::Button::new("BROWSE")).clicked() {

        }
    });

    ui.heading("");
    ui.heading(&app.locale.localization.settings_view.performance.header);
    ui.separator();
    ui.checkbox(&mut app.settings.performance_settings.disable_blur, &app.locale.localization.settings_view.performance.disable_blur);
}
