mod message;

//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::sync::{
    mpsc::{Receiver, Sender},
    Arc,
};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::{
    config::ConfigWrapper,
    error::IntegrationError,
    providers::{ModSpecification, ModStore},
    Config,
};

pub fn gui() -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };
    eframe::run_native(
        "DRG Mod Integration",
        options,
        Box::new(|_cc| Box::new(App::new().unwrap())),
    )
    .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

struct App {
    tx: Sender<message::Message>,
    rx: Receiver<message::Message>,
    store: Arc<ModStore>,
    config: ConfigWrapper<Config>,
    table: TableDemo,
    log: String,
    resolve_mod: String,
}

impl App {
    fn new() -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();

        let data_dir = std::path::Path::new("data");
        std::fs::create_dir(data_dir).ok();
        let config: ConfigWrapper<Config> = ConfigWrapper::new(data_dir.join("config.json"));
        let store = ModStore::new(data_dir, &config.provider_parameters)?.into();

        Ok(Self {
            tx,
            rx,
            store,
            config,
            table: Default::default(),
            log: Default::default(),
            resolve_mod: Default::default(),
        })
    }
}

struct Mod {
    url: String,
    required: bool,
    version: Option<usize>,
    versions: Vec<String>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                message::Message::Log(log) => {
                    self.log.push_str(&log);
                    self.log.push('\n');
                }
                message::Message::ResolveMod(res) => {
                    match res {
                        Ok(mod_) => {
                            println!("{mod_:?}");
                        }
                        Err(e) => match e.downcast::<IntegrationError>() {
                            Ok(IntegrationError::NoProvider { spec, factory }) => {
                                println!("Initializing provider for {:?}", spec);
                                let params = self
                                    .config
                                    .provider_parameters
                                    .entry(factory.id.to_owned())
                                    .or_default();
                                for p in factory.parameters {
                                    if !params.contains_key(p.name) {
                                        let value = dialoguer::Password::with_theme(
                                            &dialoguer::theme::ColorfulTheme::default(),
                                        )
                                        .with_prompt(p.description)
                                        .interact()
                                        .unwrap();
                                        params.insert(p.id.to_owned(), value);
                                    }
                                }
                                //self.store.add_provider(factory, params).unwrap();
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    if ui.button("Log stuff").clicked() {
                        self.tx
                            .send(message::Message::Log("asdf".to_owned()))
                            .unwrap();
                    }
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.log.as_str()));
                    });
                },
            );
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let resolve = ui.add(
                    egui::TextEdit::singleline(&mut self.resolve_mod).hint_text("Resolve mod..."),
                );
                if is_committed(&resolve) {
                    let ctx = ui.ctx().clone();
                    let tx = self.tx.clone();
                    let store = self.store.clone();
                    let spec = ModSpecification {
                        url: self.resolve_mod.to_owned(),
                    };
                    tokio::spawn(async move {
                        match store.resolve_mod(spec, false).await {
                            Ok((spec, mod_)) => tx
                                .send(message::Message::Log(format!("Resolved mod: {spec:?}")))
                                .unwrap(),
                            Err(e) => tx.send(message::Message::Log(format!("{e}"))).unwrap(),
                        }
                        ctx.request_repaint();
                    });
                }
            });

            ui.separator();

            self.table.ui(ui);
        });
    }
}

fn is_committed(res: &egui::Response) -> bool {
    res.lost_focus() && res.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Shows off a table with dynamic layout
pub struct TableDemo {
    mods: Vec<Mod>,
}

impl Default for TableDemo {
    fn default() -> Self {
        Self {
            mods: {
                let mut mods = vec![];
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

        let table = TableBuilder::new(ui)
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
                            .selected_text(match mod_.version {
                                Some(index) => &mod_.versions[index],
                                None => "latest",
                            })
                            .show_ui(ui, |ui| {
                                ui.style_mut().wrap = Some(false);
                                ui.set_min_width(60.0);
                                ui.selectable_value(&mut mod_.version, None, "latest");
                                for (i, v) in mod_.versions.iter().enumerate() {
                                    ui.selectable_value(&mut mod_.version, Some(i), v);
                                }
                            });
                    });
                    row.col(|ui| {
                        ui.add(egui::Checkbox::without_text(&mut mod_.required));
                    });
                });
            });
    }
}
