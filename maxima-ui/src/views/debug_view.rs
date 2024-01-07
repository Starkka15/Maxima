use egui::Ui;
use egui_extras::{Column, TableBuilder};

use crate::DemoEguiApp;

pub fn debug_view(_app: &mut DemoEguiApp, ui: &mut Ui) {
    use egui_extras::{Size, StripBuilder};
    StripBuilder::new(ui)
        .size(Size::exact(30.0))
        .size(Size::exact(300.0))
        .size(Size::exact(10.0))
        .vertical(|mut strip| {
            strip.cell(|ui| {
                ui.heading("LSX Logs");
            });
            strip.cell(|ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(false)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::exact(50.))
                        .column(Column::remainder())
                        .min_scrolled_height(0.0);

                    table
                        .header(20.0, |mut head| {
                            head.col(|ui| {
                                ui.strong("Type");
                            });
                            head.col(|ui| {
                                ui.strong("Contents");
                            });
                        })
                        .body(|mut body| {
                            for idx in 0..16 {
                                body.row(20.0, |mut row| {
                                    row.col(|col| {
                                        col.label(if idx % 3 == 0 { "RX" } else { "TX" });
                                    });
                                    row.col(|col| {
                                        col.label("idk the shit go here lmao");
                                    });
                                });
                            }
                        });
                });
            });
            strip.cell(|ui| {
                ui.heading("etc");
            });
        });
}
