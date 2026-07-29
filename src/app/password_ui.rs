use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};

use eframe::egui::{self, Color32, RichText};
use pdf_merger::document::PdfAccessError;
use zeroize::{Zeroize, Zeroizing};

use super::PdfMergerApp;

#[derive(Clone, Debug)]
pub(crate) enum PasswordPurpose {
    Import,
    OpenProject(PathBuf),
}

#[derive(Clone, Debug)]
pub(crate) struct PasswordRequest {
    pub path: PathBuf,
    pub error: PdfAccessError,
    pub purpose: PasswordPurpose,
}

#[derive(Default)]
pub(crate) struct PasswordPromptState {
    queue: VecDeque<PasswordRequest>,
    password: Zeroizing<String>,
}

impl PdfMergerApp {
    pub(super) fn passwords_for_worker(&self) -> HashMap<PathBuf, Zeroizing<String>> {
        self.pdf_passwords
            .iter()
            .map(|(path, password)| (path.clone(), password.clone()))
            .collect()
    }

    pub(super) fn enqueue_password_requests(
        &mut self,
        requests: impl IntoIterator<Item = PasswordRequest>,
    ) {
        for request in requests {
            self.pdf_passwords.remove(&request.path);
            if !self
                .password_prompt
                .queue
                .iter()
                .any(|queued| queued.path == request.path)
            {
                self.password_prompt.queue.push_back(request);
            }
        }
    }

    pub(super) fn show_password_prompt(&mut self, context: &egui::Context) {
        let Some(request) = self.password_prompt.queue.front().cloned() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("Unlock protected PDF")
            .id(egui::Id::new("pdf_password_prompt"))
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(context, |ui| {
                ui.label(RichText::new(request.path.display().to_string()).strong());
                ui.label(
                    RichText::new(match &request.error {
                        PdfAccessError::PasswordRequired => "Enter the PDF password.",
                        PdfAccessError::IncorrectPassword => {
                            "That password was incorrect. Please try again."
                        }
                        PdfAccessError::OwnerPasswordRequired => {
                            "This PDF forbids page assembly. Enter its owner password."
                        }
                        PdfAccessError::UnsupportedEncryption(error) => error,
                    })
                    .color(
                        if matches!(request.error, PdfAccessError::PasswordRequired) {
                            Color32::from_gray(190)
                        } else {
                            Color32::from_rgb(244, 118, 118)
                        },
                    ),
                );
                ui.add_space(8.0);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut *self.password_prompt.password)
                        .password(true)
                        .hint_text("Password")
                        .desired_width(f32::INFINITY),
                );
                ui.label(
                    RichText::new(
                        "The password is kept only in memory for this application session.",
                    )
                    .small()
                    .color(Color32::from_gray(145)),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button(RichText::new("Unlock").strong()).clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    {
                        submit = true;
                    }
                });
            });

        if cancel {
            self.password_prompt.queue.pop_front();
            self.password_prompt.password.zeroize();
            self.set_status(
                format!("Skipped protected PDF {}.", request.path.display()),
                true,
            );
        } else if submit {
            self.password_prompt.queue.pop_front();
            let password = std::mem::take(&mut *self.password_prompt.password);
            self.pdf_passwords
                .insert(request.path.clone(), Zeroizing::new(password));
            match request.purpose {
                PasswordPurpose::Import => self.start_import(vec![request.path], context),
                PasswordPurpose::OpenProject(project_path) => {
                    self.start_project_open(project_path, context)
                }
            }
        }
    }
}
