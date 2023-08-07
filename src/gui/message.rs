use std::collections::HashMap;

use anyhow::Result;

use crate::providers::{ModInfo, ModSpecification};

use super::{request_counter::RequestID, GitHubRelease, SpecFetchProgress};

#[derive(Debug)]
pub enum Message {
    ResolveMods {
        rid: RequestID,
        specs: Vec<ModSpecification>,
        result: Result<HashMap<ModSpecification, ModInfo>>,
        is_dependency: bool,
    },
    FetchModProgress(RequestID, ModSpecification, SpecFetchProgress),
    Integrate(RequestID, Result<()>),
    UpdateCache(RequestID, Result<()>),
    CheckUpdates(RequestID, Result<GitHubRelease>),
}
