use anyhow::Result;

use crate::providers::{ModInfo, ModSpecification};

use super::request_counter::RequestID;

#[derive(Debug)]
pub enum Message {
    ResolveMod(RequestID, Result<(ModSpecification, ModInfo)>),
    Integrate(RequestID, Result<()>),
}
