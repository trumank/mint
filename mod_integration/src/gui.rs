use crate::{
    get_approval, Approval, Config, ModId, ModProfile, Mods, Name, Path, Settings, STATIC_SETTINGS,
};

use crate::cache::{ModioCache, ModioMod};

use modio::filter::Like;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use anyhow::Result;

pub fn launch_gui(config: Config, mods: Mods) -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(500.0, 300.0)),
        min_window_size: Some(egui::vec2(500.0, 300.0)),
        ..Default::default()
    };
    eframe::run_native(
        "DRG Mod Integration",
        options,
        Box::new(|_cc| {
            Box::new(App {
                config,
                mods,
                ..Default::default()
            })
        }),
    );
    Ok(())
}

#[derive(Default)]
struct RequestCounter(u32);

impl RequestCounter {
    fn next(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

#[derive(Debug)]
struct ValidatedSetting<T: std::cmp::PartialEq + std::clone::Clone> {
    current_value: T,
    validated_value: T,
    validation_result: Result<(), String>,
}

impl<T> ValidatedSetting<T>
where
    T: std::cmp::PartialEq + std::clone::Clone,
{
    /// Create new validated setting that defaults to valid
    fn new(value: T) -> Self {
        ValidatedSetting {
            current_value: value.clone(),
            validated_value: value,
            validation_result: Ok(()),
        }
    }
    /// Returns whether the current value is the same as the validated value
    fn is_modified(&self) -> bool {
        self.current_value != self.validated_value
    }
    /// Sets the result of validation and updates the validated value
    fn set_validation_result(&mut self, result: Result<(), String>) {
        self.validated_value = self.current_value.clone();
        self.validation_result = result;
    }
    /// Get validation error if unmodified and exists
    fn get_err(&self) -> Option<&String> {
        match &self.validation_result {
            Ok(_) => None,
            Err(msg) => {
                if self.is_modified() {
                    None
                } else {
                    Some(msg)
                }
            }
        }
    }
    /// Returns whether the value is unmodified and if it is valid
    fn is_valid(&self) -> bool {
        match &self.validation_result {
            Ok(_) => !self.is_modified(),
            Err(_) => false,
        }
    }
}

struct SettingsDialog {
    validated_key: ValidatedSetting<String>,
    validation_rid: Option<u32>,
    validated_fsd_install: ValidatedSetting<String>,
}

#[derive(Default)]
struct ModSearch {
    query: String,
    search_results: Option<Result<Vec<ModId>>>,
    search_rid: Option<u32>,
}
enum ModSearchAction {
    Add(ModId),
}
impl ModSearch {
    #[must_use]
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        settings: &Settings,
        tx: &Sender<Msg>,
        rc: &mut RequestCounter,
        modio_cache: &HashMap<ModId, ModioMod>,
    ) -> Option<ModSearchAction> {
        ui.heading("Mod.io search");
        let mut action = None;
        let search_res = ui.add_enabled(
            true, //self.search_rid.is_none(),
            egui::TextEdit::singleline(&mut self.query).hint_text("Search mods..."),
        );
        if is_committed(&search_res) {
            search_res.request_focus();
            let id = rc.next();
            search_modio_mods(
                id,
                self.query.clone(),
                tx.clone(),
                ui.ctx().clone(),
                settings.clone(),
            );
            self.search_rid = Some(id);
        }
        if self.search_rid.is_some() {
            ui.spinner();
        }
        egui::ScrollArea::both()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("search_results")
                    .num_columns(4)
                    //.max_col_width(200.0)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("add");
                        ui.label("mod");
                        ui.label("approval");
                        ui.end_row();

                        if let Some(results) = &self.search_results {
                            match results {
                                Ok(mods) => {
                                    for id in mods {
                                        let mod_ = modio_cache.get(id).unwrap();
                                        if ui.button("<-").clicked() {
                                            action = Some(ModSearchAction::Add(*id));
                                        }
                                        ui.hyperlink_to(&mod_.name, &mod_.url);
                                        ui.label(match get_approval(mod_) {
                                            Approval::Verified => "Verified",
                                            Approval::Approved => "Approved",
                                            Approval::Sandbox => "Sandbox",
                                            // TODO auto verified
                                        });
                                        ui.allocate_space(ui.available_size());
                                        ui.end_row();
                                    }
                                }
                                Err(err) => {
                                    ui.colored_label(ui.visuals().error_fg_color, err.to_string());
                                }
                            }
                        }
                    });
            });
        action
    }
    fn receive(&mut self, rid: u32, res: Result<Vec<ModId>>) {
        if let Some(id) = self.search_rid {
            if id == rid {
                self.search_results = Some(res);
                self.search_rid = None;
            }
        }
    }
}

#[derive(Default)]
struct ModProfileEditor {
    selected_profile: Option<String>,
}
impl ModProfileEditor {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        mod_profiles: &mut HashMap<String, ModProfile>,
        modio_cache: &mut ModioCache,
    ) {
        ui.heading("Profile editor");
        egui::ScrollArea::both()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for profile in mod_profiles.keys() {
                        let selected = Some(profile) == self.selected_profile.as_ref();
                        if ui.selectable_label(selected, profile).clicked() {
                            self.selected_profile = if selected { None } else { Some(profile.to_owned()) };
                        }
                    }
                });
                if let Some(selected_profile) = &self.selected_profile && let Some(profile) = mod_profiles.get_mut(selected_profile) {
                    egui::Grid::new("mod_profile_editor")
                        .num_columns(4)
                        //.max_col_width(200.0)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("");
                            ui.label("mod");
                            ui.label("approval");
                            //ui.label("approval");
                            ui.end_row();

                            let mut to_remove = vec![];
                            for id in profile.mods.keys() {
                                if ui.button("x").clicked() {
                                    to_remove.push(*id);
                                }
                                if let Some(modio) = modio_cache.mods.get(id) {
                                    ui.hyperlink_to(&modio.name, &modio.url);
                                    ui.label(match get_approval(modio) {
                                        Approval::Verified => "Verified",
                                        Approval::Approved => "Approved",
                                        Approval::Sandbox => "Sandbox",
                                        // TODO auto verified
                                    });
                                }
                                ui.allocate_space(ui.available_size());
                                ui.end_row();
                            }
                            for id in to_remove {
                                profile.mods.remove(&id);
                            }
                        });
                }
            });
    }
    fn add_mod(&mut self, mod_profiles: &mut HashMap<String, ModProfile>, mod_id: ModId) {
        if let Some(selected_profile) = &self.selected_profile && let Some(profile) = mod_profiles.get_mut(selected_profile) {
            profile.mods.insert(mod_id, Default::default());
        }
    }
}

pub struct App {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    request_counter: RequestCounter,
    log: String,
    mods: Mods,
    showing_about: bool,
    settings_dialog: Option<SettingsDialog>,
    config: Config,
    mod_search: ModSearch,
    mod_profile_editor: ModProfileEditor,
}

impl Default for App {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        App {
            tx,
            rx,
            request_counter: Default::default(),
            log: "".to_owned(),
            config: Default::default(),
            mods: Default::default(),
            settings_dialog: None,
            showing_about: false,
            mod_search: Default::default(),
            mod_profile_editor: Default::default(),
        }
    }
}

fn is_committed(res: &egui::Response) -> bool {
    res.lost_focus() && res.ctx.input().key_pressed(egui::Key::Enter)
}

fn is_valid_fsd_install(path: &String) -> bool {
    Path::exists(&Path::new(path).join("FSD/Content/Paks/FSD-WindowsNoEditor.pak"))
}

impl App {
    fn about_dialog(&mut self, ui: &mut egui::Ui) {
        if ui.button("About").clicked() {
            self.showing_about = true;
        }
        if self.showing_about {
            egui::Window::new("About")
                .auto_sized()
                .collapsible(false)
                .open(&mut self.showing_about)
                .show(ui.ctx(), |ui| {
                    ui.heading(format!(
                        "DRG Mod Integration v{}",
                        env!("CARGO_PKG_VERSION")
                    ));

                    ui.horizontal(|ui| {
                        ui.label("data dir:");
                        if ui
                            .link(STATIC_SETTINGS.data_dir.display().to_string())
                            .clicked()
                        {
                            opener::open(&STATIC_SETTINGS.data_dir).ok();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("cache dir:");
                        if ui
                            .link(STATIC_SETTINGS.cache_dir.display().to_string())
                            .clicked()
                        {
                            opener::open(&STATIC_SETTINGS.cache_dir).ok();
                        }
                    });
                });
        }
    }
    fn settings_dialog(&mut self, ui: &mut egui::Ui) {
        if ui.button("Settings").clicked() {
            self.settings_dialog = Some(SettingsDialog {
                validated_key: ValidatedSetting::new(
                    self.config
                        .settings
                        .modio_key
                        .as_ref()
                        .map_or_else(|| "".to_string(), |p| p.clone()),
                ),
                validation_rid: None,
                validated_fsd_install: ValidatedSetting::new(
                    self.config
                        .settings
                        .fsd_install
                        .as_ref()
                        .map_or_else(|| "".to_string(), |p| p.clone()),
                ),
            });
        }
        let (rc, settings) = (&mut self.request_counter, &mut self.settings_dialog);
        if let Some(settings) = settings {
            let mut open = true;
            let mut try_save = false;
            egui::Window::new("Settings")
                .auto_sized()
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    egui::Grid::new("settings_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            // modio API key
                            let label =
                                ui.hyperlink_to("mod.io API key:", "https://mod.io/me/access#api");
                            ui.add_enabled_ui(settings.validation_rid.is_none(), |ui| {
                                let color = if !settings.validated_key.is_modified() {
                                    match &settings.validated_key.validation_result {
                                        Ok(_) => Some(egui::Color32::GREEN),
                                        Err(_) => Some(egui::Color32::RED),
                                    }
                                } else {
                                    None
                                };
                                let mut key_box = egui::TextEdit::singleline(
                                    &mut settings.validated_key.current_value,
                                )
                                .password(true);
                                if let Some(color) = color {
                                    key_box = key_box.text_color(color);
                                }
                                let mut key_box_res = ui.add(key_box).labelled_by(label.id);

                                key_box_res = if let Some(err) = settings.validated_key.get_err() {
                                    key_box_res.on_hover_ui(|ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.colored_label(ui.visuals().error_fg_color, err);
                                        });
                                    })
                                } else {
                                    key_box_res
                                };

                                if is_committed(&key_box_res) {
                                    try_save = true;
                                }
                            });
                            ui.end_row();

                            // fsd_install
                            let fsd_install_label = ui.label("DRG install: ");
                            let color = if !settings.validated_fsd_install.is_modified() {
                                match &settings.validated_fsd_install.validation_result {
                                    Ok(_) => Some(egui::Color32::GREEN),
                                    Err(_) => Some(egui::Color32::RED),
                                }
                            } else {
                                None
                            };
                            let mut fsd_path_box = egui::TextEdit::singleline(
                                &mut settings.validated_fsd_install.current_value,
                            );
                            if let Some(color) = color {
                                fsd_path_box = fsd_path_box.text_color(color);
                            }
                            let mut fsd_path_box_res =
                                ui.add(fsd_path_box).labelled_by(fsd_install_label.id);
                            fsd_path_box_res =
                                if let Some(err) = settings.validated_fsd_install.get_err() {
                                    fsd_path_box_res.on_hover_ui(|ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.colored_label(ui.visuals().error_fg_color, err);
                                        });
                                    })
                                } else {
                                    fsd_path_box_res
                                };
                            if is_committed(&fsd_path_box_res) {
                                try_save = true;
                            }
                            ui.end_row();

                            ui.horizontal(|ui| {
                                ui.set_enabled(
                                    settings.validated_key.get_err().is_none()
                                        && settings.validated_fsd_install.get_err().is_none(),
                                );
                                if ui.button("Save").clicked() {
                                    try_save = true;
                                }
                                if settings.validation_rid.is_some() {
                                    ui.spinner();
                                }
                            });

                            if try_save {
                                settings.validation_rid = Some(check_key(
                                    rc.next(),
                                    settings.validated_key.current_value.clone(),
                                    self.tx.clone(),
                                    ui.ctx().clone(),
                                ));
                                settings.validated_fsd_install.set_validation_result(
                                    if is_valid_fsd_install(
                                        &settings.validated_fsd_install.current_value,
                                    ) {
                                        Ok(())
                                    } else {
                                        Err("not valid because reasons".to_owned())
                                    },
                                );
                            }
                        });
                });
            if !open {
                self.settings_dialog = None;
            }
        }
    }
}

impl App {
    fn save_config(&self) {
        self.config.save(&STATIC_SETTINGS.config_path).unwrap();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut log = |msg: String| {
            println!("{msg}");
            self.log.push_str(&format!("\n{msg}"));
        };
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(msg) => log(msg),
                Msg::SearchResultMods(rid, mods_res) => {
                    self.mod_search.receive(
                        rid,
                        mods_res.map(|mods| {
                            mods.into_iter()
                                .map(|m| {
                                    let id = m.id;
                                    self.config.modio_cache.mods.insert(id, m);
                                    id
                                })
                                .collect::<Vec<_>>()
                        }),
                    );
                    self.save_config();
                }
                Msg::KeyCheck(rid, res) => {
                    if let Some(settings) = &mut self.settings_dialog {
                        if let Some(srid) = settings.validation_rid {
                            if srid == rid {
                                settings.validation_rid = None;
                                match res {
                                    Ok(_) => {
                                        settings.validated_key.set_validation_result(Ok(()));
                                    }
                                    Err(err) => {
                                        settings
                                            .validated_key
                                            .set_validation_result(Err(err.to_string()));
                                    }
                                }
                                if settings.validated_key.is_valid()
                                    && settings.validated_fsd_install.is_valid()
                                {
                                    self.config.settings.modio_key =
                                        Some(settings.validated_key.current_value.clone());
                                    self.config.settings.fsd_install =
                                        Some(settings.validated_fsd_install.current_value.clone());
                                    self.settings_dialog = None;
                                    self.save_config();
                                }
                            }
                        }
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("DRG Mod Integration");
                self.about_dialog(ui);
                self.settings_dialog(ui);
            });

            ui.separator();

            ui.columns(3, |columns| {
                columns[0].group(|ui| {
                    ui.push_id(0, |ui| {
                        ui.heading("Loaded mods");
                        egui::ScrollArea::both()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                egui::Grid::new("my_grid")
                                    .num_columns(6)
                                    //.spacing([40.0, 4.0])
                                    .striped(true)
                                    .show(ui, |ui| {
                                        ui.label("");
                                        ui.label("mod");
                                        ui.label("version");
                                        ui.label("approval");
                                        ui.label("required");
                                        ui.end_row();

                                        //for mod_ in &mut self.mods.mods {
                                        //self.mods.mods.drain_filter(|mod_| {
                                        let mods = &mut self.mods.mods;
                                        let mut i = 0;
                                        while i < mods.len() {
                                            let mut mod_ = &mut mods[i];
                                            let remove = ui.button("x").clicked();
                                            let name = &mod_.name.as_ref().unwrap_or(&mod_.id);
                                            ui.add(doc_link_label(name, name));

                                            let empty = "-".to_string();

                                            let version = mod_.version.as_ref().unwrap_or(&empty);
                                            ui.label(version);

                                            let approval = match mod_.approval {
                                                Some(Approval::Verified) => "Verified",
                                                Some(Approval::Approved) => "Approved",
                                                Some(Approval::Sandbox) => "Sandbox",
                                                None => "-",
                                            };
                                            ui.label(approval);

                                            let mut required = mod_.required.unwrap_or_default();
                                            ui.add_enabled(
                                                true,
                                                egui::Checkbox::new(&mut required, ""),
                                            );
                                            mod_.required = Some(required);

                                            ui.allocate_space(ui.available_size());
                                            ui.end_row();

                                            if remove {
                                                mods.remove(i);
                                            } else {
                                                i += 1;
                                            }
                                        }
                                    });
                            });
                    });
                    ui.allocate_space(ui.available_size());
                });
                columns[1].group(|ui| {
                    ui.push_id(1, |ui| {
                        self.mod_profile_editor.ui(
                            ui,
                            &mut self.config.mod_profiles,
                            &mut self.config.modio_cache,
                        );
                    });
                    ui.allocate_space(ui.available_size());
                });
                columns[2].group(|ui| {
                    ui.push_id(2, |ui| {
                        let action = self.mod_search.ui(
                            ui,
                            &self.config.settings,
                            &self.tx,
                            &mut self.request_counter,
                            &self.config.modio_cache.mods,
                        );
                        match action {
                            Some(ModSearchAction::Add(mod_)) => {
                                self.mod_profile_editor
                                    .add_mod(&mut self.config.mod_profiles, mod_);
                                self.save_config();
                            }
                            None => {}
                        }
                    });
                    ui.allocate_space(ui.available_size());
                });
                /*
                columns[3].group(|ui| {
                    ui.push_id(3, |ui| {
                        egui::ScrollArea::both()
                            .auto_shrink([false; 2])
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for (id, mod_) in &self.config.modio_cache.mods {
                                    ui.label(format!("{} {}", id.0, mod_.name));
                                }
                            });
                    });
                    ui.allocate_space(ui.available_size());
                });
                */
            });

            /*
            ui.push_id(1, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| ui.label(&self.log));
            });
            */
        });
    }
}

fn doc_link_label<'a>(title: &'a str, search_term: &'a str) -> impl egui::Widget + 'a {
    let label = format!("{title}:");
    let url = format!("https://drg.old.mod.io/?filter=t&kw={search_term}");
    move |ui: &mut egui::Ui| {
        ui.hyperlink_to(label, url).on_hover_ui(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Search egui docs for");
                ui.code(search_term);
            });
        })
    }
}

fn search_modio_mods(
    rid: u32,
    query: String,
    tx: Sender<Msg>,
    ctx: egui::Context,
    settings: Settings,
) {
    tokio::spawn(async move {
        let mods_res = settings
            .modio()
            .expect("could not get modio object")
            .game(STATIC_SETTINGS.game_id)
            .mods()
            .search(Name::like(format!("*{query}*")))
            .collect()
            .await;
        let _ = tx.send(Msg::SearchResultMods(
            rid,
            mods_res
                .map_err(anyhow::Error::msg)
                .map(|m| m.into_iter().map(|m| m.into()).collect()),
        ));
        ctx.request_repaint();
    });
}

fn check_key(rid: u32, key: String, tx: Sender<Msg>, ctx: egui::Context) -> u32 {
    tokio::spawn(async move {
        let r = check_key_async(key).await.map_err(anyhow::Error::msg);
        let _ = tx.send(Msg::KeyCheck(rid, r));
        ctx.request_repaint();
    });
    rid
}

async fn check_key_async(key: String) -> Result<modio::games::Game> {
    Ok(modio::Modio::new(modio::Credentials::new(key))?
        .game(STATIC_SETTINGS.game_id)
        .get()
        .await?)
}

#[derive(Debug)]
enum Msg {
    Log(String),
    SearchResultMods(u32, Result<Vec<ModioMod>>),
    KeyCheck(u32, Result<modio::games::Game>),
}
