use anyhow::Result;

use crate::providers::{Mod, ModSpecification};

use super::request_counter::RequestID;

#[derive(Debug)]
pub enum Message {
    Log(String),
    ResolveMod(RequestID, Result<(ModSpecification, Mod)>),
    Integrate(RequestID, Result<()>),
}
