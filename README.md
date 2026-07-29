# PdfMerger

PdfMerger is a private, offline desktop application for visually assembling PDF
documents. It is written in Rust and runs natively on Windows, Linux, and macOS:
there is no browser UI, web server, upload, or cloud processing.

## Features

- Drop PDF files and images directly onto the application.
- See every imported PDF page as an individual card.
- Convert PNG, JPEG, WebP, BMP, GIF, and TIFF images into A4 PDF pages.
- Reorder pages with drag and drop or the arrow controls.
- Remove individual pages without changing the source files.
- Merge in the exact visible order.
- Process and save everything locally.
- Use native open/save dialogs on all supported operating systems.

## Run from source

Install the current stable Rust toolchain, then run:

```sh
cargo run --release
```

The first build downloads and compiles the Rust dependencies. No additional PDF
runtime or web server is required.

## Build

```sh
cargo test --all-targets
cargo build --release
```

The executable is written to `target/release/pdf-merger` (or
`target\release\pdf-merger.exe` on Windows).

## How export works

Existing PDF pages are merged from their original PDF object trees so they are
not flattened into screenshots. Images are decoded locally, fitted to an A4
portrait or landscape page with margins, and embedded into the output PDF.

## Supported platforms

GitHub Actions verifies tests and release builds on Windows, Ubuntu, and macOS.
