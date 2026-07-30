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

- Completed: architecture baseline, core editing, split/export-selected, project persistence, export settings, protected PDFs, job progress/cancellation/diagnostics, source-document group cards, and PDF structure preservation.
- Next: accessibility, polish, and release readiness.

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

### 9. Accessibility, polish, and release readiness

- Complete keyboard navigation, accessible labels, focus states, and high-contrast checks.
- Add localization-ready user-facing strings.
- Add crash-safe recovery and optional update notifications.
- Extend CI with fixture tests and packaged-application smoke tests on Windows, Linux, and macOS.

## Engineering rules

- Each milestone must compile and test independently.
- Model/document behavior gets unit or fixture tests before UI wiring is considered complete.
- Background workers communicate through typed messages; they never mutate GUI state directly.
- Project format changes are versioned and migrated.
- Passwords and document contents remain local and are never logged.
