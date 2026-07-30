use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    document,
    export_settings::ExportSettings,
    model::{PageDraft, PageItem, PageRotation, PageSource},
};

#[derive(Debug)]
pub enum MaterializeFailure {
    Access {
        path: PathBuf,
        error: document::PdfAccessError,
    },
    Other(anyhow::Error),
}

impl std::fmt::Display for MaterializeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access { path, error } => write!(formatter, "{}: {error}", path.display()),
            Self::Other(error) => write!(formatter, "{error:#}"),
        }
    }
}

impl std::error::Error for MaterializeFailure {}

impl From<anyhow::Error> for MaterializeFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}
pub const PROJECT_EXTENSION: &str = "pdfmerger";
const FORMAT_VERSION: u32 = 1;
const RECENT_LIMIT: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectFile {
    pub format_version: u32,
    pub pages: Vec<ProjectPage>,
    #[serde(default)]
    pub export: ExportSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectPage {
    pub source: ProjectSource,
    #[serde(default)]
    pub group_id: Option<u64>,
    pub rotation_degrees: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectSource {
    Pdf { path: PathBuf, page_number: u32 },
    Image { path: PathBuf },
}

impl ProjectSource {
    fn path(&self) -> &Path {
        match self {
            Self::Pdf { path, .. } | Self::Image { path } => path,
        }
    }
}

pub fn save_project(path: &Path, pages: &[PageItem], settings: &ExportSettings) -> Result<()> {
    if pages.is_empty() {
        bail!("add at least one page before saving a project");
    }
    let project_directory = path.parent().unwrap_or_else(|| Path::new("."));
    let pages = pages
        .iter()
        .map(|page| ProjectPage {
            group_id: Some(page.group_id),
            source: match &page.source {
                PageSource::Pdf { path, page_number } => ProjectSource::Pdf {
                    path: portable_path(path, project_directory),
                    page_number: *page_number,
                },
                PageSource::Image { path } => ProjectSource::Image {
                    path: portable_path(path, project_directory),
                },
            },
            rotation_degrees: page.rotation.degrees() as u16,
        })
        .collect();
    let project = ProjectFile {
        format_version: FORMAT_VERSION,
        pages,
        export: settings.clone(),
    };
    let json = serde_json::to_vec_pretty(&project).context("could not serialize project")?;
    fs::write(path, json).with_context(|| format!("could not save project to {}", path.display()))
}

pub fn read_project(path: &Path) -> Result<ProjectFile> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let project: ProjectFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not a valid PdfMerger project", path.display()))?;
    if project.format_version != FORMAT_VERSION {
        bail!(
            "project format version {} is not supported (expected {})",
            project.format_version,
            FORMAT_VERSION
        );
    }
    if project.pages.is_empty() {
        bail!("project contains no pages");
    }
    for page in &project.pages {
        rotation(page.rotation_degrees)?;
    }
    Ok(project)
}

pub fn missing_sources(
    project_path: &Path,
    project: &ProjectFile,
    replacements: &HashMap<PathBuf, PathBuf>,
) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    project
        .pages
        .iter()
        .filter_map(|page| {
            let original = resolve_path(project_path, page.source.path());
            let effective = replacements.get(&original).unwrap_or(&original);
            if !effective.is_file() && seen.insert(original.clone()) {
                Some(original)
            } else {
                None
            }
        })
        .collect()
}

pub fn materialize_project(
    project_path: &Path,
    project: &ProjectFile,
    replacements: &HashMap<PathBuf, PathBuf>,
) -> Result<Vec<(PageDraft, PageRotation, Option<u64>)>> {
    materialize_project_with_passwords(project_path, project, replacements, &HashMap::new())
        .map_err(|error| anyhow::anyhow!(error))
}

pub fn materialize_project_with_passwords(
    project_path: &Path,
    project: &ProjectFile,
    replacements: &HashMap<PathBuf, PathBuf>,
    passwords: &HashMap<PathBuf, Zeroizing<String>>,
) -> std::result::Result<Vec<(PageDraft, PageRotation, Option<u64>)>, MaterializeFailure> {
    materialize_project_with_passwords_controlled(
        project_path,
        project,
        replacements,
        passwords,
        &mut |_, _, _| {},
        &|| false,
    )
}

pub fn materialize_project_with_passwords_controlled(
    project_path: &Path,
    project: &ProjectFile,
    replacements: &HashMap<PathBuf, PathBuf>,
    passwords: &HashMap<PathBuf, Zeroizing<String>>,
    progress: &mut dyn FnMut(usize, usize, &Path),
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<Vec<(PageDraft, PageRotation, Option<u64>)>, MaterializeFailure> {
    let missing = missing_sources(project_path, project, replacements);
    if !missing.is_empty() {
        return Err(MaterializeFailure::Other(anyhow::anyhow!(
            "{} project source file(s) are missing",
            missing.len()
        )));
    }

    let source_count = project
        .pages
        .iter()
        .map(|page| {
            let original = resolve_path(project_path, page.source.path());
            replacements.get(&original).unwrap_or(&original).clone()
        })
        .collect::<HashSet<_>>()
        .len();
    let mut completed_sources = 0;
    let mut imports: HashMap<PathBuf, Vec<PageDraft>> = HashMap::new();
    let mut pages = Vec::with_capacity(project.pages.len());
    for stored_page in &project.pages {
        if cancelled() {
            return Err(MaterializeFailure::Other(anyhow::anyhow!(
                "project open cancelled"
            )));
        }
        let original = resolve_path(project_path, stored_page.source.path());
        let effective = replacements.get(&original).unwrap_or(&original);
        if !imports.contains_key(effective) {
            let imported = document::import_file_with_password(
                effective,
                passwords.get(effective).map(|password| password.as_str()),
            )
            .map_err(|failure| match failure {
                document::ImportFailure::Access(error) => MaterializeFailure::Access {
                    path: effective.clone(),
                    error,
                },
                document::ImportFailure::Other(error) => MaterializeFailure::Other(error),
            })?;
            imports.insert(effective.clone(), imported);
            completed_sources += 1;
            progress(completed_sources, source_count, effective);
        }
        let imported = imports
            .get(effective)
            .expect("an imported source must remain cached");
        let draft = match &stored_page.source {
            ProjectSource::Pdf { page_number, .. } => imported
                .iter()
                .find(|draft| {
                    matches!(
                        draft.source,
                        PageSource::Pdf {
                            page_number: imported_number,
                            ..
                        } if imported_number == *page_number
                    )
                })
                .cloned()
                .with_context(|| {
                    format!(
                        "{} no longer contains PDF page {}",
                        effective.display(),
                        page_number
                    )
                })?,
            ProjectSource::Image { .. } => imported
                .iter()
                .find(|draft| matches!(draft.source, PageSource::Image { .. }))
                .cloned()
                .with_context(|| {
                    format!("{} is no longer a supported image", effective.display())
                })?,
        };
        pages.push((
            draft,
            rotation(stored_page.rotation_degrees)?,
            stored_page.group_id,
        ));
    }
    Ok(pages)
}
pub fn load_recent_projects() -> Vec<PathBuf> {
    let Some(path) = recent_projects_path() else {
        return Vec::new();
    };
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<PathBuf>>(&bytes)
        .unwrap_or_default()
        .into_iter()
        .filter(|path| path.is_file())
        .take(RECENT_LIMIT)
        .collect()
}

pub fn save_recent_projects(projects: &[PathBuf]) -> Result<()> {
    let Some(path) = recent_projects_path() else {
        return Ok(());
    };
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)
            .with_context(|| format!("could not create {}", directory.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&projects.iter().take(RECENT_LIMIT).collect::<Vec<_>>())?;
    fs::write(&path, bytes).with_context(|| format!("could not save {}", path.display()))
}

fn portable_path(source: &Path, project_directory: &Path) -> PathBuf {
    let absolute = if source.is_absolute() {
        source.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(source))
            .unwrap_or_else(|_| source.to_path_buf())
    };
    absolute
        .strip_prefix(project_directory)
        .map(Path::to_path_buf)
        .unwrap_or(absolute)
}

fn resolve_path(project_path: &Path, stored: &Path) -> PathBuf {
    if stored.is_absolute() {
        stored.to_path_buf()
    } else {
        project_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(stored)
    }
}

fn rotation(degrees: u16) -> Result<PageRotation> {
    match degrees {
        0 => Ok(PageRotation::Deg0),
        90 => Ok(PageRotation::Deg90),
        180 => Ok(PageRotation::Deg180),
        270 => Ok(PageRotation::Deg270),
        _ => bail!("invalid project page rotation: {degrees}"),
    }
}

fn recent_projects_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let directory = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let directory = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join("Library/Application Support"));
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".config"))
        });

    directory.map(|path| path.join("PdfMerger").join("recent-projects.json"))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{Rgb, RgbImage};

    use crate::model::Workspace;

    use super::*;

    fn temp_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pdf-merger-project-{nonce}"));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn saves_versioned_projects_with_relative_source_paths() {
        let directory = temp_directory();
        let image_path = directory.join("photo.png");
        let project_path = directory.join("layout.pdfmerger");
        RgbImage::from_pixel(8, 4, Rgb([10, 20, 30]))
            .save(&image_path)
            .unwrap();
        let mut workspace = Workspace::default();
        workspace.append(document::import_file(&image_path).unwrap());

        let mut settings = ExportSettings::default();
        settings.apply_preset(crate::export_settings::ExportPreset::Balanced);
        settings.metadata.title = "Saved layout".to_owned();
        save_project(&project_path, workspace.pages(), &settings).unwrap();
        let project = read_project(&project_path).unwrap();

        assert_eq!(project.format_version, 1);
        assert_eq!(project.export, settings);
        assert_eq!(
            project.pages[0].group_id,
            Some(workspace.pages()[0].group_id)
        );
        assert!(matches!(
            &project.pages[0].source,
            ProjectSource::Image { path } if path == Path::new("photo.png")
        ));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn restores_pages_and_applies_replacement_sources() {
        let directory = temp_directory();
        let missing_path = directory.join("missing.png");
        let replacement_path = directory.join("replacement.png");
        let project_path = directory.join("layout.pdfmerger");
        RgbImage::from_pixel(8, 4, Rgb([10, 20, 30]))
            .save(&replacement_path)
            .unwrap();
        let project = ProjectFile {
            format_version: FORMAT_VERSION,
            pages: vec![ProjectPage {
                source: ProjectSource::Image {
                    path: PathBuf::from("missing.png"),
                },
                group_id: None,
                rotation_degrees: 90,
            }],
            export: ExportSettings::default(),
        };
        let replacements = HashMap::from([(missing_path.clone(), replacement_path.clone())]);

        assert_eq!(
            missing_sources(&project_path, &project, &HashMap::new()),
            [missing_path]
        );
        let restored = materialize_project(&project_path, &project, &replacements).unwrap();
        assert_eq!(restored[0].1, PageRotation::Deg90);
        assert_eq!(restored[0].0.source.path(), &replacement_path);
        assert_eq!(restored[0].2, None);

        fs::remove_dir_all(directory).unwrap();
    }
}
