#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use egui::ScrollArea;
use serde::{Deserialize, Serialize};
//use serde_json::Result;
use anyhow::Result;
use anyhow::anyhow;
use std::fs::{self, OpenOptions, File};
use std::str::FromStr;
use std::path::{Path, PathBuf};

use std::io::{Read, Write, BufReader};

use modio::{Credentials, Modio};
use modio::filter::prelude::*;
use modio::download::DownloadAction;
use tokio::task::JoinSet;

use uesave::Save;
use uesave::PropertyMeta::Str;

use clap::{Parser, Subcommand};

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();
    std::thread::spawn(move || {
        rt.block_on(std::future::pending::<()>());
    });

    // Log to stdout (if you run with `RUST_LOG=debug`).
    //tracing_subscriber::fmt::init();
    //
    dotenv::dotenv().ok();
    let env = get_env()?;

    let save_buffer = std::fs::read(&env.mod_config_save)?;
    let json = extract_config_from_save(&save_buffer)?;
    let mods: Mods = serde_json::from_str(&json)?;
    println!("{:#?}", mods);

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    Ok(eframe::run_native(
        "My egui App",
        options,
        Box::new(|_cc| Box::new(MyApp {
            tx,
            rx,
            name: "*custom*".to_owned(),
            age: 42,
            log: "asdf".to_owned(),
            mods,
            env,
        })),
    ))
}

struct MyApp {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    name: String,
    age: u32,
    log: String,
    mods: Mods,
    env: Env,
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

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut log = |msg: String| {
            println!("{}", msg);
            self.log.push_str(&format!("\n{}", msg));
        };
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(msg) => {
                    log(msg)
                },
                Msg::SearchResult(mods_res) => {
                    match mods_res {
                        Ok(mods) => {
                            log("request complete".to_owned());
                            self.mods.mods = mods.into_iter().map(mod_entry_from_modio).collect::<Result<Vec<ModEntry>>>().ok().unwrap();
                        },
                        Err(err) => {
                            log(format!("request failed: {}", err.to_string()));
                        }
                    }
                },
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("DRG Mod Integration");
            ui.horizontal(|ui| {
                let name_label = ui.label("mod query: ");
                let search_box = ui.text_edit_singleline(&mut self.name)
                    .labelled_by(name_label.id);
                if search_box.lost_focus() && ctx.input().key_pressed(egui::Key::Enter) {
                    search_box.request_focus();
                    search(self.name.clone(), self.tx.clone(), ctx.clone(), self.env.clone());
                }
            });
            //ui.label(&self.log);

            ui.separator();

            ui.push_id(0, |ui| {
                ScrollArea::both()
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
                                        None => "-"
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
                ScrollArea::vertical()
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

fn search(name: String, tx: Sender<Msg>, ctx: egui::Context, env: Env) {
    tokio::spawn(async move {
        let mods_res = env.modio
            .game(env.game_id)
            .mods()
            .search(Name::like(format!("*{}*", name)))
            .collect().await;
        let _ = tx.send(Msg::SearchResult(mods_res.map_err(anyhow::Error::msg)));
        ctx.request_repaint();
    });
}
#[derive(Debug)]
enum Msg {
    Log(String),
    SearchResult(Result<Vec<modio::mods::Mod>>),
}

#[derive(Debug, Clone)]
struct Env {
    modio: modio::Modio,
    game_id: u32,
    paks_dir: PathBuf,
    mod_cache_dir: PathBuf,
    mod_config_save: PathBuf,
}

fn get_env() -> Result<Env> {
    let fsd_install = std::path::PathBuf::from(std::env::var("FSD_INSTALL").expect("Missing path to game root directory"));

    Ok(Env {
        modio: Modio::new(Credentials::new(std::env::var("MODIO_KEY").expect("Missing Mod.io API key")))?,
        //game_id: std::env::var("MODIO_GAME_ID").expect("Missing Mod.io game id").parse()?,
        game_id: 2475,
        paks_dir: fsd_install.join("FSD/Content/Paks"),
        mod_cache_dir: PathBuf::from("mods"),
        mod_config_save: fsd_install.join("FSD/Saved/SaveGames/Mods/ModIntegration.sav"),
    })
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

#[derive(Subcommand, Debug)]
enum Action {
   /// Install mods with specified config
   Install(ActionInstall),
   /// Sync mods with host using config saved in ModIntegration.sav
   Sync(ActionSync),
   /// Passthrough from steam to directly launch the game
   Run(ActionRun),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
   #[command(subcommand)]
   action: Action,
}

#[tokio::main]
async fn asdfmain() -> Result<()> {
    let mut path = std::env::current_exe()?;
    path.pop();
    //std::env::set_current_dir(path)?;
    //std::env::set_current_dir(Path::new("/home/truman/projects/drg-modding/tools/modloader-rs"))?;
    dotenv::dotenv().ok();
    let env = get_env()?;

    match Args::parse().action {
        Action::Install(args) => install(&env, args).await,
        Action::Sync(args) => sync(&env, args).await,
        Action::Run(args) => run(&env, args).await,
    }
}

async fn run(env: &Env, args: ActionRun) -> Result<()> {
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

            let save_buffer = std::fs::read(&env.mod_config_save)?;
            let json = extract_config_from_save(&save_buffer)?;
            if serde_json::from_str::<Mods>(&json)?.request_sync {
                sync(&env, ActionSync {}).await?;
            } else {
                break;
            }
        }
    } else {
        return Err(anyhow!("missing command"))
    }

    Ok(())
}

async fn sync(env: &Env, args: ActionSync) -> Result<()> {
    let save_buffer = std::fs::read(&env.mod_config_save)?;
    let json = extract_config_from_save(&save_buffer)?;
    let mods: Mods = serde_json::from_str(&json)?;
    println!("{:#?}", mods);

    let config = install_config(env, mods, false).await?;

    Ok(())
}

async fn install(env: &Env, args: ActionInstall) -> Result<()> {
    let mods = if let Some(path) = &args.config {
        let config_path = std::path::Path::new(path);

        let file = File::open(config_path)?;
        serde_json::from_reader(file)?
    } else {
        Mods {
            mods: vec![],
            request_sync: true
        }
    };
    println!("{:#?}", mods);

    let config = install_config(env, mods, args.update).await?;

    if args.update {
        if let Some(path) = &args.config {
            let file = File::create(path).unwrap();
            serde_json::to_writer_pretty(file, &config).unwrap();
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
        }?
    })
}

async fn populate_config(env: &Env, mods: Mods, update: bool, mod_hashes: &mut HashMap<u32, String>) -> Result<Mods> {
    let mut config_map: indexmap::IndexMap<_, _> = mods.mods.into_iter().map(|m| (m.id.parse::<u32>().unwrap(), m)).collect();

    let mut to_check: HashSet<u32> = config_map.keys().copied().collect();

    while !to_check.is_empty() {
        println!("to check: {:?}", &to_check);
        let mut dependency_reqs = JoinSet::new();

        for id in to_check.iter().copied() {
            let deps = env.modio.mod_(env.game_id, id).dependencies();
            dependency_reqs.spawn(async move { (id, deps.list().await) });
        }

        println!("requesting mods");
        let mods_res = env.modio.game(env.game_id).mods().search(Id::_in(to_check.iter().copied().collect::<Vec<_>>())).collect().await?;
        to_check.clear();
        for res in mods_res.into_iter() {
            let mut config = config_map.get_mut(&res.id).unwrap();
            config.name = Some(res.name.to_owned());
            config.approval = Some(get_approval(&res));
            config.required = Some(is_required(&res));
            if let Some(modfile) = res.modfile {
                mod_hashes.insert(modfile.id, modfile.filehash.md5);
                if config.version.is_none() || update {
                    config.version = Some(modfile.id.to_string());
                }
            } else {
                return Err(anyhow!("mod={} does not have any modfiles", config.id));
            }
        }
        println!("requesting dependencies");
        while let Some(Ok(res)) = dependency_reqs.join_next().await {
            for dep in res.1? {
                println!("found dependency {:?}", dep);
                if !config_map.contains_key(&dep.mod_id) {
                    config_map.insert(dep.mod_id, ModEntry {
                        id: dep.mod_id.to_string(),
                        name: None,
                        version: None,
                        approval: None,
                        required: None,
                    });
                    to_check.insert(dep.mod_id);
                }
            }
        }
    }

    Ok(Mods {
        mods: config_map.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        request_sync: false
    })
}

/// Take config, validate against mod.io, install, return populated config
async fn install_config(env: &Env, mods: Mods, update: bool) -> Result<Mods> {
    println!("installing config={:#?}", mods);

    let mut mod_hashes = HashMap::new();
    let config = populate_config(env, mods, update, &mut mod_hashes).await?;

    let mut paks = vec![];

    fs::create_dir(&env.mod_cache_dir).ok();

    for entry in &config.mods {
        let mod_id = entry.id.parse::<u32>()?;
        if let Some(version) = &entry.version {
            let file_id = version.parse::<u32>()?;
            let file_path = &env.mod_cache_dir.join(format!("{}.zip", file_id));
            if !file_path.exists() {
                println!("downloading mod={} version={} path={}", mod_id, file_id, file_path.display());
                env.modio.download(DownloadAction::File {
                    game_id: env.game_id,
                    mod_id,
                    file_id,
                }).save_to_file(&file_path).await?;
            }

            let modfile;
            let hash = if let Some(hash) = mod_hashes.get(&file_id) {
                hash
            } else {
                println!("requesting modfile={}", file_id);
                modfile = env.modio.game(env.game_id).mod_(mod_id).file(file_id).get().await?;
                &modfile.filehash.md5
            };

            use md5::{Md5, Digest};
            let mut hasher = Md5::new();
            std::io::copy(&mut File::open(&file_path)?, &mut hasher)?;
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

    for entry in fs::read_dir(&env.paks_dir).expect("Unable to list") {
        let entry = entry.expect("unable to get entry");
        if entry.file_type()?.is_dir() { continue };
        if let Some(name) = entry.file_name().to_str() {
            if name.ends_with(".pak") && name != "FSD-WindowsNoEditor.pak" {
                fs::remove_file(entry.path())?;
            }
        }
    }

    let ar_search = "AssetRegistry.bin".as_bytes();
    for (id, buf) in paks {
        let name = if contains(&buf, &ar_search) {
            format!("{}.pak", id)
        } else {
            format!("{}_P.pak", id)
        };
        let mut out_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(env.paks_dir.join(name))?;
        out_file.write_all(&buf)?;
    }

    // write config to mod integration save file
    let mut out_save = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&env.mod_config_save)?;
    out_save.write_all(&wrap_config(serde_json::to_string(&config)?)?)?;

    println!("mods installed");

    Ok(config)
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
            return approval
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
            "Verified"  => Ok(Approval::Verified),
            "Approved"  => Ok(Approval::Approved),
            "Sandbox"  => Ok(Approval::Sandbox),
            _ => Err(()),
        }
    }
}

fn extract_config_from_save(buffer: &[u8]) -> Result<String> {
    let mut save_rdr = std::io::Cursor::new(buffer);
    let save = Save::read(&mut save_rdr)?;

    if let Str{ value: json, .. } = &save.root.root[0].value {
        Ok(json.to_string())
    } else {
        Err(anyhow!("Malformed save file"))
    }
}
fn wrap_config(config: String) -> Result<Vec<u8>> {
    let buffer = include_bytes!("../ModIntegration.sav");
    let mut save_rdr = std::io::Cursor::new(&buffer[..]);
    let mut save = Save::read(&mut save_rdr)?;

    if let Str{ value: json, .. } = &mut save.root.root[0].value {
        *json = config;
        let mut out_buffer = vec![];
        save.write(&mut out_buffer)?;
        Ok(out_buffer)
    } else {
        Err(anyhow!("Malformed save file"))
    }
}
