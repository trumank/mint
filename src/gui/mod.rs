mod message;

//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context, Result};
use eframe::{
    egui::{self, TextFormat},
    epaint::{text::LayoutJob, Color32},
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::{
    error::IntegrationError,
    find_drg,
    providers::{
        FetchProgress, ModResolution, ModSpecification, ModStore, ProviderFactory, ResolvableStatus,
    },
    state::{ModConfig, State},
};

use request_counter::{RequestCounter, RequestID};

pub fn gui() -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };
    eframe::run_native(
        "DRG Mod Integration",
        options,
        Box::new(|_cc| Box::new(App::new().unwrap())),
    )
    .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

struct App {
    tx: Sender<message::Message>,
    rx: Receiver<message::Message>,
    state: State,
    profile_dropdown: String,
    log: String,
    resolve_mod: String,
    resolve_mod_rid: Option<RequestID>,
    integrate_rid: Option<(
        RequestID,
        JoinHandle<()>,
        HashMap<ModSpecification, SpecFetchProgress>,
    )>,
    request_counter: RequestCounter,
    dnd: egui_dnd::DragDropUi,
    window_provider_parameters: Option<WindowProviderParameters>,
    search_string: Option<String>,
}

impl App {
    fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let state = State::new()?;

        Ok(Self {
            tx,
            rx,
            request_counter: Default::default(),
            profile_dropdown: state.profiles.active_profile.to_string(),
            state,
            log: Default::default(),
            resolve_mod: Default::default(),
            resolve_mod_rid: None,
            integrate_rid: None,
            dnd: Default::default(),
            window_provider_parameters: None,
            search_string: Default::default(),
        })
    }

    fn ui_profile(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.ui_profile_table(ui);
            });
        });
    }

    fn ui_profile_table(&mut self, ui: &mut egui::Ui) {
        let mods = &mut self.state.profiles.get_active_profile_mut().mods;
        let mut needs_save = false;
        let mut btn_remove = None;

        use egui_dnd::utils::shift_vec;
        use egui_dnd::DragDropItem;

        struct DndItem<'item> {
            index: usize,
            item: &'item mut ModConfig,
        }

        impl<'item> DragDropItem for DndItem<'item> {
            fn id(&self) -> egui::Id {
                egui::Id::new(self.index)
            }
        }

        let mut items = mods
            .iter_mut()
            .enumerate()
            .map(|(index, item)| DndItem { index, item })
            .collect::<Vec<_>>();

        let res = self
            .dnd
            .ui::<DndItem>(ui, items.iter_mut(), |item, ui, handle| {
                ui.horizontal(|ui| {
                    handle.ui(ui, item, |ui| {
                        ui.label("☰");
                    });

                    if ui.button(" ➖ ").clicked() {
                        btn_remove = Some(item.index);
                    }

                    if ui
                        .add(egui::Checkbox::without_text(&mut item.item.required))
                        .changed()
                    {
                        needs_save = true;
                    }

                    let info = self.state.store.get_mod_info(&item.item.spec);

                    if let Some((_, _, progress)) = &self.integrate_rid {
                        match progress.get(&item.item.spec) {
                            Some(SpecFetchProgress::Progress { progress, size }) => {
                                ui.add(
                                    egui::ProgressBar::new(*progress as f32 / *size as f32)
                                        .show_percentage()
                                        .desired_width(100.0),
                                );
                            }
                            Some(SpecFetchProgress::Complete) => {
                                ui.add(egui::ProgressBar::new(1.0).desired_width(100.0));
                            }
                            None => {
                                ui.spinner();
                            }
                        }
                    }

                    if let Some(info) = &info {
                        egui::ComboBox::from_id_source(item.index)
                            .selected_text(
                                self.state
                                    .store
                                    .get_version_name(&item.item.spec)
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut item.item.spec.url,
                                    info.spec.url.to_string(),
                                    self.state
                                        .store
                                        .get_version_name(&info.spec)
                                        .unwrap_or_default(),
                                );
                                for version in &info.versions {
                                    ui.selectable_value(
                                        &mut item.item.spec.url,
                                        version.url.to_string(),
                                        self.state
                                            .store
                                            .get_version_name(version)
                                            .unwrap_or_default(),
                                    );
                                }
                            });

                        let mut job = LayoutJob::default();
                        if let Some(search_string) = &self.search_string {
                            for (is_match, chunk) in FindString::new(&info.name, search_string) {
                                let background = if is_match {
                                    TextFormat {
                                        background: Color32::from_rgb(128, 32, 32),
                                        ..Default::default()
                                    }
                                } else {
                                    Default::default()
                                };
                                job.append(chunk, 0.0, background);
                            }
                        } else {
                            job.append(&info.name, 0.0, Default::default());
                        }

                        ui.hyperlink_to(job, &item.item.spec.url);
                    } else {
                        ui.hyperlink(&item.item.spec.url);
                    }
                });
            });

        if let Some(response) = res.completed {
            shift_vec(response.from, response.to, mods);
        }

        if let Some(remove) = btn_remove {
            mods.remove(remove);
            needs_save = true;
        }
        if needs_save {
            self.state.profiles.save().unwrap();
        }
    }

    fn add_mod(&mut self, ctx: &egui::Context) {
        let rid = self.request_counter.next();
        let spec = ModSpecification {
            url: self.resolve_mod.to_string(),
        };
        let store = self.state.store.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let res = store.resolve_mod(spec, false).await;
            tx.send(message::Message::ResolveMod(rid, res))
                .await
                .unwrap();
            ctx.request_repaint();
        });
        self.resolve_mod_rid = Some(rid);
    }

    fn show_provider_parameters(&mut self, ctx: &egui::Context) {
        let Some(window) = &mut self.window_provider_parameters else { return };

        while let Ok((rid, res)) = window.rx.try_recv() {
            if window.check_rid.as_ref().map_or(false, |r| rid == r.0) {
                match res {
                    Ok(()) => {
                        let window = self.window_provider_parameters.take().unwrap();
                        self.state
                            .config
                            .provider_parameters
                            .insert(window.factory.id.to_string(), window.parameters);
                        self.state.config.save().unwrap();
                        return;
                    }
                    Err(e) => {
                        window.check_error = Some(e.to_string());
                    }
                }
                window.check_rid = None;
            }
        }

        let mut open = true;
        let mut check = false;
        egui::Window::new(format!("configure {} provider", window.factory.id))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(window.check_rid.is_none(), |ui| {
                    egui::Grid::new("grid").num_columns(2).show(ui, |ui| {
                        for p in window.factory.parameters {
                            ui.label(p.name).on_hover_text(p.description);
                            let res = ui.add(
                                egui::TextEdit::singleline(
                                    window.parameters.entry(p.id.to_string()).or_default(),
                                )
                                .password(true)
                                .desired_width(200.0),
                            );
                            if is_committed(&res) {
                                check = true;
                            }
                            ui.end_row();
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui.button("check").clicked() {
                            check = true;
                        }
                        if window.check_rid.is_some() {
                            ui.spinner();
                        }
                        if let Some(error) = &window.check_error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                    });
                });
            });
        if !open {
            self.window_provider_parameters = None;
        } else if check {
            window.check_error = None;
            let tx = window.tx.clone();
            let ctx = ctx.clone();
            let rid = self.request_counter.next();
            let store = self.state.store.clone();
            let params = window.parameters.clone();
            let factory = window.factory;
            let handle = tokio::task::spawn(async move {
                let res = store.add_provider_checked(factory, &params).await;
                tx.send((rid, res)).await.unwrap();
                ctx.request_repaint();
            });
            window.check_rid = Some((rid, handle));
        }
    }
}

mod request_counter {
    /// Simple counter that returns a new ID each time it is called
    #[derive(Default)]
    pub struct RequestCounter(u32);

    impl RequestCounter {
        /// Get next ID
        pub fn next(&mut self) -> RequestID {
            let id = self.0;
            self.0 += 1;
            RequestID { id }
        }
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct RequestID {
        id: u32,
    }
}

struct WindowProviderParameters {
    tx: Sender<(RequestID, Result<()>)>,
    rx: Receiver<(RequestID, Result<()>)>,
    check_rid: Option<(RequestID, JoinHandle<()>)>,
    check_error: Option<String>,
    factory: &'static ProviderFactory,
    parameters: HashMap<String, String>,
}
impl WindowProviderParameters {
    fn new(factory: &'static ProviderFactory, state: &mut State) -> Self {
        let (tx, rx) = mpsc::channel(10);
        Self {
            tx,
            rx,
            check_rid: None,
            check_error: None,
            parameters: state
                .config
                .provider_parameters
                .get(factory.id)
                .cloned()
                .unwrap_or_default(),
            factory,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // message handling
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                message::Message::ResolveMod(rid, res) => {
                    if Some(rid) == self.resolve_mod_rid {
                        match res {
                            Ok((_spec, mod_)) => {
                                self.state
                                    .profiles
                                    .get_active_profile_mut()
                                    .mods
                                    .push(ModConfig {
                                        spec: mod_.spec,
                                        required: mod_.suggested_require,
                                    });
                                self.state.profiles.save().unwrap();
                            }
                            Err(e) => match e.downcast::<IntegrationError>() {
                                Ok(IntegrationError::NoProvider { url: _, factory }) => {
                                    self.window_provider_parameters = Some(
                                        WindowProviderParameters::new(factory, &mut self.state),
                                    );
                                }
                                Err(e) => {
                                    self.log.push_str(&format!("{:#?}\n", e));
                                }
                            },
                        }
                        self.resolve_mod_rid = None;
                    }
                }
                message::Message::FetchModProgress(rid, spec, progress) => {
                    if let Some((r, _, progress_map)) = &mut self.integrate_rid {
                        if rid == *r {
                            progress_map.insert(spec, progress);
                        }
                    }
                }
                message::Message::Integrate(rid, res) => {
                    if self.integrate_rid.as_ref().map_or(false, |r| rid == r.0) {
                        match res {
                            Ok(()) => {
                                self.log.push_str("Integration complete\n");
                            }
                            Err(e) => match e.downcast::<IntegrationError>() {
                                Ok(IntegrationError::NoProvider { url: _, factory }) => {
                                    self.window_provider_parameters = Some(
                                        WindowProviderParameters::new(factory, &mut self.state),
                                    );
                                }
                                Err(e) => {
                                    self.log.push_str(&format!("{:#?}\n", e));
                                }
                            },
                        }
                        self.integrate_rid = None;
                    }
                }
            }
        }

        // begin draw

        self.show_provider_parameters(ctx);

        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.log.as_str()));
                    });
                },
            );
        });
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.add_enabled_ui(self.integrate_rid.is_none(), |ui| {
                    if ui.button("integrate").clicked() {
                        self.integrate_rid = integrate(
                            &mut self.request_counter,
                            self.state.store.clone(),
                            self.state
                                .profiles
                                .get_active_profile()
                                .mods
                                .iter()
                                .map(|m| m.spec.clone())
                                .collect(),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }
                });
                if self.integrate_rid.is_some() {
                    if ui.button("cancel").clicked() {
                        self.integrate_rid.take().unwrap().1.abort();
                    }
                    ui.spinner();
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_enabled(self.integrate_rid.is_none());
            // profile selection
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    self.state
                        .profiles
                        .profiles
                        .contains_key(&self.profile_dropdown)
                        && self.state.profiles.profiles.len() > 1,
                    |ui| {
                        if ui.button(" ➖ ").clicked() {
                            self.state.profiles.remove_active();
                            self.profile_dropdown = self.state.profiles.active_profile.to_string();
                            self.state.profiles.save().unwrap();
                        }
                    },
                );
                ui.add_enabled_ui(
                    self.profile_dropdown != self.state.profiles.active_profile,
                    |ui| {
                        if ui.button(" ➕ ").clicked() {
                            self.state
                                .profiles
                                .profiles
                                .entry(self.profile_dropdown.to_string())
                                .or_default();
                            self.state.profiles.active_profile = self.profile_dropdown.to_string();
                            self.state.profiles.save().unwrap();
                        }
                    },
                );

                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    let res = ui.add(egui_dropdown::DropDownBox::from_iter(
                        self.state.profiles.profiles.keys(),
                        "profile_dropdown",
                        &mut self.profile_dropdown,
                        |ui, text| {
                            let mut job = LayoutJob {
                                halign: egui::Align::LEFT,
                                ..Default::default()
                            };
                            job.append(text, 0.0, Default::default());
                            ui.selectable_label(text == self.state.profiles.active_profile, job)
                        },
                    ));
                    if res.gained_focus() {
                        self.profile_dropdown.clear();
                    }
                    if is_committed(&res) {
                        self.state
                            .profiles
                            .profiles
                            .entry(self.profile_dropdown.to_string())
                            .or_default();
                        self.state.profiles.active_profile = self.profile_dropdown.to_string();
                        self.state.profiles.save().unwrap();
                        ui.memory_mut(|m| m.close_popup());
                    }

                    if self
                        .state
                        .profiles
                        .profiles
                        .contains_key(&self.profile_dropdown)
                    {
                        self.state.profiles.active_profile = self.profile_dropdown.to_string();
                        self.state.profiles.save().unwrap();
                    }
                });
            });

            ui.separator();

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if self.resolve_mod_rid.is_some() {
                    ui.spinner();
                }
                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    let resolve = ui.add_enabled(
                        self.resolve_mod_rid.is_none(),
                        egui::TextEdit::singleline(&mut self.resolve_mod).hint_text("Add mod..."),
                    );
                    if is_committed(&resolve) {
                        self.add_mod(ctx);
                    }
                });
            });

            self.ui_profile(ui);

            if let Some(search_string) = &mut self.search_string {
                let res = ui
                    .child_ui(ui.max_rect(), egui::Layout::bottom_up(egui::Align::RIGHT))
                    .add(egui::TextEdit::singleline(search_string));
                if res.lost_focus() {
                    self.search_string = None;
                } else if !res.has_focus() {
                    res.request_focus();
                }
            }

            ctx.input(|i| {
                for e in &i.events {
                    match e {
                        egui::Event::Paste(s) => {
                            if self.integrate_rid.is_none() && ctx.memory(|m| m.focus().is_none()) {
                                self.resolve_mod = s.to_string();
                                self.add_mod(ctx);
                            }
                        }
                        egui::Event::Text(text) => {
                            if ctx.memory(|m| m.focus().is_none()) {
                                self.search_string = Some(text.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            });
        });
    }
}

fn is_committed(res: &egui::Response) -> bool {
    res.lost_focus() && res.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

#[derive(Debug)]
pub enum SpecFetchProgress {
    Progress { progress: u64, size: u64 },
    Complete,
}
impl From<FetchProgress> for SpecFetchProgress {
    fn from(value: FetchProgress) -> Self {
        match value {
            FetchProgress::Progress { progress, size, .. } => Self::Progress { progress, size },
            FetchProgress::Complete { .. } => Self::Complete,
        }
    }
}

fn integrate(
    rc: &mut RequestCounter,
    store: Arc<ModStore>,
    mods: Vec<ModSpecification>,
    tx: Sender<message::Message>,
    ctx: egui::Context,
) -> Option<(
    RequestID,
    JoinHandle<()>,
    HashMap<ModSpecification, SpecFetchProgress>,
)> {
    let rid = rc.next();

    async fn integrate(
        store: Arc<ModStore>,
        ctx: egui::Context,
        mod_specs: Vec<ModSpecification>,
        rid: RequestID,
        message_tx: Sender<message::Message>,
    ) -> Result<()> {
        let path_game = find_drg().context("Could not find DRG install directory")?;

        let update = false;

        let mods = store.resolve_mods(&mod_specs, update).await?;

        let to_integrate = mod_specs
            .iter()
            .map(|u| mods[u].clone())
            .collect::<Vec<_>>();
        let res_map: HashMap<ModResolution, ModSpecification> = mods
            .iter()
            .flat_map(|(spec, info)| {
                if let ResolvableStatus::Resolvable(res) = &info.status {
                    Some((res.clone(), spec.clone()))
                } else {
                    None
                }
            })
            .collect();
        let urls = to_integrate
            .iter()
            .filter_map(|m| {
                if let ResolvableStatus::Resolvable(res) = &m.status {
                    Some(res)
                } else {
                    None
                }
            })
            .collect::<Vec<&ModResolution>>();

        let (tx, mut rx) = mpsc::channel::<FetchProgress>(10);

        tokio::spawn(async move {
            while let Some(progress) = rx.recv().await {
                if let Some(spec) = res_map.get(progress.resolution()) {
                    message_tx
                        .send(message::Message::FetchModProgress(
                            rid,
                            spec.clone(),
                            progress.into(),
                        ))
                        .await
                        .unwrap();
                    ctx.request_repaint();
                }
            }
        });

        let paths = store.fetch_mods(&urls, update, Some(tx)).await?;

        tokio::task::spawn_blocking(|| {
            crate::integrate::integrate(path_game, to_integrate.into_iter().zip(paths).collect())
        })
        .await??;

        Ok(())
    }

    Some((
        rid,
        tokio::task::spawn(async move {
            let res = integrate(store, ctx.clone(), mods, rid, tx.clone()).await;
            tx.send(message::Message::Integrate(rid, res))
                .await
                .unwrap();
            ctx.request_repaint();
        }),
        Default::default(),
    ))
}
struct FindString<'data> {
    string: &'data str,
    string_lower: String,
    needle: &'data str,
    needle_lower: String,
    curr: usize,
    curr_match: bool,
    finished: bool,
}
impl<'data> FindString<'data> {
    fn new(string: &'data str, needle: &'data str) -> Self {
        Self {
            string,
            string_lower: string.to_lowercase(),
            needle,
            needle_lower: needle.to_lowercase(),
            curr: 0,
            curr_match: false,
            finished: false,
        }
    }
    fn next_internal(&mut self) -> Option<(bool, &'data str)> {
        if self.finished {
            None
        } else if self.needle.is_empty() {
            self.finished = true;
            Some((false, self.string))
        } else if self.curr_match {
            self.curr_match = false;
            Some((true, &self.string[self.curr - self.needle.len()..self.curr]))
        } else if let Some(index) = self.string_lower[self.curr..].find(&self.needle_lower) {
            let next = self.curr + index;
            let chunk = &self.string[self.curr..next];
            self.curr = next + self.needle.len();
            self.curr_match = true;
            Some((false, chunk))
        } else {
            self.finished = true;
            Some((false, &self.string[self.curr..]))
        }
    }
}

impl<'data> Iterator for FindString<'data> {
    type Item = (bool, &'data str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.string.is_empty() {
            return None;
        }
        // skip empty chunks
        while let Some(chunk) = self.next_internal() {
            if !chunk.1.is_empty() {
                return Some(chunk);
            }
        }
        None
    }
}
