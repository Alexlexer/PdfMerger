# Changelog

All notable changes to PdfMerger are documented in this file.

The project follows [Semantic Versioning](https://semver.org/), and releases are
published from matching `vMAJOR.MINOR.PATCH` tags.

## [Unreleased]

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

[Unreleased]: https://github.com/Alexlexer/PdfMerger/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/Alexlexer/PdfMerger/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Alexlexer/PdfMerger/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Alexlexer/PdfMerger/releases/tag/v0.1.0
