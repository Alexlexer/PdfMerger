use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const OPTIONAL_BACKENDS: &[(&str, &str)] = &[
    ("ggml-cuda.dll", "NVIDIA CUDA"),
    ("ggml-vulkan.dll", "Vulkan (experimental)"),
];
const PACK_MANIFEST: &str = "backend-pack.json";

#[derive(Deserialize)]
struct PackManifest {
    format: u32,
    pdf_merger_version: String,
    #[serde(default)]
    runtime_files: Vec<String>,
}

pub(super) struct BackendStatus {
    pub name: &'static str,
    pub installed: bool,
}

pub(super) fn backend_directory() -> Result<PathBuf> {
    let executable = std::env::current_exe().context("could not locate PdfMerger")?;
    let parent = executable
        .parent()
        .context("PdfMerger has no parent directory")?;
    Ok(parent.join("backends"))
}

pub(super) fn statuses() -> Vec<BackendStatus> {
    let directory = backend_directory().ok();
    OPTIONAL_BACKENDS
        .iter()
        .map(|(file, name)| BackendStatus {
            name,
            installed: directory
                .as_ref()
                .is_some_and(|directory| directory.join(file).is_file()),
        })
        .collect()
}

pub(super) fn install_pack(source: &Path) -> Result<usize> {
    if !source.is_dir() {
        bail!("the selected backend pack is not a directory");
    }
    let (source_root, backend_source) = if source.join("backends").is_dir() {
        (source.to_owned(), source.join("backends"))
    } else {
        (
            source.parent().unwrap_or(source).to_owned(),
            source.to_owned(),
        )
    };
    let manifest_path = backend_source.join(PACK_MANIFEST);
    let manifest: PackManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("backend pack has no {}", manifest_path.display()))?,
    )
    .context("backend pack manifest is invalid")?;
    if manifest.format != 1 || manifest.pdf_merger_version != env!("CARGO_PKG_VERSION") {
        bail!(
            "backend pack is not compatible with PdfMerger {}",
            env!("CARGO_PKG_VERSION")
        );
    }
    let destination = backend_directory()?;
    let files = OPTIONAL_BACKENDS
        .iter()
        .filter_map(|(file, _)| {
            let path = backend_source.join(file);
            path.is_file().then_some((path, *file))
        })
        .collect::<Vec<_>>();
    if files.is_empty() {
        bail!("the selected folder contains no supported PdfMerger backend");
    }

    fs::create_dir_all(&destination)
        .with_context(|| format!("could not create {}", destination.display()))?;
    for (source, file) in &files {
        fs::copy(source, destination.join(file)).with_context(|| {
            format!("could not install {file}; the application folder may be read-only")
        })?;
    }
    let application_directory = destination
        .parent()
        .context("backend directory has no application directory")?;
    for file in &manifest.runtime_files {
        if Path::new(file).file_name().and_then(|name| name.to_str()) != Some(file.as_str()) {
            bail!("backend pack contains an invalid runtime filename");
        }
        fs::copy(source_root.join(file), application_directory.join(file))
            .with_context(|| format!("could not install runtime dependency {file}"))?;
    }
    fs::copy(&manifest_path, destination.join(PACK_MANIFEST))
        .context("could not record the installed backend pack")?;
    Ok(files.len())
}

pub(super) fn remove_optional_backends() -> Result<usize> {
    let directory = backend_directory()?;
    let runtime_files = fs::read(directory.join(PACK_MANIFEST))
        .ok()
        .and_then(|contents| serde_json::from_slice::<PackManifest>(&contents).ok())
        .map_or_else(Vec::new, |manifest| manifest.runtime_files);
    let mut removed = 0;
    for (file, _) in OPTIONAL_BACKENDS {
        let path = directory.join(file);
        if path.is_file() {
            fs::remove_file(&path).with_context(|| {
                format!(
                    "could not remove {}; restart PdfMerger first",
                    path.display()
                )
            })?;
            removed += 1;
        }
    }
    if let Some(application_directory) = directory.parent() {
        for file in runtime_files {
            if Path::new(&file).file_name().and_then(|name| name.to_str()) == Some(file.as_str()) {
                let path = application_directory.join(file);
                if path.is_file() {
                    fs::remove_file(&path).with_context(|| {
                        format!(
                            "could not remove {}; restart PdfMerger first",
                            path.display()
                        )
                    })?;
                }
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::OPTIONAL_BACKENDS;

    #[test]
    fn optional_backends_have_unique_files() {
        for (index, (file, _)) in OPTIONAL_BACKENDS.iter().enumerate() {
            assert!(file.starts_with("ggml-"));
            assert!(file.ends_with(".dll"));
            assert!(
                !OPTIONAL_BACKENDS[..index]
                    .iter()
                    .any(|entry| entry.0 == *file)
            );
        }
    }
}
