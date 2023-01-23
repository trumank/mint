#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(let_chains)]

mod cache;
mod gui;
mod integrate;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;

use anyhow::{anyhow, Result};

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use std::io::{BufReader, Read};

use modio::filter::prelude::*;

use uesave::PropertyMeta::Str;
use uesave::Save;

use clap::{Parser, Subcommand};

use crate::cache::{ModioCache, ModioMod};

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
    let mut config = Config::load_or_create_default(&STATIC_SETTINGS.config_path)?;
    println!("{config:#?}");

    let command = Args::parse().action;
    match command {
        Action::Gui(_) => {
            let mods = File::open(
                config
                    .settings
                    .mod_config_save()
                    .expect("could not find mod config save"),
            )
            .ok()
            .and_then(|mut f| extract_config_from_save(&mut f).ok())
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();

            println!("{mods:#?}");

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui::launch_gui(config, mods)
        }
        _ => rt.block_on(async {
            match command {
                Action::Install(args) => install(&mut config, args).await?,
                Action::Sync(args) => sync(&mut config, args).await?,
                Action::Run(args) => run(&mut config, args).await?,
                Action::Info(args) => info(&mut config, args).await?,
                Action::Gui(_) => panic!("unreachable"),
            }
            config.save(&STATIC_SETTINGS.config_path).unwrap();
            Ok(())
        }),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    settings: Settings,
    mod_profiles: HashMap<String, ModProfile>,
    modio_cache: ModioCache,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Settings {
    modio_key: Option<String>,
    fsd_install: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModProfile {
    mods: BTreeMap<ModId, ModProfileEntry>,
}

// TODO "overrideable" struct field? (version, required)
#[derive(Debug, Default, Serialize, Deserialize)]
struct ModProfileEntry {
    /// Mod version (file ID in the case of a mod.io mod)
    version: String,
    /// Whether the user should be prompted if there is a newer version of the mod available
    pinned_version: bool,
    /// Whether clients should be required to install the mod. Can be configured by the user
    required: bool,
}

impl Config {
    fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        match File::open(&path) {
            Ok(f) => Ok(serde_json::from_reader(BufReader::new(f))?),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                config.save(path)?;
                Ok(config)
            }
            Err(err) => Err(err.into()),
        }
    }
    fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        serde_json::to_writer_pretty(std::io::BufWriter::new(File::create(&path)?), &self)?;
        Ok(())
    }
}
impl Settings {
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

#[serde_with::serde_as]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
// TODO define similar struct for Files
pub struct ModId(#[serde_as(as = "serde_with::DisplayFromStr")] u32);

impl std::fmt::Display for ModId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ModId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(ModId(s.parse::<u32>()?))
    }
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

#[derive(Parser, Debug)]
struct ActionInfo {}

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
    /// Info
    Info(ActionInfo),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

async fn run(config: &mut Config, args: ActionRun) -> Result<()> {
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

            let mut f = File::open(
                config
                    .settings
                    .mod_config_save()
                    .ok_or_else(|| anyhow!("mod config save not found"))?,
            )?;
            let json = extract_config_from_save(&mut f)?;
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

async fn info(_config: &mut Config, _args: ActionInfo) -> Result<()> {
    println!("data_dir: {}", STATIC_SETTINGS.data_dir.display());
    println!("cache_dir: {}", STATIC_SETTINGS.cache_dir.display());
    Ok(())
}

async fn sync(config: &mut Config, _args: ActionSync) -> Result<()> {
    let mut f = File::open(
        config
            .settings
            .mod_config_save()
            .ok_or_else(|| anyhow!("mod config save not found"))?,
    )?;
    let json = extract_config_from_save(&mut f)?;
    let mods: Mods = serde_json::from_str(&json)?;
    println!("{mods:#?}");

    let _mod_config = integrate::install_config(config, mods, false).await?;

    Ok(())
}

async fn install(config: &mut Config, args: ActionInstall) -> Result<()> {
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
    println!("{mods:#?}");

    let mod_config = integrate::install_config(config, mods, args.update).await?;

    if args.update {
        if let Some(path) = &args.config {
            let file = File::create(path).unwrap();
            serde_json::to_writer_pretty(file, &mod_config).unwrap();
        }
    }

    Ok(())
}

async fn populate_config(
    config: &mut Config,
    mods: Mods,
    update: bool,
    mod_hashes: &mut HashMap<u32, String>,
) -> Result<Mods> {
    let mut config_map: indexmap::IndexMap<_, _> = mods
        .mods
        .into_iter()
        .map(|m| (ModId(m.id.parse::<u32>().unwrap()), m))
        .collect();

    let mut to_check: HashSet<ModId> = config_map.keys().copied().collect();
    let mut deps_checked: HashSet<ModId> = Default::default();

    // adds new empty mod to config and returns true if so
    let add_mod = |config_map: &mut indexmap::IndexMap<ModId, ModEntry>, id: &ModId| -> bool {
        println!("found dependency {id:?}");
        if !config_map.contains_key(id) {
            config_map.insert(
                *id,
                ModEntry {
                    id: id.to_string(),
                    name: None,
                    version: None,
                    approval: None,
                    required: None,
                },
            );
            true
        } else {
            false
        }
    };

    // force update from mod.io regardless of cache
    let u = false;
    while !to_check.is_empty() {
        println!("to check: {:?}", &to_check);

        let mut deps_to_check = to_check.iter().cloned().collect::<Vec<_>>();
        let mut dependency_reqs = tokio::task::JoinSet::new();
        while let Some(dep) = deps_to_check.pop() {
            if !u && let Some(deps) = config.modio_cache.dependencies.get(&dep) {
                for id in deps {
                    if !deps_checked.contains(id) && add_mod(&mut config_map, id) {
                        deps_to_check.push(*id);
                        deps_checked.insert(*id);
                        to_check.insert(*id);
                    }
                }
            } else {
                let deps = config.settings
                    .modio()
                    .expect("could not create modio object")
                    .mod_(STATIC_SETTINGS.game_id, dep.0)
                    .dependencies();
                dependency_reqs.spawn(async move { (dep, deps.list().await) });
            }
        }

        let ids: Vec<u32> = if u {
            to_check.iter().map(|id| id.0).collect()
        } else {
            to_check
                .iter()
                .filter(|id| !config.modio_cache.mods.contains_key(id))
                .map(|id| id.0)
                .collect()
        };
        if !ids.is_empty() {
            println!("requesting mods {ids:?}");
            let mods_res = config
                .settings
                .modio()
                .expect("could not create modio object")
                .game(STATIC_SETTINGS.game_id)
                .mods()
                .search(Id::_in(ids))
                .collect()
                .await?;
            for mod_ in mods_res.into_iter() {
                config.modio_cache.mods.insert(ModId(mod_.id), mod_.into());
            }
        }

        for id in &to_check {
            let res = config.modio_cache.mods.get(id).unwrap(); // previously inserted so shouldn't be missing
            let mut mod_config = config_map.get_mut(&res.id).unwrap();
            mod_config.name = Some(res.name.to_owned());
            mod_config.approval = Some(get_approval(res));
            mod_config.required = Some(is_required(res));
            if let Some((&id, modfile)) = &res.versions.iter().next_back() {
                mod_hashes.insert(id, modfile.filehash.to_owned());
                if mod_config.version.is_none() || update {
                    mod_config.version = Some(id.to_string());
                }
            } else {
                return Err(anyhow!("mod={} does not have any modfiles", mod_config.id));
            }
        }

        to_check.clear();

        println!("requesting dependencies");
        while let Some(Ok(res)) = dependency_reqs.join_next().await {
            let deps = res.1?;
            config
                .modio_cache
                .dependencies
                .insert(res.0, deps.iter().map(|res| ModId(res.mod_id)).collect());
            for dep in deps {
                if add_mod(&mut config_map, &ModId(dep.mod_id)) {
                    to_check.insert(ModId(dep.mod_id));
                }
            }
        }
    }

    Ok(Mods {
        mods: config_map.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        request_sync: false,
    })
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
                println!("Entry {raw_path} has a suspicious path");
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

fn get_approval(mod_: &ModioMod) -> Approval {
    for tag in &mod_.tags {
        if let Ok(approval) = Approval::from_str(tag) {
            return approval;
        }
    }
    Approval::Sandbox
}

fn is_required(mod_: &ModioMod) -> bool {
    mod_.tags.contains(&"RequiredByAll".to_owned())
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Mods {
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
pub enum Approval {
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

fn extract_config_from_save(file: &mut File) -> Result<String> {
    let save = Save::read(&mut BufReader::new(file))?;

    if let Str { value: json, .. } = &save.root.root[0].value {
        Ok(json.to_string())
    } else {
        Err(anyhow!("Malformed save file"))
    }
}
