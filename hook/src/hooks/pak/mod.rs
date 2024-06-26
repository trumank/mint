mod file;
mod network;

use anyhow::{Context, Result};
use serde::Deserialize;

use std::io::{Read, Seek, Write};
use std::sync::{Mutex, OnceLock};

use crate::globals;
use crate::ue::{FString, TArray};

use self::file::PlainFileProviderConfig;
use self::network::EditorNetworkConfig;

#[derive(Debug, Default)]
#[allow(unused)]
pub struct FileInfo {
    pub file_exists: bool,
    pub read_only: bool,
    pub size: i64,
    pub timestamp: u64,
    pub access_timestamp: u64,
}

#[derive(Debug, Default, Deserialize)]
struct FileProviderConfig {
    layers: Vec<LayerConfigEntry>,
}
impl FileProviderConfig {
    fn new() -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(
            globals()
                .bin_dir
                .as_ref()
                .context("binaries directory unknown")?
                .join("cook-server.json"),
        )?)?)
    }
}

fn return_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct LayerConfigEntry {
    #[serde(default = "return_true")]
    enable: bool,
    #[serde(flatten)]
    config: LayerConfig,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum LayerConfig {
    File(PlainFileProviderConfig),
    EditorNetwork(EditorNetworkConfig),
}
impl LayerConfig {
    fn build(self) -> Result<Box<dyn FileProvider>> {
        fn map(p: Result<impl FileProvider + 'static>) -> Result<Box<dyn FileProvider>> {
            p.map(|p| Box::new(p) as Box<dyn FileProvider>)
        }
        match self {
            LayerConfig::File(c) => map(c.build()),
            LayerConfig::EditorNetwork(c) => map(c.build()),
        }
    }
}

#[repr(C)]
pub struct IFileHandle<S: Read + Write + Seek> {
    vtable: *mut IFileHandleVTable<S>,
    inner: S,
}
impl<S: Read + Write + Seek> IFileHandle<S> {
    pub fn new(inner: S) -> Self {
        let vtable = Box::new(IFileHandleVTable::<S> {
            __vec_del_dtor: Self::__vec_del_dtor,
            tell: Self::tell,
            seek: Self::seek,
            seek_from_end: Self::seek_from_end,
            read: Self::read,
            write: Self::write,
            flush: Self::flush,
            truncate: Self::truncate,
            size: Self::size,
        });
        Self {
            vtable: Box::into_raw(vtable),
            inner,
        }
    }
    unsafe extern "system" fn __vec_del_dtor(this: *mut IFileHandle<S>, _unknown: u32) {
        drop(Box::from_raw((*this).vtable));
        drop(Box::from_raw(this));
    }
    unsafe extern "system" fn tell(&mut self) -> i64 {
        self.inner.stream_position().expect("seek failed") as i64
    }
    unsafe extern "system" fn seek(&mut self, new_position: i64) -> bool {
        self.inner
            .seek(std::io::SeekFrom::Start(new_position as u64))
            .is_ok()
    }
    unsafe extern "system" fn seek_from_end(
        self: &mut IFileHandle<S>,
        new_position_relative_to_end: i64,
    ) -> bool {
        self.inner
            .seek(std::io::SeekFrom::End(new_position_relative_to_end))
            .is_ok()
    }
    unsafe extern "system" fn read(
        self: &mut IFileHandle<S>,
        destination: *mut u8,
        bytes_to_read: i64,
    ) -> bool {
        self.inner
            .read_exact(std::slice::from_raw_parts_mut(
                destination,
                bytes_to_read as usize,
            ))
            .is_ok()
    }
    unsafe extern "system" fn write(&mut self, _source: *const u8, _bytes_to_write: i64) -> bool {
        unimplemented!("cannot write")
    }
    unsafe extern "system" fn flush(&mut self, _b_full_flush: bool) -> bool {
        unimplemented!("cannot flush")
    }
    unsafe extern "system" fn truncate(&mut self, _new_size: i64) -> bool {
        unimplemented!("cannot truncate")
    }
    unsafe extern "system" fn size(&mut self) -> i64 {
        let Ok(cur) = self.inner.stream_position() else {
            return -1;
        };
        let Ok(size) = self.inner.seek(std::io::SeekFrom::End(0)) else {
            return -1;
        };
        let Ok(_) = self.inner.seek(std::io::SeekFrom::Start(cur)) else {
            return -1;
        };
        size as i64
    }
}

#[repr(C)]
struct IFileHandleVTable<S: Read + Write + Seek> {
    __vec_del_dtor: unsafe extern "system" fn(*mut IFileHandle<S>, u32),
    tell: unsafe extern "system" fn(&mut IFileHandle<S>) -> i64,
    seek: unsafe extern "system" fn(&mut IFileHandle<S>, i64) -> bool,
    seek_from_end: unsafe extern "system" fn(&mut IFileHandle<S>, i64) -> bool,
    read: unsafe extern "system" fn(&mut IFileHandle<S>, *mut u8, i64) -> bool,
    write: unsafe extern "system" fn(&mut IFileHandle<S>, *const u8, i64) -> bool,
    flush: unsafe extern "system" fn(&mut IFileHandle<S>, bool) -> bool,
    truncate: unsafe extern "system" fn(&mut IFileHandle<S>, i64) -> bool,
    size: unsafe extern "system" fn(&mut IFileHandle<S>) -> i64,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_connect() -> Result<()> {
        println!("Hello, world!");

        let mut pak = EditorNetworkFileProvider::new(FileProviderConfig::default().network)?;

        let list = [
            "../../../FSD/Content/_AssemblyStorm/TestMod/Pause/InitSpacerig.uexp",
            "../../../FSD/Content/_AssemblyStorm/TestMod/Pause/InitSpacerig.uasset",
            "../../../FSD/Content/_AssemblyStorm/TestMod/Pause/MOD_Pause.uasset",
            "../../../FSD/Content/_AssemblyStorm/TestMod/Pause/MOD_Pause.uexp",
        ];

        let out = std::path::Path::new("dumped-assets");
        std::fs::create_dir(out).ok();
        for f in list {
            let path = std::path::Path::new(&f);
            let bytes = pak.get_file(f)?;
            dbg!(pak.get_file_info(f)?);
            dbg!(bytes.len());
            std::fs::write(out.join(path.file_name().unwrap()), bytes)?;
            //std::fs::OpenOptions::new()
            //    .create(true)
            //    .truncate(true)
            //    .open(out.join(path.file_name().unwrap()))?
            //    .write_all(&bytes)?;
        }

        Ok(())
    }

    //if let Message::IterateDirectoryRecursively(m) = msg {
    //    for f in m.files {
    //        let path = std::path::Path::new(&f);
    //        dbg!(path);
    //        if !path.extension().map(|e| e == "uasset").unwrap_or(false) {
    //            continue;
    //        }
    //        if let Some(disk_path) =
    //            path.strip_prefix("/home/truman/projects/drg-modding/").ok()
    //        {
    //            if !disk_path.starts_with("FSD/Content/_AssemblyStorm/SandboxUtilities") {
    //                continue;
    //            }
    //            let disk_path = std::path::Path::new("FSD/Saved/Cooked/LinuxNoEditor/")
    //                //let disk_path = std::path::Path::new("../../FSD/Saved/Cooked/LinuxNoEditor/")
    //                .join(disk_path);
    //            //let disk_path = disk_path.join()?;
    //            println!("{}", disk_path.display());
    //            let bytes = get_file(
    //                &mut input,
    //                &mut output,
    //                String::from(path.to_string_lossy()),
    //            )?;
    //            std::fs::create_dir_all(disk_path.parent().unwrap())?;
    //            std::fs::write(&disk_path, bytes)?;

    //            let path = path.with_extension("uexp");
    //            let disk_path = disk_path.with_extension("uexp");
    //            println!("{}", disk_path.display());
    //            let bytes = get_file(
    //                &mut input,
    //                &mut output,
    //                String::from(path.to_string_lossy()),
    //            )?;
    //            std::fs::create_dir_all(disk_path.parent().unwrap())?;
    //            std::fs::write(disk_path, bytes)?;
    //        }
    //    }
    //    //println!("{:#?}", m.files);
    //}
}

#[derive(Debug)]
#[repr(C)]
struct FPakFile {
    vtable: *const (),
    idk: u64,
    idk2: *const (),
    pak_filename: FString,
}

#[derive(Debug)]
#[repr(C)]
struct FPakListEntry {
    read_order: u32,
    pak_file: *const FPakFile,
}

#[derive(Debug)]
#[repr(C)]
struct FPakPlatformFile {
    vtable: *const FPakVTable,
    lower_level: *const (),
    pak_files: TArray<FPakListEntry>, // TODO ...
}

type FnVirt = unsafe extern "system" fn(a: *mut (), b: *mut (), c: *mut (), d: *mut ()) -> *mut ();

struct FPakVTable([Option<FnVirt>; 55]);

#[rustfmt::skip]
const VTABLE_NAMES: &[(*const (), &str)] = &[
    (hook_virt_n::< 0> as *const (), "__vecDelDtor"),
    (hook_virt_n::< 1> as *const (), "SetSandboxEnabled"),
    (hook_virt_n::< 2> as *const (), "IsSandboxEnabled"),
    (hook_virt_n::< 3> as *const (), "ShouldBeUsed"),
    (hook_virt_n::< 4> as *const (), "Initialize"),
    (hook_virt_n::< 5> as *const (), "InitializeAfterSetActive"),
    (hook_virt_n::< 6> as *const (), "MakeUniquePakFilesForTheseFiles"),
    (hook_virt_n::< 7> as *const (), "InitializeNewAsyncIO"),
    (hook_virt_n::< 8> as *const (), "AddLocalDirectories"),
    (hook_virt_n::< 9> as *const (), "BypassSecurity"),
    (hook_virt_n::<10> as *const (), "Tick"),
    (hook_virt_n::<11> as *const (), "GetLowerLevel"),
    (hook_virt_n::<12> as *const (), "SetLowerLevel"),
    (hook_virt_n::<13> as *const (), "GetName"),
    (hook_file_exists as *const (), "FileExists"),
    (hook_file_size as *const (), "FileSize"),
    (hook_virt_n::<16> as *const (), "DeleteFile"),
    (hook_virt_n::<17> as *const (), "IsReadOnly"),
    (hook_virt_n::<18> as *const (), "MoveFile"),
    (hook_virt_n::<19> as *const (), "SetReadOnly"),
    (hook_virt_n::<20> as *const (), "GetTimeStamp"),
    (hook_virt_n::<21> as *const (), "SetTimeStamp"),
    (hook_virt_n::<22> as *const (), "GetAccessTimeStamp"),
    (hook_virt_n::<23> as *const (), "GetFilenameOnDisk"),
    (hook_open_read as *const (), "OpenRead"),
    (hook_open_read as *const (), "OpenReadNoBuffering"),
    (hook_virt_n::<26> as *const (), "OpenWrite"),
    (hook_virt_n::<27> as *const (), "DirectoryExists"),
    (hook_virt_n::<28> as *const (), "CreateDirectory"),
    (hook_virt_n::<29> as *const (), "DeleteDirectory"),
    (hook_virt_n::<30> as *const (), "GetStatData"),
    (hook_virt_n::<31> as *const (), "IterateDirectoryA"),
    (hook_virt_n::<32> as *const (), "IterateDirectoryB"),
    (hook_virt_n::<33> as *const (), "IterateDirectoryStatA"),
    (hook_virt_n::<34> as *const (), "IterateDirectoryStatB"),
    (hook_open_async_read as *const (), "OpenAsyncRead"),
    (hook_virt_n::<36> as *const (), "SetAsyncMinimumPriority"),
    (hook_virt_n::<37> as *const (), "OpenMapped"),
    (hook_virt_n::<38> as *const (), "GetTimeStampPair"),
    (hook_virt_n::<39> as *const (), "GetTimeStampLocal"),
    (hook_virt_n::<40> as *const (), "IterateDirectoryRecursivelyA"),
    (hook_virt_n::<41> as *const (), "IterateDirectoryRecursivelyB"),
    (hook_virt_n::<42> as *const (), "IterateDirectoryStatRecursivelyA"),
    (hook_virt_n::<43> as *const (), "IterateDirectoryStatRecursivelyB"),
    (hook_virt_n::<44> as *const (), "FindFiles"),
    (hook_virt_n::<45> as *const (), "FindFilesRecursively"),
    (hook_virt_n::<46> as *const (), "DeleteDirectoryRecursively"),
    (hook_virt_n::<47> as *const (), "CreateDirectoryTree"),
    (hook_virt_n::<48> as *const (), "CopyFile"),
    (hook_virt_n::<49> as *const (), "CopyDirectoryTree"),
    (hook_virt_n::<50> as *const (), "ConvertToAbsolutePathForExternalAppForRead"),
    (hook_virt_n::<51> as *const (), "ConvertToAbsolutePathForExternalAppForWrite"),
    (hook_virt_n::<52> as *const (), "SendMessageToServer"),
    (hook_virt_n::<53> as *const (), "DoesCreatePublicFiles"),
    (hook_virt_n::<54> as *const (), "SetCreatePublicFiles"),
];

struct LayeredFileProvider {
    layers: Vec<Box<dyn FileProvider>>,
}
impl LayeredFileProvider {
    fn new(config: FileProviderConfig) -> Result<Self> {
        let mut layers: Vec<Box<dyn FileProvider>> = vec![];

        for l in config.layers {
            if l.enable {
                layers.push(l.config.build()?);
            }
        }

        Ok(Self { layers })
    }
}
impl FileProvider for LayeredFileProvider {
    fn matches(&self, path: &str) -> bool {
        self.layers.iter().any(|l| l.matches(path))
    }
    fn get_file_info(&mut self, path: &str) -> Result<FileInfo> {
        self.layers
            .iter_mut()
            .find_map(|l| l.matches(path).then(|| l.get_file_info(path)))
            .unwrap()
    }
    fn get_file(&mut self, path: &str) -> Result<Vec<u8>> {
        self.layers
            .iter_mut()
            .find_map(|l| l.matches(path).then(|| l.get_file(path)))
            .unwrap()
    }
}

static NET_PAK: OnceLock<Mutex<LayeredFileProvider>> = OnceLock::new();
fn net_pak() -> &'static Mutex<LayeredFileProvider> {
    NET_PAK.get_or_init(|| {
        let config = FileProviderConfig::new().unwrap_or_else(|e| {
            tracing::warn!("Failed to load cook-server.json: {e}");
            FileProviderConfig::default()
        });
        tracing::info!("Using cook server config: {config:#?}");

        Mutex::new(
            LayeredFileProvider::new(config)
                .expect("failed to create FStreamingNetworkPlatformFile"),
        )
    })
}

static mut VTABLE_ORIG: *const FPakVTable = std::ptr::null();
static mut VTABLE_HOOKED: FPakVTable = FPakVTable([None; 55]);

type CursorFileHandle = IFileHandle<std::io::Cursor<Vec<u8>>>;

pub trait FileProvider: Send + Sync {
    fn matches(&self, path: &str) -> bool;
    fn get_file_info(&mut self, path: &str) -> Result<FileInfo>;
    fn get_file(&mut self, path: &str) -> Result<Vec<u8>>;
}

type FnFileExists =
    unsafe extern "system" fn(this: *mut FPakPlatformFile, file_name: *const u16) -> bool;
unsafe extern "system" fn hook_file_exists(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
) -> bool {
    let name = widestring::U16CStr::from_ptr_str(file_name)
        .to_string()
        .unwrap();

    let mut lock = net_pak().lock();
    let pak = lock.as_mut().unwrap();

    if let Some(name) = name.strip_prefix("../../../") {
        if pak.matches(name) {
            return pak
                .get_file_info(name)
                .expect("failed to get file info {name}")
                .file_exists;
        }
    }

    std::mem::transmute::<_, FnFileExists>((*VTABLE_ORIG).0[14].unwrap())(this, file_name)
}

type FnFileSize =
    unsafe extern "system" fn(this: *mut FPakPlatformFile, file_name: *const u16) -> i64;
unsafe extern "system" fn hook_file_size(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
) -> i64 {
    let name = widestring::U16CStr::from_ptr_str(file_name)
        .to_string()
        .unwrap();

    let mut lock = net_pak().lock();
    let pak = lock.as_mut().unwrap();

    if let Some(name) = name.strip_prefix("../../../") {
        if pak.matches(name) {
            return pak
                .get_file_info(name)
                .expect("failed to get file info {name}")
                .size;
        }
    }
    std::mem::transmute::<_, FnFileSize>((*VTABLE_ORIG).0[15].unwrap())(this, file_name)
}

type FnHookOpenRead = unsafe extern "system" fn(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
    b_allow_write: bool,
) -> *mut CursorFileHandle;
unsafe extern "system" fn hook_open_read(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
    b_allow_write: bool,
) -> *mut CursorFileHandle {
    let name = widestring::U16CStr::from_ptr_str(file_name)
        .to_string()
        .unwrap();

    let mut lock = net_pak().lock();
    let pak = lock.as_mut().unwrap();

    if let Some(name) = name.strip_prefix("../../../") {
        if pak.matches(name) {
            tracing::info!("Fetching file from editor {name}");
            let data = pak.get_file(name).expect("failed to get file {name}");
            return Box::into_raw(Box::new(CursorFileHandle::new(std::io::Cursor::new(data))));
        }
    }
    std::mem::transmute::<_, FnHookOpenRead>((*VTABLE_ORIG).0[24].unwrap())(
        this,
        file_name,
        b_allow_write,
    )
}

type FnHookOpenAsyncRead =
    unsafe extern "system" fn(this: *mut FPakPlatformFile, file_name: *const u16) -> *mut ();
unsafe extern "system" fn hook_open_async_read(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
) -> *mut () {
    //let name = widestring::U16CStr::from_ptr_str(file_name)
    //    .to_string()
    //    .unwrap();
    //tracing::info!("OpenAsyncRead({name})");
    std::mem::transmute::<_, FnHookOpenAsyncRead>((*VTABLE_ORIG).0[35].unwrap())(this, file_name)
}

unsafe extern "system" fn hook_virt_n<const N: usize>(
    a: *mut (),
    b: *mut (),
    c: *mut (),
    d: *mut (),
) -> *mut () {
    //tracing::info!("FPakPlatformFile({N}={})", VTABLE_NAMES[N].1);
    ((*VTABLE_ORIG).0[N].unwrap())(a, b, c, d)
}

unsafe fn hook_vtable(pak: &mut FPakPlatformFile) {
    for (i, (virt, _name)) in VTABLE_NAMES.iter().enumerate() {
        (VTABLE_HOOKED.0)[i] = Some(std::mem::transmute(*virt));
    }

    VTABLE_ORIG = pak.vtable;
    pak.vtable = std::ptr::addr_of!(VTABLE_HOOKED);
}

retour::static_detour! {
    static FPakPlatformFileInitialize: unsafe extern "system" fn(*mut FPakPlatformFile, *mut (), *const ()) -> bool;
}

pub unsafe fn init() -> Result<()> {
    FPakPlatformFileInitialize.initialize(
        std::mem::transmute(
            globals()
                .resolution
                .core
                .as_ref()
                .unwrap()
                .fpak_platform_file_initialize
                .0,
        ),
        |this, inner, cmd_line| {
            tracing::info!("FPakPlatformFile::Initialize");
            let ret = FPakPlatformFileInitialize.call(this, inner, cmd_line);
            hook_vtable(&mut *this);
            ret
        },
    )?;
    FPakPlatformFileInitialize.enable()?;

    Ok(())
}
