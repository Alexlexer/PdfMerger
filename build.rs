fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-gnu")) {
        println!("cargo:rustc-link-lib=advapi32");
    }

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icons/icon.ico");
        resource.set("ProductName", "PdfMerger");
        resource.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
        resource.set("InternalName", "pdf-merger");
        resource.set("OriginalFilename", "pdf-merger.exe");
        resource.set("LegalCopyright", "Copyright (c) 2026 Alexlexer");
        resource
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
