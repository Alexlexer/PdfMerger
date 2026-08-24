use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    thread,
};

use eframe::egui;
use pdf_merger::{document, model::PreviewData};

use super::{AppMessage, PdfMergerApp};

const MAX_CACHED_PREVIEWS: usize = 96;
const MAX_PENDING_PREVIEWS: usize = 16;

#[derive(Clone)]
pub(super) struct PreviewRequest {
    pub id: u64,
    pub path: PathBuf,
    pub page_number: u32,
}

impl PdfMergerApp {
    pub(super) fn request_pdf_previews(
        &mut self,
        requests: Vec<PreviewRequest>,
        context: &egui::Context,
    ) {
        let available = MAX_PENDING_PREVIEWS.saturating_sub(self.pending_pdf_previews.len());
        if available == 0 {
            return;
        }

        let mut seen = HashSet::new();
        let requests = requests
            .into_iter()
            .filter(|request| seen.insert(request.id))
            .filter(|request| {
                !self.pdf_previews.contains_key(&request.id)
                    && !self.pending_pdf_previews.contains(&request.id)
                    && !self.failed_pdf_previews.contains(&request.id)
            })
            .take(available)
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return;
        }

        for request in &requests {
            self.pending_pdf_previews.insert(request.id);
        }
        let mut grouped = HashMap::<PathBuf, Vec<(u64, u32)>>::new();
        for request in requests {
            grouped
                .entry(request.path)
                .or_default()
                .push((request.id, request.page_number));
        }
        let batches = grouped
            .into_iter()
            .map(|(path, pages)| {
                let password = self
                    .pdf_passwords
                    .get(&path)
                    .map(|password| password.to_string());
                (path, password, pages)
            })
            .collect::<Vec<_>>();
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let mut results = Vec::new();
            for (path, password, pages) in batches {
                let page_numbers = pages.iter().map(|(_, page)| *page).collect::<Vec<_>>();
                match document::render_pdf_previews(&path, password.as_deref(), &page_numbers) {
                    Ok(rendered) => {
                        let rendered = rendered.into_iter().collect::<HashMap<_, _>>();
                        for (id, page_number) in pages {
                            let result = rendered
                                .get(&page_number)
                                .cloned()
                                .ok_or_else(|| format!("could not render PDF page {page_number}"));
                            results.push((id, result));
                        }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        results.extend(pages.into_iter().map(|(id, _)| (id, Err(error.clone()))));
                    }
                }
            }
            let _ = sender.send(AppMessage::PdfPreviewsReady { results });
            repaint.request_repaint();
        });
    }

    pub(super) fn receive_pdf_previews(
        &mut self,
        results: Vec<(u64, Result<PreviewData, String>)>,
    ) {
        let existing = self
            .workspace
            .pages()
            .iter()
            .map(|page| page.id)
            .collect::<HashSet<_>>();
        for (id, result) in results {
            self.pending_pdf_previews.remove(&id);
            match result {
                Ok(preview) if existing.contains(&id) => {
                    self.pdf_previews.insert(id, preview);
                    self.pdf_preview_order.retain(|cached| *cached != id);
                    self.pdf_preview_order.push_back(id);
                }
                Err(_) => {
                    self.failed_pdf_previews.insert(id);
                }
                Ok(_) => {}
            }
        }
        while self.pdf_preview_order.len() > MAX_CACHED_PREVIEWS {
            if let Some(id) = self.pdf_preview_order.pop_front() {
                self.pdf_previews.remove(&id);
                self.preview_textures.remove(&id);
            }
        }
    }
}

pub(super) type PreviewCache = HashMap<u64, PreviewData>;
pub(super) type PreviewOrder = VecDeque<u64>;
