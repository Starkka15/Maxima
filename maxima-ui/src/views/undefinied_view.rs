use egui::Ui;

use crate::DemoEguiApp;

pub fn undefined_view(app: &mut DemoEguiApp, ui: &mut Ui) {
    ui.with_layout(
        egui::Layout::centered_and_justified(egui::Direction::RightToLeft),
        |ui| {
            ui.heading(&app.locale.localization.errors.view_not_impl);
        },
    );
}
