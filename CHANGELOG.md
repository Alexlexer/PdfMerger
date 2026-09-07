# Changelog

All notable changes to PdfMerger are documented in this file.

The project follows [Semantic Versioning](https://semver.org/), and releases are
published from matching `vMAJOR.MINOR.PATCH` tags.

## [Unreleased]

## [0.5.0-beta.1] - 2026-09-07

### Added

- LM Studio backend with automatic local-server discovery when opening the AI dialog.
- Automatic selection of a loaded language model, model selection, refresh, and custom local ports.
- Local HTTP summarization with cancellation, output-language selection, and page references.
- Compatibility with LM Studio's native v1 and older v0 model discovery endpoints.

### Beta notes

- Enable LM Studio's local server in the Developer tab before opening the AI dialog.
  The default port is 1234. API authentication is not yet supported.
- Long documents are summarized section by section. LM Studio manages its own models
  and GPU settings; PdfMerger does not unload server models when jobs finish.
- Windows and Linux packages use CPU for built-in GGUF inference; macOS Apple Silicon
  uses Metal. LM Studio can use its configured GPU on any supported platform.
- Packages are unsigned. Windows SmartScreen and macOS Gatekeeper may prompt before opening.
- This is a beta release; verify generated summaries against the source PDF.

## [0.4.0] - 2026-08-24

### Added

- Private, fully local PDF summarization with GGUF models and no document uploads.
- Explicit model selection, a recommended downloadable model, output-language selection, and
  diagnostics showing the active inference backend.
- Multi-pass summarization for documents larger than a model's context window, with cancellation
  and automatic model unloading when each job finishes.
- Opt-in NVIDIA CUDA builds covering RTX 20, 30, 40, and 50 series GPUs, plus Metal acceleration
  for Apple Silicon macOS builds and CPU fallback.

### Changed

- PDF imports now load page metadata immediately and render only visible previews on demand,
  keeping very large documents responsive and memory-bounded.
- PDF text extraction better recovers uncommon font encodings and strips invalid control bytes.

## [0.3.0] - 2026-08-23

### Added

- Complete keyboard navigation for document groups, page cards, dialogs, and jobs.
- Accessible labels, state reporting, focus management, and non-drag editing alternatives.
- Light and dark themes, high-contrast palettes, and 100%–200% interface scaling.
- Live status announcements and descriptive page-preview alternatives for screen readers.

### Changed

- Dialogs now trap keyboard focus while open and restore it to the invoking control when closed.
- Responsive layout sizing keeps controls and dialogs usable at large interface scales.

## [0.2.0] - 2026-07-31

### Added

- Native Windows NSIS, macOS DMG, and Linux AppImage release targets.
- Portable Windows and Linux archives.
- Embedded window and Windows executable identity metadata.
- Packaged-binary smoke-test mode for release validation.

## [0.1.0] - 2026-07-30

### Added

- Initial open-source release.
- Visual page arrangement, source-document cards, grouped page transfer, rotation,
  selection, undo/redo, splitting, project persistence, background jobs, and
  structure-preserving PDF export.

[Unreleased]: https://github.com/Alexlexer/PdfMerger/compare/v0.5.0-beta.1...HEAD
[0.5.0-beta.1]: https://github.com/Alexlexer/PdfMerger/compare/v0.4.0...v0.5.0-beta.1
[0.4.0]: https://github.com/Alexlexer/PdfMerger/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Alexlexer/PdfMerger/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Alexlexer/PdfMerger/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Alexlexer/PdfMerger/releases/tag/v0.1.0
