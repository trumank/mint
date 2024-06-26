use std::{
    io::{BufReader, BufWriter, Cursor, Read, Write},
    net::TcpStream,
};

use anyhow::{bail, Result};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use serde::Deserialize;

use super::{FileInfo, FileProvider};

fn default_host() -> String {
    "127.0.0.1:41899".to_string()
}

#[derive(Debug, Deserialize)]
pub struct EditorNetworkConfig {
    #[serde(default = "default_host")]
    host: String,
    platform: String,
    globs: Vec<String>,
}
impl Default for EditorNetworkConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            platform: "WindowsNoEditor".into(),
            globs: vec![],
        }
    }
}
impl EditorNetworkConfig {
    pub fn build(self) -> Result<EditorNetworkFileProvider> {
        EditorNetworkFileProvider::new(self)
    }
}

pub struct EditorNetworkFileProvider {
    input: BufReader<TcpStream>,
    output: BufWriter<TcpStream>,
    pub file_list: Vec<FileEntry>,
    config: EditorNetworkConfig,
    globs: Vec<glob::Pattern>,
}
impl EditorNetworkFileProvider {
    pub fn new(config: EditorNetworkConfig) -> Result<Self> {
        let conn = TcpStream::connect(&config.host)?;
        let input = BufReader::new(conn.try_clone()?);
        let output = BufWriter::new(conn);

        let mut new = Self {
            input,
            output,
            file_list: vec![],
            globs: config
                .globs
                .iter()
                .flat_map(|g| glob::Pattern::new(g))
                .collect(),
            config,
        };

        new.init()?;

        Ok(new)
    }
    fn init(&mut self) -> Result<()> {
        let msg = Message::GetFileList(Box::new(MessageGetFileList {
            platforms: vec![self.config.platform.to_string()],
            game_name: "../../../FSD/FSD.uproject".to_owned(),
            engine_rel_path: "../../../Engine/".to_owned(),
            game_rel_path: "../../../FSD/".to_owned(),
            engine_platforms_extensions_dir: "../../../Engine/Platforms/".to_owned(),
            project_platforms_extensions_dir: "../../../FSD/Platforms/".to_owned(),
            engine_rel_plugin_path: "../../../Engine/Plugins/".to_owned(),
            game_rel_plugin_path: "../../../FSD/Plugins/".to_owned(),
            directories: vec!["../../../FSD/Content/".to_owned()],
            connection_flags: 1,
            version_info: "".to_owned(),
            host_address: "".to_owned(),
            custom_platform_data: 0,
        }));
        write_message(&mut self.output, &msg)?;
        let msg = read_message(&mut self.input)?;

        if let Message::IterateDirectoryRecursively(m) = msg {
            self.file_list = m.files;
        } else {
            bail!("expected IterateDirectoryRecursively");
        }

        Ok(())
    }
}
impl FileProvider for EditorNetworkFileProvider {
    fn matches(&self, path: &str) -> bool {
        self.globs.iter().any(|g| g.matches(path))
    }
    fn get_file_info(&mut self, path: &str) -> Result<FileInfo> {
        write_message(
            &mut self.output,
            &Message::GetFileInfo(MessageFileInfo {
                file_name: format!("../../../{path}"),
            }),
        )?;
        let mut reply = Cursor::new(read_payload(&mut self.input)?);
        Ok(FileInfo {
            file_exists: reply.read_u32::<LE>()? != 0,
            read_only: reply.read_u32::<LE>()? != 0,
            size: reply.read_i64::<LE>()?,
            timestamp: reply.read_u64::<LE>()?,
            access_timestamp: reply.read_u64::<LE>()?,
        })
    }
    fn get_file(&mut self, path: &str) -> Result<Vec<u8>> {
        write_message(
            &mut self.output,
            &Message::OpenRead(MessageOpenRead {
                filename: format!("../../../{path}"),
            }),
        )?;
        let mut reply = Cursor::new(read_payload(&mut self.input)?);
        let handle_id = reply.read_u64::<LE>()?;
        let _timestamp = reply.read_u64::<LE>()?;
        let file_size = reply.read_u64::<LE>()?;

        //tracing::info!("file handle {}", handle_id);
        //tracing::info!("timestamp {}", timestamp);
        //tracing::info!("file size {}", file_size);

        write_message(
            &mut self.output,
            &Message::Read(MessageRead {
                handle_id,
                bytes_to_read: file_size,
            }),
        )?;

        let mut reply = Cursor::new(read_payload(&mut self.input)?);
        let bytes_read = reply.read_u64::<LE>()?;
        assert_eq!(bytes_read, file_size, "did not read full file");
        //println!("bytes read {}", bytes_read);
        let rest = reply.into_inner();

        Ok(rest[8..rest.len()].to_vec())
    }
}

#[rustfmt::skip]
pub const CRC_TABLE: [u32; 256] = [
    0x00000000, 0xb71dc104, 0x6e3b8209, 0xd926430d, 0xdc760413, 0x6b6bc517, 0xb24d861a, 0x0550471e, 0xb8ed0826, 0x0ff0c922, 0xd6d68a2f, 0x61cb4b2b, 0x649b0c35, 0xd386cd31, 0x0aa08e3c, 0xbdbd4f38,
    0x70db114c, 0xc7c6d048, 0x1ee09345, 0xa9fd5241, 0xacad155f, 0x1bb0d45b, 0xc2969756, 0x758b5652, 0xc836196a, 0x7f2bd86e, 0xa60d9b63, 0x11105a67, 0x14401d79, 0xa35ddc7d, 0x7a7b9f70, 0xcd665e74,
    0xe0b62398, 0x57abe29c, 0x8e8da191, 0x39906095, 0x3cc0278b, 0x8bdde68f, 0x52fba582, 0xe5e66486, 0x585b2bbe, 0xef46eaba, 0x3660a9b7, 0x817d68b3, 0x842d2fad, 0x3330eea9, 0xea16ada4, 0x5d0b6ca0,
    0x906d32d4, 0x2770f3d0, 0xfe56b0dd, 0x494b71d9, 0x4c1b36c7, 0xfb06f7c3, 0x2220b4ce, 0x953d75ca, 0x28803af2, 0x9f9dfbf6, 0x46bbb8fb, 0xf1a679ff, 0xf4f63ee1, 0x43ebffe5, 0x9acdbce8, 0x2dd07dec,
    0x77708634, 0xc06d4730, 0x194b043d, 0xae56c539, 0xab068227, 0x1c1b4323, 0xc53d002e, 0x7220c12a, 0xcf9d8e12, 0x78804f16, 0xa1a60c1b, 0x16bbcd1f, 0x13eb8a01, 0xa4f64b05, 0x7dd00808, 0xcacdc90c,
    0x07ab9778, 0xb0b6567c, 0x69901571, 0xde8dd475, 0xdbdd936b, 0x6cc0526f, 0xb5e61162, 0x02fbd066, 0xbf469f5e, 0x085b5e5a, 0xd17d1d57, 0x6660dc53, 0x63309b4d, 0xd42d5a49, 0x0d0b1944, 0xba16d840,
    0x97c6a5ac, 0x20db64a8, 0xf9fd27a5, 0x4ee0e6a1, 0x4bb0a1bf, 0xfcad60bb, 0x258b23b6, 0x9296e2b2, 0x2f2bad8a, 0x98366c8e, 0x41102f83, 0xf60dee87, 0xf35da999, 0x4440689d, 0x9d662b90, 0x2a7bea94,
    0xe71db4e0, 0x500075e4, 0x892636e9, 0x3e3bf7ed, 0x3b6bb0f3, 0x8c7671f7, 0x555032fa, 0xe24df3fe, 0x5ff0bcc6, 0xe8ed7dc2, 0x31cb3ecf, 0x86d6ffcb, 0x8386b8d5, 0x349b79d1, 0xedbd3adc, 0x5aa0fbd8,
    0xeee00c69, 0x59fdcd6d, 0x80db8e60, 0x37c64f64, 0x3296087a, 0x858bc97e, 0x5cad8a73, 0xebb04b77, 0x560d044f, 0xe110c54b, 0x38368646, 0x8f2b4742, 0x8a7b005c, 0x3d66c158, 0xe4408255, 0x535d4351,
    0x9e3b1d25, 0x2926dc21, 0xf0009f2c, 0x471d5e28, 0x424d1936, 0xf550d832, 0x2c769b3f, 0x9b6b5a3b, 0x26d61503, 0x91cbd407, 0x48ed970a, 0xfff0560e, 0xfaa01110, 0x4dbdd014, 0x949b9319, 0x2386521d,
    0x0e562ff1, 0xb94beef5, 0x606dadf8, 0xd7706cfc, 0xd2202be2, 0x653deae6, 0xbc1ba9eb, 0x0b0668ef, 0xb6bb27d7, 0x01a6e6d3, 0xd880a5de, 0x6f9d64da, 0x6acd23c4, 0xddd0e2c0, 0x04f6a1cd, 0xb3eb60c9,
    0x7e8d3ebd, 0xc990ffb9, 0x10b6bcb4, 0xa7ab7db0, 0xa2fb3aae, 0x15e6fbaa, 0xccc0b8a7, 0x7bdd79a3, 0xc660369b, 0x717df79f, 0xa85bb492, 0x1f467596, 0x1a163288, 0xad0bf38c, 0x742db081, 0xc3307185,
    0x99908a5d, 0x2e8d4b59, 0xf7ab0854, 0x40b6c950, 0x45e68e4e, 0xf2fb4f4a, 0x2bdd0c47, 0x9cc0cd43, 0x217d827b, 0x9660437f, 0x4f460072, 0xf85bc176, 0xfd0b8668, 0x4a16476c, 0x93300461, 0x242dc565,
    0xe94b9b11, 0x5e565a15, 0x87701918, 0x306dd81c, 0x353d9f02, 0x82205e06, 0x5b061d0b, 0xec1bdc0f, 0x51a69337, 0xe6bb5233, 0x3f9d113e, 0x8880d03a, 0x8dd09724, 0x3acd5620, 0xe3eb152d, 0x54f6d429,
    0x7926a9c5, 0xce3b68c1, 0x171d2bcc, 0xa000eac8, 0xa550add6, 0x124d6cd2, 0xcb6b2fdf, 0x7c76eedb, 0xc1cba1e3, 0x76d660e7, 0xaff023ea, 0x18ede2ee, 0x1dbda5f0, 0xaaa064f4, 0x738627f9, 0xc49be6fd,
    0x09fdb889, 0xbee0798d, 0x67c63a80, 0xd0dbfb84, 0xd58bbc9a, 0x62967d9e, 0xbbb03e93, 0x0cadff97, 0xb110b0af, 0x060d71ab, 0xdf2b32a6, 0x6836f3a2, 0x6d66b4bc, 0xda7b75b8, 0x035d36b5, 0xb440f7b1
];

fn calc_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for b in data {
        crc = (crc >> 8) ^ CRC_TABLE[((crc & 0xFF) ^ *b as u32) as usize];
    }
    (!crc).swap_bytes()
}

fn read_array<R, T, E>(
    length: u32,
    reader: &mut R,
    f: fn(&mut R) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    (0..length).map(|_| f(reader)).collect()
}
fn write_array<W, T>(
    writer: &mut W,
    array: &Vec<T>,
    f: fn(&mut W, item: &T) -> Result<()>,
) -> Result<()> {
    for item in array {
        f(writer, item)?;
    }
    Ok(())
}

fn read_string<R: Read>(reader: &mut R) -> Result<String> {
    let mut chars = vec![0; reader.read_u32::<LE>()? as usize];
    reader.read_exact(&mut chars)?;
    let length = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    Ok(String::from_utf8_lossy(&chars[..length]).into_owned())
}
fn write_string<W: Write>(writer: &mut W, string: &str) -> Result<()> {
    let bytes = string.as_bytes();
    if bytes.is_empty() {
        // special case empty string
        writer.write_u32::<LE>(0)?;
    } else {
        writer.write_u32::<LE>(bytes.len() as u32 + 1)?;
        writer.write_all(bytes)?;
        writer.write_u8(0)?;
    }
    Ok(())
}

const MAGIC: u32 = 0x9e2b83c1;

fn read_payload<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let magic = reader.read_u32::<LE>()?;
    assert_eq!(magic, MAGIC, "bad magic");
    let mut buf = vec![0; reader.read_u32::<LE>()? as usize];
    let crc = reader.read_u32::<LE>()?;
    reader.read_exact(&mut buf)?;
    assert_eq!(crc, calc_crc(&buf), "bad crc");
    Ok(buf.to_vec())
}
fn write_payload<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    writer.write_u32::<LE>(MAGIC)?;
    writer.write_u32::<LE>(payload.len() as u32)?;
    writer.write_u32::<LE>(calc_crc(payload))?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

fn read_message<R: Read>(reader: &mut R) -> Result<Message> {
    Message::read(&mut Cursor::new(read_payload(reader)?))
}
fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<()> {
    let mut payload = Cursor::new(vec![]);
    message.write(&mut payload)?;
    write_payload(writer, &payload.into_inner()[..])?;
    Ok(())
}

#[derive(Debug)]
#[allow(unused)]
enum Message {
    SyncFile,
    DeleteFile,
    MoveFile,
    SetReadOnly,
    OpenRead(MessageOpenRead),
    OpenWrite,
    OpenAppend,
    CreateDirectory,
    DeleteDirectory,
    IterateDirectory,
    IterateDirectoryRecursively(MessageIterateDirectoryRecursively),
    DeleteDirectoryRecursively,
    CopyFile,
    GetFileInfo(MessageFileInfo),
    Read(MessageRead),
    Write,
    Close,
    Seek,
    SetTimeStamp,
    ToAbsolutePathForRead,
    ToAbsolutePathForWrite,
    ReportLocalFiles,
    GetFileList(Box<MessageGetFileList>),
    Heartbeat,
    RecompileShaders,
}
impl Message {
    fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let t = reader.read_u32::<LE>()?;
        Ok(match t & 0xff {
            10 => Message::IterateDirectoryRecursively(MessageIterateDirectoryRecursively::read(
                reader,
            )?),
            22 => Message::GetFileList(Box::new(MessageGetFileList::read(reader)?)),
            _ => todo!("missing packet type {}", t & 0xff),
        })
    }
}
impl Message {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        match self {
            Message::OpenRead(m) => {
                writer.write_u32::<LE>(4)?;
                m.write(writer)?;
            }
            Message::GetFileInfo(m) => {
                writer.write_u32::<LE>(13)?;
                m.write(writer)?;
            }
            Message::Read(m) => {
                writer.write_u32::<LE>(14)?;
                m.write(writer)?;
            }
            Message::GetFileList(m) => {
                writer.write_u32::<LE>(22)?;
                m.write(writer)?;
            }
            _ => todo!("missing packet type {:?}", self),
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MessageOpenRead {
    filename: String,
}
impl MessageOpenRead {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        write_string(writer, &self.filename)?;
        Ok(())
    }
}

#[derive(Debug)]
struct MessageIterateDirectoryRecursively {
    files: Vec<FileEntry>,
}
impl MessageIterateDirectoryRecursively {
    fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let _package_file_version = reader.read_u32::<LE>()?;
        let _local_engine_dir = read_string(reader)?;
        let _local_project_dir = read_string(reader)?;
        let _local_engine_platform_extensions = read_string(reader)?;
        let _local_project_platform_extensions = read_string(reader)?;

        Ok(Self {
            files: read_array(reader.read_u32::<LE>()?, reader, |r| -> Result<FileEntry> {
                Ok(FileEntry {
                    path: read_string(r)?,
                    timestamp: r.read_u64::<LE>()?,
                })
            })?,
        })
    }
}

#[derive(Debug)]
#[allow(unused)]
pub struct FileEntry {
    pub path: String,
    pub timestamp: u64,
}

#[derive(Debug)]
struct MessageGetFileList {
    platforms: Vec<String>,
    game_name: String,
    engine_rel_path: String,
    game_rel_path: String,
    engine_platforms_extensions_dir: String,
    project_platforms_extensions_dir: String,
    engine_rel_plugin_path: String,
    game_rel_plugin_path: String,
    directories: Vec<String>,
    connection_flags: u8,
    version_info: String,
    host_address: String,
    custom_platform_data: u32, // TODO Map<String, String>
}
impl MessageGetFileList {
    fn read<R: Read>(reader: &mut R) -> Result<Self> {
        Ok(Self {
            platforms: read_array(reader.read_u32::<LE>()?, reader, read_string)?,
            game_name: read_string(reader)?,
            engine_rel_path: read_string(reader)?,
            game_rel_path: read_string(reader)?,
            engine_platforms_extensions_dir: read_string(reader)?,
            project_platforms_extensions_dir: read_string(reader)?,
            engine_rel_plugin_path: read_string(reader)?,
            game_rel_plugin_path: read_string(reader)?,
            directories: read_array(reader.read_u32::<LE>()?, reader, read_string)?,
            connection_flags: reader.read_u8()?,
            version_info: read_string(reader)?,
            host_address: read_string(reader)?,
            custom_platform_data: reader.read_u32::<LE>()?,
        })
    }
}
impl MessageGetFileList {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u32::<LE>(self.platforms.len() as u32)?;
        write_array(writer, &self.platforms, |w, i| write_string(w, i))?;
        write_string(writer, &self.game_name)?;
        write_string(writer, &self.engine_rel_path)?;
        write_string(writer, &self.game_rel_path)?;
        write_string(writer, &self.engine_platforms_extensions_dir)?;
        write_string(writer, &self.project_platforms_extensions_dir)?;
        write_string(writer, &self.engine_rel_plugin_path)?;
        write_string(writer, &self.game_rel_plugin_path)?;
        writer.write_u32::<LE>(self.directories.len() as u32)?;
        write_array(writer, &self.directories, |w, i| write_string(w, i))?;
        writer.write_u8(self.connection_flags)?;
        write_string(writer, &self.version_info)?;
        write_string(writer, &self.host_address)?;
        writer.write_u32::<LE>(self.custom_platform_data)?;
        Ok(())
    }
}

#[derive(Debug)]
struct MessageFileInfo {
    file_name: String,
}
impl MessageFileInfo {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        write_string(writer, &self.file_name)?;
        Ok(())
    }
}

#[derive(Debug)]
struct MessageRead {
    handle_id: u64,
    bytes_to_read: u64,
}
impl MessageRead {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u64::<LE>(self.handle_id)?;
        writer.write_u64::<LE>(self.bytes_to_read)?;
        Ok(())
    }
}
