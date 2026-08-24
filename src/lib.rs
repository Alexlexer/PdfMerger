pub mod document;
pub mod export_settings;
pub mod llama_backend;
pub mod model;
pub mod project;
pub mod split;
pub mod summarization;

// llama-cpp-sys 0.1.154 omits these C++ build-info symbols on Windows GNU targets.
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
