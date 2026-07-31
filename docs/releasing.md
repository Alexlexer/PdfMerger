# Releasing PdfMerger

Releases are built from version tags by `.github/workflows/release.yml`. The
workflow builds each target on a native runner, smoke-tests the release binary,
packages it, generates checksums and attestations, and publishes the release only
when all required targets succeed.

## Supported release targets

| Platform | Architecture | Native package | Portable package |
| --- | --- | --- | --- |
| Windows | x86-64 | NSIS installer | ZIP |
| Linux | x86-64 | AppImage | `.tar.gz` |
| macOS | Intel | DMG | — |
| macOS | Apple Silicon | DMG | — |

The packages are unsigned until protected Windows and Apple signing credentials
are configured. Release notes must call this out clearly.

## Prepare a release

1. Update the version in both `Cargo.toml` and `Packager.toml`.
2. Move completed entries from `Unreleased` into a dated version section in
   `CHANGELOG.md`.
3. If artwork changed, install Pillow and run `python scripts/generate_icons.py`.
4. Regenerate third-party notices:

   ```sh
   cargo about generate about.hbs --locked --fail --output-file THIRD_PARTY_LICENSES.html
   ```

5. Validate metadata and run the local quality gate:

   ```sh
   python scripts/validate_release.py
   cargo fmt --all -- --check
   cargo test --all-targets --locked
   cargo clippy --all-targets --locked -- -D warnings
   cargo build --release --locked
   ```

6. Run `target/release/pdf-merger --smoke-test` (`.exe` on Windows).
7. Merge the release change only after the Build and Release workflows pass.

## Publish

Create an annotated tag matching the Cargo version and push it:

```sh
git tag -a v0.2.0 -m "PdfMerger v0.2.0"
git push origin v0.2.0
```

Do not manually upload replacement binaries. Rerun the tag workflow instead so
checksums and provenance remain tied to the producing workflow.

## Verify

- Confirm every supported target is attached to the GitHub release.
- Download an artifact and verify `SHA256SUMS`.
- Verify provenance with GitHub CLI, for example:

  ```sh
  gh attestation verify PdfMerger-0.2.0-windows-x86_64.exe --repo Alexlexer/PdfMerger
  ```

- Install or extract each package on a clean machine and complete an import/export
  smoke test before announcing the release.

## Rollback

If a published package is incorrect, mark the release as a prerelease or draft,
post a clear notice, fix the problem in a new patch version, and retain the old tag
for traceability. Never move a published version tag.
