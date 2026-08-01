use eframe::egui::{self, Context, Id, Response};

use super::PdfMergerApp;

#[derive(Clone, Copy)]
pub(super) enum AnnouncementPriority {
    Polite,
    Assertive,
}

pub(super) fn mark_live(response: &Response, priority: AnnouncementPriority) {
    let live = match priority {
        AnnouncementPriority::Polite => egui::accesskit::Live::Polite,
        AnnouncementPriority::Assertive => egui::accesskit::Live::Assertive,
    };
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_live(live));
}

pub(super) fn mark_expanded(response: &Response, expanded: bool) {
    response
        .ctx
        .accesskit_node_builder(response.id, |node| node.set_expanded(expanded));
}

pub(super) fn label_button(response: &Response, label: impl Into<String>) {
    let label = label.into();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label.clone())
    });
}

pub(super) fn label_toggle(response: &Response, selected: bool, label: impl Into<String>) {
    let label = label.into();
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            response.enabled(),
            selected,
            label.clone(),
        )
    });
}

#[derive(Default)]
pub(super) struct ModalFocusState {
    was_open: bool,
    return_to: Option<Id>,
}

impl ModalFocusState {
    fn sync(&mut self, context: &Context, is_open: bool) {
        if is_open == self.was_open {
            return;
        }

        if is_open {
            self.return_to = context.memory(|memory| memory.focused());
        } else if let Some(id) = self.return_to.take() {
            context.memory_mut(|memory| memory.request_focus(id));
        }
        self.was_open = is_open;
    }
}

impl PdfMergerApp {
    pub(super) fn has_active_modal(&self) -> bool {
        self.export_dialog.is_open()
            || self.split_dialog.is_open()
            || self.project_ui.has_open_dialog()
            || self.password_prompt.is_open()
            || self.jobs.details_are_open()
    }

    pub(super) fn sync_modal_focus(&mut self, context: &Context) {
        let is_open = self.has_active_modal();
        self.modal_focus.sync(context, is_open);
    }

    pub(super) fn global_shortcuts_allowed(&self, context: &Context) -> bool {
        global_shortcuts_allowed(self.has_active_modal(), context.egui_wants_keyboard_input())
    }
}

fn global_shortcuts_allowed(modal_open: bool, wants_keyboard_input: bool) -> bool {
    !modal_open && !wants_keyboard_input
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, Id};

    use super::{ModalFocusState, global_shortcuts_allowed};

    #[test]
    fn blocks_global_shortcuts_for_modals_and_text_input() {
        assert!(global_shortcuts_allowed(false, false));
        assert!(!global_shortcuts_allowed(true, false));
        assert!(!global_shortcuts_allowed(false, true));
        assert!(!global_shortcuts_allowed(true, true));
    }

    #[test]
    fn restores_focus_after_the_modal_closes() {
        let context = Context::default();
        let opener = Id::new("opener");
        let dialog_control = Id::new("dialog_control");
        context.memory_mut(|memory| memory.request_focus(opener));
        let mut state = ModalFocusState::default();

        state.sync(&context, true);
        context.memory_mut(|memory| memory.request_focus(dialog_control));
        state.sync(&context, true);
        state.sync(&context, false);

        assert_eq!(context.memory(|memory| memory.focused()), Some(opener));
    }
}
