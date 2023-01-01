use std::collections::{HashMap, HashSet};
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

struct Env {
    modio_key: String,
    game_id: u32,
    paks_dir: PathBuf,
    mod_config_save: PathBuf,
}

fn get_env() -> Result<Env> {
    let fsd_install = std::path::PathBuf::from(std::env::var("FSD_INSTALL").expect("Missing path to game root directory"));

    Ok(Env {
        modio_key: std::env::var("MODIO_KEY").expect("Missing Mod.io API key"),
        //game_id: std::env::var("MODIO_GAME_ID").expect("Missing Mod.io game id").parse()?,
        game_id: 2475,
        paks_dir: fsd_install.join("FSD/Content/Paks"),
        mod_config_save: fsd_install.join("FSD/Saved/SaveGames/Mods/ModIntegration.sav"),
    })
}

#[derive(Parser, Debug)]
struct ActionInstall {
   #[arg(required = true, index = 1)]
   config: String,

   #[arg(short, long)]
   update: bool,
}

#[derive(Parser, Debug)]
struct ActionSync {}

#[derive(Subcommand, Debug)]
enum Action {
   /// Install mods with specified config
   Install(ActionInstall),
   /// Sync mods with host using config saved in ModIntegration.sav
   Sync(ActionSync),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
   #[command(subcommand)]
   action: Action,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let env = get_env()?;

    match Args::parse().action {
        Action::Install(args) => install(&env, args).await,
        Action::Sync(args) => sync(&env, args).await,
    }
}

async fn sync(env: &Env, args: ActionSync) -> Result<()> {
    println!("syncing nothing");

    let save_buffer = std::fs::read(&env.mod_config_save)?;
    let json = extract_config_from_save(&save_buffer)?;
    let mods: Mods = serde_json::from_str(&json)?;
    println!("{:#?}", mods);

    let config = install_config(env, mods).await?;

    Ok(())
}

async fn install(env: &Env, args: ActionInstall) -> Result<()> {
    let config_path = std::path::Path::new(&args.config);

    let file = File::open(config_path)?;
    let mods: Mods = serde_json::from_reader(file)?;
    println!("{:#?}", mods);

    let config = install_config(env, mods).await?;

    if args.update {
        let file = File::create(config_path).unwrap();
        serde_json::to_writer_pretty(file, &config).unwrap();
    }

    Ok(())
}

/// Take config, validate against mod.io, install, return populated config
async fn install_config(env: &Env, mods: Mods) -> Result<Mods> {
    println!("installing config={:#?}", mods);
    let modio = Modio::new(Credentials::new(&env.modio_key))?;

    let mut config_map: indexmap::IndexMap<_, _> = mods.mods.into_iter().map(|m| (m.id.parse::<u32>().unwrap(), m)).collect();

    let mut to_check: HashSet<u32> = config_map.keys().copied().collect();

    let mut modio_data = HashMap::new();

    while !to_check.is_empty() {
        println!("to check: {:?}", &to_check);
        let mut dependency_reqs = JoinSet::new();

        for id in to_check.iter().copied() {
            let deps = modio.mod_(env.game_id, id).dependencies();
            dependency_reqs.spawn(async move { (id, deps.list().await) });
        }

        println!("requesting mods");
        let mods_res = modio.game(env.game_id).mods().search(Id::_in(to_check.iter().copied().collect::<Vec<_>>())).collect().await?;
        to_check.clear();
        for res in mods_res.into_iter() {
            let mut config = config_map.get_mut(&res.id).unwrap();
            config.name = Some(res.name.to_owned());
            config.approval = Some(get_approval(&res));
            config.required = Some(is_required(&res));
            if let Some(modfile) = &res.modfile {
                config.version = Some(modfile.id.to_string());
            }
            modio_data.insert(res.id, res);
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

    let config = Mods {
        mods: config_map.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        request_sync: false
    };

    let mut paks = vec![];

    fs::create_dir("mods").ok();

    for (id, mod_) in modio_data {
        if let Some(file) = mod_.modfile {
            let path_str = format!("mods/{}.zip", file.id);
            let path = Path::new(&path_str);
            let hash = file.filehash.md5.to_owned();
            if !path.exists() {
                println!("downloading mod id={} path={}", id, path.display());
                modio.download(DownloadAction::FileObj(Box::new(file))).save_to_file(&path).await?;
            }

            use md5::{Md5, Digest};

            let mut local_file = File::open(&path)?;
            let mut hasher = Md5::new();
            std::io::copy(&mut local_file, &mut hasher)?;
            let local_hash = hex::encode(hasher.finalize());
            println!("checking file hash modio={} local={}", hash, local_hash);
            assert_eq!(hash, local_hash);

            let buf = get_pak_from_file(path)?;
            paks.push((format!("{}", mod_.id), buf));
        } else {
            panic!("mod id={} does not have a file uploaded", id);
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
            .open(&env.paks_dir.join(name))?;
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
    let mut save_rdr = std::io::Cursor::new(&buffer[..]);
    let save = Save::read(&mut save_rdr)?;

    if let Str{ value: json, .. } = &save.root.root[0].value {
        Ok(json.to_string())
    } else {
        Err(anyhow!(""))
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
        Err(anyhow!(""))
    }
}
