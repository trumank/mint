use std::collections::BTreeSet;
use std::ops::DerefMut;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Result;
use eframe::egui;
use tokio::{
    sync::mpsc::{self, Sender},
    task::JoinHandle,
};
use tracing::{error, info};

use crate::integrate::{IntegrationErr, IntegrationErrKind};
use crate::mod_lints::{LintId, LintReport};
use crate::state::{ModData_v0_1_0 as ModData, ModOrGroup};
use crate::{
    error::IntegrationError,
    providers::{FetchProgress, ModInfo, ModResolution, ModSpecification, ModStore},
    state::ModConfig,
};

use super::{
    request_counter::{RequestCounter, RequestID},
    App, GitHubRelease, LastActionStatus, SpecFetchProgress, WindowProviderParameters,
};

#[derive(Debug)]
pub struct MessageHandle<S> {
    pub rid: RequestID,
    pub handle: JoinHandle<()>,
    pub state: S,
}

#[derive(Debug)]
pub enum Message {
    ResolveMods(ResolveMods),
    Integrate(Integrate),
    FetchModProgress(FetchModProgress),
    UpdateCache(UpdateCache),
    CheckUpdates(CheckUpdates),
    LintMods(LintMods),
}

impl Message {
    pub fn handle(self, app: &mut App) {
        match self {
            Self::ResolveMods(msg) => msg.receive(app),
            Self::Integrate(msg) => msg.receive(app),
            Self::FetchModProgress(msg) => msg.receive(app),
            Self::UpdateCache(msg) => msg.receive(app),
            Self::CheckUpdates(msg) => msg.receive(app),
            Self::LintMods(msg) => msg.receive(app),
        }
    }
}

#[derive(Debug)]
pub struct ResolveMods {
    rid: RequestID,
    specs: Vec<ModSpecification>,
    result: Result<HashMap<ModSpecification, ModInfo>>,
    is_dependency: bool,
}

impl ResolveMods {
    pub fn send(
        app: &mut App,
        ctx: &egui::Context,
        specs: Vec<ModSpecification>,
        is_dependency: bool,
    ) {
        let rid = app.request_counter.next();
        let store = app.state.store.clone();
        let tx = app.tx.clone();
        let ctx = ctx.clone();
        let handle = tokio::spawn(async move {
            let result = store.resolve_mods(&specs, false).await;
            tx.send(Message::ResolveMods(Self {
                rid,
                specs,
                result,
                is_dependency,
            }))
            .await
            .unwrap();
            ctx.request_repaint();
        });
        app.last_action_status = LastActionStatus::Idle;
        app.resolve_mod_rid = Some(MessageHandle {
            rid,
            handle,
            state: (),
        });
    }

    fn receive(self, app: &mut App) {
        if Some(self.rid) == app.resolve_mod_rid.as_ref().map(|r| r.rid) {
            match self.result {
                Ok(resolved_mods) => {
                    let primary_mods = self
                        .specs
                        .into_iter()
                        .collect::<HashSet<ModSpecification>>();
                    for (resolved_spec, info) in resolved_mods {
                        let is_dep = self.is_dependency || !primary_mods.contains(&resolved_spec);
                        let add = if is_dep {
                            // if mod is a dependency then check if there is a disabled
                            // mod that satisfies the dependency and enable it. if it
                            // is not a dependency then assume the user explicitly
                            // wants to add a specific mod version.
                            let active_profile = app.state.mod_data.active_profile.clone();
                            !app.state.mod_data.any_mod_mut(
                                &active_profile,
                                |mc, mod_group_enabled| {
                                    if mc.spec.satisfies_dependency(&resolved_spec) {
                                        mc.enabled = true;
                                        if let Some(mod_group_enabled) = mod_group_enabled {
                                            *mod_group_enabled = true;
                                        }
                                        true
                                    } else {
                                        false
                                    }
                                },
                            )
                        } else {
                            true
                        };

                        if add {
                            let ModData {
                                active_profile,
                                profiles,
                                ..
                            } = app.state.mod_data.deref_mut().deref_mut();

                            profiles.get_mut(active_profile).unwrap().mods.insert(
                                0,
                                ModOrGroup::Individual(ModConfig {
                                    spec: info.spec.clone(),
                                    required: info.suggested_require,
                                    enabled: true,
                                }),
                            );
                        }
                    }
                    app.resolve_mod.clear();
                    app.state.mod_data.save().unwrap();
                    app.last_action_status =
                        LastActionStatus::Success("mods successfully resolved".to_string());
                }
                Err(e) => match e.downcast::<IntegrationError>() {
                    Ok(IntegrationError::NoProvider { url: _, factory }) => {
                        app.window_provider_parameters =
                            Some(WindowProviderParameters::new(factory, &app.state));
                        app.last_action_status =
                            LastActionStatus::Failure("no provider".to_string());
                    }
                    Err(e) => {
                        error!("{:#?}\n{}", e, e.backtrace());
                        app.last_action_status = LastActionStatus::Failure(e.to_string());
                    }
                },
            }
            app.resolve_mod_rid = None;
        }
    }
}

#[derive(Debug)]
pub struct Integrate {
    rid: RequestID,
    result: Result<(), IntegrationErr>,
}

impl Integrate {
    pub fn send(
        rc: &mut RequestCounter,
        store: Arc<ModStore>,
        mods: Vec<ModSpecification>,
        fsd_pak: PathBuf,
        tx: Sender<Message>,
        ctx: egui::Context,
    ) -> MessageHandle<HashMap<ModSpecification, SpecFetchProgress>> {
        let rid = rc.next();
        MessageHandle {
            rid,
            handle: tokio::task::spawn(async move {
                let res = integrate_async(store, ctx.clone(), mods, fsd_pak, rid, tx.clone()).await;
                tx.send(Message::Integrate(Integrate { rid, result: res }))
                    .await
                    .unwrap();
                ctx.request_repaint();
            }),
            state: Default::default(),
        }
    }
    fn receive(self, app: &mut App) {
        if Some(self.rid) == app.integrate_rid.as_ref().map(|r| r.rid) {
            match self.result {
                Ok(()) => {
                    info!("integration complete");
                    app.last_action_status =
                        LastActionStatus::Success("integration complete".to_string());
                }
                Err(IntegrationErr { mod_ctxt, kind }) => match kind {
                    IntegrationErrKind::Generic(e) => match e.downcast::<IntegrationError>() {
                        Ok(IntegrationError::NoProvider { url: _, factory }) => {
                            app.window_provider_parameters =
                                Some(WindowProviderParameters::new(factory, &app.state));
                            app.last_action_status =
                                LastActionStatus::Failure("no provider".to_string());
                        }
                        Err(e) => {
                            match mod_ctxt {
                                        Some(mod_ctxt) => error!("error encountered during integration while working with mod `{:?}`\n{:#?}\n{}", mod_ctxt, e, e.backtrace()),
                                        None => error!("{:#?}\n{}", e, e.backtrace()),
                                    };
                            app.last_action_status = LastActionStatus::Failure(e.to_string());
                        }
                    },
                    IntegrationErrKind::Repak(e) => {
                        match mod_ctxt {
                                Some(mod_ctxt) => error!("`repak` error encountered during integration while working with mod `{:?}`\n{:#?}", mod_ctxt, e),
                                None => error!("`repak` error encountered during integration: {:#?}", e),
                            };
                        app.last_action_status = LastActionStatus::Failure(e.to_string());
                    }
                    IntegrationErrKind::UnrealAsset(e) => {
                        match mod_ctxt {
                                Some(mod_ctxt) => error!("`unreal_asset` error encountered during integration while working with mod `{:?}`\n{:#?}", mod_ctxt, e),
                                None => error!("`unreal_asset` error encountered during integration: {:#?}", e),
                            };
                        app.last_action_status = LastActionStatus::Failure(e.to_string());
                    }
                },
            }
            app.integrate_rid = None;
        }
    }
}

#[derive(Debug)]
pub struct FetchModProgress {
    rid: RequestID,
    spec: ModSpecification,
    progress: SpecFetchProgress,
}

impl FetchModProgress {
    fn receive(self, app: &mut App) {
        if let Some(MessageHandle { rid, state, .. }) = &mut app.integrate_rid {
            if *rid == self.rid {
                state.insert(self.spec, self.progress);
            }
        }
    }
}

#[derive(Debug)]
pub struct UpdateCache {
    rid: RequestID,
    result: Result<()>,
}

impl UpdateCache {
    pub fn send(app: &mut App, mod_specs: Vec<ModSpecification>) {
        let rid = app.request_counter.next();
        let tx = app.tx.clone();
        let store = app.state.store.clone();
        let handle = tokio::spawn(async move {
            let res = store.resolve_mods(&mod_specs, true).await.map(|_| ());
            tx.send(Message::UpdateCache(UpdateCache { rid, result: res }))
                .await
                .unwrap();
        });
        app.last_action_status = LastActionStatus::Idle;
        app.update_rid = Some(MessageHandle {
            rid,
            handle,
            state: (),
        });
    }

    fn receive(self, app: &mut App) {
        if Some(self.rid) == app.update_rid.as_ref().map(|r| r.rid) {
            match self.result {
                Ok(()) => {
                    info!("cache update complete");
                    app.last_action_status =
                        LastActionStatus::Success("successfully updated cache".to_string());
                }
                Err(e) => match e.downcast::<IntegrationError>() {
                    // TODO make provider initializing more generic
                    Ok(IntegrationError::NoProvider { url: _, factory }) => {
                        app.window_provider_parameters =
                            Some(WindowProviderParameters::new(factory, &app.state));
                        app.last_action_status =
                            LastActionStatus::Failure("no provider".to_string());
                    }
                    Err(e) => {
                        error!("{:#?}\n{}", e, e.backtrace());
                        app.last_action_status = LastActionStatus::Failure(e.to_string());
                    }
                },
            }
            app.update_rid = None;
        }
    }
}

#[derive(Debug)]
pub struct CheckUpdates {
    rid: RequestID,
    result: Result<GitHubRelease>,
}

impl CheckUpdates {
    pub fn send(app: &mut App, ctx: &egui::Context) {
        let rid = app.request_counter.next();
        let tx = app.tx.clone();
        let ctx = ctx.clone();

        async fn req() -> Result<GitHubRelease> {
            Ok(reqwest::Client::builder()
                .user_agent("trumank/drg-mod-integration")
                .build()?
                .get("https://api.github.com/repos/trumank/drg-mod-integration/releases/latest")
                .send()
                .await?
                .json::<GitHubRelease>()
                .await?)
        }

        let handle = tokio::spawn(async move {
            tx.send(Message::CheckUpdates(Self {
                rid,
                result: req().await,
            }))
            .await
            .unwrap();
            ctx.request_repaint();
        });
        app.check_updates_rid = Some(MessageHandle {
            rid,
            handle,
            state: (),
        });
    }
    fn receive(self, app: &mut App) {
        if Some(self.rid) == app.check_updates_rid.as_ref().map(|r| r.rid) {
            app.check_updates_rid = None;
            if let Ok(release) = self.result {
                if let (Ok(version), Some(Ok(release_version))) = (
                    semver::Version::parse(env!("CARGO_PKG_VERSION")),
                    release
                        .tag_name
                        .strip_prefix('v')
                        .map(semver::Version::parse),
                ) {
                    if release_version > version {
                        app.available_update = Some(release);
                    }
                }
            }
        }
    }
}

async fn integrate_async(
    store: Arc<ModStore>,
    ctx: egui::Context,
    mod_specs: Vec<ModSpecification>,
    fsd_pak: PathBuf,
    rid: RequestID,
    message_tx: Sender<Message>,
) -> Result<(), IntegrationErr> {
    let update = false;

    let mods = store
        .resolve_mods(&mod_specs, update)
        .await
        .map_err(|e| IntegrationErr {
            mod_ctxt: None,
            kind: IntegrationErrKind::Generic(e),
        })?;

    let to_integrate = mod_specs
        .iter()
        .map(|u| mods[u].clone())
        .collect::<Vec<_>>();
    let res_map: HashMap<ModResolution, ModSpecification> = mods
        .iter()
        .map(|(spec, info)| (info.resolution.clone(), spec.clone()))
        .collect();
    let urls = to_integrate
        .iter()
        .map(|m| &m.resolution)
        .collect::<Vec<&ModResolution>>();

    let (tx, mut rx) = mpsc::channel::<FetchProgress>(10);

    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            if let Some(spec) = res_map.get(progress.resolution()) {
                message_tx
                    .send(Message::FetchModProgress(FetchModProgress {
                        rid,
                        spec: spec.clone(),
                        progress: progress.into(),
                    }))
                    .await
                    .unwrap();
                ctx.request_repaint();
            }
        }
    });

    let paths = store
        .fetch_mods(&urls, update, Some(tx))
        .await
        .map_err(|e| IntegrationErr {
            mod_ctxt: None,
            kind: IntegrationErrKind::Generic(e),
        })?;

    tokio::task::spawn_blocking(|| {
        crate::integrate::integrate(fsd_pak, to_integrate.into_iter().zip(paths).collect())
    })
    .await
    .map_err(|e| IntegrationErr {
        mod_ctxt: None,
        kind: IntegrationErrKind::Generic(e.into()),
    })??;

    Ok(())
}

#[derive(Debug)]
pub struct LintMods {
    rid: RequestID,
    result: Result<LintReport>,
}

impl LintMods {
    pub fn send(
        rc: &mut RequestCounter,
        store: Arc<ModStore>,
        mods: Vec<ModSpecification>,
        enabled_lints: BTreeSet<LintId>,
        game_pak_path: Option<PathBuf>,
        tx: Sender<Message>,
        ctx: egui::Context,
    ) -> MessageHandle<()> {
        let rid = rc.next();

        let handle = tokio::task::spawn(async move {
            let paths_res =
                resolve_async_ordered(store, ctx.clone(), mods.clone(), rid, tx.clone()).await;
            let mod_path_pairs_res =
                paths_res.map(|paths| mods.into_iter().zip(paths).collect::<Vec<_>>());

            let report_res = match mod_path_pairs_res {
                Ok(pairs) => tokio::task::spawn_blocking(move || {
                    crate::mod_lints::run_lints(
                        &enabled_lints,
                        BTreeSet::from_iter(pairs),
                        game_pak_path,
                    )
                })
                .await
                .unwrap(),
                Err(e) => Err(e),
            };

            tx.send(Message::LintMods(LintMods {
                rid,
                result: report_res,
            }))
            .await
            .unwrap();
            ctx.request_repaint();
        });

        MessageHandle {
            rid,
            handle,
            state: Default::default(),
        }
    }
    fn receive(self, app: &mut App) {
        if Some(self.rid) == app.lint_rid.as_ref().map(|r| r.rid) {
            match self.result {
                Ok(report) => {
                    info!("lint mod report complete");
                    app.lint_report = Some(report);
                    app.last_action_status =
                        LastActionStatus::Success("lint mod report complete".to_string());
                }
                Err(e) => match e.downcast::<IntegrationError>() {
                    Ok(IntegrationError::NoProvider { url: _, factory }) => {
                        app.window_provider_parameters =
                            Some(WindowProviderParameters::new(factory, &app.state));
                        app.last_action_status =
                            LastActionStatus::Failure("no provider".to_string());
                    }
                    Err(e) => {
                        error!("{:#?}\n{}", e, e.backtrace());
                        app.last_action_status = LastActionStatus::Failure(e.to_string());
                    }
                },
            }
            app.integrate_rid = None;
        }
    }
}

async fn resolve_async_ordered(
    store: Arc<ModStore>,
    ctx: egui::Context,
    mod_specs: Vec<ModSpecification>,
    rid: RequestID,
    message_tx: Sender<Message>,
) -> Result<Vec<PathBuf>> {
    let update = false;

    let mods = store.resolve_mods(&mod_specs, update).await?;

    let to_integrate = mod_specs
        .iter()
        .map(|u| mods[u].clone())
        .collect::<Vec<_>>();
    let res_map: HashMap<ModResolution, ModSpecification> = mods
        .iter()
        .map(|(spec, info)| (info.resolution.clone(), spec.clone()))
        .collect();
    let urls = to_integrate
        .iter()
        .map(|m| &m.resolution)
        .collect::<Vec<&ModResolution>>();

    let (tx, mut rx) = mpsc::channel::<FetchProgress>(10);

    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            if let Some(spec) = res_map.get(progress.resolution()) {
                message_tx
                    .send(Message::FetchModProgress(FetchModProgress {
                        rid,
                        spec: spec.clone(),
                        progress: progress.into(),
                    }))
                    .await
                    .unwrap();
                ctx.request_repaint();
            }
        }
    });

    store.fetch_mods_ordered(&urls, update, Some(tx)).await
}
