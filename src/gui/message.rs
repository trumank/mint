use anyhow::Result;

use crate::providers::{ModInfo, ModSpecification};

use super::{request_counter::RequestID, SpecFetchProgress};

#[derive(Debug)]
pub enum Message {
    ResolveMod(RequestID, Result<(ModSpecification, ModInfo)>),
    FetchModProgress(RequestID, ModSpecification, SpecFetchProgress),
    Integrate(RequestID, Result<()>),
    UpdateCache(RequestID, Result<()>),
}
