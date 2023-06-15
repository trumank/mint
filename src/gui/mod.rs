mod message;

//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
};

use anyhow::{anyhow, Result};
use eframe::{egui, epaint::text::LayoutJob};

use crate::{
    state::{config::ConfigWrapper, ModProfiles, ModConfig},
    error::IntegrationError,
    providers::{ModSpecification, ModStore},
    Config,
};

use request_counter::{RequestCounter, RequestID};

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
    profiles: ConfigWrapper<ModProfiles>,
    profile_dropdown: String,
    log: String,
    resolve_mod: String,
    resolve_mod_rid: Option<RequestID>,
    integrate_rid: Option<RequestID>,
    request_counter: RequestCounter,
    dnd: egui_dnd::DragDropUi,
}

impl App {
    fn new() -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();

        let data_dir = std::path::Path::new("data");
        std::fs::create_dir(data_dir).ok();
        let config: ConfigWrapper<Config> = ConfigWrapper::new(data_dir.join("config.json"));
        let profiles: ConfigWrapper<ModProfiles> =
            ConfigWrapper::new(data_dir.join("profiles.json"));
        let store = ModStore::new(data_dir, &config.provider_parameters)?.into();

        Ok(Self {
            tx,
            rx,
            request_counter: Default::default(),
            store,
            config,
            profiles,
            profile_dropdown: "default".to_string(),
            log: Default::default(),
            resolve_mod: Default::default(),
            resolve_mod_rid: None,
            integrate_rid: None,
            dnd: Default::default(),
        })
    }

    fn ui_profile(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.ui_profile_table(ui);
            });
        });
    }

    fn ui_profile_table(&mut self, ui: &mut egui::Ui) {
        let mods = &mut self.profiles.get_active_profile_mut().mods;
        let mut needs_save = false;
        let mut btn_remove = None;

        use egui_dnd::utils::shift_vec;
        use egui_dnd::DragDropItem;

        struct DndItem<'item> {
            index: usize,
            item: &'item mut ModConfig,
        }

        impl<'item> DragDropItem for DndItem<'item> {
            fn id(&self) -> egui::Id {
                egui::Id::new(self.index)
            }
        }

        let mut items = mods
            .iter_mut()
            .enumerate()
            .map(|(index, item)| DndItem { index, item })
            .collect::<Vec<_>>();

        let res = self
            .dnd
            .ui::<DndItem>(ui, items.iter_mut(), |item, ui, handle| {
                ui.horizontal(|ui| {
                    handle.ui(ui, item, |ui| {
                        ui.label("☰");
                    });

                    if ui.button("remove").clicked() {
                        btn_remove = Some(item.index);
                    }

                    if ui
                        .add(egui::Checkbox::without_text(&mut item.item.required))
                        .changed()
                    {
                        needs_save = true;
                    }

                    let info = self.store.get_mod_info(&item.item.spec);
                    if let Some(info) = info {
                        ui.label(&info.name);
                    } else {
                        ui.label(&item.item.spec.url);
                    }
                });
            });

        if let Some(response) = res.completed {
            shift_vec(response.from, response.to, mods);
        }

        if let Some(remove) = btn_remove {
            mods.remove(remove);
            needs_save = true;
        }
        if needs_save {
            self.profiles.save().unwrap();
        }
    }

    fn add_mod(&mut self, ctx: &egui::Context) {
        let rid = self.request_counter.next();
        let spec = ModSpecification {
            url: self.resolve_mod.to_string(),
        };
        let store = self.store.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let res = store.resolve_mod(spec, false).await;
            tx.send(message::Message::ResolveMod(rid, res)).unwrap();
            ctx.request_repaint();
        });
        self.resolve_mod_rid = Some(rid);
    }
}

mod request_counter {
    /// Simple counter that returns a new ID each time it is called
    #[derive(Default)]
    pub struct RequestCounter(u32);

    impl RequestCounter {
        /// Get next ID
        pub fn next(&mut self) -> RequestID {
            let id = self.0;
            self.0 += 1;
            RequestID { id }
        }
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct RequestID {
        id: u32,
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // message handling
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                message::Message::Log(log) => {
                    self.log.push_str(&log);
                    self.log.push('\n');
                }
                message::Message::ResolveMod(rid, res) => {
                    if Some(rid) == self.resolve_mod_rid {
                        match res {
                            Ok((_spec, mod_)) => {
                                self.profiles.get_active_profile_mut().mods.push(ModConfig {
                                    spec: mod_.spec,
                                    required: mod_.suggested_require,
                                });
                                self.profiles.save().unwrap();
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
                                Err(e) => {
                                    self.log.push_str(&format!("{:#?}\n", e));
                                }
                            },
                        }
                        self.resolve_mod_rid = None;
                    }
                }
                message::Message::Integrate(rid, res) => {
                    if Some(rid) == self.integrate_rid {
                        match res {
                            Ok(()) => {
                                self.log.push_str("Integration complete\n");
                            }
                            Err(e) => {
                                self.log.push_str(&format!("{:#?}\n", e));
                            }
                        }
                        self.integrate_rid = None;
                    }
                }
            }
        }

        // begin draw
        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.log.as_str()));
                    });
                },
            );
        });
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    ui.add(egui::widgets::ProgressBar::new(0.5));
                    ui.add_enabled_ui(self.integrate_rid.is_none(), |ui| {
                        if ui.button("integrate").clicked() {
                            self.integrate_rid = integrate(
                                &mut self.request_counter,
                                self.store.clone(),
                                self.profiles
                                    .get_active_profile()
                                    .mods
                                    .iter()
                                    .map(|m| m.spec.clone())
                                    .collect(),
                                self.tx.clone(),
                                ctx.clone(),
                            );
                        }
                    });
                },
            );
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let resolve = ui.add_enabled(
                    self.resolve_mod_rid.is_none(),
                    egui::TextEdit::singleline(&mut self.resolve_mod).hint_text("Resolve mod..."),
                );
                if is_committed(&resolve) {
                    self.add_mod(ctx);
                }
                if self.resolve_mod_rid.is_some() {
                    ui.spinner();
                }
            });

            // profile selection
            ui.horizontal(|ui| {
                let res = ui.add(egui_dropdown::DropDownBox::from_iter(
                    self.profiles.profiles.keys(),
                    "profile_dropdown",
                    &mut self.profile_dropdown,
                    |ui, text| {
                        let mut job = LayoutJob {
                            halign: egui::Align::LEFT,
                            ..Default::default()
                        };
                        job.append(text, 0.0, Default::default());
                        ui.selectable_label(text == self.profiles.active_profile, job)
                    },
                ));
                if res.gained_focus() {
                    self.profile_dropdown.clear();
                }

                if self.profiles.profiles.contains_key(&self.profile_dropdown) {
                    self.profiles.active_profile = self.profile_dropdown.to_string();
                    self.profiles.save().unwrap();
                }

                ui.add_enabled_ui(
                    self.profiles.profiles.contains_key(&self.profile_dropdown)
                        && self.profiles.profiles.len() > 1,
                    |ui| {
                        if ui.button("-").clicked() {
                            self.profiles.remove_active();
                            self.profile_dropdown = self.profiles.active_profile.to_string();
                            self.profiles.save().unwrap();
                        }
                    },
                );
                ui.add_enabled_ui(
                    self.profile_dropdown != self.profiles.active_profile,
                    |ui| {
                        if ui.button("+").clicked() {
                            self.profiles
                                .profiles
                                .entry(self.profile_dropdown.to_string())
                                .or_default();
                            self.profiles.active_profile = self.profile_dropdown.to_string();
                            self.profiles.save().unwrap();
                        }
                    },
                );
            });

            ui.separator();

            self.ui_profile(ui);

            ctx.input(|i| {
                for e in &i.events {
                    if let egui::Event::Paste(s) = e {
                        if ctx.memory(|m| m.focus().is_none()) {
                            self.resolve_mod = s.to_string();
                            self.add_mod(ctx);
                        }
                    }
                }
            });
        });
    }
}

fn is_committed(res: &egui::Response) -> bool {
    res.lost_focus() && res.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

fn integrate(
    rc: &mut RequestCounter,
    store: Arc<ModStore>,
    mods: Vec<ModSpecification>,
    tx: Sender<message::Message>,
    ctx: egui::Context,
) -> Option<RequestID> {
    let rid = rc.next();

    async fn integrate(store: Arc<ModStore>, mod_specs: Vec<ModSpecification>) -> Result<()> {
        use anyhow::Context;

        let path_game = if let Some(mut steamdir) = steamlocate::SteamDir::locate() {
            steamdir.app(&548430).map(|a| a.path.clone())
        } else {
            None
        }
        .context(
            "Could not find DRG install directory, please specify manually with the --drg flag",
        )?;

        let update = false;

        let mods = loop {
            match store.resolve_mods(&mod_specs, update).await {
                Ok(mods) => break mods,
                Err(e) => match e.downcast::<IntegrationError>() {
                    Ok(IntegrationError::NoProvider {
                        spec: _,
                        factory: _,
                    }) => {
                        // TODO providers should already be initialized by now?
                        unimplemented!();
                    }
                    Err(e) => return Err(e),
                },
            }
        };

        let to_integrate = mod_specs
            .iter()
            .map(|u| mods[u].clone())
            .collect::<Vec<_>>();
        let urls = to_integrate
            .iter()
            .map(|m| &m.spec) // TODO this should be a ModResolution not a ModSpecification, we're missing a step here
            .collect::<Vec<&ModSpecification>>();

        println!("fetching mods...");
        let paths = store.fetch_mods(&urls, update).await?;

        crate::integrate::integrate(path_game, to_integrate.into_iter().zip(paths).collect())?;

        Ok(())
    }

    tokio::task::spawn(async move {
        let res = integrate(store, mods).await;
        tx.send(message::Message::Integrate(rid, res)).unwrap();
        ctx.request_repaint();
    });
    Some(rid)
}
