#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::ffi::OsStr;

use app::PdfMergerApp;
use eframe::egui;

const APP_NAME: &str = "PdfMerger";
const SMOKE_TEST_ARGUMENT: &str = "--smoke-test";

// llama-cpp-sys 0.1.154 omits these C++ build-info symbols on Windows GNU targets.
// Keeping the shim here places it after dependency archives during final linking.
#[cfg(all(target_os = "windows", target_env = "gnu"))]
mod windows_llama_build_info {
    use std::ffi::c_char;

    #[unsafe(export_name = "_Z18llama_build_numberv")]
    pub extern "C" fn build_number() -> i32 {
        0
    }

    #[unsafe(export_name = "_Z12llama_commitv")]
    pub extern "C" fn commit() -> *const c_char {
        c"embedded".as_ptr()
    }

    #[unsafe(export_name = "_Z14llama_compilerv")]
    pub extern "C" fn compiler() -> *const c_char {
        c"mingw-w64".as_ptr()
    }

    #[unsafe(export_name = "_Z18llama_build_targetv")]
    pub extern "C" fn build_target() -> *const c_char {
        c"x86_64-windows-gnu".as_ptr()
    }
}

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
