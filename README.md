# PdfMerger

PdfMerger is a private, offline desktop application for visually assembling PDF
documents. It is written in Rust and runs natively on Windows, Linux, and macOS:
there is no browser UI, web server, upload, or cloud processing.

## Features

- Drop PDF files and images directly onto the application.
- See every imported PDF page as an individual card.
- Keep pages organized in collapsible source-document cards with group-level actions.
- Convert PNG, JPEG, WebP, BMP, GIF, and TIFF images into A4 PDF pages.
- Reorder pages with drag and drop or the arrow controls.
- Select multiple pages for grouped rotation, movement, deletion, or export.
- Undo and redo workspace edits with a bounded local history.
- Remove individual pages without changing the source files.
- Merge all pages—or only the selection—in the exact visible order.
- Split selected pages into individual PDFs, source-file groups, or custom ranges.
- Save and reopen versioned projects with recent-project access and missing-source recovery.
- Choose lossless, balanced, or smaller-file export presets with custom image layouts and PDF metadata.
- Unlock password-protected PDFs for importing, projects, merging, and splitting.
- Preserve page boxes/resources, external links, annotations, compatible metadata, and page-targeted bookmarks during export.
- Report PDF structures that cannot be safely remapped, including forms, labels, named destinations, layers, and tagged-PDF trees.
- Track background imports and exports with progress, cancellation, and copyable diagnostics.
- Process and save everything locally.
- Use native open/save dialogs on all supported operating systems.

## Source document groups

Each imported PDF or image batch is shown in a labeled source card. Groups can be collapsed, selected, rotated, exported, removed, or moved earlier and later as a unit. Pages remain individually selectable and reorderable. Drag a page anywhere onto another document card to transfer it. To transfer several pages together, select them and drag any selected page onto the destination card. While dragging, use the mouse wheel or trackpad to scroll, or hold the pointer near the top or bottom edge for automatic scrolling. Transferred pages retain their original PDF/image source, and the flattened group order remains the exact order used for export. Group identity is stored in new projects; older projects are grouped automatically from consecutive source files when opened.

## Export settings

Every full-workspace or selected-page export opens a settings dialog. Split exports reuse the most recently applied settings.

- **Lossless** keeps image pixels and uses lossless compression.
- **Balanced** limits large images to 2400 pixels and uses 85% quality.
- **Smaller file** limits images to 1600 pixels and uses 65% JPEG compression.

Images can be placed on automatically oriented A4 pages, sized from their pixel dimensions at a chosen DPI, or fitted to a custom page size. Margins, downsampling, quality, and title/author/subject/keyword metadata are configurable. Imported PDF pages continue to be copied from their original object trees rather than rasterized. Inherited page resources and boxes are materialized before extraction; external links and ordinary page annotations remain active, compatible source metadata is retained unless overridden, and bookmarks targeting exported pages are rebuilt against their new page IDs. Unsafe cross-page links are disabled instead of left dangling. Structures that cannot be safely remapped produce export warnings in the Details view. Export settings are stored in `.pdfmerger` projects.

## Projects

Use the **Project** menu to save the current page order, rotations, and source references as a `.pdfmerger` project. Source paths inside the project directory are stored relatively so the project folder can be moved as a unit.

When opening a project, PdfMerger restores previews in the background. If a source file moved, a recovery dialog lets you locate its replacement. Recent projects are available from the Project menu. An asterisk in the window title marks unsaved changes, and the application prompts before replacing the workspace or exiting.

## Protected PDFs

When a PDF is encrypted, PdfMerger asks for its password and retries the interrupted import or project open. Passwords are retained only in application memory for the current session, are never written to project files, and are cleared from prompt buffers after use. A document that forbids page assembly requires its owner password. Unsupported encryption is reported as an error instead of repeatedly prompting.

## Splitting PDFs

Select the pages to process and choose **Split…**. Outputs can be created as:

- one PDF per selected page;
- one PDF per original source file; or
- one PDF per custom range, such as `1-3, 5, 7-9`.

Ranges refer to positions within the current selection. PdfMerger validates filenames and refuses to overwrite existing files before starting the background export.

## Background jobs

Imports, exports, split operations, and project restoration run outside the UI thread. The status bar shows the active phase and item/page progress and offers cancellation. Completed warnings and errors are kept in a copyable **Details** view. Cancellation stops at safe document boundaries, and incomplete split outputs are removed.

## Keyboard shortcuts

- Ctrl/Cmd+O: add files
- Ctrl/Cmd+Shift+O: open a project
- Ctrl/Cmd+S: export the full workspace
- Ctrl/Cmd+Shift+S: save the project
- `Ctrl/Cmd+A`: select all pages
- `Ctrl/Cmd+Z`: undo
- `Ctrl/Cmd+Y` or `Ctrl/Cmd+Shift+Z`: redo
- `R`: rotate selected pages clockwise
- `Delete`: remove selected pages
- `Escape`: clear the selection

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
not flattened into screenshots. Safe page-level structures are retained and catalog-level
structures are rebuilt where possible; unsupported rewrites are reported explicitly. Images are decoded locally, fitted to an A4
portrait or landscape page with margins, and embedded into the output PDF.

## Supported platforms

GitHub Actions verifies tests and release builds on Windows, Ubuntu, and macOS.
