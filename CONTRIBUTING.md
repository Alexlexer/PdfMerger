# Contributing to PdfMerger

Thank you for helping improve PdfMerger.

## Before opening a change

- Search existing issues and pull requests for related work.
- Open an issue first for large behavioral or architectural changes.
- Report security problems through the private process in [SECURITY.md](SECURITY.md).
- Keep document contents, passwords, personal paths, and other private data out of issues, tests, and logs.

## Development setup

Install the current stable Rust toolchain. Platform prerequisites are listed in
the [README](README.md#platform-prerequisites).

```sh
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

Run `cargo fmt --all` before committing. PDF/model behavior should include a
focused regression test. UI changes should remain responsive and preserve
keyboard access where practical.

## Pull requests

- Keep each pull request focused and describe the user-visible impact.
- Explain how the change was tested.
- Update the README or implementation plan when behavior changes.
- Do not commit generated build output, private documents, or credentials.
- Contributions are submitted under the repository's MIT license.

## Architecture

GUI event collection belongs in `src/app`, domain mutations in `src/model.rs`,
and document/PDF processing in `src/document.rs`. Long-running file work must
remain outside the UI thread and communicate through typed job messages.
