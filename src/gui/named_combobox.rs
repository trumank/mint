use std;

use super::{custom_popup_above_or_below_widget, is_committed};

use eframe::egui;

use crate::state::{ModData_v0_1_0 as ModData, ModProfile_v0_1_0 as ModProfile};

#[derive(Debug, Clone)]
struct NamePopup {
    buffer_needs_prefill_and_focus: bool,
    buffer: String,
}

impl Default for NamePopup {
    fn default() -> Self {
        Self {
            buffer_needs_prefill_and_focus: true,
            buffer: String::new(),
        }
    }
}

pub trait NamedEntries<E> {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn contains(&self, name: &str) -> bool;
    fn select(&mut self, name: String);
    fn selected_name(&self) -> &str;
    fn add_new(&mut self, name: &str);
    fn remove_selected(&mut self);
    fn rename_selected(&mut self, new_name: String);
    fn duplicate_selected(&mut self, new_name: String);
    fn entries<'s>(&'s mut self) -> Box<dyn Iterator<Item = (&'s String, &'s E)> + 's>;
}

impl NamedEntries<ModProfile> for ModData {
    fn len(&self) -> usize {
        self.profiles.len()
    }
    fn contains(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }
    fn select(&mut self, name: String) {
        self.active_profile = name;
    }
    fn selected_name(&self) -> &str {
        &self.active_profile
    }
    fn add_new(&mut self, name: &str) {
        self.profiles.insert(name.to_owned(), Default::default());
        self.active_profile = name.to_owned();
    }
    fn remove_selected(&mut self) {
        self.remove_active_profile();
    }
    fn rename_selected(&mut self, new_name: String) {
        let tmp = self.profiles.remove(&self.active_profile).unwrap();
        self.profiles.insert(new_name.clone(), tmp);
        self.active_profile = new_name;
    }
    fn duplicate_selected(&mut self, new_name: String) {
        let new = self.get_active_profile().clone();
        self.profiles.insert(new_name.clone(), new);
        self.active_profile = new_name;
    }
    fn entries<'s>(&'s mut self) -> Box<dyn Iterator<Item = (&'s String, &'s ModProfile)> + 's> {
        Box::new(self.profiles.iter())
    }
}

/// Render and return whether any changes were made
pub(crate) fn ui<E, N>(
    ui: &mut egui::Ui,
    name: &str,
    entries: &mut N,
    additional_ui: Option<impl FnOnce(&mut egui::Ui, &mut N)>,
) -> bool
where
    N: NamedEntries<E>,
{
    let mut modified = false;
    ui.push_id(name, |ui| {
        ui.horizontal(|ui| {
            mk_delete(ui, name, entries, &mut modified);
            mk_add(ui, name, entries, &mut modified);
            mk_rename(ui, name, entries, &mut modified);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                mk_duplicate(ui, name, entries, &mut modified);

                if let Some(additional_ui) = additional_ui {
                    additional_ui(ui, entries);
                }

                ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                    mk_dropdown(ui, name, entries, &mut modified);
                });
            });
        });
    });
    modified
}

fn mk_delete<E, N>(ui: &mut egui::Ui, name: &str, entries: &mut N, modified: &mut bool)
where
    N: NamedEntries<E>,
{
    ui.add_enabled_ui(entries.len() > 1, |ui| {
        if ui
            .button(" ➖ ")
            .on_hover_text_at_pointer(format!("Delete {name}"))
            .clicked()
        {
            entries.remove_selected();
            *modified = true;
        }
    });
}

fn mk_add<E, N>(ui: &mut egui::Ui, name: &str, entries: &mut N, modified: &mut bool)
where
    N: NamedEntries<E>,
{
    ui.add_enabled_ui(true, |ui| {
        let response = ui
            .button(" ➕ ")
            .on_hover_text_at_pointer(format!("Add new {name}"));
        let popup_id = ui.make_persistent_id(format!("add-{name}"));
        if response.clicked() {
            ui.memory_mut(|mem| mem.open_popup(popup_id));
        }
        mk_name_popup(
            entries,
            ui,
            name,
            popup_id,
            response,
            |_state| String::new(),
            |entries, name| {
                entries.add_new(&name);
                *modified = true;
            },
        );
    });
}

fn mk_rename<E, N>(ui: &mut egui::Ui, name: &str, entries: &mut N, modified: &mut bool)
where
    N: NamedEntries<E>,
{
    ui.add_enabled_ui(true, |ui| {
        let response = ui
            .button("Rename")
            .on_hover_text_at_pointer(format!("Rename {name}"));
        let popup_id = ui.make_persistent_id(format!("rename-{name}"));
        if response.clicked() {
            ui.memory_mut(|mem| mem.open_popup(popup_id));
        }
        mk_name_popup(
            entries,
            ui,
            name,
            popup_id,
            response,
            |entries| entries.selected_name().to_string(),
            |entries, name| {
                entries.rename_selected(name);
                *modified = true;
            },
        );
    });
}

fn mk_duplicate<E, N>(ui: &mut egui::Ui, name: &str, entries: &mut N, modified: &mut bool)
where
    N: NamedEntries<E>,
{
    let response = ui
        .button("🗐")
        .on_hover_text_at_pointer(format!("Duplicate {name}"));
    let popup_id = ui.make_persistent_id(format!("duplicate-{name}"));
    if response.clicked() {
        ui.memory_mut(|mem| mem.open_popup(popup_id));
    }
    mk_name_popup(
        entries,
        ui,
        name,
        popup_id,
        response,
        |state| format!("{} - Copy", state.selected_name()),
        |state, name| {
            state.duplicate_selected(name);
            *modified = true;
        },
    );
}

fn mk_dropdown<E, N>(ui: &mut egui::Ui, name: &str, entries: &mut N, modified: &mut bool)
where
    N: NamedEntries<E>,
{
    let mut selected = entries.selected_name().to_owned();

    egui::ComboBox::from_id_source(format!("dropdown-{name}"))
        .width(ui.available_width())
        .selected_text(selected.clone())
        .show_ui(ui, |ui| {
            entries.entries().for_each(|(k, _)| {
                ui.selectable_value(&mut selected, k.to_owned(), k);
            })
        });

    if selected != entries.selected_name() {
        entries.select(selected);
        *modified = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn mk_name_popup<E, N>(
    entries: &mut N,
    ui: &egui::Ui,
    name: &str,
    popup_id: egui::Id,
    response: egui::Response,
    default_name: impl Fn(&mut N) -> String,
    mut accept: impl FnMut(&mut N, String),
) where
    N: NamedEntries<E>,
{
    let data_id = popup_id.with("data");
    let mut popup: NamePopup = ui.data(|data| data.get_temp(data_id)).unwrap_or_default();
    popup.buffer_needs_prefill_and_focus = custom_popup_above_or_below_widget(
        ui,
        popup_id,
        &response,
        egui::AboveOrBelow::Below,
        |ui| {
            ui.set_min_width(200.0);
            ui.vertical(|ui| {
                if popup.buffer_needs_prefill_and_focus {
                    popup.buffer = default_name(entries);
                }

                let res = ui.add(
                    egui::TextEdit::singleline(&mut popup.buffer)
                        .hint_text(format!("Enter new {name} name")),
                );
                if popup.buffer_needs_prefill_and_focus {
                    res.request_focus();
                }

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        ui.memory_mut(|mem| mem.close_popup());
                    }

                    let invalid_name = popup.buffer.is_empty() || entries.contains(&popup.buffer);
                    let clicked = ui
                        .add_enabled(!invalid_name, egui::Button::new("OK"))
                        .clicked();
                    if !invalid_name && (clicked || is_committed(&res)) {
                        ui.memory_mut(|mem| mem.close_popup());
                        accept(entries, std::mem::take(&mut popup.buffer));
                    }
                });
            });
        },
    )
    .is_none();

    ui.data_mut(|data| data.insert_temp(data_id, popup));
}
