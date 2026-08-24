# PdfMerger implementation plan

## Architecture first

The desktop shell is split before new behavior is added:

- `src/app/mod.rs` — application state, message handling, and the `eframe::App` lifecycle.
- `src/app/actions.rs` — file dialogs and background import/export orchestration.
- `src/app/ui.rs` — top, central, empty, status, and drag-overlay panels.
- `src/app/page_grid.rs` — page-grid layout, page cards, drag/drop, and card actions.
- `src/app/style.rs` — shared visual constants and theme configuration.
- `src/model.rs` — domain state and operations that can be unit-tested without the GUI.
- `src/document.rs` — PDF/image import, transformation, and export.

New features should keep GUI event collection in `app`, domain mutations in `model`, and file/PDF work in `document`. Long-running document work must stay off the UI thread.

## Implementation status

- Completed: architecture baseline, core editing, split/export-selected, project persistence, export settings, protected PDFs, job progress/cancellation/diagnostics, source-document group cards, PDF structure preservation, native distribution/release automation, and accessibility/input polish.
- Next: local document summarization, performance, recovery, localization, and packaged-app testing.

## Delivery milestones

### 0. Refactor baseline

- Split the monolithic `app.rs` into the modules above without changing behavior.
- Run formatting, unit tests, and release compilation.

### 1. Core editing

- Add stable multi-selection by page ID, select-all, and clear-selection actions.
- Add grouped delete and grouped move-to-start/end operations.
- Add clockwise page rotation with rotated thumbnails and correct PDF export.
- Add bounded undo/redo history for import, clear, delete, reorder, and rotation.
- Add keyboard shortcuts: open, export, select all, undo, redo, delete, rotate, and escape.
- Add focused model tests for every reversible operation.

### 2. Export selected pages and splitting

- Export the current selection in visible order.
- Provide split modes: one PDF per selected page, ranges, and source-document groups.
- Validate filenames and collisions before starting background work.
- Report partial failures without losing successful outputs.

### 3. Project persistence

- Define a versioned project format containing source paths, source page numbers, order, rotation, selection-independent editing metadata, and export preferences.
- Implement Save Project, Save Project As, Open Project, recent projects, and dirty-state prompts.
- Resolve missing/moved sources interactively and preserve forward compatibility.

### 4. Export settings and optimization

- Add page-size and margin policies for images.
- Add image quality/downsampling and compression controls with useful presets.
- Add title, author, subject, and keyword metadata fields.
- Keep lossless passthrough as the default for imported PDF pages.

### 5. Protected PDFs

- Detect encrypted PDFs during import.
- Prompt for passwords without persisting them by default.
- Cache credentials only for the current process and zero sensitive buffers where practical.
- Produce clear errors for unsupported encryption and owner restrictions.

### 6. Progress, cancellation, and diagnostics

- Replace the simple active-job count with job IDs, phases, progress, and cancellation tokens.
- Show per-file import and per-page export progress.
- Add a copyable details view for warnings/errors while keeping concise status text.
- Ensure cancellation cleans up incomplete output files.

### 7. Source-document group cards

- Represent each imported source as a labeled, collapsible document group.
- Preserve exact merge order while supporting whole-group and individual-page reordering.
- Transfer one or many pages between groups without changing their underlying source references.
- Add group-level selection, rotation, removal, and export actions.
- Keep project serialization backward compatible and restore group organization on open.

### 8. PDF structure preservation (completed)

- Inventory and test preservation of metadata, outlines/bookmarks, links, annotations, forms, named destinations, labels, and page boxes.
- Preserve structures only when references can be remapped safely; warn when an operation necessarily drops or rewrites them.
- Add fixture-based integration tests covering mixed source PDFs.

### 9. Native distribution (`v0.2.0`)

- Add source artwork, runtime/window icons, and Windows executable identity metadata.
- Package Windows x86-64 as NSIS plus portable ZIP, Apple Silicon macOS as DMG,
  and Linux x86-64 as AppImage plus portable tarball.
- Validate that the Cargo version, packager version, changelog, and release tag agree.
- Build packages on native GitHub runners, smoke-test their binaries, generate checksums and
  provenance attestations, and publish only after every required target succeeds.
- Add signing and notarization once protected platform credentials are available.

### 10. Accessibility and input polish (`v0.3.0`, completed)

- Complete keyboard navigation through document groups, page cards, dialogs, and jobs.
- Add accessible labels, predictable focus order, visible focus states, and non-drag
  alternatives for every operation.
- Test high contrast, light/dark themes, display scaling, and screen-reader output.

### 11. Local document summarization (`v0.4.0`)

- Add a **Summarize this PDF** action to each source-document card, with optional summarization
  of only the selected pages and page-number references supporting the generated summary.
- Extract searchable text locally and clearly report image-only/scanned pages that require a
  future OCR stage; never upload documents, extracted text, prompts, or generated summaries.
- Integrate a compact GGUF model behind a backend abstraction, with a CPU baseline, bounded
  memory/context use, cancellation, progress reporting, and configurable summary length.
- Ship NVIDIA CUDA builds with kernels for RTX 20, 30, 40, and 50 series GPUs.
- Ship the Apple Silicon macOS build with llama.cpp Metal acceleration, using the same GGUF model
  as CPU and CUDA builds. Show the active backend in diagnostics and retain runtime CPU fallback.
- Add Radeon acceleration through Vulkan next, keeping it optional and capability-detected so an
  unsupported GPU never prevents startup or document editing. Evaluate Core ML for direct Apple
  Neural Engine use only when the chosen model and runtime support it reliably.
- Make model installation explicitly opt-in. Verify model hashes and licenses, show disk/RAM
  requirements, allow removal, and keep the application fully usable without a model.
- Treat PDF text as untrusted input: the model may summarize content but cannot execute tools,
  access local files beyond the chosen document, or use the network. Label output as generated
  and potentially inaccurate.
- Run summarization jobs off the UI thread and provide copy, regenerate, length, and audience-level
  controls. Cache only with explicit permission and key cached results by document, pages, model,
  and prompt version.
- Add deterministic tests with a mock model backend, extraction/grounding fixtures, performance
  budgets, and packaged-app checks that confirm the optional model is not accidentally bundled.

### 12. Large-document performance (`v0.5.0`)

- Benchmark 100, 500, and 1,000-page workspaces before optimization.
- Virtualize page cards, load visible previews first, cancel stale work, and bound texture
  cache size and background concurrency.
- Track import latency, scrolling frame time, memory, export time, and cancellation latency.

### 13. Crash recovery and data integrity (`v0.6.0`)

- Add versioned, password-free recovery snapshots written with debounce and atomic replacement.
- Offer restore/inspect/discard after an unclean shutdown and make normal project saves atomic.
- Test corruption, disk-full, read-only, missing-source, and interrupted-write paths.

### 14. Localization and optional updates (`v0.7.0`)

- Move user-facing text into a typed message catalog and add a pseudo-locale.
- Add an opt-in, privacy-respecting update check using only public release metadata.
- Keep offline use unchanged and never send document data or local paths.

### 15. UI, fixture, and packaged-application testing (`v0.8.0`)

- Add redistributable fixtures for PDF structures, encryption, malformed inputs, mixed page
  sizes, and project migrations.
- Add interaction tests for editing, transfer, recovery, dirty prompts, and keyboard navigation.
- Install/extract and smoke-test every native package before release publication.
- Maintain a release checklist covering versions, changelog, notices, audits, checksums,
  attestations, rollback, and stable-release promotion.

Undo/redo, project persistence, multi-page selection, and the documented shortcuts are already
implemented. Future milestones add regression, accessibility, and optional local-AI coverage
rather than rebuilding those capabilities.

## Engineering rules
- Each milestone must compile and test independently.
- Model/document behavior gets unit or fixture tests before UI wiring is considered complete.
- Background workers communicate through typed messages; they never mutate GUI state directly.
- Project format changes are versioned and migrated.
- Passwords and document contents remain local and are never logged.
