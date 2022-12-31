use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
//use serde_json::Result;
use anyhow::Result;
use anyhow::anyhow;
use std::fs::{self, OpenOptions};
use std::str::FromStr;
use std::path::Path;
use std::io::{Read, Write, Seek, SeekFrom};

use modio::{Credentials, Modio};
use modio::filter::prelude::*;
use modio::download::DownloadAction;
use tokio::task::JoinSet;
use ue4pak::{PakFile, PakFileBuilder, PakIndex, PakVersion};
use ue4pak::archive::*;

use std::io;

/*
fn main() -> Result<(), io::Error> {
    let mut reader = io::BufReader::new(fs::File::open(
        std::env::args().nth(1).unwrap_or_default(),
    )?);
    let pak = PakFile::load_any(&mut reader)?;
    println!("{:#?}", pak);

    //let version = PakVersion::Fnv64BugFix;
    let version = PakVersion::Fnv64BugFix;

    let mut out_buf: Vec<u8> = vec![];
    let mut out_buf_builder: Vec<u8> = vec![];
    //let mut out_cur = std::io::Cursor::new(&mut out_buf);
    let mut aw = ArchiveWriter(&mut out_buf);
    //let mut buf_writer = std::io::BufWriter::new(&mut out_buf);
    let mut builder = PakFileBuilder::new(version);
    let mut tmp: &[u8] = &[];
    //let mut writer = std::io::BufWriter::new(&mut tmp);
    //let mut c = Cursor::new(Vec::new());

    let index = pak.index();
    println!("{:#?}", index);

    match index {
        PakIndex::V1(index) => {
            panic!("pak v1");
        }
        PakIndex::V2(index) => {
            //for (dir, entry, location) in index.full_entries() {
                //println!("pak entry {} {} {} {:?}", index.mount_point, dir, entry, location);
                //builder.import(location, "".to_owned());
            //}
            for entry in index.entries().into_iter() {
                println!("pak entry {:#?}", entry);
                entry.ser_with(&mut aw, version)?;

                //let padding = vec![0; entry.ser_len_with(version) as usize];
                //ue4pak::archive::Archive::write_all(&mut aw, &padding)?;
                //builder.seek(&mut aw, entry.ser_len_with(version))?;

                let mut writer = builder.import(&mut aw, "".to_owned(), entry.clone());


                reader.seek(SeekFrom::Start(entry.offset + 0x35))?;
                println!("reader pos={}", reader.stream_position()?);
                println!("entry size={}", entry.size);
                let mut entry_data = vec![0; entry.size.try_into().unwrap()];
                std::io::Read::read_exact(&mut reader, &mut entry_data)?;
                //ue4pak::archive::Archive::read_exact(&mut reader, &mut entry_data)?;
                println!("{:?}", &entry_data);
                println!("{}", String::from_utf8_lossy(&entry_data));

                //entry.ser_with(&mut writer, version);
                writer.write_all(&entry_data)?;
                writer.finalize()?;
                //writer.finalize()?;
                //.finalize().unwrap();
            }
        }
    }
    let mut pak = builder.finalize(&mut aw)?;
    //let mut info = pak.info().clone();
    //info.ser_de(&mut aw)?;
    //println!("{:#?}", out_buf);

    let mut opt = OpenOptions::new().create(true).truncate(true).write(true).open("out2.pak")?;
    opt.write_all(&out_buf)?;

    //std::thread::sleep(std::time::Duration::from_secs(10));
    Ok(())
}
*/


#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let key = std::env::var("MODIO_KEY").expect("Missing Mod.io API key");
    let game_id: u32 = std::env::var("MODIO_GAME_ID").expect("Missing Mod.io game id").parse()?;

    let arg = &std::env::args().nth(1).unwrap();
    let config_path = std::path::Path::new(arg);

    let file = fs::File::open(config_path).unwrap();
    let mods: Mods = serde_json::from_reader(file).unwrap();
    println!("{:#?}", mods);

    let modio = Modio::new(Credentials::new(key))?;

    let mut config_map: indexmap::IndexMap<_, _> = mods.mods.into_iter().map(|m| (m.id.parse::<u32>().unwrap(), m)).collect();

    let mut to_check: HashSet<u32> = config_map.keys().copied().collect();

    let mut modio_data = HashMap::new();

    while !to_check.is_empty() {
        println!("to check: {:?}", &to_check);
        let mut dependency_reqs = JoinSet::new();

        for id in to_check.iter().copied() {
            let deps = modio.mod_(game_id, id).dependencies();
            dependency_reqs.spawn(async move { (id, deps.list().await) });
        }

        println!("requesting mods");
        let mods_res = modio.game(game_id).mods().search(Id::_in(to_check.iter().copied().collect::<Vec<_>>())).collect().await?;
        to_check.clear();
        for res in mods_res.into_iter() {
            let mut config = config_map.get_mut(&res.id).unwrap();
            config.name = Some(res.name.to_owned());
            config.approval = Some(get_approval(&res));
            if let Some(modfile) = &res.modfile {
                config.version = Some(modfile.id.to_string());
            }
            modio_data.insert(res.id, res);
        }
        println!("requesting dependencies");
        while let Some(Ok(res)) = dependency_reqs.join_next().await {
            for dep in res.1? {
                println!("found dependency {:#?}", dep);
                if !config_map.contains_key(&dep.mod_id) {
                    config_map.insert(dep.mod_id, ModEntry {
                        id: dep.mod_id.to_string(),
                        name: None,
                        version: None,
                        approval: None,
                    });
                    to_check.insert(dep.mod_id);
                }
            }
        }
    }

    let config = config_map.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
    let file = fs::File::create(config_path).unwrap();
    serde_json::to_writer_pretty(file, &Mods { mods: config }).unwrap();

    let mut paks = vec![];

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
            use std::{fs, io};

            let mut local_file = fs::File::open(&path)?;
            let mut hasher = Md5::new();
            let n = io::copy(&mut local_file, &mut hasher)?;
            let local_hash = hex::encode(hasher.finalize());
            println!("checking file hash modio={} local={}", hash, local_hash);
            assert_eq!(hash, local_hash);

            let buf = get_pak_from_file(path)?;
            println!("loading pak");
            let mut tmp = fs::File::create("tmp.pak").unwrap();
            tmp.write_all(&buf)?;
            paks.push((format!("{}", mod_.id), buf));
        } else {
            panic!("mod id={} does not have a file uploaded", id);
        }
    }
    let loader = include_bytes!("../../../packed-mods/native-spawner.pak").to_vec();
    paks.push(("loader".to_string(), loader));

    let paks_dir = Path::new("/home/truman/.local/share/Steam/steamapps/common/Deep Rock Galactic/FSD/Content/Paks");

    for entry in fs::read_dir(paks_dir).expect("Unable to list") {
        let entry = entry.expect("unable to get entry");
        if entry.file_type()?.is_dir() { continue };
        if let Some(name) = entry.file_name().to_str() {
            if name.ends_with(".pak") && name != "FSD-WindowsNoEditor.pak" {
                fs::remove_file(entry.path())?;
            }
        }
    }

    let ar_from = "AssetRegistry.bin".as_bytes();
    let ar_to = "AssetRegistry.xyz".as_bytes();
    for (id, buf) in paks {
        let name = if contains(&buf, &ar_from) {
            format!("{}.pak", id)
        } else {
            format!("{}_P.pak", id)
        };
        let mut out_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(paks_dir.join(name))?;
        //replace_slice(&mut buf, &ar_from, &ar_to);
        out_file.write_all(&buf)?;
        /*
        let mut cur = std::io::Cursor::new(buf.clone());
        let pak = u4pak::Pak::from_reader(&mut cur, u4pak::pak::Options::default()).map_err(|e| anyhow!("Reading pack failed: {:?}", e))?;
        println!("{:#?}", pak);
        u4pak::unpack_buf::unpack(&pak, &buf[..], std::path::Path::new("tmp"), u4pak::unpack_buf::UnpackOptions::default()).map_err(|e| anyhow!("Unpacking pack failed: {:?}", e))?;
        */
    }

    Ok(())
}

fn replace_slice(source: &mut [u8], from: &[u8], to: &[u8]) {
    'outer: for i in 0..(source.len() - from.len() + 1) {
        for j in 0..from.len() {
            if source[i + j] != from[j] {
                continue 'outer;
            }
        }
        for j in 0..from.len() {
            source[i + j] = to[j];
        }
    }
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
    let reader = std::io::BufReader::new(file);

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
    Err(anyhow::anyhow!("Zip does not contain pak"))
}

/*
fn get_pack_files(path: String) -> Result<(i64, Vec<PackFile>)> {
    let path = Path::new("path");

    let files = list_zip_files(&path)?;
    Ok((id_modfile, files.into_iter().map(|path| {
        let p = std::path::Path::new(&path);
        let extension = p.extension().and_then(std::ffi::OsStr::to_str).map(|s| s.to_string());
        let name = p.file_stem().and_then(std::ffi::OsStr::to_str).map(|s| s.to_string());
        let path_no_extension = if let Some(ext) = &extension {
            path.strip_suffix(ext).unwrap().to_string()
        } else {
            path.to_owned()
        };
        PackFile {
            id_modfile,
            path,
            path_no_extension,
            name,
            extension,
        }
    }).collect()))
}
*/

fn get_approval(mod_: &modio::mods::Mod) -> Approval {
    for tag in &mod_.tags {
        if let Ok(approval) = Approval::from_str(&tag.name) {
            return approval
        }
    }
    Approval::Sandbox
}

#[derive(Debug, Serialize, Deserialize)]
struct Mods {
    mods: Vec<ModEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModEntry {
    id: String,
    name: Option<String>,
    version: Option<String>,
    approval: Option<Approval>,
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
