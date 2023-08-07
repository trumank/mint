mod message;

//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    ops::DerefMut,
    path::PathBuf,
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use eframe::{
    egui::{self, FontSelection, TextFormat},
    epaint::{text::LayoutJob, Color32, Stroke},
};
use egui_dnd::utils::shift_vec;
use egui_dnd::DragDropItem;
use indexmap::IndexMap;
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::state::{ModData, ModProfile};
use crate::{
    error::IntegrationError,
    integrate::uninstall,
    is_drg_pak,
    providers::ModioTags,
    providers::{FetchProgress, ModResolution, ModSpecification, ModStore, ProviderFactory},
    state::{ModConfig, State},
};

use request_counter::{RequestCounter, RequestID};

pub fn gui(args: Option<Vec<String>>) -> Result<()> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(1200.0, 400.0)),
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

struct App {
    args: Option<Vec<String>>,
    tx: Sender<message::Message>,
    rx: Receiver<message::Message>,
    state: State,
    log: Log,
    resolve_mod: String,
    resolve_mod_rid: Option<RequestID>,
    integrate_rid: Option<(
        RequestID,
        JoinHandle<()>,
        HashMap<ModSpecification, SpecFetchProgress>,
    )>,
    update_rid: Option<(RequestID, JoinHandle<()>)>,
    check_updates_rid: Option<RequestID>,
    checked_updates_initially: bool,
    request_counter: RequestCounter,
    mod_group_dnd: egui_dnd::DragDropUi,
    window_provider_parameters: Option<WindowProviderParameters>,
    search_string: Option<String>,
    scroll_to_match: bool,
    settings_window: Option<WindowSettings>,
    modio_texture_handle: Option<egui::TextureHandle>,
    last_action_status: LastActionStatus,
    rename_mod_group_popup: NamePopup,
    add_mod_group_popup: NamePopup,
    duplicate_mod_group_popup: NamePopup,
    available_update: Option<GitHubRelease>,
    add_profile_popup: NamePopup,
    rename_profile_popup: NamePopup,
    duplicate_profile_popup: NamePopup,
    profile_dnd: egui_dnd::DragDropUi,
}

enum LastActionStatus {
    Idle,
    Success(String),
    Failure(String),
}

struct NamePopup {
    buffer_needs_prefill_and_focus: bool,
    buffer: String,
}
impl NamePopup {
    fn new() -> Self {
        Self {
            buffer_needs_prefill_and_focus: true,
            buffer: String::new(),
        }
    }
}

impl App {
    fn new(args: Option<Vec<String>>) -> Result<Self> {
        let (tx, rx) = mpsc::channel(10);
        let mut log = Log::default();
        let state = State::new()?;
        log.println(format!(
            "config dir: {}",
            state.project_dirs.config_dir().display()
        ));
        log.println(format!(
            "cache dir: {}",
            state.project_dirs.cache_dir().display()
        ));

        Ok(Self {
            args,
            tx,
            rx,
            request_counter: Default::default(),
            state,
            log,
            resolve_mod: Default::default(),
            resolve_mod_rid: None,
            integrate_rid: None,
            update_rid: None,
            check_updates_rid: None,
            checked_updates_initially: false,
            mod_group_dnd: Default::default(),
            window_provider_parameters: None,
            search_string: Default::default(),
            scroll_to_match: false,
            settings_window: None,
            modio_texture_handle: None,
            last_action_status: LastActionStatus::Idle,
            rename_mod_group_popup: NamePopup::new(),
            add_mod_group_popup: NamePopup::new(),
            duplicate_mod_group_popup: NamePopup::new(),
            available_update: None,
            add_profile_popup: NamePopup::new(),
            rename_profile_popup: NamePopup::new(),
            duplicate_profile_popup: NamePopup::new(),
            profile_dnd: Default::default(),
        })
    }

    fn ui_mod_group(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
            ui.push_id("mod-group-scrollarea", |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.ui_mod_group_table(ui);
                });
            });
        });
    }

    fn ui_mod_group_table(&mut self, ui: &mut egui::Ui) {
        let mods = &mut self.state.mod_data.get_active_group_mut().mods;
        let mut needs_save = false;
        let mut btn_remove = None;
        let mut add_deps = None;

        struct DndItem<'item> {
            index: usize,
            item: &'item mut ModConfig,
        }

        impl<'item> DragDropItem for DndItem<'item> {
            fn id(&self) -> egui::Id {
                egui::Id::new(format!("mod-group-dnd {}", self.index))
            }
        }

        let enabled_specs = mods
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.enabled.then_some((i, m.spec.clone())))
            .collect::<Vec<_>>();

        let mut items = mods
            .iter_mut()
            .enumerate()
            .map(|(index, item)| DndItem { index, item })
            .collect::<Vec<_>>();

        let res = self
            .mod_group_dnd
            .ui::<DndItem>(ui, items.iter_mut(), |item, ui, handle| {
                ui.horizontal(|ui| {
                    handle.ui(ui, item, |ui| {
                        ui.label("☰");
                    });

                    if ui.button(" ➖ ").clicked() {
                        btn_remove = Some(item.index);
                    }

                    if ui
                        .add(egui::Checkbox::without_text(&mut item.item.enabled))
                        .on_hover_text_at_pointer("Enabled?")
                        .changed()
                    {
                        needs_save = true;
                    }

                    let info = self.state.store.get_mod_info(&item.item.spec);

                    if item.item.enabled {
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
                                for version in info.versions.iter().rev() {
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

                        if ui
                            .button("📋")
                            .on_hover_text_at_pointer("Copy URL")
                            .clicked()
                        {
                            ui.output_mut(|o| o.copied_text = item.item.spec.url.to_owned());
                        }

                        let is_duplicate = enabled_specs.iter().any(|(i, spec)| {
                            item.index != *i && info.spec.satisfies_dependency(spec)
                        });
                        if is_duplicate
                            && ui
                                .button(
                                    egui::RichText::new("\u{26A0}")
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .on_hover_text_at_pointer("Remove duplicate")
                                .clicked()
                        {
                            btn_remove = Some(item.index);
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
                            _ => unimplemented!(),
                        }

                        let res = ui.hyperlink_to(job, &item.item.spec.url);
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
                            let mut mk_searchable_modio_tag = |tag_str: &str, ui: &mut egui::Ui, color: Option<egui::Color32>| {
                                let text_color = if color.is_some() { Color32::BLACK } else { Color32::GRAY };
                                let mut job = LayoutJob::default();
                                let mut is_match = false;
                                if let Some(search_string) = &self.search_string {
                                    for (m, chunk) in FindString::new(tag_str, search_string) {
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

                                let res = ui.add_enabled(false, button);

                                if is_match && self.scroll_to_match {
                                    res.scroll_to_me(None);
                                    self.scroll_to_match = false;
                                }
                            };

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                match approval_status {
                                    crate::providers::ApprovalStatus::Verified => {
                                        mk_searchable_modio_tag("Verified", ui, Some(egui::Color32::LIGHT_GREEN));
                                    }
                                    crate::providers::ApprovalStatus::Approved => {
                                        mk_searchable_modio_tag("Approved", ui, Some(egui::Color32::LIGHT_BLUE));
                                    }
                                    crate::providers::ApprovalStatus::Sandbox => {
                                        mk_searchable_modio_tag("Sandbox", ui, Some(egui::Color32::LIGHT_YELLOW));
                                    }
                                }

                                match required_status {
                                    crate::providers::RequiredStatus::RequiredByAll => {
                                        mk_searchable_modio_tag("RequiredByAll", ui, None);
                                    }
                                    crate::providers::RequiredStatus::Optional => {
                                        mk_searchable_modio_tag("Optional", ui, None);
                                    }
                                }

                                if *qol {
                                    mk_searchable_modio_tag("QoL", ui, None);
                                }
                                if *gameplay {
                                    mk_searchable_modio_tag("Gameplay", ui, None);
                                }
                                if *audio {
                                    mk_searchable_modio_tag("Audio", ui, None);
                                }
                                if *visual {
                                    mk_searchable_modio_tag("Visual", ui, None);
                                }
                                if *framework {
                                    mk_searchable_modio_tag("Framework", ui, None);
                                }
                            });
                        }
                    } else {
                        if ui
                            .button("📋")
                            .on_hover_text_at_pointer("copy URL")
                            .clicked()
                        {
                            ui.output_mut(|o| o.copied_text = item.item.spec.url.to_owned());
                        }
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
        if let Some(add_deps) = add_deps {
            self.add_mods(ui.ctx(), add_deps, true);
        }
        if needs_save {
            self.state.mod_data.save().unwrap();
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

    fn add_mods(&mut self, ctx: &egui::Context, specs: Vec<ModSpecification>, is_dependency: bool) {
        let rid = self.request_counter.next();
        let store = self.state.store.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = store.resolve_mods(&specs, false).await;
            tx.send(message::Message::ResolveMods {
                rid,
                specs,
                result,
                is_dependency,
            })
            .await
            .unwrap();
            ctx.request_repaint();
        });
        self.last_action_status = LastActionStatus::Idle;
        self.resolve_mod_rid = Some(rid);
    }

    fn check_updates(&mut self, ctx: &egui::Context) {
        let rid = self.request_counter.next();
        let tx = self.tx.clone();
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

        tokio::spawn(async move {
            tx.send(message::Message::CheckUpdates(rid, req().await))
                .await
                .unwrap();
            ctx.request_repaint();
        });
        self.check_updates_rid = Some(rid);
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
                            if ui.button("Browse").clicked() {
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
                        if ui.add_enabled(window.drg_pak_path_err.is_none(), egui::Button::new("Save")).clicked() {
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

    fn mk_name_popup(
        state: &mut State,
        popup: &mut NamePopup,
        ui: &egui::Ui,
        popup_id: egui::Id,
        response: egui::Response,
        default_name: impl Fn(&mut State) -> String,
        name_exists: impl Fn(&mut State, &str) -> bool,
        hint_text: &str,
        accept: impl Fn(&mut State, String),
    ) {
        popup.buffer_needs_prefill_and_focus = custom_popup_above_or_below_widget(
            ui,
            popup_id,
            &response,
            egui::AboveOrBelow::Below,
            |ui| {
                ui.set_min_width(200.0);
                ui.vertical(|ui| {
                    if popup.buffer_needs_prefill_and_focus {
                        popup.buffer = default_name(state);
                    }

                    let res =
                        ui.add(egui::TextEdit::singleline(&mut popup.buffer).hint_text(hint_text));
                    if popup.buffer_needs_prefill_and_focus {
                        res.request_focus();
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            ui.memory_mut(|mem| mem.close_popup());
                        }

                        let invalid_name =
                            popup.buffer.is_empty() || name_exists(state, &popup.buffer);
                        let clicked = ui
                            .add_enabled(!invalid_name, egui::Button::new("OK"))
                            .clicked();
                        if !invalid_name && (clicked || is_committed(&res)) {
                            ui.memory_mut(|mem| mem.close_popup());
                            accept(state, std::mem::take(&mut popup.buffer));
                            state.mod_data.save().unwrap();
                        }
                    });
                });
            },
        )
        .is_none();
    }

    fn mk_mod_groups_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(egui::RichText::new("Mod Groups").font(egui::FontId::proportional(16.0)));
        ui.set_enabled(self.integrate_rid.is_none() && self.update_rid.is_none());
        ui.horizontal(|ui| {
            ui.add_enabled_ui(
                self.state
                    .mod_data
                    .groups
                    .contains_key(&self.state.mod_data.active_group)
                    && self.state.mod_data.groups.len() > 0,
                |ui| {
                    if ui
                        .button(" ➖ ")
                        .on_hover_text_at_pointer("Delete mod group")
                        .clicked()
                    {
                        let active = self.state.mod_data.active_group.clone();
                        self.state.mod_data.remove_active_group();
                        self.state.mod_data.save().unwrap();

                        for profile in self.state.mod_data.profiles.values_mut() {
                            profile.mod_groups.remove(&active);
                        }
                        self.state.mod_data.save().unwrap();
                    }
                },
            );
            ui.add_enabled_ui(true, |ui| {
                let response = ui
                    .button(" ➕ ")
                    .on_hover_text_at_pointer("Add new mod group");
                let popup_id = ui.make_persistent_id("add-mod-group-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.add_mod_group_popup,
                    ui,
                    popup_id,
                    response,
                    |_state| String::new(),
                    |state, name| state.mod_data.groups.contains_key(name),
                    "Enter new mod group name",
                    |state, name| {
                        state.mod_data.groups.entry(name.clone()).or_default();
                        state.mod_data.active_group = name.clone();

                        for profile in state.mod_data.profiles.values_mut() {
                            profile.mod_groups.insert(name.clone(), false);
                        }
                        state.mod_data.save().unwrap();
                    },
                );
            });

            ui.add_enabled_ui(true, |ui| {
                let response = ui
                    .button("Rename")
                    .on_hover_text_at_pointer("Edit mod group name");
                let popup_id = ui.make_persistent_id("edit-mod-group-name-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.rename_mod_group_popup,
                    ui,
                    popup_id,
                    response,
                    |state| state.mod_data.active_group.clone(),
                    |state, name| state.mod_data.groups.contains_key(name),
                    "Enter new mod group name",
                    |state, name| {
                        let mod_group_to_remove = state.mod_data.active_group.clone();
                        let enabled = state.mod_data.groups.remove(&mod_group_to_remove).unwrap();
                        state.mod_data.groups.insert(name.clone(), enabled);
                        for (_, profile) in &mut state.mod_data.profiles {
                            let enabled = profile.mod_groups.remove(&mod_group_to_remove).unwrap();
                            profile.mod_groups.insert(name.clone(), enabled);
                        }
                        state.mod_data.active_group = name;
                        state.mod_data.save().unwrap();
                    },
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                let response = ui
                    .button("🗐")
                    .on_hover_text_at_pointer("Duplicate mod group");
                let popup_id = ui.make_persistent_id("duplicate-mod-group-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.duplicate_mod_group_popup,
                    ui,
                    popup_id,
                    response,
                    |state| format!("{} - Copy", state.mod_data.active_profile),
                    |state, name| state.mod_data.groups.contains_key(name),
                    "Enter new mod group name",
                    |state, name| {
                        let mod_group = state.mod_data.groups[&state.mod_data.active_group].clone();
                        state.mod_data.groups.insert(name.clone(), mod_group);
                        for (_, profile) in &mut state.mod_data.profiles {
                            profile.mod_groups.insert(name.clone(), false);
                        }
                        state.mod_data.active_group = name;
                        state.mod_data.save().unwrap();
                    },
                );

                if ui
                    .button("📋")
                    .on_hover_text_at_pointer("Copy mod group mods")
                    .clicked()
                {
                    let mods = Self::build_mod_string(&self.state.mod_data.get_active_group().mods);
                    ui.output_mut(|o| o.copied_text = mods);
                }
                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    let ModData {
                        groups,
                        ref mut active_group,
                        ..
                    } = self.state.mod_data.deref_mut();
                    let res = egui::ComboBox::from_id_source("mod-group-dropdown")
                        .width(ui.available_width())
                        .selected_text(active_group.clone())
                        .show_ui(ui, |ui| {
                            groups.keys().for_each(|k| {
                                ui.selectable_value(active_group, k.to_string(), k);
                            })
                        });
                    if res.response.changed() {
                        self.state.mod_data.save().unwrap();
                    }
                });
            });
        });

        ui.separator();

        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            if self.resolve_mod_rid.is_some() {
                ui.spinner();
            }
            ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                // define multiline layouter to be able to show multiple lines in a single line widget
                let font_id = FontSelection::default().resolve(ui.style());
                let text_color = ui.visuals().widgets.inactive.text_color();
                let mut multiline_layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
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
                        .hint_text("Add mods..."),
                );
                if is_committed(&resolve) {
                    self.add_mods(ctx, self.parse_mods());
                }
            });
        });

        self.ui_mod_group(ui);

        if let Some(search_string) = &mut self.search_string {
            let lower = search_string.to_lowercase();
            let mod_group = self.state.mod_data.get_active_group();
            let any_matches = mod_group.mods.iter().any(|m| {
                self.state
                    .store
                    .get_mod_info(&m.spec)
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
                self.add_mods(ctx, self.parse_mods());
            }
            for e in &i.events {
                match e {
                    egui::Event::Paste(s) => {
                        if self.integrate_rid.is_none()
                            && self.update_rid.is_none()
                            && ctx.memory(|m| m.focus().is_none())
                        {
                            self.resolve_mod = s.trim().to_string();
                            self.add_mods(ctx, self.parse_mods());
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
    }

    fn mk_profiles_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(egui::RichText::new("Profiles").font(egui::FontId::proportional(16.0)));
        self.ui_profile(ui, ctx);
    }

    fn ui_profile(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
            ui.push_id("profile-scrollarea", |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.ui_profile_table(ui, ctx);
                });
            });
        });
    }

    fn ui_profile_table(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(
                self.state
                    .mod_data
                    .profiles
                    .contains_key(&self.state.mod_data.active_profile)
                    && self.state.mod_data.profiles.len() > 1,
                |ui| {
                    if ui
                        .button(" ➖ ")
                        .on_hover_text_at_pointer("Delete profile")
                        .clicked()
                    {
                        self.state.mod_data.remove_active_profile();
                        self.state.mod_data.save().unwrap();
                    }
                },
            );

            ui.add_enabled_ui(true, |ui| {
                let response = ui
                    .button(" ➕ ")
                    .on_hover_text_at_pointer("Add new profile");
                let popup_id = ui.make_persistent_id("add-profile-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.add_profile_popup,
                    ui,
                    popup_id,
                    response,
                    |_state| String::new(),
                    |state, name| state.mod_data.profiles.contains_key(name),
                    "Enter new profile name",
                    |state, name| {
                        let ModData {
                            profiles, groups, ..
                        } = state.mod_data.deref_mut();

                        profiles.entry(name.clone()).or_insert_with(|| {
                            let mut mod_groups = IndexMap::new();
                            for mg in groups.keys() {
                                mod_groups.insert(mg.clone(), false);
                            }
                            ModProfile { mod_groups }
                        });
                        state.mod_data.active_profile = name.clone();
                        state.mod_data.save().unwrap();
                    },
                );
            });

            ui.add_enabled_ui(true, |ui| {
                let response = ui
                    .button("Rename")
                    .on_hover_text_at_pointer("Edit profile name");
                let popup_id = ui.make_persistent_id("edit-profile-name-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.rename_profile_popup,
                    ui,
                    popup_id,
                    response,
                    |state| state.mod_data.active_profile.clone(),
                    |state, name| state.mod_data.profiles.contains_key(name),
                    "Enter new profile name",
                    |state, name| {
                        let profile_to_remove = state.mod_data.active_profile.clone();
                        let profile = state.mod_data.profiles.remove(&profile_to_remove).unwrap();
                        state.mod_data.profiles.insert(name.clone(), profile);
                        state.mod_data.active_profile = name;
                        state.mod_data.save().unwrap();
                    },
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                let response = ui.button("🗐").on_hover_text_at_pointer("Duplicate profile");
                let popup_id = ui.make_persistent_id("duplicate-profile-popup");
                if response.clicked() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                }
                Self::mk_name_popup(
                    &mut self.state,
                    &mut self.duplicate_profile_popup,
                    ui,
                    popup_id,
                    response,
                    |state| format!("{} - Copy", state.mod_data.active_profile),
                    |state, name| state.mod_data.profiles.contains_key(name),
                    "Enter new profile name",
                    |state, name| {
                        let profile =
                            state.mod_data.profiles[&state.mod_data.active_profile].clone();
                        state.mod_data.profiles.insert(name.clone(), profile);
                        state.mod_data.active_profile = name;
                        state.mod_data.save().unwrap();
                    },
                );

                if ui
                    .button("📋")
                    .on_hover_text_at_pointer("Copy profile mods")
                    .clicked()
                {
                    let mut mods = Vec::new();
                    let mod_groups = &self.state.mod_data.profiles
                        [&self.state.mod_data.active_profile]
                        .mod_groups;
                    for (mod_group, enabled) in mod_groups {
                        if *enabled {
                            let mods_inner = &self.state.mod_data.groups[mod_group].mods;
                            for r#mod in mods_inner {
                                if r#mod.enabled {
                                    mods.push(r#mod.clone());
                                }
                            }
                        }
                    }

                    let mods = Self::build_mod_string(&mods);
                    ui.output_mut(|o| o.copied_text = mods);
                }
                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    let ModData {
                        profiles,
                        ref mut active_profile,
                        ..
                    } = self.state.mod_data.deref_mut();
                    let res = egui::ComboBox::from_id_source("profile-dropdown")
                        .width(ui.available_width())
                        .selected_text(active_profile.clone())
                        .show_ui(ui, |ui| {
                            profiles.keys().for_each(|k| {
                                ui.selectable_value(active_profile, k.to_string(), k);
                            })
                        });
                    if res.response.changed() {
                        self.state.mod_data.save().unwrap();
                    }
                });
            });
        });

        ui.separator();

        let active_profile = &self.state.mod_data.active_profile.clone();
        let mod_groups = &mut self
            .state
            .mod_data
            .profiles
            .get_mut(active_profile)
            .unwrap()
            .mod_groups;

        struct DndItem<'item> {
            index: usize,
            item: (&'item String, &'item mut bool),
        }

        impl<'item> DragDropItem for DndItem<'item> {
            fn id(&self) -> egui::Id {
                egui::Id::new(format!("profile-dnd {}", self.index))
            }
        }

        let mut items = mod_groups
            .iter_mut()
            .enumerate()
            .map(|(index, item)| DndItem { index, item })
            .collect::<Vec<_>>();

        let mut needs_save = false;
        let res = self
            .profile_dnd
            .ui::<DndItem>(ui, items.iter_mut(), |item, ui, handle| {
                ui.horizontal(|ui| {
                    handle.ui(ui, item, |ui| {
                        ui.label("☰");
                    });

                    if ui
                        .add(egui::Checkbox::without_text(&mut item.item.1))
                        .on_hover_text_at_pointer("Enabled?")
                        .changed()
                    {
                        needs_save = true;
                    }

                    ui.label(item.item.0);
                });
            });

        if let Some(response) = res.completed {
            let mut vec = mod_groups
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>();
            shift_vec(response.from, response.to, &mut vec);
            mod_groups.clear();
            mod_groups.extend(vec);
        }

        if needs_save {
            self.state.mod_data.save().unwrap();
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
            self.check_updates(ctx);
        }

        // message handling
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                message::Message::ResolveMods {
                    rid,
                    specs: mods,
                    result,
                    is_dependency,
                } => {
                    if Some(rid) == self.resolve_mod_rid {
                        match result {
                            Ok(resolved_mods) => {
                                let mod_group = self.state.mod_data.get_active_group_mut();
                                let primary_mods =
                                    mods.into_iter().collect::<HashSet<ModSpecification>>();
                                for (resolved_spec, info) in resolved_mods {
                                    let is_dep =
                                        is_dependency || !primary_mods.contains(&resolved_spec);
                                    let add = if is_dep {
                                        // if mod is a dependency then check if there is a disabled
                                        // mod that satisfies the dependency and enable it. if it
                                        // is not a dependency then assume the user explicitly
                                        // wants to add a specific mod version
                                        !profile.mods.iter_mut().any(|m| {
                                            if m.spec.satisfies_dependency(&resolved_spec) {
                                                m.enabled = true;
                                                true
                                            } else {
                                                false
                                            }
                                        })
                                    } else {
                                        true
                                    };
                                    if add {
                                        mod_group.mods.insert(
                                            0,
                                            ModConfig {
                                                spec: info.spec,
                                                required: info.suggested_require,
                                                enabled: true,
                                            },
                                        );
                                    }
                                }
                                self.resolve_mod.clear();
                                self.state.mod_data.save().unwrap();
                                self.last_action_status = LastActionStatus::Success(
                                    "mods successfully resolved".to_string(),
                                );
                            }
                            Err(e) => match e.downcast::<IntegrationError>() {
                                Ok(IntegrationError::NoProvider { url: _, factory }) => {
                                    self.window_provider_parameters =
                                        Some(WindowProviderParameters::new(factory, &self.state));
                                    self.last_action_status =
                                        LastActionStatus::Failure("no provider".to_string());
                                }
                                Err(e) => {
                                    self.log.println(format!("{:#?}\n{}", e, e.backtrace()));
                                    self.last_action_status =
                                        LastActionStatus::Failure(e.to_string());
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
                                self.log.println("Integration complete");
                                self.last_action_status =
                                    LastActionStatus::Success("integration complete".to_string());
                            }
                            Err(e) => match e.downcast::<IntegrationError>() {
                                Ok(IntegrationError::NoProvider { url: _, factory }) => {
                                    self.window_provider_parameters =
                                        Some(WindowProviderParameters::new(factory, &self.state));
                                    self.last_action_status =
                                        LastActionStatus::Failure("no provider".to_string());
                                }
                                Err(e) => {
                                    self.log.println(format!("{:#?}\n{}", e, e.backtrace()));
                                    self.last_action_status =
                                        LastActionStatus::Failure(e.to_string());
                                }
                            },
                        }
                        self.integrate_rid = None;
                    }
                }
                message::Message::UpdateCache(rid, res) => {
                    if self.update_rid.as_ref().map_or(false, |r| rid == r.0) {
                        match res {
                            Ok(()) => {
                                self.log.println("Cache update complete");
                                self.last_action_status = LastActionStatus::Success(
                                    "successfully updated cache".to_string(),
                                );
                            }
                            Err(e) => match e.downcast::<IntegrationError>() {
                                // TODO make provider initializing more generic
                                Ok(IntegrationError::NoProvider { url: _, factory }) => {
                                    self.window_provider_parameters =
                                        Some(WindowProviderParameters::new(factory, &self.state));
                                    self.last_action_status =
                                        LastActionStatus::Failure("no provider".to_string());
                                }
                                Err(e) => {
                                    self.log.println(format!("{:#?}", e));
                                    self.last_action_status =
                                        LastActionStatus::Failure(e.to_string());
                                }
                            },
                        }
                        self.update_rid = None;
                    }
                }
                message::Message::CheckUpdates(rid, res) => {
                    if self.check_updates_rid == Some(rid) {
                        self.check_updates_rid = None;
                        if let Ok(release) = res {
                            if let (Ok(version), Some(Ok(release_version))) = (
                                semver::Version::parse(env!("CARGO_PKG_VERSION")),
                                release
                                    .tag_name
                                    .strip_prefix('v')
                                    .map(semver::Version::parse),
                            ) {
                                if release_version > version {
                                    self.available_update = Some(release);
                                }
                            }
                        }
                    }
                }
            }
        }

        // begin draw

        self.show_provider_parameters(ctx);
        self.show_settings(ctx);

        egui::TopBottomPanel::bottom("bottom-panel").show(ctx, |ui| {
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
                            if button.clicked() {
                                self.last_action_status = LastActionStatus::Idle;

                                let mut mod_specs = Vec::new();
                                let profile = &self.state.mod_data.profiles[&self.state.mod_data.active_profile];
                                for mod_group in profile.mod_groups.iter().filter(|(_, enabled)| **enabled).map(|(mod_group, _)| mod_group) {
                                    mod_specs.extend(
                                        self.state
                                            .mod_data
                                            .groups[mod_group]
                                            .mods
                                            .iter()
                                            .filter_map(|m| m.enabled.then(|| m.spec.clone()))
                                    );
                                }

                                self.integrate_rid = integrate(
                                    &mut self.request_counter,
                                    self.state.store.clone(),
                                    mod_specs,
                                    self.state.config.drg_pak_path.as_ref().unwrap().clone(),
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
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
                                    let mut mods = HashSet::new();
                                    for (mod_group_name, enabled) in &self.state.mod_data.get_active_profile().mod_groups {
                                        if *enabled {
                                            for ModConfig { enabled, spec, .. } in &self.state.mod_data.groups[mod_group_name].mods {
                                                if let Some(modio_id) = enabled.then(|| self.state.store.get_mod_info(spec).and_then(|i| i.modio_id)).flatten() {
                                                    mods.insert(modio_id);
                                                }
                                            }
                                        }
                                    }

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
                            let mut mod_specs = Vec::new();
                            let profile = &self.state.mod_data.profiles[&self.state.mod_data.active_profile];
                            for mod_group in profile.mod_groups.iter().filter(|(_, enabled)| **enabled).map(|(mod_group, _)| mod_group) {
                                mod_specs.extend(
                                    self.state
                                        .mod_data
                                        .groups[mod_group]
                                        .mods
                                        .iter()
                                        .filter_map(|m| m.enabled.then(|| m.spec.clone()))
                                );
                            }
                            let store = self.state.store.clone();

                            let rid = self.request_counter.next();
                            let tx = self.tx.clone();
                            let handle = tokio::spawn(async move {
                                let res = store.resolve_mods(&mod_specs, true).await.map(|_| ());
                                tx.send(message::Message::UpdateCache(rid, res))
                                    .await
                                    .unwrap();
                            });
                            self.last_action_status = LastActionStatus::Idle;
                            self.update_rid = Some((rid, handle));
                        }
                    },
                );
                if self.integrate_rid.is_some() {
                    if ui.button("Cancel").clicked() {
                        self.integrate_rid.take().unwrap().1.abort();
                    }
                    ui.spinner();
                }
                if self.update_rid.is_some() {
                    if ui.button("Cancel").clicked() {
                        self.update_rid.take().unwrap().1.abort();
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
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    self.state
                        .profiles
                        .profiles
                        .contains_key(&self.state.profiles.active_profile)
                        && self.state.profiles.profiles.len() > 1,
                    |ui| {
                        if ui
                            .button(" ➖ ")
                            .on_hover_text_at_pointer("Delete profile")
                            .clicked()
                        {
                            self.state.profiles.remove_active();
                            self.state.profiles.save().unwrap();
                        }
                    },
                );
                ui.add_enabled_ui(true, |ui| {
                    let response = ui
                        .button(" ➕ ")
                        .on_hover_text_at_pointer("Add new profile");
                    let popup_id = ui.make_persistent_id("add-profile-popup");
                    if response.clicked() {
                        ui.memory_mut(|mem| mem.open_popup(popup_id));
                    }
                    Self::mk_profile_name_popup(
                        &mut self.state,
                        &mut self.add_profile_popup,
                        ui,
                        popup_id,
                        response,
                        |_state| String::new(),
                        |state, name| {
                            state.profiles.profiles.entry(name.clone()).or_default();
                            state.profiles.active_profile = name;
                        },
                    );
                });

                ui.add_enabled_ui(true, |ui| {
                    let response = ui
                        .button("Rename")
                        .on_hover_text_at_pointer("Edit profile name");
                    let popup_id = ui.make_persistent_id("edit-profile-name-popup");
                    if response.clicked() {
                        ui.memory_mut(|mem| mem.open_popup(popup_id));
                    }
                    Self::mk_profile_name_popup(
                        &mut self.state,
                        &mut self.rename_profile_popup,
                        ui,
                        popup_id,
                        response,
                        |state| state.profiles.active_profile.clone(),
                        |state, name| {
                            let profile_to_remove = state.profiles.active_profile.clone();
                            let profile =
                                state.profiles.profiles.remove(&profile_to_remove).unwrap();
                            state.profiles.profiles.insert(name.clone(), profile);
                            state.profiles.active_profile = name;
                        },
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    let response = ui.button("🗐").on_hover_text_at_pointer("duplicate profile");
                    let popup_id = ui.make_persistent_id("duplicate-profile-popup");
                    if response.clicked() {
                        ui.memory_mut(|mem| mem.open_popup(popup_id));
                    }
                    Self::mk_profile_name_popup(
                        &mut self.state,
                        &mut self.duplicate_profile_popup,
                        ui,
                        popup_id,
                        response,
                        |state| format!("{} - Copy", state.profiles.active_profile),
                        |state, name| {
                            let active_profile_mods =
                                state.profiles.get_active_profile().mods.clone();
                            state.profiles.profiles.insert(
                                name.clone(),
                                crate::state::ModProfile {
                                    mods: active_profile_mods,
                                },
                            );
                            state.profiles.active_profile = name;
                        },
                    );

                    if ui
                        .button("📋")
                        .on_hover_text_at_pointer("copy profile mods")
                        .clicked()
                    {
                        let mods =
                            Self::build_mod_string(&self.state.profiles.get_active_profile().mods);
                        ui.output_mut(|o| o.copied_text = mods);
                    }
                    ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                        let ModProfiles {
                            profiles,
                            ref mut active_profile,
                        } = self.state.profiles.deref_mut();
                        let res = egui::ComboBox::from_id_source("profile-dropdown")
                            .width(ui.available_width())
                            .selected_text(active_profile.clone())
                            .show_ui(ui, |ui| {
                                profiles.keys().for_each(|k| {
                                    ui.selectable_value(active_profile, k.to_string(), k);
                                })
                            });
                        if res.response.changed() {
                            self.state.profiles.save().unwrap();
                        }
                    });
                });
            });

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
                        self.add_mods(ctx, self.parse_mods(), false);
                    }
                });
            });

            self.ui_profile(ui);

            if let Some(search_string) = &mut self.search_string {
                let lower = search_string.to_lowercase();
                let profile = self.state.profiles.get_active_profile();
                let any_matches = profile.mods.iter().any(|m| {
                    self.state
                        .store
                        .get_mod_info(&m.spec)
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
                    self.add_mods(ctx, self.parse_mods(), false);
                }
                for e in &i.events {
                    match e {
                        egui::Event::Paste(s) => {
                            if self.integrate_rid.is_none()
                                && self.update_rid.is_none()
                                && ctx.memory(|m| m.focus().is_none())
                            {
                                self.resolve_mod = s.trim().to_string();
                                self.add_mods(ctx, self.parse_mods(), false);
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

fn integrate(
    rc: &mut RequestCounter,
    store: Arc<ModStore>,
    mods: Vec<ModSpecification>,
    fsd_pak: PathBuf,
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
        fsd_pak: PathBuf,
        rid: RequestID,
        message_tx: Sender<message::Message>,
    ) -> Result<()> {
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
            crate::integrate::integrate(fsd_pak, to_integrate.into_iter().zip(paths).collect())
        })
        .await??;

        Ok(())
    }

    Some((
        rid,
        tokio::task::spawn(async move {
            let res = integrate(store, ctx.clone(), mods, fsd_pak, rid, tx.clone()).await;
            tx.send(message::Message::Integrate(rid, res))
                .await
                .unwrap();
            ctx.request_repaint();
        }),
        Default::default(),
    ))
}
#[derive(Default)]
struct Log {
    buffer: String,
}
impl Log {
    fn println(&mut self, msg: impl Display) {
        println!("{}", msg);
        let msg = msg.to_string();
        self.buffer.push_str(&msg);
        self.buffer.push('\n');
    }
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
