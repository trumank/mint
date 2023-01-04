#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use anyhow::Result;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use std::io::{BufReader, Read, Write};

use modio::download::DownloadAction;
use modio::filter::prelude::*;

use uesave::PropertyMeta::Str;
use uesave::Save;

use clap::{Parser, Subcommand};

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

#[derive(Debug, Clone)]
struct StaticSettings {
    game_id: u32,
    data_dir: PathBuf,
    cache_dir: PathBuf,

    config_path: PathBuf,
    mod_cache_dir: PathBuf,
}

lazy_static::lazy_static! {
    static ref STATIC_SETTINGS: StaticSettings = {
        let name = env!("CARGO_PKG_NAME");
        let data_dir = dirs::data_dir().expect("Could not find user home directory").join(name);
        fs::create_dir(&data_dir).ok();
        let cache_dir = dirs::cache_dir().expect("Could not find user cache directory").join(name);
        fs::create_dir(&cache_dir).ok();

        StaticSettings {
            game_id: 2475,
            config_path: data_dir.join("config.json"),
            mod_cache_dir: cache_dir.join("mods"),
            data_dir,
            cache_dir,
        }
    };
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    // Log to stdout (if you run with `RUST_LOG=debug`).
    //tracing_subscriber::fmt::init();
    //
    let config = Settings::load_or_create_default(&STATIC_SETTINGS.config_path)?;

    let command = Args::parse().action;
    match command {
        Action::Gui(_) => {
            let save_buffer = std::fs::read(
                config
                    .mod_config_save()
                    .expect("could not find mod config save"),
            )?;
            let json = extract_config_from_save(&save_buffer)?;
            let mods: Mods = serde_json::from_str(&json)?;
            println!("{:#?}", mods);

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            let options = eframe::NativeOptions {
                initial_window_size: Some(egui::vec2(500.0, 300.0)),
                min_window_size: Some(egui::vec2(500.0, 300.0)),
                ..Default::default()
            };
            let (tx, rx) = std::sync::mpsc::channel();
            eframe::run_native(
                "DRG Mod Integration",
                options,
                Box::new(|_cc| {
                    Box::new(App {
                        tx,
                        rx,
                        request_counter: Default::default(),
                        name: "custom".to_owned(),
                        log: "asdf".to_owned(),
                        mods,
                        showing_about: false,
                        settings_dialog: None,
                        config,
                    })
                }),
            );
            Ok(())
        }
        _ => rt.block_on(async {
            match command {
                Action::Install(args) => install(&config, args).await,
                Action::Sync(args) => sync(&config, args).await,
                Action::Run(args) => run(&config, args).await,
                Action::Gui(_) => panic!("unreachable"),
            }
        }),
    }
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Settings {
    modio_key: Option<String>,
    fsd_install: Option<String>,
}

impl Settings {
    fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(serde_json::from_reader::<_, Settings>(File::open(path)?)?)
    }
    fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        match File::open(&path) {
            Ok(f) => Ok(serde_json::from_reader::<_, Settings>(f)?),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Settings::default();
                config.save(path)?;
                Ok(config)
            }
            Err(err) => Err(err.into()),
        }
    }
    fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        serde_json::to_writer_pretty(File::create(path)?, &self)?;
        Ok(())
    }
    fn paks_dir(&self) -> Option<PathBuf> {
        self.fsd_install
            .as_ref()
            .map(|p| Path::new(p).join("FSD/Content/Paks"))
    }
    fn mod_config_save(&self) -> Option<PathBuf> {
        self.fsd_install
            .as_ref()
            .map(|p| Path::new(p).join("FSD/Saved/SaveGames/Mods/ModIntegration.sav"))
    }
    fn modio(&self) -> Option<modio::Modio> {
        self.modio_key
            .as_ref()
            .and_then(|k| modio::Modio::new(modio::Credentials::new(k)).ok())
    }
}

struct App {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    request_counter: RequestCounter,
    name: String,
    log: String,
    mods: Mods,
    showing_about: bool,
    settings_dialog: Option<SettingsDialog>,
    config: Settings,
}

/*
impl Default for MyApp {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tx,
            rx,
            name: "Arthur".to_owned(),
            age: 42,
            log: "asdf".to_owned(),
            mods: Mods {
                mods: vec![],
                request_sync: false
            },
        }
    }
}
*/

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
                        .modio_key
                        .as_ref()
                        .map_or_else(|| "".to_string(), |p| p.clone()),
                ),
                validation_rid: None,
                validated_fsd_install: ValidatedSetting::new(
                    self.config
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut log = |msg: String| {
            println!("{}", msg);
            self.log.push_str(&format!("\n{}", msg));
        };
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(msg) => log(msg),
                Msg::SearchResult(mods_res) => match mods_res {
                    Ok(mods) => {
                        log("request complete".to_owned());
                        self.mods.mods = mods
                            .into_iter()
                            .map(mod_entry_from_modio)
                            .collect::<Result<Vec<ModEntry>>>()
                            .ok()
                            .unwrap();
                    }
                    Err(err) => {
                        log(format!("request failed: {}", err));
                    }
                },
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
                                    self.config.modio_key =
                                        Some(settings.validated_key.current_value.clone());
                                    self.config.fsd_install =
                                        Some(settings.validated_fsd_install.current_value.clone());
                                    self.settings_dialog = None;
                                    self.config.save(&STATIC_SETTINGS.config_path).unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("DRG Mod Integration");

            ui.horizontal(|ui| {
                self.about_dialog(ui);
                self.settings_dialog(ui);
            });

            ui.horizontal(|ui| {
                let name_label = ui.label("mod query: ");
                let search_box = ui
                    .text_edit_singleline(&mut self.name)
                    .labelled_by(name_label.id);
                if is_committed(&search_box) {
                    search_box.request_focus();
                    search(
                        self.name.clone(),
                        self.tx.clone(),
                        ctx.clone(),
                        self.config.clone(),
                    );
                }
            });
            //ui.label(&self.log);

            ui.separator();

            ui.push_id(0, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("my_grid")
                            .num_columns(5)
                            //.spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("mod");
                                ui.label("version");
                                ui.label("approval");
                                ui.label("required");
                                ui.end_row();

                                for mod_ in &mut self.mods.mods {
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
                                    ui.add_enabled(false, egui::Checkbox::new(&mut required, ""));
                                    mod_.required = Some(required);

                                    ui.allocate_space(ui.available_size());
                                    ui.end_row();
                                }
                            });
                    });
            });

            ui.separator();

            ui.push_id(1, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| ui.label(&self.log));
            });
        });
    }
}

fn doc_link_label<'a>(title: &'a str, search_term: &'a str) -> impl egui::Widget + 'a {
    let label = format!("{}:", title);
    let url = format!("https://drg.old.mod.io/?filter=t&kw={}", search_term);
    move |ui: &mut egui::Ui| {
        ui.hyperlink_to(label, url).on_hover_ui(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Search egui docs for");
                ui.code(search_term);
            });
        })
    }
}

fn search(name: String, tx: Sender<Msg>, ctx: egui::Context, config: Settings) {
    tokio::spawn(async move {
        let mods_res = config
            .modio()
            .expect("could not get modio object")
            .game(STATIC_SETTINGS.game_id)
            .mods()
            .search(Name::like(format!("*{}*", name)))
            .collect()
            .await;
        let _ = tx.send(Msg::SearchResult(mods_res.map_err(anyhow::Error::msg)));
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
    SearchResult(Result<Vec<modio::mods::Mod>>),
    KeyCheck(u32, Result<modio::games::Game>),
}

#[derive(Parser, Debug)]
struct ActionInstall {
    /// Path to mod config. If empty, will install the mod integration without any mods.
    #[arg(index = 1)]
    config: Option<String>,

    #[arg(short, long)]
    update: bool,
}

#[derive(Parser, Debug)]
struct ActionSync {}

#[derive(Parser, Debug)]
struct ActionRun {
    #[arg(index = 1, trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}
#[derive(Parser, Debug)]
struct ActionGui {}

#[derive(Subcommand, Debug)]
enum Action {
    /// Install mods with specified config
    Install(ActionInstall),
    /// Sync mods with host using config saved in ModIntegration.sav
    Sync(ActionSync),
    /// Passthrough from steam to directly launch the game
    Run(ActionRun),
    /// Launch GUI
    Gui(ActionGui),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

async fn run(config: &Settings, args: ActionRun) -> Result<()> {
    use std::process::Command;
    if let Some((cmd, args)) = args.args.split_first() {
        //install(&env, ActionInstall { config: None, update: false }).await?;
        loop {
            Command::new(cmd)
                .args(args)
                .arg("-disablemodding")
                .spawn()
                .expect("failed to execute process")
                .wait()?;

            let save_buffer = std::fs::read(
                config
                    .mod_config_save()
                    .ok_or_else(|| anyhow!("mod config save not found"))?,
            )?;
            let json = extract_config_from_save(&save_buffer)?;
            if serde_json::from_str::<Mods>(&json)?.request_sync {
                sync(config, ActionSync {}).await?;
            } else {
                break;
            }
        }
    } else {
        return Err(anyhow!("missing command"));
    }

    Ok(())
}

async fn sync(config: &Settings, args: ActionSync) -> Result<()> {
    let save_buffer = std::fs::read(
        config
            .mod_config_save()
            .ok_or_else(|| anyhow!("mod config save not found"))?,
    )?;
    let json = extract_config_from_save(&save_buffer)?;
    let mods: Mods = serde_json::from_str(&json)?;
    println!("{:#?}", mods);

    let mod_config = install_config(config, mods, false).await?;

    Ok(())
}

async fn install(config: &Settings, args: ActionInstall) -> Result<()> {
    let mods = if let Some(path) = &args.config {
        let config_path = std::path::Path::new(path);

        let file = File::open(config_path)?;
        serde_json::from_reader(file)?
    } else {
        Mods {
            mods: vec![],
            request_sync: true,
        }
    };
    println!("{:#?}", mods);

    let mod_config = install_config(config, mods, args.update).await?;

    if args.update {
        if let Some(path) = &args.config {
            let file = File::create(path).unwrap();
            serde_json::to_writer_pretty(file, &mod_config).unwrap();
        }
    }

    Ok(())
}

fn mod_entry_from_modio(mod_: modio::mods::Mod) -> Result<ModEntry> {
    Ok(ModEntry {
        id: mod_.id.to_string(),
        name: Some(mod_.name.to_owned()),
        approval: Some(get_approval(&mod_)),
        required: Some(is_required(&mod_)),
        version: if let Some(modfile) = mod_.modfile {
            Ok(Some(mod_.id.to_string()))
        } else {
            Err(anyhow!("mod={} does not have any modfiles", mod_.id))
        }?,
    })
}

async fn populate_config(
    config: &Settings,
    mods: Mods,
    update: bool,
    mod_hashes: &mut HashMap<u32, String>,
) -> Result<Mods> {
    let mut config_map: indexmap::IndexMap<_, _> = mods
        .mods
        .into_iter()
        .map(|m| (m.id.parse::<u32>().unwrap(), m))
        .collect();

    let mut to_check: HashSet<u32> = config_map.keys().copied().collect();

    while !to_check.is_empty() {
        println!("to check: {:?}", &to_check);
        let mut dependency_reqs = tokio::task::JoinSet::new();

        for id in to_check.iter().copied() {
            let deps = config
                .modio()
                .expect("could not create modio object")
                .mod_(STATIC_SETTINGS.game_id, id)
                .dependencies();
            dependency_reqs.spawn(async move { (id, deps.list().await) });
        }

        println!("requesting mods");
        let mods_res = config
            .modio()
            .expect("could not create modio object")
            .game(STATIC_SETTINGS.game_id)
            .mods()
            .search(Id::_in(to_check.iter().copied().collect::<Vec<_>>()))
            .collect()
            .await?;
        to_check.clear();
        for res in mods_res.into_iter() {
            let mut mod_config = config_map.get_mut(&res.id).unwrap();
            mod_config.name = Some(res.name.to_owned());
            mod_config.approval = Some(get_approval(&res));
            mod_config.required = Some(is_required(&res));
            if let Some(modfile) = res.modfile {
                mod_hashes.insert(modfile.id, modfile.filehash.md5);
                if mod_config.version.is_none() || update {
                    mod_config.version = Some(modfile.id.to_string());
                }
            } else {
                return Err(anyhow!("mod={} does not have any modfiles", mod_config.id));
            }
        }
        println!("requesting dependencies");
        while let Some(Ok(res)) = dependency_reqs.join_next().await {
            for dep in res.1? {
                println!("found dependency {:?}", dep);
                if !config_map.contains_key(&dep.mod_id) {
                    config_map.insert(
                        dep.mod_id,
                        ModEntry {
                            id: dep.mod_id.to_string(),
                            name: None,
                            version: None,
                            approval: None,
                            required: None,
                        },
                    );
                    to_check.insert(dep.mod_id);
                }
            }
        }
    }

    Ok(Mods {
        mods: config_map.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        request_sync: false,
    })
}

/// Take config, validate against mod.io, install, return populated config
async fn install_config(config: &Settings, mods: Mods, update: bool) -> Result<Mods> {
    println!("installing config={:#?}", mods);

    let mut mod_hashes = HashMap::new();
    let mod_config = populate_config(config, mods, update, &mut mod_hashes).await?;

    let mut paks = vec![];

    fs::create_dir(&STATIC_SETTINGS.mod_cache_dir).ok();

    for entry in &mod_config.mods {
        let mod_id = entry.id.parse::<u32>()?;
        if let Some(version) = &entry.version {
            let file_id = version.parse::<u32>()?;
            let file_path = &STATIC_SETTINGS
                .mod_cache_dir
                .join(format!("{}.zip", file_id));
            if !file_path.exists() {
                println!(
                    "downloading mod={} version={} path={}",
                    mod_id,
                    file_id,
                    file_path.display()
                );
                config
                    .modio()
                    .expect("could not create modio object")
                    .download(DownloadAction::File {
                        game_id: STATIC_SETTINGS.game_id,
                        mod_id,
                        file_id,
                    })
                    .save_to_file(&file_path)
                    .await?;
            }

            let modfile;
            let hash = if let Some(hash) = mod_hashes.get(&file_id) {
                hash
            } else {
                println!("requesting modfile={}", file_id);
                modfile = config
                    .modio()
                    .expect("could not create modio object")
                    .game(STATIC_SETTINGS.game_id)
                    .mod_(mod_id)
                    .file(file_id)
                    .get()
                    .await?;
                &modfile.filehash.md5
            };

            use md5::{Digest, Md5};
            let mut hasher = Md5::new();
            std::io::copy(&mut File::open(file_path)?, &mut hasher)?;
            let local_hash = hex::encode(hasher.finalize());
            println!("checking file hash modio={} local={}", hash, local_hash);
            assert_eq!(hash, &local_hash);

            let buf = get_pak_from_file(file_path)?;
            paks.push((format!("{}", mod_id), buf));
        } else {
            panic!("unreachable");
        }
    }
    let loader = include_bytes!("../mod-integration.pak").to_vec();
    paks.push(("loader".to_string(), loader));

    for entry in fs::read_dir(config.paks_dir().expect("could not find paks directory"))
        .expect("Unable to list")
    {
        let entry = entry.expect("unable to get entry");
        if entry.file_type()?.is_dir() {
            continue;
        };
        if let Some(name) = entry.file_name().to_str() {
            if name.ends_with(".pak") && name != "FSD-WindowsNoEditor.pak" {
                fs::remove_file(entry.path())?;
            }
        }
    }

    let ar_search = "AssetRegistry.bin".as_bytes();
    for (id, buf) in paks {
        let name = if contains(&buf, ar_search) {
            format!("{}.pak", id)
        } else {
            format!("{}_P.pak", id)
        };
        let mut out_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(
                config
                    .paks_dir()
                    .expect("could not find paks dir")
                    .join(name),
            )?;
        out_file.write_all(&buf)?;
    }

    // write config to mod integration save file
    let mut out_save = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(
            config
                .mod_config_save()
                .expect("could not find mod config save"),
        )?;
    out_save.write_all(&wrap_config(serde_json::to_string(&mod_config)?)?)?;

    println!("mods installed");

    Ok(mod_config)
}

fn contains(source: &[u8], needle: &[u8]) -> bool {
    'outer: for i in 0..(source.len() - needle.len() + 1) {
        for j in 0..needle.len() {
            if source[i + j] != needle[j] {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

// TODO implement for raw paks
fn get_pak_from_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).unwrap();
    let reader = BufReader::new(file);

    let mut archive = zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let raw_path = file.name().to_owned();
        match file.enclosed_name() {
            Some(path) => path,
            None => {
                println!("Entry {} has a suspicious path", raw_path);
                continue;
            }
        };

        if file.is_file() {
            let mut buffer: Vec<u8> = vec![];
            file.read_to_end(&mut buffer)?;
            return Ok(buffer);
        }
    }
    Err(anyhow!("Zip does not contain pak"))
}

fn get_approval(mod_: &modio::mods::Mod) -> Approval {
    for tag in &mod_.tags {
        if let Ok(approval) = Approval::from_str(&tag.name) {
            return approval;
        }
    }
    Approval::Sandbox
}

fn is_required(mod_: &modio::mods::Mod) -> bool {
    for tag in &mod_.tags {
        if tag.name == "RequiredByAll" {
            return true;
        }
    }
    false
}

#[derive(Debug, Serialize, Deserialize)]
struct Mods {
    mods: Vec<ModEntry>,
    #[serde(default)]
    request_sync: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModEntry {
    id: String,
    name: Option<String>,
    version: Option<String>,
    approval: Option<Approval>,
    required: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Approval {
    Sandbox,
    Verified,
    Approved,
}

impl FromStr for Approval {
    type Err = ();

    fn from_str(input: &str) -> Result<Approval, Self::Err> {
        match input {
            "Verified" => Ok(Approval::Verified),
            "Approved" => Ok(Approval::Approved),
            "Sandbox" => Ok(Approval::Sandbox),
            _ => Err(()),
        }
    }
}

fn extract_config_from_save(buffer: &[u8]) -> Result<String> {
    let mut save_rdr = std::io::Cursor::new(buffer);
    let save = Save::read(&mut save_rdr)?;

    if let Str { value: json, .. } = &save.root.root[0].value {
        Ok(json.to_string())
    } else {
        Err(anyhow!("Malformed save file"))
    }
}
fn wrap_config(config: String) -> Result<Vec<u8>> {
    let buffer = include_bytes!("../ModIntegration.sav");
    let mut save_rdr = std::io::Cursor::new(&buffer[..]);
    let mut save = Save::read(&mut save_rdr)?;

    if let Str { value: json, .. } = &mut save.root.root[0].value {
        *json = config;
        let mut out_buffer = vec![];
        save.write(&mut out_buffer)?;
        Ok(out_buffer)
    } else {
        Err(anyhow!("Malformed save file"))
    }
}
