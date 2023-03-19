//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use anyhow::{anyhow, Result};
use eframe::egui;

pub fn gui() -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };
    eframe::run_native(
        "DRG Mod Integration",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    )
    .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

struct MyApp {
    table: TableDemo,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            table: Default::default(),
        }
    }
}

struct Mod {
    url: String,
    required: bool,
    version: Option<usize>,
    versions: Vec<String>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.table.ui(ui);
        });
    }
}

/// Shows off a table with dynamic layout
pub struct TableDemo {
    mods: Vec<Mod>,
}

impl Default for TableDemo {
    fn default() -> Self {
        Self {
            mods: { let mut mods = vec![];
                for _ in 0..100 {
                    mods.push(Mod {
                        url: "asdf".to_owned(),
                        required: false,
                        version: Some(1),
                        versions: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
                    });
                    mods.push(Mod {
                        url: "asdf2".to_owned(),
                        required: true,
                        version: Some(1),
                        versions: vec!["1".to_owned(), "2".to_owned(), "3".to_owned()],
                    });
                    mods.push(Mod {
                        url: "asdf2".to_owned(),
                        required: true,
                        version: None,
                        versions: vec![],
                    });
                }
                mods
            },
        }
    }
}

impl TableDemo {
    fn ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Size, StripBuilder};
        StripBuilder::new(ui)
            .size(Size::remainder().at_least(100.0)) // for the table
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        self.table_ui(ui);
                    });
                });
            });
    }
}

impl TableDemo {
    fn table_ui(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Column, TableBuilder};

        let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 6.0;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto())
            //.column(Column::initial(100.0).range(40.0..=300.0).resizable(true))
            .column(
                Column::initial(100.0)
                    .at_least(40.0)
                    .resizable(true)
                    .clip(true),
            )
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Mod");
                });
                header.col(|ui| {
                    ui.strong("Version");
                });
                header.col(|ui| {
                    ui.strong("Required");
                });
            })
            .body(|body| {
                body.rows(text_height, self.mods.len(), |row_index, mut row| {
                    let mod_ = &mut self.mods[row_index];
                    row.col(|ui| {
                        ui.label(&mod_.url);
                    });
                    row.col(|ui| {
                            egui::ComboBox::from_id_source(row_index)
                                .selected_text(
                                    match mod_.version {
                                        Some(index) => &mod_.versions[index],
                                        None => "latest",
                                    }
                                )
                                .show_ui(ui, |ui| {
                                    ui.style_mut().wrap = Some(false);
                                    ui.set_min_width(60.0);
                                    ui.selectable_value(&mut mod_.version, None, "latest");
                                    for (i, v) in mod_.versions.iter().enumerate() {
                                        ui.selectable_value(&mut mod_.version, Some(i), v);
                                    }
                                });
                            /*
                            ui.label(
                            match mod_.version {
                                Some(index) => &mod_.versions[index],
                                None => "-",
                            }
                            );
                            */
                            //expanding_content(ui);
                    });
                    row.col(|ui| {
                        ui.add(egui::Checkbox::without_text(&mut mod_.required));
                    });
                });
            });
    }
}

fn expanding_content(ui: &mut egui::Ui) {
    let width = ui.available_width().clamp(20.0, 200.0);
    let height = ui.available_height();
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        (1.0, ui.visuals().text_color()),
    );
}
