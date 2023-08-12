mod find_string;
mod message;
mod named_combobox;
mod request_counter;

//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
    path::PathBuf,
};

use anyhow::{anyhow, Context, Result};
use eframe::{
    egui::{self, FontSelection, TextFormat},
    epaint::{text::LayoutJob, Color32, Stroke},
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};
use tracing::{debug, info};

use crate::state::ModOrGroup;
use crate::{
    integrate::uninstall,
    is_drg_pak,
    providers::ModioTags,
    providers::{FetchProgress, ModSpecification, ModStore, ProviderFactory},
    state::{ModConfig, ModData_v0_1_0 as ModData, State},
};
use find_string::FindString;
use message::MessageHandle;
use request_counter::{RequestCounter, RequestID};

pub fn gui(args: Option<Vec<String>>) -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(800.0, 400.0)),
        drag_and_drop_support: true,
        ..Default::default()
    };
    eframe::run_native(
        "DRG Mod Integration",
        options,
        Box::new(|_cc| Box::new(App::new(args).unwrap())),
    )
    .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

const MODIO_LOGO_PNG: &[u8] = include_bytes!("../../assets/modio-cog-blue.png");

pub struct App {
    args: Option<Vec<String>>,
    tx: Sender<message::Message>,
    rx: Receiver<message::Message>,
    state: State,
    resolve_mod: String,
    resolve_mod_rid: Option<MessageHandle<()>>,
    integrate_rid: Option<MessageHandle<HashMap<ModSpecification, SpecFetchProgress>>>,
    update_rid: Option<MessageHandle<()>>,
    check_updates_rid: Option<MessageHandle<()>>,
    checked_updates_initially: bool,
    request_counter: RequestCounter,
    window_provider_parameters: Option<WindowProviderParameters>,
    search_string: Option<String>,
    scroll_to_match: bool,
    settings_window: Option<WindowSettings>,
    modio_texture_handle: Option<egui::TextureHandle>,
    last_action_status: LastActionStatus,
    available_update: Option<GitHubRelease>,
    open_profiles: HashSet<String>,
}

enum LastActionStatus {
    Idle,
    Success(String),
    Failure(String),
}

impl App {
    fn new(args: Option<Vec<String>>) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let state = State::init()?;
        info!("config dir = {}", state.project_dirs.config_dir().display());
        info!("cache dir = {}", state.project_dirs.cache_dir().display());

        Ok(Self {
            args,
            tx,
            rx,
            request_counter: Default::default(),
            state,
            resolve_mod: Default::default(),
            resolve_mod_rid: None,
            integrate_rid: None,
            update_rid: None,
            check_updates_rid: None,
            checked_updates_initially: false,
            window_provider_parameters: None,
            search_string: Default::default(),
            scroll_to_match: false,
            settings_window: None,
            modio_texture_handle: None,
            last_action_status: LastActionStatus::Idle,
            available_update: None,
            open_profiles: Default::default(),
        })
    }

    fn ui_profile(&mut self, ui: &mut egui::Ui, profile: &str) {
        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.ui_profile_table(ui, profile);
            });
        });
    }

    /// TODO: actually implement mod groups; for now we rely on a 1-1 mapping between mod groups
    /// and profiles for migration.
    fn ui_profile_table(&mut self, ui: &mut egui::Ui, profile: &str) {
        if let Some(profile) = self.state.mod_data.profiles.get_mut(profile) {
            let mut needs_save = false;
            let mut btn_remove = None;
            let mut add_deps = None;

            let enabled_specs = profile
                .mods
                .iter()
                .enumerate()
                .filter_map(|(i, m)| match m {
                    ModOrGroup::Individual(mc) => mc.enabled.then_some((i, mc.spec.clone())),
                    ModOrGroup::Group { .. } => {
                        unimplemented!("mod group feature not yet implemented")
                    }
                })
                .collect::<Vec<_>>();

            let res = egui_dnd::dnd(ui, ui.id()).show(profile.mods.iter_mut().enumerate(), |ui: &mut egui::Ui, item: (usize, &mut ModOrGroup), handle, state| {
                ui.horizontal(|ui| {
                    let item = match item.1 {
                        ModOrGroup::Individual(mc) => mc,
                        ModOrGroup::Group { .. } => unimplemented!("mod group feature not yet implemented"),
                    };
                    handle.ui(ui, |ui| {
                        ui.label("☰");
                    });

                    if ui.button(" ➖ ").clicked() {
                        btn_remove = Some(state.index);
                    }

                    if ui
                        .add(egui::Checkbox::without_text(&mut item.enabled))
                        .on_hover_text_at_pointer("enabled?")
                        .changed()
                    {
                        needs_save = true;
                    }

                    /*
                    if ui
                        .add(egui::Checkbox::without_text(&mut item.required))
                        .changed()
                    {
                        needs_save = true;
                    }
                    */

                    let info = self.state.store.get_mod_info(&item.spec);

                    if item.enabled {
                        if let Some(req) = &self.integrate_rid {
                            match req.state.get(&item.spec) {
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
                    }

                    if let Some(info) = &info {
                        egui::ComboBox::from_id_source(state.index)
                            .selected_text(
                                self.state
                                    .store
                                    .get_version_name(&item.spec)
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut item.spec.url,
                                    info.spec.url.to_string(),
                                    self.state
                                        .store
                                        .get_version_name(&info.spec)
                                        .unwrap_or_default(),
                                );
                                for version in info.versions.iter().rev() {
                                    ui.selectable_value(
                                        &mut item.spec.url,
                                        version.url.to_string(),
                                        self.state
                                            .store
                                            .get_version_name(version)
                                            .unwrap_or_default(),
                                    );
                                }
                            });

                        if ui
                            .button("📋")
                            .on_hover_text_at_pointer("copy URL")
                            .clicked()
                        {
                            ui.output_mut(|o| o.copied_text = item.spec.url.to_owned());
                        }

                        let is_duplicate = enabled_specs.iter().any(|(i, spec)| {
                            state.index != *i && info.spec.satisfies_dependency(spec)
                        });
                        if is_duplicate
                            && ui
                                .button(
                                    egui::RichText::new("\u{26A0}")
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .on_hover_text_at_pointer("remove duplicate")
                                .clicked()
                        {
                            btn_remove = Some(state.index);
                        }

                        let missing_deps = info
                            .suggested_dependencies
                            .iter()
                            .filter(|d| {
                                !enabled_specs.iter().any(|(_, s)| s.satisfies_dependency(d))
                            })
                            .collect::<Vec<_>>();

                        if !missing_deps.is_empty() {
                            let mut msg = "Add missing dependencies:".to_string();
                            for dep in &missing_deps {
                                msg.push('\n');
                                msg.push_str(&dep.url);
                            }
                            if ui
                                .button(
                                    egui::RichText::new("\u{26A0}")
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .on_hover_text(msg)
                                .clicked()
                            {
                                add_deps = Some(missing_deps.into_iter().cloned().collect());
                            }
                        }

                        let mut job = LayoutJob::default();
                        let mut is_match = false;
                        if let Some(search_string) = &self.search_string {
                            for (m, chunk) in FindString::new(&info.name, search_string) {
                                let background = if m {
                                    is_match = true;
                                    TextFormat {
                                        background: Color32::YELLOW,
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

                        match info.provider {
                            "modio" => {
                                let texture: &egui::TextureHandle =
                                    self.modio_texture_handle.get_or_insert_with(|| {
                                        let image =
                                            image::load_from_memory(MODIO_LOGO_PNG).unwrap();
                                        let size = [image.width() as _, image.height() as _];
                                        let image_buffer = image.to_rgba8();
                                        let pixels = image_buffer.as_flat_samples();
                                        let image = egui::ColorImage::from_rgba_unmultiplied(
                                            size,
                                            pixels.as_slice(),
                                        );

                                        ui.ctx().load_texture(
                                            "modio-logo",
                                            image,
                                            Default::default(),
                                        )
                                    });
                                ui.image(texture, [16.0, 16.0]);
                            }
                            "http" => {
                                ui.label("🌐");
                            }
                            "file" => {
                                ui.label("📁");
                            }
                            _ => unimplemented!("unimplemented provider kind"),
                        }

                        let res = ui.hyperlink_to(job, &item.spec.url);
                        if is_match && self.scroll_to_match {
                            res.scroll_to_me(None);
                            self.scroll_to_match = false;
                        }

                        if let Some(ModioTags {
                            qol,
                            gameplay,
                            audio,
                            visual,
                            framework,
                            required_status,
                            approval_status,
                            .. // version ignored
                        }) = &info.modio_tags
                        {
                            let mut mk_searchable_modio_tag = |tag_str: &str, ui: &mut egui::Ui, color: Option<egui::Color32>, hover_str: Option<&str>| {
                                let text_color = if color.is_some() { Color32::BLACK } else { Color32::GRAY };
                                let mut job = LayoutJob::default();
                                let mut is_match = false;
                                if let Some(search_string) = &self.search_string {
                                    for (m, chunk) in find_string::FindString::new(tag_str, search_string) {
                                        let background = if m {
                                            is_match = true;
                                            TextFormat {
                                                background: Color32::YELLOW,
                                                color: text_color,
                                                ..Default::default()
                                            }
                                        } else {
                                            TextFormat {
                                                color: text_color,
                                                ..Default::default()
                                            }
                                        };
                                        job.append(chunk, 0.0, background);
                                    }
                                } else {
                                    job.append(tag_str, 0.0, TextFormat {
                                        color: text_color,
                                        ..Default::default()
                                    });
                                }

                                let button = if let Some(color) = color {
                                    egui::Button::new(job).small().fill(color).stroke(egui::Stroke::NONE)
                                } else {
                                    egui::Button::new(job).small().stroke(egui::Stroke::NONE)
                                };

                                let res = if let Some(hover_str) = hover_str {
                                    ui.add_enabled(false, button).on_disabled_hover_text(hover_str)
                                } else {
                                    ui.add_enabled(false, button)
                                };

                                if is_match && self.scroll_to_match {
                                    res.scroll_to_me(None);
                                    self.scroll_to_match = false;
                                }
                            };

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                match approval_status {
                                    crate::providers::ApprovalStatus::Verified => {
                                        mk_searchable_modio_tag("Verified", ui, Some(egui::Color32::LIGHT_GREEN), Some("Does not contain any gameplay affecting features or changes"));
                                    }
                                    crate::providers::ApprovalStatus::Approved => {
                                        mk_searchable_modio_tag("Approved", ui, Some(egui::Color32::LIGHT_BLUE), Some("Contains gameplay affecting features or changes"));
                                    }
                                    crate::providers::ApprovalStatus::Sandbox => {
                                        mk_searchable_modio_tag("Sandbox", ui, Some(egui::Color32::LIGHT_YELLOW), Some("Contains significant, possibly progression breaking, changes to gameplay"));
                                    }
                                }

                                match required_status {
                                    crate::providers::RequiredStatus::RequiredByAll => {
                                        mk_searchable_modio_tag("RequiredByAll", ui, Some(egui::Color32::LIGHT_RED), Some("All lobby members must use this mod for it to work correctly!"));
                                    }
                                    crate::providers::RequiredStatus::Optional => {
                                        mk_searchable_modio_tag("Optional", ui, None, Some("Clients are not required to install this mod to function"));
                                    }
                                }

                                if *qol {
                                    mk_searchable_modio_tag("QoL", ui, None, None);
                                }
                                if *gameplay {
                                    mk_searchable_modio_tag("Gameplay", ui, None, None);
                                }
                                if *audio {
                                    mk_searchable_modio_tag("Audio", ui, None, None);
                                }
                                if *visual {
                                    mk_searchable_modio_tag("Visual", ui, None, None);
                                }
                                if *framework {
                                    mk_searchable_modio_tag("Framework", ui, None, None);
                                }
                            });
                        }
                    } else {
                        if ui
                            .button("📋")
                            .on_hover_text_at_pointer("Copy URL")
                            .clicked()
                        {
                            ui.output_mut(|o| o.copied_text = item.spec.url.to_owned());
                        }
                        ui.hyperlink(&item.spec.url);
                    }
                });
            });

            if res.final_update().is_some() {
                res.update_vec(&mut profile.mods);
                needs_save = true;
            }

            if let Some(remove) = btn_remove {
                profile.mods.remove(remove);
                needs_save = true;
            }

            if let Some(add_deps) = add_deps {
                message::ResolveMods::send(self, ui.ctx(), add_deps, true);
            }

            if needs_save {
                self.state.mod_data.save().unwrap();
            }
        } else {
            ui.label("no such profile");
        }
    }

    fn parse_mods(&self) -> Vec<ModSpecification> {
        self.resolve_mod
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| ModSpecification::new(l.to_string()))
            .collect()
    }

    fn build_mod_string(mods: &Vec<ModConfig>) -> String {
        let mut string = String::new();
        for m in mods {
            if m.enabled {
                string.push_str(&m.spec.url);
                string.push('\n');
            }
        }
        string
    }

    fn show_provider_parameters(&mut self, ctx: &egui::Context) {
        let Some(window) = &mut self.window_provider_parameters else {
            return;
        };

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
        egui::Window::new(format!("Configure {} provider", window.factory.id))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(window.check_rid.is_none(), |ui| {
                    egui::Grid::new("grid").num_columns(2).show(ui, |ui| {
                        for p in window.factory.parameters {
                            if let Some(link) = p.link {
                                ui.hyperlink_to(p.name, link).on_hover_text(p.description);
                            } else {
                                ui.label(p.name).on_hover_text(p.description);
                            }
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
                        if ui.button("Save").clicked() {
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

    fn show_profile_windows(&mut self, ctx: &egui::Context) {
        let mut to_remove = vec![];
        for profile in &self.open_profiles.clone() {
            let mut open = true;
            egui::Window::new(format!("Profile \"{profile}\""))
                .open(&mut open)
                .show(ctx, |ui| {
                    self.ui_profile(ui, profile);
                });
            if !open {
                to_remove.push(profile.clone());
            }
        }
        for r in to_remove {
            self.open_profiles.remove(&r);
        }
    }

    fn show_settings(&mut self, ctx: &egui::Context) {
        if let Some(window) = &mut self.settings_window {
            let mut open = true;
            let mut try_save = false;
            egui::Window::new("Settings")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("grid").num_columns(2).show(ui, |ui| {
                        let mut job = LayoutJob::default();
                        job.append(
                            "DRG pak",
                            0.0,
                            TextFormat {
                                color: ui.visuals().text_color(),
                                underline: Stroke::new(1.0, ui.visuals().text_color()),
                                ..Default::default()
                            },
                        );
                        ui.label(job).on_hover_cursor(egui::CursorIcon::Help).on_hover_text("Path to FSD-WindowsNoEditor.pak (FSD-WinGDK.pak for Microsoft Store version)\nLocated inside the \"Deep Rock Galactic\" installation directory under FSD/Content/Paks.");
                        ui.horizontal(|ui| {
                            let res = ui.add(
                                egui::TextEdit::singleline(
                                    &mut window.drg_pak_path
                                )
                                .desired_width(200.0),
                            );
                            if res.changed() {
                                window.drg_pak_path_err = None;
                            }
                            if is_committed(&res) {
                                try_save = true;
                            }
                            if ui.button("browse").clicked() {
                                if let Some(fsd_pak) = rfd::FileDialog::new()
                                    .add_filter("DRG Pak", &["pak"])
                                    .pick_file()
                                {
                                    window.drg_pak_path = fsd_pak.to_string_lossy().to_string();
                                    window.drg_pak_path_err = None;
                                }
                            }
                        });
                        ui.end_row();

                        let config_dir = self.state.project_dirs.config_dir();
                        ui.label("Config directory:");
                        if ui.link(config_dir.display().to_string()).clicked() {
                            opener::open(config_dir).ok();
                        }
                        ui.end_row();

                        let cache_dir = self.state.project_dirs.cache_dir();
                        ui.label("Cache directory:");
                        if ui.link(cache_dir.display().to_string()).clicked() {
                            opener::open(cache_dir).ok();
                        }
                        ui.end_row();

                        ui.label("Mod providers:");
                        ui.end_row();

                        for provider_factory in ModStore::get_provider_factories() {
                            ui.label(provider_factory.id);
                            if ui.add_enabled(!provider_factory.parameters.is_empty(), egui::Button::new("⚙"))
                                    .on_hover_text(format!("Open \"{}\" settings", provider_factory.id))
                                    .clicked() {
                                self.window_provider_parameters = Some(
                                    WindowProviderParameters::new(provider_factory, &self.state),
                                );
                            }
                            ui.end_row();
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui.add_enabled(window.drg_pak_path_err.is_none(), egui::Button::new("save")).clicked() {
                            try_save = true;
                        }
                        if let Some(error) = &window.drg_pak_path_err {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                    });

                });
            if try_save {
                if let Err(e) = is_drg_pak(&window.drg_pak_path).context("Is not valid DRG pak") {
                    window.drg_pak_path_err = Some(e.to_string());
                } else {
                    self.state.config.drg_pak_path = Some(PathBuf::from(
                        self.settings_window.take().unwrap().drg_pak_path,
                    ));
                    self.state.config.save().unwrap();
                }
            } else if !open {
                self.settings_window = None;
            }
        }
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
    fn new(factory: &'static ProviderFactory, state: &State) -> Self {
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

struct WindowSettings {
    drg_pak_path: String,
    drg_pak_path_err: Option<String>,
}

impl WindowSettings {
    fn new(state: &State) -> Self {
        let path = state
            .config
            .drg_pak_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            drg_pak_path: path,
            drg_pak_path_err: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.checked_updates_initially {
            self.checked_updates_initially = true;
            message::CheckUpdates::send(self, ctx);
        }

        // message handling
        while let Ok(msg) = self.rx.try_recv() {
            msg.handle(self);
        }

        // begin draw

        self.show_provider_parameters(ctx);
        self.show_profile_windows(ctx);
        self.show_settings(ctx);

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.add_enabled_ui(
                    self.integrate_rid.is_none()
                        && self.update_rid.is_none()
                        && self.state.config.drg_pak_path.is_some(),
                    |ui| {
                        if let Some(args) = &self.args {
                            if ui.button("Launch game").on_hover_ui(|ui| for arg in args {
                                ui.label(arg);
                            }).clicked() {
                                let args = args.clone();
                                tokio::task::spawn_blocking(move || {
                                    let mut iter = args.iter();
                                    std::process::Command::new(
                                        iter.next().unwrap(),
                                        ).args(iter).spawn().unwrap();
                                });
                            }
                        }

                        ui.add_enabled_ui(self.state.config.drg_pak_path.is_some(), |ui| {
                            let mut button = ui.button("Install mods");
                            if self.state.config.drg_pak_path.is_none() {
                                button = button.on_disabled_hover_text(
                                    "DRG install not found. Configure it in the settings menu.",
                                );
                            }

                            let mut mods = Vec::new();
                            let active_profile = self.state.mod_data.active_profile.clone();
                            self.state.mod_data.for_each_enabled_mod(&active_profile, |mc| {
                                mods.push(mc.spec.clone());
                            });

                            if button.clicked() {
                                self.last_action_status = LastActionStatus::Idle;
                                self.integrate_rid = Some(message::Integrate::send(
                                    &mut self.request_counter,
                                    self.state.store.clone(),
                                    mods,
                                    self.state.config.drg_pak_path.as_ref().unwrap().clone(),
                                    self.tx.clone(),
                                    ctx.clone(),
                                ));
                            }
                        });

                        ui.add_enabled_ui(self.state.config.drg_pak_path.is_some(), |ui| {
                            let mut button = ui.button("Uninstall mods");
                            if self.state.config.drg_pak_path.is_none() {
                                button = button.on_disabled_hover_text(
                                    "DRG install not found. Configure it in the settings menu.",
                                );
                            }
                            if button.clicked() {
                                self.last_action_status = LastActionStatus::Idle;
                                if let Some(pak_path) = &self.state.config.drg_pak_path {
                                    let mut mods = HashSet::default();
                                    let active_profile = self.state.mod_data.active_profile.clone();
                                    self.state.mod_data.for_each_enabled_mod(&active_profile, |mc| {
                                        if let Some(modio_id) = self.state.store
                                            .get_mod_info(&mc.spec)
                                            .and_then(|i| i.modio_id) {
                                                mods.insert(modio_id);
                                        }
                                    });

                                    debug!("uninstalling mods: pak_path = {}", pak_path.display());
                                    match uninstall(pak_path, mods) {
                                        Ok(()) => {
                                            self.last_action_status =
                                            LastActionStatus::Success("Successfully uninstalled mods".to_string());
                                        },
                                        Err(e) => {
                                            self.last_action_status =
                                            LastActionStatus::Failure(format!("Failed to uninstall mods: {e}"))
                                        }
                                    }
                                }
                            }
                        });

                        if ui
                            .button("Update cache")
                            .on_hover_text(
                                "Checks for updates for all mods and updates local cache\n\
                                due to strict mod.io rate-limiting, can take a long time for large mod lists",
                            )
                            .clicked()
                        {
                            let mut mods = Vec::new();
                            let active_profile = self.state.mod_data.active_profile.clone();
                            self.state.mod_data.for_each_mod(&active_profile, |mc| {
                                mods.push(mc.spec.clone());
                            });

                            message::UpdateCache::send(self, mods);
                        }
                    },
                );
                if self.integrate_rid.is_some() {
                    if ui.button("Cancel").clicked() {
                        self.integrate_rid.take().unwrap().handle.abort();
                    }
                    ui.spinner();
                }
                if self.update_rid.is_some() {
                    if ui.button("Cancel").clicked() {
                        self.update_rid.take().unwrap().handle.abort();
                    }
                    ui.spinner();
                }
                if ui.button("⚙").on_hover_text("Open settings").clicked() {
                    self.settings_window = Some(WindowSettings::new(&self.state));
                }
                if let Some(available_update) = &self.available_update {
                    if ui.button(egui::RichText::new("\u{26A0}").color(ui.visuals().warn_fg_color))
                        .on_hover_text(format!("Update available: {}\n{}", available_update.tag_name, available_update.html_url))
                        .clicked() {
                            ui.ctx().output_mut(|o| {
                                o.open_url = Some(egui::output::OpenUrl {
                                    url: available_update.html_url.clone(),
                                    new_tab: true,
                                });
                            });
                        }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    match &self.last_action_status {
                        LastActionStatus::Success(msg) => {
                            ui.label(
                                egui::RichText::new("STATUS")
                                    .color(Color32::BLACK)
                                    .background_color(Color32::LIGHT_GREEN)
                            );
                            ui.label(msg);
                        },
                        LastActionStatus::Failure(msg) => {
                            ui.label(
                                egui::RichText::new("STATUS")
                                    .color(Color32::BLACK)
                                    .background_color(Color32::LIGHT_RED)
                            );
                            ui.label(msg);
                        },
                        _ => {},
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_enabled(self.integrate_rid.is_none() && self.update_rid.is_none());
            // profile selection

            let buttons = |ui: &mut egui::Ui, mod_data: &mut ModData| {
                if ui
                    .button("📋")
                    .on_hover_text_at_pointer("Copy profile mods")
                    .clicked()
                {
                    let mut mods = Vec::new();
                    let active_profile = mod_data.active_profile.clone();
                    mod_data.for_each_enabled_mod(&active_profile, |mc| {
                        mods.push(mc.clone());
                    });
                    let mods = Self::build_mod_string(&mods);
                    ui.output_mut(|o| o.copied_text = mods);
                }

                // TODO find better icon, flesh out multiple-view usage, fix GUI locking
                /*
                if ui
                    .button("pop out")
                    .on_hover_text_at_pointer("pop out")
                    .clicked()
                {
                    self.open_profiles.insert(mod_data.active_profile.clone());
                }
                */
            };

            if named_combobox::ui(
                ui,
                "profile",
                self.state.mod_data.deref_mut().deref_mut(),
                Some(buttons),
            ) {
                self.state.mod_data.save().unwrap();
            }

            ui.separator();

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if self.resolve_mod_rid.is_some() {
                    ui.spinner();
                }
                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    // define multiline layouter to be able to show multiple lines in a single line widget
                    let font_id = FontSelection::default().resolve(ui.style());
                    let text_color = ui.visuals().widgets.inactive.text_color();
                    let mut multiline_layouter =
                        move |ui: &egui::Ui, text: &str, wrap_width: f32| {
                            let layout_job = LayoutJob::simple(
                                text.to_string(),
                                font_id.clone(),
                                text_color,
                                wrap_width,
                            );
                            ui.fonts(|f| f.layout_job(layout_job))
                        };

                    let resolve = ui.add_enabled(
                        self.resolve_mod_rid.is_none(),
                        egui::TextEdit::singleline(&mut self.resolve_mod)
                            .layouter(&mut multiline_layouter)
                            .hint_text("Add mod..."),
                    );
                    if is_committed(&resolve) {
                        message::ResolveMods::send(self, ctx, self.parse_mods(), false);
                    }
                });
            });

            let profile = self.state.mod_data.active_profile.clone();
            self.ui_profile(ui, &profile);

            // TODO: actually implement mod groups.
            if let Some(search_string) = &mut self.search_string {
                let lower = search_string.to_lowercase();
                let any_matches = self.state.mod_data.any_mod(&profile, |mc, _| {
                    self.state
                        .store
                        .get_mod_info(&mc.spec)
                        .map(|i| i.name.to_lowercase().contains(&lower))
                        .unwrap_or(false)
                });

                let mut text_edit = egui::TextEdit::singleline(search_string);
                if !any_matches {
                    text_edit = text_edit.text_color(ui.visuals().error_fg_color);
                }
                let res = ui
                    .child_ui(ui.max_rect(), egui::Layout::bottom_up(egui::Align::RIGHT))
                    .add(text_edit);
                if res.changed() {
                    self.scroll_to_match = true;
                }
                if res.lost_focus() {
                    self.search_string = None;
                    self.scroll_to_match = false;
                } else if !res.has_focus() {
                    res.request_focus();
                }
            }

            ctx.input(|i| {
                if !i.raw.dropped_files.is_empty()
                    && self.integrate_rid.is_none()
                    && self.update_rid.is_none()
                {
                    let mut mods = String::new();
                    for f in i
                        .raw
                        .dropped_files
                        .iter()
                        .filter_map(|f| f.path.as_ref().map(|p| p.to_string_lossy()))
                    {
                        mods.push_str(&f);
                        mods.push('\n');
                    }

                    self.resolve_mod = mods.trim().to_string();
                    message::ResolveMods::send(self, ctx, self.parse_mods(), false);
                }
                for e in &i.events {
                    match e {
                        egui::Event::Paste(s) => {
                            if self.integrate_rid.is_none()
                                && self.update_rid.is_none()
                                && ctx.memory(|m| m.focus().is_none())
                            {
                                self.resolve_mod = s.trim().to_string();
                                message::ResolveMods::send(self, ctx, self.parse_mods(), false);
                            }
                        }
                        egui::Event::Text(text) => {
                            if ctx.memory(|m| m.focus().is_none()) {
                                self.search_string = Some(text.to_string());
                                self.scroll_to_match = true;
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

/// A custom popup which does not automatically close when clicked.
fn custom_popup_above_or_below_widget<R>(
    ui: &egui::Ui,
    popup_id: egui::Id,
    widget_response: &egui::Response,
    above_or_below: egui::AboveOrBelow,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    if ui.memory(|mem| mem.is_popup_open(popup_id)) {
        let (pos, pivot) = match above_or_below {
            egui::AboveOrBelow::Above => {
                (widget_response.rect.left_top(), egui::Align2::LEFT_BOTTOM)
            }
            egui::AboveOrBelow::Below => {
                (widget_response.rect.left_bottom(), egui::Align2::LEFT_TOP)
            }
        };

        let inner = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .constrain(true)
            .fixed_pos(pos)
            .pivot(pivot)
            .show(ui.ctx(), |ui| {
                // Note: we use a separate clip-rect for this area, so the popup can be outside the parent.
                // See https://github.com/emilk/egui/issues/825
                let frame = egui::Frame::popup(ui.style());
                let frame_margin = frame.total_margin();
                frame
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            ui.set_width(widget_response.rect.width() - frame_margin.sum().x);
                            add_contents(ui)
                        })
                        .inner
                    })
                    .inner
            })
            .inner;

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.memory_mut(|mem| mem.close_popup());
        }
        Some(inner)
    } else {
        None
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct GitHubRelease {
    html_url: String,
    tag_name: String,
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
