use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke,
    Vec2,
};

use pdf_merger::{
    document::{self, ExportReport},
    model::{PageDraft, PageItem, Workspace},
};

const ACCENT: Color32 = Color32::from_rgb(94, 106, 210);
const CARD_WIDTH: f32 = 166.0;
const CARD_MARGIN: f32 = 10.0;
const CARD_SPACING: f32 = 10.0;
const CARD_OUTER_WIDTH: f32 = CARD_WIDTH + CARD_MARGIN * 2.0;
const PREVIEW_SIZE: Vec2 = Vec2::new(CARD_WIDTH, 221.0);

enum AppMessage {
    ImportFinished {
        files: usize,
        pages: Vec<PageDraft>,
        errors: Vec<String>,
    },
    ExportFinished(Result<ExportReport, String>),
}

enum CardAction {
    Remove(usize),
    MoveLeft(usize),
    MoveRight(usize),
}

pub struct PdfMergerApp {
    workspace: Workspace,
    sender: Sender<AppMessage>,
    receiver: Receiver<AppMessage>,
    active_jobs: usize,
    status: String,
    status_is_error: bool,
    preview_textures: HashMap<u64, egui::TextureHandle>,
}

impl PdfMergerApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        configure_style(&creation_context.egui_ctx);
        let (sender, receiver) = mpsc::channel();
        Self {
            workspace: Workspace::default(),
            sender,
            receiver,
            active_jobs: 0,
            status: "Drop PDFs or images here to begin.".to_owned(),
            status_is_error: false,
            preview_textures: HashMap::new(),
        }
    }

    fn receive_messages(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            self.active_jobs = self.active_jobs.saturating_sub(1);
            match message {
                AppMessage::ImportFinished {
                    files,
                    pages,
                    errors,
                } => {
                    let imported = pages.len();
                    self.workspace.append(pages);
                    if errors.is_empty() {
                        self.set_status(
                            format!("Added {files} file(s) as {imported} page(s)."),
                            false,
                        );
                    } else {
                        self.set_status(errors.join("  "), true);
                    }
                }
                AppMessage::ExportFinished(result) => match result {
                    Ok(report) => {
                        let warning_suffix = if report.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" ({} warning(s))", report.warnings.len())
                        };
                        self.set_status(
                            format!(
                                "Saved {} page(s) to {}{warning_suffix}",
                                report.page_count,
                                report.path.display()
                            ),
                            false,
                        );
                    }
                    Err(error) => self.set_status(error, true),
                },
            }
        }
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = message.into();
        self.status_is_error = is_error;
    }

    fn choose_files(&mut self, context: &egui::Context) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "PDFs and images",
                &[
                    "pdf", "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff",
                ],
            )
            .pick_files()
        {
            self.start_import(paths, context);
        }
    }

    fn start_import(&mut self, paths: Vec<PathBuf>, context: &egui::Context) {
        let mut unique_paths = HashSet::new();
        let paths = paths
            .into_iter()
            .filter(|path| document::is_supported(path))
            .filter(|path| unique_paths.insert(path.clone()))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.set_status("No supported PDF or image files were selected.", true);
            return;
        }

        let file_count = paths.len();
        self.active_jobs += 1;
        self.set_status(format!("Importing {file_count} file(s)…"), false);
        let sender = self.sender.clone();
        let context = context.clone();
        thread::spawn(move || {
            let mut pages = Vec::new();
            let mut errors = Vec::new();
            for path in paths {
                match document::import_file(&path) {
                    Ok(mut imported) => pages.append(&mut imported),
                    Err(error) => errors.push(format!("{}: {error:#}", path.display())),
                }
            }
            let _ = sender.send(AppMessage::ImportFinished {
                files: file_count,
                pages,
                errors,
            });
            context.request_repaint();
        });
    }

    fn choose_export_path(&mut self, context: &egui::Context) {
        if self.workspace.is_empty() {
            self.set_status("Add at least one page before exporting.", true);
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF document", &["pdf"])
            .set_file_name("merged.pdf")
            .save_file()
        else {
            return;
        };

        let path = if path.extension().is_none() {
            path.with_extension("pdf")
        } else {
            path
        };
        let pages = self.workspace.pages().to_vec();
        let sender = self.sender.clone();
        let context = context.clone();
        self.active_jobs += 1;
        self.set_status("Building PDF…", false);
        thread::spawn(move || {
            let result =
                document::export_pages(&pages, &path).map_err(|error| format!("{error:#}"));
            let _ = sender.send(AppMessage::ExportFinished(result));
            context.request_repaint();
        });
    }

    fn top_bar(&mut self, root_ui: &mut egui::Ui, context: &egui::Context) {
        egui::Panel::top("top_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(24, 27, 35))
                    .inner_margin(Margin::symmetric(22, 14)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(" P ")
                            .size(22.0)
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(ACCENT),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new("PdfMerger").size(21.0).strong());
                    ui.label(
                        RichText::new("native · private · offline")
                            .small()
                            .color(Color32::from_gray(150)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let export = egui::Button::new(
                            RichText::new("Export PDF").strong().color(Color32::WHITE),
                        )
                        .fill(ACCENT)
                        .corner_radius(8);
                        if ui.add_enabled(!self.workspace.is_empty(), export).clicked() {
                            self.choose_export_path(context);
                        }

                        if ui.button("Add files").clicked() {
                            self.choose_files(context);
                        }

                        if ui
                            .add_enabled(!self.workspace.is_empty(), egui::Button::new("Clear"))
                            .clicked()
                        {
                            self.workspace.clear();
                            self.preview_textures.clear();
                            self.set_status("Workspace cleared.", false);
                        }
                    });
                });
            });
    }

    fn page_strip(&mut self, ui: &mut egui::Ui) {
        let pages = self.workspace.pages().to_vec();
        let mut card_action = None;
        let mut move_request = None;

        ScrollArea::vertical()
            .id_salt("page_strip")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let columns = ((available_width + CARD_SPACING) / (CARD_OUTER_WIDTH + CARD_SPACING))
                    .floor()
                    .max(1.0) as usize;

                egui::Grid::new("page_grid")
                    .min_col_width(CARD_OUTER_WIDTH)
                    .max_col_width(CARD_OUTER_WIDTH)
                    .spacing(Vec2::splat(CARD_SPACING))
                    .show(ui, |ui| {
                        let mut cell = 0;
                        for (index, page) in pages.iter().enumerate() {
                            let frame = Frame::new()
                                .fill(Color32::from_rgb(35, 39, 49))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(57, 63, 78)))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(Margin::same(CARD_MARGIN as i8));
                            let (_zone, dropped) = ui.dnd_drop_zone::<usize, _>(frame, |ui| {
                                ui.set_min_width(CARD_WIDTH);
                                ui.set_max_width(CARD_WIDTH);
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.set_width(CARD_WIDTH);
                                    if let Some(action) = page_card(
                                        ui,
                                        page,
                                        index,
                                        pages.len(),
                                        &mut self.preview_textures,
                                    ) {
                                        card_action = Some(action);
                                    }
                                });
                            });
                            if let Some(from) = dropped {
                                move_request = Some((*from, index));
                            }
                            cell += 1;
                            if cell % columns == 0 {
                                ui.end_row();
                            }
                        }

                        let (_end_zone, dropped) = ui.dnd_drop_zone::<usize, _>(
                            Frame::new()
                                .fill(Color32::from_rgb(28, 31, 40))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(61, 66, 80)))
                                .corner_radius(10)
                                .inner_margin(Margin::same(8)),
                            |ui| {
                                ui.set_min_size(Vec2::new(CARD_WIDTH, 64.0));
                                ui.set_max_width(CARD_WIDTH);
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("Drop at end").color(Color32::from_gray(130)),
                                    );
                                });
                            },
                        );
                        if let Some(from) = dropped {
                            move_request = Some((*from, pages.len()));
                        }
                        if (cell + 1) % columns != 0 {
                            ui.end_row();
                        }
                    });
            });

        if let Some((from, to)) = move_request {
            self.workspace.move_page(from, to);
            self.set_status("Page order updated.", false);
        }

        if let Some(action) = card_action {
            match action {
                CardAction::Remove(index) => {
                    if let Some(page) = pages.get(index) {
                        self.preview_textures.remove(&page.id);
                    }
                    self.workspace.remove(index);
                    self.set_status("Page removed.", false);
                }
                CardAction::MoveLeft(index) => {
                    self.workspace.move_page(index, index - 1);
                }
                CardAction::MoveRight(index) => {
                    self.workspace.move_page(index, index + 2);
                }
            }
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);
            ui.label(RichText::new("⇩").size(54.0).color(ACCENT));
            ui.add_space(12.0);
            ui.heading("Drop PDFs and pictures here");
            ui.label(
                RichText::new("Each PDF page and image becomes a draggable page card.")
                    .color(Color32::from_gray(160)),
            );
            ui.add_space(18.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Choose files").strong())
                        .fill(ACCENT)
                        .corner_radius(8)
                        .min_size(Vec2::new(140.0, 38.0)),
                )
                .clicked()
            {
                self.choose_files(context);
            }
            ui.add_space(12.0);
            ui.label(
                RichText::new("PDF · PNG · JPEG · WebP · BMP · GIF · TIFF")
                    .small()
                    .color(Color32::from_gray(120)),
            );
        });
    }

    fn bottom_bar(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(24, 27, 35))
                    .inner_margin(Margin::symmetric(20, 9)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    let status_color = if self.status_is_error {
                        Color32::from_rgb(244, 118, 118)
                    } else {
                        Color32::from_gray(155)
                    };
                    if self.active_jobs > 0 {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small().color(status_color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} page(s)", self.workspace.len()))
                                .small()
                                .color(Color32::from_gray(145)),
                        );
                    });
                });
            });
    }
}

impl eframe::App for PdfMergerApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root_ui.ctx().clone();
        self.receive_messages();

        let dropped_paths = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped_paths.is_empty() {
            self.start_import(dropped_paths, &context);
        }

        self.top_bar(root_ui, &context);
        self.bottom_bar(root_ui);

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(20, 23, 30))
                    .inner_margin(Margin::same(22)),
            )
            .show(root_ui, |ui| {
                if self.workspace.is_empty() {
                    self.empty_state(ui, &context);
                } else {
                    ui.horizontal(|ui| {
                        ui.heading("Arrange pages");
                        ui.label(
                            RichText::new("Drag cards to reorder them")
                                .color(Color32::from_gray(145)),
                        );
                    });
                    ui.add_space(14.0);
                    self.page_strip(ui);
                }
            });

        let hovering_files = context.input(|input| !input.raw.hovered_files.is_empty());
        if hovering_files {
            let rect = context.content_rect();
            let painter =
                context.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop")));
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(180));
            painter.rect_stroke(
                rect.shrink(24.0),
                16.0,
                Stroke::new(3.0, ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to add pages",
                egui::FontId::proportional(30.0),
                Color32::WHITE,
            );
        }
    }
}

fn page_card(
    ui: &mut egui::Ui,
    page: &PageItem,
    index: usize,
    page_count: usize,
    preview_textures: &mut HashMap<u64, egui::TextureHandle>,
) -> Option<CardAction> {
    let mut action = None;
    ui.set_min_width(CARD_WIDTH);
    ui.set_max_width(CARD_WIDTH);
    ui.set_width(CARD_WIDTH);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{:02}", index + 1))
                .strong()
                .color(ACCENT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("×")
                .on_hover_text("Remove this page")
                .clicked()
            {
                action = Some(CardAction::Remove(index));
            }
        });
    });
    ui.add_space(5.0);

    let (preview_rect, _) = ui.allocate_exact_size(PREVIEW_SIZE, egui::Sense::hover());
    ui.painter().rect_filled(preview_rect, 5.0, Color32::WHITE);
    ui.painter().rect_stroke(
        preview_rect,
        5.0,
        Stroke::new(1.0, Color32::from_gray(72)),
        egui::StrokeKind::Inside,
    );
    if let Some(preview) = &page.preview {
        let texture = preview_textures.entry(page.id).or_insert_with(|| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied(preview.size, preview.rgba.as_ref());
            ui.ctx().load_texture(
                format!("page-preview-{}", page.id),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        let source_size = Vec2::new(preview.size[0] as f32, preview.size[1] as f32);
        let scale = (PREVIEW_SIZE.x / source_size.x).min(PREVIEW_SIZE.y / source_size.y);
        let image_rect = egui::Rect::from_center_size(preview_rect.center(), source_size * scale);
        egui::Image::from_texture(&*texture)
            .fit_to_exact_size(image_rect.size())
            .paint_at(ui, image_rect);
    } else {
        ui.painter().text(
            preview_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("PDF\nPAGE {}", index + 1),
            egui::FontId::proportional(19.0),
            Color32::from_gray(60),
        );
    }

    ui.add_space(7.0);
    ui.add(
        egui::Label::new(
            RichText::new(&page.title)
                .strong()
                .color(Color32::from_gray(225)),
        )
        .truncate(),
    )
    .on_hover_text(page.source.path().display().to_string());
    ui.add(
        egui::Label::new(
            RichText::new(&page.subtitle)
                .small()
                .color(Color32::from_gray(135)),
        )
        .truncate(),
    );
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                index > 0,
                egui::Button::new(RichText::new("Back").strong()).min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page backward")
            .clicked()
        {
            action = Some(CardAction::MoveLeft(index));
        }
        if ui
            .add_enabled(
                index + 1 < page_count,
                egui::Button::new(RichText::new("Next").strong()).min_size(Vec2::new(48.0, 28.0)),
            )
            .on_hover_text("Move page forward")
            .clicked()
        {
            action = Some(CardAction::MoveRight(index));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let drag_handle = ui.add(
                egui::Label::new(
                    RichText::new("Drag")
                        .small()
                        .strong()
                        .color(Color32::from_gray(155)),
                )
                .sense(egui::Sense::drag()),
            );
            drag_handle
                .on_hover_text("Drag to reorder this page")
                .dnd_set_drag_payload(index);
        });
    });
    action
}

fn configure_style(context: &egui::Context) {
    context.set_theme(egui::Theme::Dark);
    context.set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
    context.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = Vec2::new(9.0, 8.0);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(7);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(7);
        style.visuals.widgets.active.corner_radius = CornerRadius::same(7);
    });
}
