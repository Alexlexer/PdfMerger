use std::collections::HashSet;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use super::PdfMergerApp;

impl PdfMergerApp {
    pub(super) fn handle_shortcuts(&mut self, context: &egui::Context) {
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let command_shift = Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        };

        if context.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(command_shift, Key::O))
        }) {
            self.choose_open_project(context);
        } else if context.input_mut(|input| input.consume_shortcut(&command(Key::O))) {
            self.choose_files(context);
        }
        if context.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(command_shift, Key::S))
        }) {
            self.save_project(false);
        } else if context.input_mut(|input| input.consume_shortcut(&command(Key::S))) {
            self.choose_export_path(context);
        }
        if context.input_mut(|input| input.consume_shortcut(&command(Key::A))) {
            self.select_all();
        }
        if context.input_mut(|input| input.consume_shortcut(&command(Key::Z))) {
            self.undo();
        }
        if context.input_mut(|input| {
            input.consume_shortcut(&command(Key::Y))
                || input.consume_shortcut(&KeyboardShortcut::new(command_shift, Key::Z))
        }) {
            self.redo();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete)) {
            self.remove_selected();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::R)) {
            self.rotate_selected();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.clear_selection();
        }
    }

    pub(super) fn toggle_selection(&mut self, id: u64) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
    }

    pub(super) fn select_all(&mut self) {
        self.selected = self.workspace.pages().iter().map(|page| page.id).collect();
        if !self.selected.is_empty() {
            self.set_status(format!("Selected {} page(s).", self.selected.len()), false);
        }
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub(super) fn remove_selected(&mut self) {
        let removed = self.workspace.remove_ids(&self.selected);
        if removed > 0 {
            for id in self.selected.drain() {
                self.preview_textures.remove(&id);
            }
            self.set_status(format!("Removed {removed} page(s)."), false);
        }
    }

    pub(super) fn rotate_selected(&mut self) {
        let ids = self.selected.clone();
        self.rotate_ids(&ids);
    }

    pub(super) fn rotate_page_or_selection(&mut self, id: u64) {
        let ids = if self.selected.contains(&id) {
            self.selected.clone()
        } else {
            HashSet::from([id])
        };
        self.rotate_ids(&ids);
    }

    pub(super) fn move_selected_to_start(&mut self) {
        if self.workspace.move_ids_to_start(&self.selected) {
            self.set_status("Moved selected pages to the start.", false);
        }
    }

    pub(super) fn move_selected_to_end(&mut self) {
        if self.workspace.move_ids_to_end(&self.selected) {
            self.set_status("Moved selected pages to the end.", false);
        }
    }

    pub(super) fn undo(&mut self) {
        if self.workspace.undo() {
            self.after_history_change("Undid last change.");
        }
    }

    pub(super) fn redo(&mut self) {
        if self.workspace.redo() {
            self.after_history_change("Redid last change.");
        }
    }

    pub(super) fn retain_existing_selection(&mut self) {
        let existing = self
            .workspace
            .pages()
            .iter()
            .map(|page| page.id)
            .collect::<HashSet<_>>();
        self.selected.retain(|id| existing.contains(id));
    }

    fn rotate_ids(&mut self, ids: &HashSet<u64>) {
        let rotated = self.workspace.rotate_ids_clockwise(ids);
        if rotated > 0 {
            for id in ids {
                self.preview_textures.remove(id);
            }
            self.set_status(format!("Rotated {rotated} page(s) clockwise."), false);
        }
    }

    fn after_history_change(&mut self, status: &str) {
        self.preview_textures.clear();
        self.retain_existing_selection();
        self.set_status(status, false);
    }
}
