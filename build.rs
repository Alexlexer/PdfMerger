fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

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
