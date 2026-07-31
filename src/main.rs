#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::ffi::OsStr;

use app::PdfMergerApp;
use eframe::egui;

const APP_NAME: &str = "PdfMerger";
const SMOKE_TEST_ARGUMENT: &str = "--smoke-test";

fn main() -> eframe::Result {
    if smoke_test_requested(std::env::args_os().skip(1)) {
        validate_embedded_assets();
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_icon(application_icon())
            .with_inner_size([1100.0, 650.0])
            .with_min_inner_size([760.0, 520.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|creation_context| Ok(Box::new(PdfMergerApp::new(creation_context)))),
    )
}

fn smoke_test_requested(arguments: impl IntoIterator<Item = impl AsRef<OsStr>>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == OsStr::new(SMOKE_TEST_ARGUMENT))
}

fn application_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/icons/icon-256.png"))
        .expect("embedded application icon must be a valid image")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn validate_embedded_assets() {
    let icon = application_icon();
    assert_eq!((icon.width, icon.height), (256, 256));
    assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    assert!(!env!("CARGO_PKG_VERSION").is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_the_packaged_smoke_test_argument() {
        assert!(smoke_test_requested([SMOKE_TEST_ARGUMENT]));
        assert!(smoke_test_requested(["ignored", SMOKE_TEST_ARGUMENT]));
        assert!(!smoke_test_requested(["--help", "smoke-test"]));
    }

    #[test]
    fn embedded_icon_is_valid() {
        validate_embedded_assets();
    }
}
