#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::PdfMergerApp;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PdfMerger")
            .with_inner_size([1100.0, 650.0])
            .with_min_inner_size([760.0, 520.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "PdfMerger",
        options,
        Box::new(|creation_context| Ok(Box::new(PdfMergerApp::new(creation_context)))),
    )
}
