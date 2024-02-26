use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use mockall::{automock, predicate::*};

use ::modio;

use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use serde::{Deserialize, Serialize};
use task_local_extensions::Extensions;
use tracing::*;

use crate::providers::*;

static RE_MOD: OnceLock<regex::Regex> = OnceLock::new();
fn re_mod() -> &'static regex::Regex {
    RE_MOD.get_or_init(|| regex::Regex::new("^https://mod.io/g/drg/m/(?P<name_id>[^/#]+)(:?#(?P<mod_id>\\d+)(:?/(?P<modfile_id>\\d+))?)?$").unwrap())
}

const MODIO_DRG_ID: u32 = 2475;
const MODIO_PROVIDER_ID: &str = "modio";

inventory::submit! {
    super::ProviderFactory {
        id: MODIO_PROVIDER_ID,
        new: ModioProvider::<modio::Modio>::new_provider,
        can_provide: |url| re_mod().is_match(url),
        parameters: &[
            super::ProviderParameter {
                id: "oauth",
                name: "OAuth Token",
                description: "mod.io OAuth token",
                link: Some("https://mod.io/me/access"),
            },
        ]
    }
}

fn format_spec(name_id: &str, mod_id: u32, file_id: Option<u32>) -> ModSpecification {
    ModSpecification::new(if let Some(file_id) = file_id {
        format!("https://mod.io/g/drg/m/{}#{}/{}", name_id, mod_id, file_id)
    } else {
        format!("https://mod.io/g/drg/m/{}#{}", name_id, mod_id)
    })
}

pub struct ModioProvider<M: DrgModio> {
    modio: M,
}

impl<M: DrgModio + 'static> ModioProvider<M> {
    fn new_provider(
        parameters: &HashMap<String, String>,
    ) -> Result<Arc<dyn ModProvider>, ProviderError> {
        Ok(Arc::new(Self::new(M::with_parameters(parameters)?)))
    }
    fn new(modio: M) -> Self {
        Self { modio }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModioCache {
    mod_id_map: HashMap<String, u32>,
    modfile_blobs: HashMap<u32, BlobRef>,
    dependencies: HashMap<u32, Vec<u32>>,
    mods: HashMap<u32, ModioMod>,
    last_update_time: Option<SystemTime>,
}

impl Default for ModioCache {
    fn default() -> Self {
        Self {
            mod_id_map: Default::default(),
            modfile_blobs: Default::default(),
            dependencies: Default::default(),
            mods: Default::default(),
            last_update_time: Some(SystemTime::now()),
        }
    }
}

#[typetag::serde]
impl ModProviderCache for ModioCache {
    fn new() -> Self {
        Default::default()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModioMod {
    name_id: String,
    name: String,
    latest_modfile: Option<u32>,
    modfiles: Vec<ModioFile>,
    tags: HashSet<String>,
}

impl ModioMod {
    fn new(mod_: modio::mods::Mod, files: Vec<modio::files::File>) -> Self {
        Self {
            name_id: mod_.name_id,
            name: mod_.name,
            latest_modfile: mod_.modfile.map(|f| f.id),
            modfiles: files.into_iter().map(ModioFile::new).collect(),
            tags: mod_.tags.into_iter().map(|t| t.name).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModioModResponse {
    id: u32,
}

impl From<modio::mods::Mod> for ModioModResponse {
    fn from(value: modio::mods::Mod) -> Self {
        Self { id: value.id }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModioFile {
    id: u32,
    date_added: u64,
    version: Option<String>,
    changelog: Option<String>,
}
impl ModioFile {
    fn new(file: modio::files::File) -> Self {
        Self {
            id: file.id,
            date_added: file.date_added,
            version: file.version,
            changelog: file.changelog,
        }
    }
}

#[derive(Default)]
struct LoggingMiddleware {
    requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        loop {
            info!(
                "request started {} {:?}",
                self.requests
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                req.url().path()
            );
            let res = next.clone().run(req.try_clone().unwrap(), extensions).await;
            if let Ok(res) = &res {
                if let Some(retry) = res.headers().get("retry-after") {
                    info!("retrying after: {}...", retry.to_str().unwrap());
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        retry.to_str().unwrap().parse::<u64>().unwrap(),
                    ))
                    .await;
                    continue;
                }
            }
            return res;
        }
    }
}

#[derive(Debug, Snafu)]
pub enum DrgModioError {
    #[snafu(display("missing OAuth token"))]
    MissingOauthToken,
    #[snafu(display("mod.io error: {source}"))]
    GenericModioError { source: modio::Error },
    #[snafu(display("failed to perform basic mod.io probe: {source}"))]
    CheckFailed { source: modio::Error },
    #[snafu(display("failed to fetch mod files for {mod_id}: {source}"))]
    FetchModFilesFailed { source: modio::Error, mod_id: u32 },
    #[snafu(display("failed to fetch mod file {modfile_id} for {mod_id}: {source}"))]
    FetchModFileFailed {
        source: modio::Error,
        mod_id: u32,
        modfile_id: u32,
    },
    #[snafu(display("failed to fetch mod {mod_id}: {source}"))]
    FetchModFailed { source: modio::Error, mod_id: u32 },
    #[snafu(display("failed to fetch dependencies for mod {mod_id}: {source}"))]
    FetchDependenciesFailed { source: modio::Error, mod_id: u32 },
    #[snafu(display("encountered mod.io-related error: {msg}"))]
    GenericError { msg: &'static str },
}

impl DrgModioError {
    pub fn opt_mod_id(&self) -> Option<u32> {
        match self {
            DrgModioError::FetchModFilesFailed { mod_id, .. }
            | DrgModioError::FetchModFileFailed { mod_id, .. }
            | DrgModioError::FetchModFailed { mod_id, .. }
            | DrgModioError::FetchDependenciesFailed { mod_id, .. } => Some(*mod_id),
            _ => None,
        }
    }
}

#[cfg_attr(test, automock)]
#[async_trait::async_trait]
pub trait DrgModio: Sync + Send {
    fn with_parameters(parameters: &HashMap<String, String>) -> Result<Self, DrgModioError>
    where
        Self: Sized;
    async fn check(&self) -> Result<(), DrgModioError>;
    async fn fetch_mod(&self, id: u32) -> Result<ModioMod, DrgModioError>;
    async fn fetch_files(&self, mod_id: u32) -> Result<ModioMod, DrgModioError>;
    async fn fetch_file(
        &self,
        mod_id: u32,
        modfile_id: u32,
    ) -> Result<modio::files::File, DrgModioError>;
    async fn fetch_dependencies(&self, mod_id: u32) -> Result<Vec<u32>, DrgModioError>;
    async fn fetch_mods_by_name(
        &self,
        name_id: &str,
    ) -> Result<Vec<ModioModResponse>, DrgModioError>;
    async fn fetch_mods_by_ids(
        &self,
        filter_ids: Vec<u32>,
    ) -> Result<Vec<modio::mods::Mod>, DrgModioError>;
    async fn fetch_mod_updates_since(
        &self,
        mod_ids: Vec<u32>,
        last_update: u64,
    ) -> Result<HashSet<u32>, DrgModioError>;
    fn download<A: 'static>(&self, action: A) -> modio::download::Downloader
    where
        modio::download::DownloadAction: From<A>;
}

#[async_trait::async_trait]
impl DrgModio for modio::Modio {
    fn with_parameters(parameters: &HashMap<String, String>) -> Result<Self, DrgModioError> {
        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with::<LoggingMiddleware>(Default::default())
            .build();
        let modio = modio::Modio::new(
            modio::Credentials::with_token(
                "".to_owned(), // TODO patch modio to not use API key at all
                parameters.get("oauth").context(MissingOauthTokenSnafu)?,
            ),
            client,
        )
        .context(GenericModioSnafu)?;

        Ok(modio)
    }

    async fn check(&self) -> Result<(), DrgModioError> {
        use modio::filter::Eq;
        use modio::mods::filters::Id;

        self.game(MODIO_DRG_ID)
            .mods()
            .search(Id::eq(0))
            .collect()
            .await
            .context(CheckFailedSnafu)?;
        Ok(())
    }

    async fn fetch_mod(&self, id: u32) -> Result<ModioMod, DrgModioError> {
        use modio::filter::NotEq;
        use modio::mods::filters::Id;

        let files = self
            .game(MODIO_DRG_ID)
            .mod_(id)
            .files()
            .search(Id::ne(0))
            .collect()
            .await
            .context(FetchModFilesFailedSnafu { mod_id: id })?;
        let r#mod = self
            .game(MODIO_DRG_ID)
            .mod_(id)
            .get()
            .await
            .context(FetchModFailedSnafu { mod_id: id })?;

        Ok(ModioMod::new(r#mod, files))
    }

    async fn fetch_files(&self, mod_id: u32) -> Result<ModioMod, DrgModioError> {
        use modio::filter::NotEq;
        use modio::mods::filters::Id;

        let files = self
            .game(MODIO_DRG_ID)
            .mod_(mod_id)
            .files()
            .search(Id::ne(0))
            .collect()
            .await
            .context(FetchModFilesFailedSnafu { mod_id })?;
        let r#mod = self
            .game(MODIO_DRG_ID)
            .mod_(mod_id)
            .get()
            .await
            .context(FetchModFailedSnafu { mod_id })?;

        Ok(ModioMod::new(r#mod, files))
    }

    async fn fetch_file(
        &self,
        mod_id: u32,
        modfile_id: u32,
    ) -> Result<modio::files::File, DrgModioError> {
        let file = self
            .game(MODIO_DRG_ID)
            .mod_(mod_id)
            .file(modfile_id)
            .get()
            .await
            .context(FetchModFileFailedSnafu { mod_id, modfile_id })?;
        Ok(file)
    }

    async fn fetch_dependencies(&self, mod_id: u32) -> Result<Vec<u32>, DrgModioError> {
        Ok(self
            .game(MODIO_DRG_ID)
            .mod_(mod_id)
            .dependencies()
            .list()
            .await
            .context(FetchDependenciesFailedSnafu { mod_id })?
            .into_iter()
            .map(|d| d.mod_id)
            .collect::<Vec<_>>())
    }

    async fn fetch_mods_by_name(
        &self,
        name_id: &str,
    ) -> Result<Vec<ModioModResponse>, DrgModioError> {
        use modio::filter::{Eq, In};
        use modio::mods::filters::{NameId, Visible};

        let filter = NameId::eq(name_id).and(Visible::_in(vec![0, 1]));
        Ok(self
            .game(MODIO_DRG_ID)
            .mods()
            .search(filter)
            .collect()
            .await
            .context(GenericModioSnafu)?
            .into_iter()
            .map(|m| m.into())
            .collect())
    }

    async fn fetch_mods_by_ids(
        &self,
        filter_ids: Vec<u32>,
    ) -> Result<Vec<modio::mods::Mod>, DrgModioError> {
        use modio::filter::In;
        use modio::mods::filters::Id;

        let filter = Id::_in(filter_ids);

        Ok(self
            .game(MODIO_DRG_ID)
            .mods()
            .search(filter)
            .collect()
            .await
            .context(GenericModioSnafu)?)
    }

    async fn fetch_mod_updates_since(
        &self,
        mod_ids: Vec<u32>,
        last_update: u64,
    ) -> Result<HashSet<u32>, DrgModioError> {
        use modio::filter::Cmp;
        use modio::filter::In;
        use modio::filter::NotIn;

        use modio::mods::filters::events::EventType;
        use modio::mods::filters::events::ModId;
        use modio::mods::filters::DateAdded;
        use modio::mods::EventType as EventTypes;

        let events = self
            .game(MODIO_DRG_ID)
            .mods()
            .events(
                EventType::not_in(vec![
                    EventTypes::ModCommentAdded,
                    EventTypes::ModCommentDeleted,
                ])
                .and(ModId::_in(mod_ids))
                .and(DateAdded::gt(last_update)),
            )
            .collect()
            .await
            .context(GenericModioSnafu)?;
        Ok(events.iter().map(|e| e.mod_id).collect::<HashSet<_>>())
    }

    fn download<A>(&self, action: A) -> modio::download::Downloader
    where
        modio::download::DownloadAction: From<A>,
    {
        self.download(action)
    }
}

#[async_trait::async_trait]
impl<M: DrgModio + Send + Sync> ModProvider for ModioProvider<M> {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        update: bool,
        cache: ProviderCache,
    ) -> Result<ModResponse, ProviderError> {
        ensure!(
            !spec.url.contains("?preview="),
            PreviewLinkSnafu {
                url: spec.url.to_string()
            }
        );

        fn read_cache<F, R>(cache: &ProviderCache, update: bool, f: F) -> Option<R>
        where
            F: Fn(&ModioCache) -> Option<R>,
        {
            (!update)
                .then(|| {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(f)
                })
                .flatten()
        }

        fn write_cache<F>(cache: &ProviderCache, f: F)
        where
            F: FnOnce(&mut ModioCache),
        {
            f(cache
                .write()
                .unwrap()
                .get_mut::<ModioCache>(MODIO_PROVIDER_ID))
        }

        let url = &spec.url;
        let captures = re_mod().captures(url).context(InvalidUrlSnafu {
            url: url.to_string(),
        })?;

        if let (Some(mod_id), Some(_modfile_id)) =
            (captures.name("mod_id"), captures.name("modfile_id"))
        {
            // both mod ID and modfile ID specified, but not necessarily name
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();

            let mod_ =
                if let Some(mod_) = read_cache(&cache, update, |c| c.mods.get(&mod_id).cloned()) {
                    mod_
                } else {
                    let mod_ = self.modio.fetch_mod(mod_id).await?;

                    write_cache(&cache, |c| {
                        c.mods.insert(mod_id, mod_.clone());
                        c.mod_id_map.insert(mod_.name_id.to_owned(), mod_id);
                    });

                    mod_
                };

            let dep_ids = match read_cache(&cache, update, |c| c.dependencies.get(&mod_id).cloned())
            {
                Some(deps) => deps,
                None => {
                    let deps = self.modio.fetch_dependencies(mod_id).await?;
                    write_cache(&cache, |c| {
                        c.dependencies.insert(mod_id, deps.clone());
                    });
                    deps
                }
            };

            let deps = {
                // build map of (id -> name) for deps
                let mut name_map = read_cache(&cache, false, |c| {
                    Some(
                        dep_ids
                            .iter()
                            .flat_map(|id| c.mods.get(id).map(|m| (*id, m.name_id.to_string())))
                            .collect::<HashMap<_, _>>(),
                    )
                })
                .unwrap_or_default();

                let filter_ids = dep_ids
                    .iter()
                    .filter(|id| !name_map.contains_key(id))
                    .cloned()
                    .collect::<Vec<_>>();
                if !filter_ids.is_empty() {
                    let mods = self.modio.fetch_mods_by_ids(filter_ids).await?;

                    for m in &mods {
                        name_map.insert(m.id, m.name_id.to_string());
                    }

                    for m in mods {
                        let id = m.id;
                        // TODO avoid fetching mod a second time
                        let m = self.modio.fetch_files(id).await?;
                        write_cache(&cache, |c| {
                            c.mod_id_map.insert(m.name_id.to_owned(), id);
                            c.mods.insert(id, m);
                        });
                    }
                }

                let deps = dep_ids
                    .iter()
                    .filter_map(|id| match name_map.get(id) {
                        Some(name) => Some(format_spec(name, *id, None)),
                        None => {
                            warn!("dependency ID missing from name_map: {id}");
                            None
                        }
                    })
                    .collect();

                deps
            };

            Ok(ModResponse::Resolve(ModInfo {
                provider: MODIO_PROVIDER_ID,
                spec: format_spec(&mod_.name_id, mod_id, None),
                name: mod_.name,
                versions: mod_
                    .modfiles
                    .into_iter()
                    .map(|f| format_spec(&mod_.name_id, mod_id, Some(f.id)))
                    .collect(),
                resolution: ModResolution::resolvable(url.as_str().into()),
                suggested_require: mod_.tags.contains("RequiredByAll"),
                suggested_dependencies: deps,
                modio_tags: Some(process_modio_tags(&mod_.tags)),
                modio_id: Some(mod_id),
            }))
        } else if let Some(mod_id) = captures.name("mod_id") {
            // only mod ID specified, use latest version (either cached local or remote depending)
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();

            let mod_ = match read_cache(&cache, update, |c| c.mods.get(&mod_id).cloned()) {
                Some(mod_) => mod_,
                None => {
                    let mod_ = self.modio.fetch_mod(mod_id).await?;
                    write_cache(&cache, |c| {
                        c.mods.insert(mod_id, mod_.clone());
                        c.mod_id_map.insert(mod_.name_id.to_owned(), mod_id);
                    });
                    mod_
                }
            };

            Ok(ModResponse::Redirect(format_spec(
                &mod_.name_id,
                mod_id,
                Some(
                    mod_.latest_modfile
                        .with_context(|| NoAssociatedModfileSnafu {
                            url: url.to_string(),
                        })?,
                ),
            )))
        } else {
            let name_id = captures.name("name_id").unwrap().as_str();

            let cached_id = read_cache(&cache, update, |c| c.mod_id_map.get(name_id).cloned());

            if let Some(id) = cached_id {
                let cached = read_cache(&cache, update, |c| {
                    c.mods.get(&id).and_then(|m| m.latest_modfile)
                });

                let modfile_id = match cached {
                    Some(modfile_id) => modfile_id,
                    None => {
                        let mod_ = self.modio.fetch_mod(id).await?;
                        let modfile_id = mod_.latest_modfile;
                        write_cache(&cache, |c| {
                            c.mods.insert(id, mod_.clone());
                            c.mod_id_map.insert(mod_.name_id, id);
                        });
                        modfile_id.with_context(|| NoAssociatedModfileSnafu {
                            url: url.to_string(),
                        })?
                    }
                };

                Ok(ModResponse::Redirect(format_spec(
                    name_id,
                    id,
                    Some(modfile_id),
                )))
            } else {
                let mut mods = self.modio.fetch_mods_by_name(name_id).await?;
                if mods.len() > 1 {
                    AmbiguousModNameIdSnafu {
                        name_id: name_id.to_string(),
                    }
                    .fail()?
                } else if let Some(mod_) = mods.pop() {
                    let mod_id = mod_.id;
                    let mod_ = self.modio.fetch_mod(mod_id).await?;
                    let modfile_id = mod_.latest_modfile;
                    write_cache(&cache, |c| {
                        c.mods.insert(mod_id, mod_.clone());
                        c.mod_id_map.insert(mod_.name_id, mod_id);
                    });
                    let file = modfile_id.with_context(|| NoAssociatedModfileSnafu {
                        url: url.to_string(),
                    })?;

                    Ok(ModResponse::Redirect(format_spec(
                        name_id,
                        mod_id,
                        Some(file),
                    )))
                } else {
                    NoModsForNameIdSnafu {
                        name_id: name_id.to_string(),
                    }
                    .fail()?
                }
            }
        }
    }

    async fn fetch_mod(
        &self,
        res: &ModResolution,
        _update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<PathBuf, ProviderError> {
        let url = &res.url;
        let captures = re_mod()
            .captures(&res.url.0)
            .with_context(|| InvalidUrlSnafu {
                url: url.0.to_string(),
            })?;

        if let (Some(_name_id), Some(mod_id), Some(modfile_id)) = (
            captures.name("name_id"),
            captures.name("mod_id"),
            captures.name("modfile_id"),
        ) {
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();
            let modfile_id = modfile_id.as_str().parse::<u32>().unwrap();

            Ok(
                if let Some(path) = {
                    let path = cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(|c| c.modfile_blobs.get(&modfile_id))
                        .and_then(|r| blob_cache.get_path(r));
                    path
                } {
                    if let Some(tx) = tx {
                        tx.send(FetchProgress::Complete {
                            resolution: res.clone(),
                        })
                        .await
                        .unwrap();
                    }
                    path
                } else {
                    let file = self.modio.fetch_file(mod_id, modfile_id).await?;

                    let size = file.filesize;
                    let download: modio::download::DownloadAction = file.into();

                    info!("downloading mod {url:?}...");

                    use futures::stream::TryStreamExt;
                    use tokio::io::AsyncWriteExt;

                    let mut cursor = std::io::Cursor::new(vec![]);
                    let mut stream = Box::pin(self.modio.download(download).stream());
                    while let Some(bytes) = stream
                        .try_next()
                        .await
                        .with_context(|_| ModCtxtModioSnafu { mod_id })?
                    {
                        cursor
                            .write_all(&bytes)
                            .await
                            .with_context(|_| ModCtxtIoSnafu { mod_id })?;
                        if let Some(tx) = &tx {
                            tx.send(FetchProgress::Progress {
                                resolution: res.clone(),
                                progress: cursor.get_ref().len() as u64,
                                size,
                            })
                            .await
                            .unwrap();
                        }
                    }

                    let blob = blob_cache.write(&cursor.into_inner())?;
                    let path = blob_cache.get_path(&blob).unwrap();

                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(MODIO_PROVIDER_ID)
                        .modfile_blobs
                        .insert(modfile_id, blob);

                    if let Some(tx) = tx {
                        tx.send(FetchProgress::Complete {
                            resolution: res.clone(),
                        })
                        .await
                        .unwrap();
                    }

                    path
                },
            )
        } else {
            InvalidUrlSnafu {
                url: url.0.to_string(),
            }
            .fail()?
        }
    }

    async fn update_cache(&self, cache: ProviderCache) -> Result<(), ProviderError> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        let now = SystemTime::now();

        let (last_update, name_map) = {
            let cache = cache.read().unwrap();
            let Some(prov) = cache.get::<ModioCache>(MODIO_PROVIDER_ID) else {
                return Ok(()); // no existing mods, nothing to update
            };
            (
                prov.last_update_time,
                prov.mods
                    .iter()
                    .map(|(id, mod_)| (*id, mod_.name_id.clone()))
                    .collect::<HashMap<_, _>>(),
            )
        };

        let last_update = last_update
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();

        let mod_ids = self
            .modio
            .fetch_mod_updates_since(
                name_map.keys().cloned().collect::<Vec<u32>>(),
                last_update.as_secs(),
            )
            .await?;

        // TODO most of this is ripped from generic provider code. the resolution process is overly
        // complex and should be redone now that there's a much better understanding of what
        // exactly is required
        let mut to_resolve = mod_ids
            .iter()
            .filter_map(|id| name_map.get(id).map(|name| format_spec(name, *id, None)))
            .collect::<HashSet<_>>();

        let mut mods_map = HashMap::new();

        // used to deduplicate dependencies from mods already present in the mod list
        let mut precise_mod_specs = HashSet::new();

        pub async fn resolve_mod<M: DrgModio>(
            prov: &ModioProvider<M>,
            cache: ProviderCache,
            original_spec: ModSpecification,
        ) -> Result<(ModSpecification, ModInfo), ProviderError> {
            let mut spec = original_spec.clone();
            loop {
                match prov.resolve_mod(&spec, true, cache.clone()).await? {
                    ModResponse::Resolve(m) => {
                        return Ok((original_spec, m));
                    }
                    ModResponse::Redirect(redirected_spec) => spec = redirected_spec,
                };
            }
        }

        while !to_resolve.is_empty() {
            for (u, m) in stream::iter(
                to_resolve
                    .iter()
                    .map(|u| resolve_mod(self, cache.clone(), u.to_owned())),
            )
            .boxed()
            .buffer_unordered(5)
            .try_collect::<Vec<_>>()
            .await?
            {
                precise_mod_specs.insert(m.spec.clone());
                mods_map.insert(u, m);
                to_resolve.clear();
                for m in mods_map.values() {
                    for d in &m.suggested_dependencies {
                        if !precise_mod_specs.contains(d) {
                            to_resolve.insert(d.clone());
                        }
                    }
                }
            }
        }

        let mut lock = cache.write().unwrap();
        let c = lock.get_mut::<ModioCache>(MODIO_PROVIDER_ID);
        c.last_update_time = Some(now);

        Ok(())
    }

    async fn check(&self) -> Result<(), ProviderError> {
        self.modio.check().await.map_err(Into::into)
    }

    fn get_mod_info(&self, spec: &ModSpecification, cache: ProviderCache) -> Option<ModInfo> {
        let url = &spec.url;
        let captures = re_mod().captures(url)?;

        let cache = cache.read().unwrap();
        let prov = cache.get::<ModioCache>(MODIO_PROVIDER_ID)?;

        let mod_id = if let Some(mod_id) = captures.name("mod_id") {
            mod_id.as_str().parse::<u32>().ok()
        } else if let Some(name_id) = captures.name("name_id") {
            prov.mod_id_map.get(name_id.as_str()).cloned()
        } else {
            None
        }?;
        let mod_ = prov.mods.get(&mod_id)?;
        let modfile_id = if let Some(modfile_id) = captures.name("modfile_id") {
            modfile_id.as_str().parse::<u32>().ok()
        } else {
            mod_.modfiles.last().map(|f| f.id)
        }?;

        let deps = prov
            .dependencies
            .get(&mod_id)?
            .iter()
            .map(|id| {
                prov.mods
                    .get(id)
                    .map(|m| format_spec(&m.name_id, *id, None))
            })
            .collect::<Option<Vec<_>>>()?;

        Some(ModInfo {
            provider: MODIO_PROVIDER_ID,
            spec: format_spec(&mod_.name_id, mod_id, None),
            name: mod_.name.clone(),
            versions: mod_
                .modfiles
                .iter()
                .map(|f| format_spec(&mod_.name_id, mod_id, Some(f.id)))
                .collect(),
            resolution: ModResolution::resolvable(
                format_spec(&mod_.name_id, mod_id, Some(modfile_id))
                    .url
                    .into(),
            ),
            suggested_require: mod_.tags.contains("RequiredByAll"),
            suggested_dependencies: deps,
            modio_tags: Some(process_modio_tags(&mod_.tags)),
            modio_id: Some(mod_id),
        })
    }

    fn is_pinned(&self, spec: &ModSpecification, _cache: ProviderCache) -> bool {
        let url = &spec.url;
        let captures = re_mod().captures(url).unwrap();

        captures.name("modfile_id").is_some()
    }

    fn get_version_name(&self, spec: &ModSpecification, cache: ProviderCache) -> Option<String> {
        let url = &spec.url;
        let captures = re_mod().captures(url).unwrap();

        let cache = cache.read().unwrap();
        let prov = cache.get::<ModioCache>(MODIO_PROVIDER_ID);

        let mod_id = if let Some(mod_id) = captures.name("mod_id") {
            mod_id.as_str().parse::<u32>().ok()
        } else if let Some(name_id) = captures.name("name_id") {
            prov.and_then(|c| c.mod_id_map.get(name_id.as_str()).cloned())
        } else {
            None
        };

        if let Some(mod_id) = mod_id {
            if let Some(mod_) = prov.and_then(|c| c.mods.get(&mod_id).cloned()) {
                if let Some(file_id_str) = captures.name("modfile_id") {
                    let file_id = file_id_str.as_str().parse::<u32>().unwrap();
                    if let Some(file) = mod_.modfiles.iter().find(|f| f.id == file_id) {
                        if let Some(version) = &file.version {
                            Some(format!("{} - {}", file.id, version))
                        } else {
                            Some(file_id_str.as_str().to_string())
                        }
                    } else {
                        Some(file_id_str.as_str().to_string())
                    }
                } else {
                    Some("latest".to_string())
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

fn process_modio_tags(set: &HashSet<String>) -> ModioTags {
    let qol = set.contains("QoL");
    let gameplay = set.contains("Gameplay");
    let audio = set.contains("Audio");
    let visual = set.contains("Visual");
    let framework = set.contains("Framework");
    let required_status = if set.contains("RequiredByAll") {
        RequiredStatus::RequiredByAll
    } else {
        RequiredStatus::Optional
    };
    let approval_status = if set.contains("Verified") || set.contains("Auto-Verified") {
        ApprovalStatus::Verified
    } else if set.contains("Approved") {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::Sandbox
    };
    // Basic heuristic to collect all the tags which begin with a number, like `1.38`.
    let versions = set
        .iter()
        .filter(|i| i.starts_with(char::is_numeric))
        .cloned()
        .collect::<BTreeSet<String>>();

    ModioTags {
        qol,
        gameplay,
        audio,
        visual,
        framework,
        versions,
        required_status,
        approval_status,
    }
}

#[cfg(test)]
mod test {
    use super::{
        Arc, DrgModioError, HashMap, HashSet, MockDrgModio, ModProvider, ModResponse,
        ModSpecification, ModioCache, ModioFile, ModioMod, ModioModResponse, ModioProvider,
        OnceLock, RwLock, VersionAnnotatedCache, MODIO_PROVIDER_ID,
    };
    use crate::state::config::ConfigWrapper;

    #[tokio::test]
    async fn test_check_pass() {
        let mut mock = MockDrgModio::new();
        mock.expect_check().times(1).returning(|| Ok(()));
        let modio_provider = ModioProvider::new(mock);
        assert!(modio_provider.check().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_fail() {
        let mut mock = MockDrgModio::new();
        mock.expect_check()
            .times(1)
            .returning(|| Err(DrgModioError::MissingOauthToken));
        let modio_provider = ModioProvider::new(mock);
        assert!(modio_provider.check().await.is_err());
    }

    struct FullMod {
        mod_: ModioMod,
        dependencies: Vec<u32>,
    }

    static MODS: OnceLock<HashMap<u32, FullMod>> = OnceLock::new();

    #[tokio::test]
    async fn test_fetch_mod_simple() {
        let mods = MODS.get_or_init(|| {
            [(
                3,
                FullMod {
                    mod_: ModioMod {
                        name_id: "test-mod".to_string(),
                        name: "Test Mod".to_string(),
                        latest_modfile: Some(5),
                        modfiles: vec![ModioFile {
                            id: 5,
                            date_added: 12345,
                            version: None,
                            changelog: None,
                        }],
                        tags: HashSet::new(),
                    },
                    dependencies: vec![],
                },
            )]
            .into_iter()
            .collect::<HashMap<_, _>>()
        });
        let mod_names = mods
            .iter()
            .map(|(id, m)| (m.mod_.name_id.as_str(), id))
            .collect::<HashMap<_, _>>();
        let mut mock = MockDrgModio::new();

        mock.expect_fetch_mods_by_name()
            .times(1)
            .returning(move |name| {
                mod_names
                    .get(name)
                    .map(|id| vec![ModioModResponse { id: **id }])
                    .ok_or(DrgModioError::GenericError { msg: "not found" })
            });
        mock.expect_fetch_mod().times(1).returning(move |id| {
            mods.get(&id)
                .map(|m| m.mod_.clone())
                .ok_or(DrgModioError::GenericError { msg: "not found" })
        });
        mock.expect_fetch_dependencies()
            .times(1)
            .returning(move |id| {
                mods.get(&id)
                    .map(|m| m.dependencies.clone())
                    .ok_or(DrgModioError::GenericError { msg: "not found" })
            });

        let cache = Arc::new(RwLock::new(ConfigWrapper::<VersionAnnotatedCache>::memory(
            VersionAnnotatedCache::default(),
        )));

        let modio_provider = ModioProvider::new(mock);
        let resolved_mod = modio_provider
            .resolve_mod(
                &ModSpecification::new("https://mod.io/g/drg/m/test-mod".to_string()),
                false,
                cache.clone(),
            )
            .await
            .unwrap();

        let resolved_mod = match resolved_mod {
            ModResponse::Redirect(spec) => spec,
            _ => unreachable!(),
        };
        let _resolved_mod = modio_provider
            .resolve_mod(&resolved_mod, false, cache.clone())
            .await
            .unwrap();
        let lock = cache.read().unwrap();
        let modio_cache = lock.get::<ModioCache>(MODIO_PROVIDER_ID).unwrap();

        assert_eq!(
            modio_cache.mod_id_map,
            [("test-mod".to_string(), 3)].into_iter().collect()
        );
        assert_eq!(
            modio_cache.dependencies,
            [(3, vec![])].into_iter().collect()
        );
        assert_eq!(
            modio_cache.mods,
            [(3, mods.get(&3).map(|m| m.mod_.clone()).unwrap())]
                .into_iter()
                .collect()
        );
    }
}
