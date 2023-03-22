use anyhow::Result;

use crate::providers::Mod;

#[derive(Debug)]
pub enum Message {
    Log(String),
    ResolveMod(Result<Mod>),
}
