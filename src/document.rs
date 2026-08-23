use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use indexmap::IndexMap;
use lopdf::{Bookmark, Dictionary, Document, Object, ObjectId, Outline, dictionary};
use zeroize::Zeroizing;

use printpdf::{
    ImageCompression, ImageOptimizationOptions, Mm, Op, PdfDocument, PdfPage, PdfSaveOptions, Pt,
    RawImage, XObjectTransform,
};

use crate::{
    export_settings::{ExportPreset, ExportSettings, ImagePagePolicy},
    model::{PageDraft, PageItem, PageRotation, PageSource, PreviewData},
};

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];
const PREVIEW_MAX_WIDTH: u32 = 312;
const PREVIEW_MAX_HEIGHT: u32 = 416;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfAccessError {
    PasswordRequired,
    IncorrectPassword,
    OwnerPasswordRequired,
    UnsupportedEncryption(String),
}

impl std::fmt::Display for PdfAccessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PasswordRequired => formatter.write_str("this PDF requires a password"),
            Self::IncorrectPassword => formatter.write_str("the PDF password is incorrect"),
            Self::OwnerPasswordRequired => formatter
                .write_str("the PDF does not allow page assembly; enter its owner password"),
            Self::UnsupportedEncryption(error) => {
                write!(formatter, "unsupported PDF encryption: {error}")
            }
        }
    }
}

impl std::error::Error for PdfAccessError {}

#[derive(Debug)]
pub enum ImportFailure {
    Access(PdfAccessError),
    Other(anyhow::Error),
}

impl std::fmt::Display for ImportFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access(error) => error.fmt(formatter),
            Self::Other(error) => write!(formatter, "{error:#}"),
        }
    }
}

impl std::error::Error for ImportFailure {}
#[derive(Debug)]
pub struct ExportReport {
    pub path: PathBuf,
    pub page_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
struct PreparedDocument {
    document: Document,
    bookmarks: Vec<String>,
    metadata: Option<Dictionary>,
}

impl PreparedDocument {
    fn plain(document: Document) -> Self {
        Self {
            document,
            bookmarks: Vec::new(),
            metadata: None,
        }
    }
}

pub fn is_supported(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);

    matches!(extension.as_deref(), Some("pdf"))
        || extension
            .as_deref()
            .is_some_and(|value| IMAGE_EXTENSIONS.contains(&value))
}

pub fn import_file(path: &Path) -> Result<Vec<PageDraft>> {
    import_file_with_password(path, None).map_err(|error| anyhow!(error))
}

pub fn import_file_with_password(
    path: &Path,
    password: Option<&str>,
) -> std::result::Result<Vec<PageDraft>, ImportFailure> {
    if !path.is_file() {
        return Err(ImportFailure::Other(anyhow!(
            "{} is not a file",
            path.display()
        )));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "pdf" {
        import_pdf(path, password)
    } else if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        import_image(path).map_err(ImportFailure::Other)
    } else {
        Err(ImportFailure::Other(anyhow!(
            "unsupported file type: {}",
            path.display()
        )))
    }
}

fn import_pdf(
    path: &Path,
    password: Option<&str>,
) -> std::result::Result<Vec<PageDraft>, ImportFailure> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read {}", path.display()))
        .map_err(ImportFailure::Other)?;
    let mut source = Document::load_mem(&bytes)
        .with_context(|| format!("could not parse {}", path.display()))
        .map_err(ImportFailure::Other)?;
    unlock_pdf(&mut source, password).map_err(ImportFailure::Access)?;
    let page_count = source.get_pages().len();
    if page_count == 0 {
        return Err(ImportFailure::Other(anyhow!(
            "{} contains no pages",
            path.display()
        )));
    }

    let preview_pdf =
        hayro::hayro_syntax::Pdf::new_with_password(bytes, password.unwrap_or_default()).ok();
    let preview_cache = hayro::RenderCache::new();
    let interpreter_settings = hayro::hayro_interpret::InterpreterSettings::default();
    let render_settings = hayro::RenderSettings {
        x_scale: 0.5,
        y_scale: 0.5,
        ..Default::default()
    };
    let file_name = display_name(path);

    Ok((1..=page_count)
        .map(|index| {
            let preview = preview_pdf
                .as_ref()
                .and_then(|pdf| pdf.pages().get(index - 1))
                .and_then(|page| {
                    preview_from_pdf_page(
                        page,
                        &preview_cache,
                        &interpreter_settings,
                        &render_settings,
                    )
                    .ok()
                });

            PageDraft {
                source: PageSource::Pdf {
                    path: path.to_path_buf(),
                    page_number: index as u32,
                },
                title: file_name.clone(),
                subtitle: format!("PDF page {index} of {page_count}"),
                preview,
            }
        })
        .collect())
}
fn import_image(path: &Path) -> Result<Vec<PageDraft>> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let decoded = image::load_from_memory(&bytes)
        .with_context(|| format!("could not decode image {}", path.display()))?;
    let subtitle = format!("Image · {} × {} px", decoded.width(), decoded.height());
    let preview = preview_from_image(decoded);

    Ok(vec![PageDraft {
        source: PageSource::Image {
            path: path.to_path_buf(),
        },
        title: display_name(path),
        subtitle,
        preview: Some(preview),
    }])
}

fn preview_from_image(image: image::DynamicImage) -> PreviewData {
    let thumbnail = image
        .resize(
            PREVIEW_MAX_WIDTH,
            PREVIEW_MAX_HEIGHT,
            image::imageops::FilterType::Triangle,
        )
        .into_rgba8();
    PreviewData::new(
        thumbnail.width() as usize,
        thumbnail.height() as usize,
        thumbnail.into_raw(),
    )
}

fn preview_from_pdf_page<'a>(
    page: &'a hayro::hayro_syntax::page::Page<'a>,
    cache: &hayro::RenderCache<'a>,
    interpreter_settings: &hayro::hayro_interpret::InterpreterSettings,
    render_settings: &hayro::RenderSettings,
) -> Result<PreviewData> {
    let png = hayro::render(page, cache, interpreter_settings, render_settings)
        .into_png()
        .map_err(|error| anyhow!("could not encode rendered PDF preview: {error}"))?;
    let image = image::load_from_memory(&png).context("could not decode rendered PDF preview")?;
    Ok(preview_from_image(image))
}

pub fn export_pages(pages: &[PageItem], output_path: &Path) -> Result<ExportReport> {
    export_pages_with_settings(pages, output_path, &ExportSettings::default())
}

pub fn export_pages_with_settings(
    pages: &[PageItem],
    output_path: &Path,
    settings: &ExportSettings,
) -> Result<ExportReport> {
    export_pages_with_settings_and_passwords(pages, output_path, settings, &HashMap::new())
}

pub fn export_pages_with_settings_and_passwords(
    pages: &[PageItem],
    output_path: &Path,
    settings: &ExportSettings,
    passwords: &HashMap<PathBuf, Zeroizing<String>>,
) -> Result<ExportReport> {
    export_pages_with_settings_and_passwords_controlled(
        pages,
        output_path,
        settings,
        passwords,
        &mut |_, _| {},
        &|| false,
    )
}

pub fn export_pages_with_settings_and_passwords_controlled(
    pages: &[PageItem],
    output_path: &Path,
    settings: &ExportSettings,
    passwords: &HashMap<PathBuf, Zeroizing<String>>,
    progress: &mut dyn FnMut(usize, usize),
    cancelled: &dyn Fn() -> bool,
) -> Result<ExportReport> {
    if pages.is_empty() {
        bail!("add at least one page before exporting");
    }
    settings.validate()?;
    if cancelled() {
        bail!("export cancelled");
    }

    let mut pdf_cache: HashMap<PathBuf, Document> = HashMap::new();
    let mut documents = Vec::with_capacity(pages.len());
    let mut warnings = Vec::new();

    for (index, page) in pages.iter().enumerate() {
        if cancelled() {
            bail!("export cancelled");
        }
        let prepared = match &page.source {
            PageSource::Pdf { path, page_number } => {
                if !pdf_cache.contains_key(path) {
                    let mut loaded = Document::load(path)
                        .with_context(|| format!("could not load {}", path.display()))?;
                    unlock_pdf(
                        &mut loaded,
                        passwords.get(path).map(|password| password.as_str()),
                    )
                    .map_err(|error| anyhow!("{}: {error}", path.display()))?;
                    warnings.extend(catalog_structure_warnings(&loaded, path));
                    pdf_cache.insert(path.clone(), loaded);
                }
                let source = pdf_cache
                    .get(path)
                    .expect("a PDF inserted into the cache must be available")
                    .clone();
                let mut prepared = prepare_pdf_page(source, *page_number, path, &mut warnings)?;
                rotate_pdf_page(&mut prepared.document, page.rotation)?;
                prepared
            }
            PageSource::Image { path } => {
                PreparedDocument::plain(image_as_pdf(path, page.rotation, settings, &mut warnings)?)
            }
        };
        documents.push(prepared);
        progress(index + 1, pages.len());
    }

    if cancelled() {
        bail!("export cancelled");
    }
    let base_metadata = documents
        .iter()
        .find_map(|prepared| prepared.metadata.clone());
    let mut merged = merge_documents(documents)?;
    apply_pdf_metadata(&mut merged, settings, base_metadata.as_ref());
    merged.compress();
    if cancelled() {
        bail!("export cancelled");
    }
    merged
        .save(output_path)
        .with_context(|| format!("could not save {}", output_path.display()))?;

    Ok(ExportReport {
        path: output_path.to_path_buf(),
        page_count: pages.len(),
        warnings,
    })
}
fn unlock_pdf(
    document: &mut Document,
    password: Option<&str>,
) -> std::result::Result<(), PdfAccessError> {
    if !document.is_encrypted() {
        return Ok(());
    }

    let password_value = password.unwrap_or_default();
    let owner_authenticated = document.authenticate_owner_password(password_value).is_ok();
    let assembly_allowed = document
        .get_encrypted()
        .ok()
        .and_then(|dictionary| dictionary.get(b"P").ok())
        .and_then(|value| value.as_i64().ok())
        .is_some_and(|permissions| (permissions as u64 & (1 << 10)) != 0);

    match document.decrypt(password_value) {
        Ok(()) => {
            if owner_authenticated || assembly_allowed {
                Ok(())
            } else {
                Err(PdfAccessError::OwnerPasswordRequired)
            }
        }
        Err(lopdf::Error::Decryption(lopdf::encryption::DecryptionError::IncorrectPassword))
        | Err(lopdf::Error::InvalidPassword) => {
            if password.is_some() {
                Err(PdfAccessError::IncorrectPassword)
            } else {
                Err(PdfAccessError::PasswordRequired)
            }
        }
        Err(lopdf::Error::Decryption(error)) => {
            Err(PdfAccessError::UnsupportedEncryption(error.to_string()))
        }
        Err(lopdf::Error::UnsupportedSecurityHandler(handler)) => Err(
            PdfAccessError::UnsupportedEncryption(String::from_utf8_lossy(&handler).into_owned()),
        ),
        Err(error) => Err(PdfAccessError::UnsupportedEncryption(error.to_string())),
    }
}
fn prepare_pdf_page(
    source: Document,
    page_number: u32,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<PreparedDocument> {
    let page_id = source
        .get_pages()
        .get(&page_number)
        .copied()
        .with_context(|| format!("PDF page {page_number} no longer exists"))?;
    let metadata = extract_source_metadata(&source);
    let bookmarks = match extract_bookmarks_for_page(&source, page_id) {
        Ok(bookmarks) => bookmarks,
        Err(error) => {
            warnings.push(format!(
                "{} page {page_number}: bookmarks could not be read ({error})",
                path.display()
            ));
            Vec::new()
        }
    };
    let mut document = source;
    let disabled_links = disable_unsafe_internal_links(&mut document, page_id)?;
    if disabled_links > 0 {
        warnings.push(format!(
            "{} page {page_number}: disabled {disabled_links} internal link(s) whose destinations cannot be safely remapped",
            path.display()
        ));
    }
    let document = retain_single_page(document, page_number)?;

    Ok(PreparedDocument {
        document,
        bookmarks,
        metadata,
    })
}

fn extract_source_metadata(document: &Document) -> Option<Dictionary> {
    const KEYS: &[&[u8]] = &[
        b"Title",
        b"Author",
        b"Subject",
        b"Keywords",
        b"Creator",
        b"CreationDate",
        b"ModDate",
        b"Trapped",
    ];

    let info_object = document.trailer.get(b"Info").ok()?;
    let info = match info_object {
        Object::Dictionary(dictionary) => dictionary,
        Object::Reference(id) => document.get_dictionary(*id).ok()?,
        _ => return None,
    };
    let mut copied = Dictionary::new();
    for key in KEYS {
        if let Ok(value) = info.get(key)
            && matches!(
                value,
                Object::Boolean(_)
                    | Object::Integer(_)
                    | Object::Real(_)
                    | Object::Name(_)
                    | Object::String(_, _)
            )
        {
            copied.set(*key, value.clone());
        }
    }
    (!copied.is_empty()).then_some(copied)
}

fn extract_bookmarks_for_page(document: &Document, page_id: ObjectId) -> Result<Vec<String>> {
    if document
        .catalog()
        .ok()
        .is_none_or(|catalog| catalog.get(b"Outlines").is_err())
    {
        return Ok(Vec::new());
    }

    let mut named_destinations = IndexMap::new();
    let outlines = document
        .get_outlines(None, None, &mut named_destinations)?
        .unwrap_or_default();
    let mut titles = Vec::new();
    collect_bookmarks_for_page(&outlines, page_id, &mut titles);
    Ok(titles)
}

fn collect_bookmarks_for_page(outlines: &[Outline], page_id: ObjectId, titles: &mut Vec<String>) {
    for outline in outlines {
        match outline {
            Outline::Destination(destination) => {
                if destination.page().and_then(Object::as_reference).ok() == Some(page_id)
                    && let Ok(title) = destination.title().and_then(Object::as_str)
                {
                    titles.push(decode_pdf_text(title));
                }
            }
            Outline::SubOutlines(children) => {
                collect_bookmarks_for_page(children, page_id, titles);
            }
        }
    }
}

fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair));
        String::from_utf16_lossy(&units.collect::<Vec<_>>())
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn catalog_structure_warnings(document: &Document, path: &Path) -> Vec<String> {
    let Ok(catalog) = document.catalog() else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    let label = path.display();

    if catalog.get(b"Outlines").is_ok() {
        warnings.push(format!(
            "{label}: bookmarks targeting exported pages are preserved; hierarchy, styles, and zoom settings are simplified"
        ));
    }
    if catalog.get(b"AcroForm").is_ok() {
        warnings.push(format!(
            "{label}: interactive form fields may lose document-level behavior; page appearances are preserved"
        ));
    }
    if catalog.get(b"PageLabels").is_ok() {
        warnings.push(format!(
            "{label}: custom page labels are not preserved after page reordering"
        ));
    }
    if catalog.get(b"Names").is_ok() || catalog.get(b"Dests").is_ok() {
        warnings.push(format!(
            "{label}: document-level named destinations and name trees are rewritten or omitted"
        ));
    }
    if catalog.get(b"Metadata").is_ok() {
        warnings.push(format!(
            "{label}: XMP metadata is omitted; compatible document information fields are preserved"
        ));
    }
    if catalog.get(b"StructTreeRoot").is_ok() {
        warnings.push(format!(
            "{label}: the tagged-PDF structure tree cannot be safely preserved during page assembly"
        ));
    }
    if catalog.get(b"OCProperties").is_ok() {
        warnings.push(format!(
            "{label}: optional-content layer controls may not be preserved"
        ));
    }
    if catalog.get(b"OpenAction").is_ok() || catalog.get(b"AA").is_ok() {
        warnings.push(format!(
            "{label}: document-level open/additional actions are omitted"
        ));
    }

    warnings
}

fn materialize_inherited_page_attributes(document: &mut Document, page_id: ObjectId) -> Result<()> {
    const INHERITED_KEYS: &[&[u8]] = &[
        b"Resources",
        b"MediaBox",
        b"CropBox",
        b"BleedBox",
        b"TrimBox",
        b"ArtBox",
        b"Rotate",
        b"UserUnit",
    ];

    let page = document
        .get_dictionary(page_id)
        .context("selected PDF page is not a dictionary")?;
    let mut missing = INHERITED_KEYS
        .iter()
        .copied()
        .filter(|key| page.get(key).is_err())
        .collect::<Vec<_>>();
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    let mut inherited = Vec::new();
    let mut visited = Vec::new();

    while let Some(parent_id) = parent {
        if visited.contains(&parent_id) {
            bail!("selected PDF has a cyclic page tree");
        }
        visited.push(parent_id);
        let parent_dictionary = document
            .get_dictionary(parent_id)
            .context("selected PDF has an invalid page parent")?;
        let mut found = Vec::new();
        for key in &missing {
            if let Ok(value) = parent_dictionary.get(key) {
                inherited.push((key.to_vec(), value.clone()));
                found.push(*key);
            }
        }
        missing.retain(|key| !found.contains(key));
        if missing.is_empty() {
            break;
        }
        parent = parent_dictionary
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok();
    }

    let page = document
        .get_dictionary_mut(page_id)
        .context("could not update selected PDF page")?;
    for (key, value) in inherited {
        page.set(key, value);
    }
    Ok(())
}

fn disable_unsafe_internal_links(document: &mut Document, page_id: ObjectId) -> Result<usize> {
    let annotations = document
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
        .cloned();
    let mut referenced_annotations = Vec::new();
    let mut rewritten = 0;

    match annotations {
        Some(Object::Array(mut annotations)) => {
            rewritten += rewrite_annotation_array(
                document,
                &mut annotations,
                page_id,
                &mut referenced_annotations,
            );
            document
                .get_dictionary_mut(page_id)
                .context("could not update selected PDF annotations")?
                .set("Annots", Object::Array(annotations));
        }
        Some(Object::Reference(array_id)) => {
            if let Some(Object::Array(mut annotations)) = document.objects.get(&array_id).cloned() {
                rewritten += rewrite_annotation_array(
                    document,
                    &mut annotations,
                    page_id,
                    &mut referenced_annotations,
                );
                document.set_object(array_id, Object::Array(annotations));
            }
        }
        _ => {}
    }

    for annotation_id in referenced_annotations {
        let decision = document
            .get_dictionary(annotation_id)
            .ok()
            .map(|annotation| link_rewrite_decision(document, annotation, page_id));
        if let Some((remove_destination, remove_action)) = decision
            && (remove_destination || remove_action)
        {
            rewrite_link_dictionary(
                document
                    .get_dictionary_mut(annotation_id)
                    .context("could not update a link annotation")?,
                remove_destination,
                remove_action,
            );
            rewritten += 1;
        }
    }
    Ok(rewritten)
}

fn rewrite_annotation_array(
    document: &Document,
    annotations: &mut [Object],
    page_id: ObjectId,
    referenced_annotations: &mut Vec<ObjectId>,
) -> usize {
    let mut rewritten = 0;
    for annotation in annotations {
        match annotation {
            Object::Reference(id) => referenced_annotations.push(*id),
            Object::Dictionary(dictionary) => {
                let (remove_destination, remove_action) =
                    link_rewrite_decision(document, dictionary, page_id);
                if remove_destination || remove_action {
                    rewrite_link_dictionary(dictionary, remove_destination, remove_action);
                    rewritten += 1;
                }
            }
            _ => {}
        }
    }
    rewritten
}

fn link_rewrite_decision(
    document: &Document,
    annotation: &Dictionary,
    page_id: ObjectId,
) -> (bool, bool) {
    if annotation.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link") {
        return (false, false);
    }
    let remove_destination = annotation
        .get(b"Dest")
        .ok()
        .is_some_and(|destination| destination_page(document, destination, 0) != Some(page_id));
    let remove_action = annotation
        .get(b"A")
        .ok()
        .is_some_and(|action| unsafe_goto_action(document, action, page_id));
    (remove_destination, remove_action)
}

fn rewrite_link_dictionary(
    annotation: &mut Dictionary,
    remove_destination: bool,
    remove_action: bool,
) {
    if remove_destination {
        annotation.remove(b"Dest");
    }
    if remove_action {
        annotation.remove(b"A");
    }
}

fn unsafe_goto_action(document: &Document, action: &Object, page_id: ObjectId) -> bool {
    let action = match action {
        Object::Dictionary(action) => Some(action),
        Object::Reference(id) => document.get_dictionary(*id).ok(),
        _ => None,
    };
    let Some(action) = action else {
        return false;
    };
    if action.get(b"S").and_then(Object::as_name).ok() != Some(b"GoTo") {
        return false;
    }
    action
        .get(b"D")
        .ok()
        .is_none_or(|destination| destination_page(document, destination, 0) != Some(page_id))
}

fn destination_page(document: &Document, destination: &Object, depth: usize) -> Option<ObjectId> {
    if depth > 8 {
        return None;
    }
    match destination {
        Object::Array(items) => items.first().and_then(|item| item.as_reference().ok()),
        Object::Reference(id) => {
            if document.get_pages().values().any(|page_id| page_id == id) {
                Some(*id)
            } else {
                document
                    .get_object(*id)
                    .ok()
                    .and_then(|resolved| destination_page(document, resolved, depth + 1))
            }
        }
        Object::Dictionary(dictionary) => dictionary
            .get(b"D")
            .ok()
            .and_then(|resolved| destination_page(document, resolved, depth + 1)),
        _ => None,
    }
}
fn retain_single_page(mut document: Document, page_number: u32) -> Result<Document> {
    let pages = document.get_pages();
    if !pages.contains_key(&page_number) {
        bail!("PDF page {page_number} no longer exists");
    }

    let page_id = pages[&page_number];
    materialize_inherited_page_attributes(&mut document, page_id)?;
    let to_delete = pages
        .keys()
        .copied()
        .filter(|number| *number != page_number)
        .collect::<Vec<_>>();
    document.delete_pages(&to_delete);
    document.prune_objects();
    Ok(document)
}

fn rotate_pdf_page(document: &mut Document, rotation: PageRotation) -> Result<()> {
    if rotation == PageRotation::Deg0 {
        return Ok(());
    }
    let page_id = document
        .get_pages()
        .into_values()
        .next()
        .context("selected PDF contains no page")?;
    let page = document
        .get_dictionary_mut(page_id)
        .context("could not update selected PDF page")?;
    let current = page
        .get(b"Rotate")
        .and_then(Object::as_i64)
        .unwrap_or_default();
    page.set("Rotate", (current + rotation.degrees()).rem_euclid(360));
    Ok(())
}

fn image_as_pdf(
    path: &Path,
    rotation: PageRotation,
    settings: &ExportSettings,
    warning_messages: &mut Vec<String>,
) -> Result<Document> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let decoded = image::load_from_memory(&bytes)
        .with_context(|| format!("could not decode image {}", path.display()))?;
    let mut decoded = match rotation {
        PageRotation::Deg0 => decoded,
        PageRotation::Deg90 => decoded.rotate90(),
        PageRotation::Deg180 => decoded.rotate180(),
        PageRotation::Deg270 => decoded.rotate270(),
    };
    let layout_width = decoded.width();
    let layout_height = decoded.height();
    if let Some(maximum) = settings.max_image_dimension
        && decoded.width().max(decoded.height()) > maximum
    {
        decoded = decoded.resize(maximum, maximum, image::imageops::FilterType::Lanczos3);
    }

    let raw = RawImage::from_dynamic_image(image::DynamicImage::ImageRgba8(decoded.to_rgba8()))
        .map_err(|error| anyhow!("could not prepare {}: {error}", path.display()))?;
    let image_width = raw.width as f32;
    let image_height = raw.height as f32;
    let (page_width_mm, page_height_mm) =
        image_page_dimensions(settings, layout_width, layout_height);
    let points_per_mm = 72.0_f32 / 25.4_f32;
    let available_width = (page_width_mm - settings.margin_mm * 2.0) * points_per_mm;
    let available_height = (page_height_mm - settings.margin_mm * 2.0) * points_per_mm;
    let scale = (available_width / image_width).min(available_height / image_height);
    let rendered_width = image_width * scale;
    let rendered_height = image_height * scale;
    let page_width_points = page_width_mm * points_per_mm;
    let page_height_points = page_height_mm * points_per_mm;

    let mut pdf = PdfDocument::new(&display_name(path));
    let image_id = pdf.add_image(&raw);
    let page = PdfPage::new(
        Mm(page_width_mm),
        Mm(page_height_mm),
        vec![Op::UseXobject {
            id: image_id,
            transform: XObjectTransform {
                translate_x: Some(Pt((page_width_points - rendered_width) / 2.0)),
                translate_y: Some(Pt((page_height_points - rendered_height) / 2.0)),
                scale_x: Some(scale),
                scale_y: Some(scale),
                dpi: Some(72.0),
                ..Default::default()
            },
        }],
    );
    pdf.with_pages(vec![page]);

    let image_optimization = match settings.preset {
        ExportPreset::Lossless => ImageOptimizationOptions {
            quality: None,
            max_image_size: None,
            dither_greyscale: None,
            convert_to_greyscale: Some(false),
            auto_optimize: Some(false),
            format: Some(ImageCompression::Flate),
        },
        ExportPreset::Balanced => ImageOptimizationOptions {
            quality: Some(settings.image_quality as f32 / 100.0),
            max_image_size: None,
            dither_greyscale: None,
            convert_to_greyscale: Some(false),
            auto_optimize: Some(true),
            format: Some(ImageCompression::Auto),
        },
        ExportPreset::SmallerFile => ImageOptimizationOptions {
            quality: Some(settings.image_quality as f32 / 100.0),
            max_image_size: None,
            dither_greyscale: None,
            convert_to_greyscale: Some(false),
            auto_optimize: Some(true),
            format: Some(ImageCompression::Jpeg),
        },
    };
    let options = PdfSaveOptions {
        image_optimization: Some(image_optimization),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    let document = pdf.to_lopdf_document(&options, &mut warnings);
    if !warnings.is_empty() {
        warning_messages.push(format!(
            "{} generated {} PDF warning(s)",
            path.display(),
            warnings.len()
        ));
    }
    Ok(document)
}

fn image_page_dimensions(
    settings: &ExportSettings,
    image_width: u32,
    image_height: u32,
) -> (f32, f32) {
    match settings.image_page_policy {
        ImagePagePolicy::A4Auto => {
            if image_width > image_height {
                (297.0, 210.0)
            } else {
                (210.0, 297.0)
            }
        }
        ImagePagePolicy::OriginalAtDpi => (
            image_width as f32 / settings.original_dpi * 25.4 + settings.margin_mm * 2.0,
            image_height as f32 / settings.original_dpi * 25.4 + settings.margin_mm * 2.0,
        ),
        ImagePagePolicy::Custom => (settings.custom_width_mm, settings.custom_height_mm),
    }
}

fn apply_pdf_metadata(
    document: &mut Document,
    settings: &ExportSettings,
    source_metadata: Option<&Dictionary>,
) {
    let metadata = &settings.metadata;
    let mut info = source_metadata.cloned().unwrap_or_default();
    if !metadata.title.trim().is_empty() {
        info.set("Title", Object::string_literal(metadata.title.trim()));
    }
    if !metadata.author.trim().is_empty() {
        info.set("Author", Object::string_literal(metadata.author.trim()));
    }
    if !metadata.subject.trim().is_empty() {
        info.set("Subject", Object::string_literal(metadata.subject.trim()));
    }
    if !metadata.keywords.trim().is_empty() {
        info.set("Keywords", Object::string_literal(metadata.keywords.trim()));
    }
    info.set("Producer", Object::string_literal("PdfMerger"));
    let info_id = document.add_object(info);
    document.trailer.set("Info", info_id);
}

fn merge_documents(documents: Vec<PreparedDocument>) -> Result<Document> {
    if documents.is_empty() {
        bail!("there are no documents to merge");
    }

    let mut output = Document::with_version("1.7");
    let mut next_object_id = 1;
    let mut page_tree_roots = Vec::with_capacity(documents.len());
    let mut total_pages = 0_u32;

    for prepared in documents {
        let PreparedDocument {
            mut document,
            bookmarks,
            ..
        } = prepared;
        document.renumber_objects_with(next_object_id);
        next_object_id = document.max_id + 1;
        total_pages += document.get_pages().len() as u32;
        let page_id = document
            .get_pages()
            .into_values()
            .next()
            .context("prepared PDF contains no page")?;
        for title in bookmarks {
            output.add_bookmark(Bookmark::new(title, [0.0, 0.0, 0.0], 0, page_id), None);
        }

        let catalog_id = document
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .context("source PDF has no catalog")?;
        let source_pages_id = document
            .get_dictionary(catalog_id)
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
            .context("source PDF has no page tree")?;
        page_tree_roots.push(source_pages_id);

        output.objects.extend(document.objects);
    }

    output.max_id = next_object_id.saturating_sub(1);
    let pages_id = output.new_object_id();
    for root_id in &page_tree_roots {
        let root = output
            .get_dictionary_mut(*root_id)
            .context("could not update a source page tree")?;
        root.set("Parent", pages_id);
    }

    output.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_tree_roots.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => total_pages,
        }),
    );
    let mut catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLayout" => "OneColumn",
    };
    if let Some(outlines_id) = output.build_outline() {
        catalog.set("Outlines", outlines_id);
    }
    let catalog_id = output.add_object(catalog);
    output.trailer.set("Root", catalog_id);
    output.prune_objects();
    output.renumber_objects();
    Ok(output)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encrypted_blank_document(
        owner_password: &str,
        user_password: &str,
        permissions: lopdf::encryption::Permissions,
    ) -> Vec<u8> {
        use lopdf::encryption::{EncryptionState, EncryptionVersion};

        let mut pdf = PdfDocument::new("encrypted test");
        pdf.with_pages(vec![PdfPage::new(Mm(105.0), Mm(148.0), Vec::new())]);
        let mut warnings = Vec::new();
        let mut document = pdf.to_lopdf_document(&PdfSaveOptions::default(), &mut warnings);
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal("pdf-merger-test-id"),
                Object::string_literal("pdf-merger-test-id"),
            ]),
        );
        document.max_id = document
            .objects
            .keys()
            .map(|(object_number, _)| *object_number)
            .max()
            .unwrap_or_default();
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &document,
            owner_password,
            user_password,
            key_length: 128,
            permissions,
        })
        .unwrap();
        document.encrypt(&state).unwrap();
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn distinguishes_required_incorrect_and_valid_pdf_passwords() {
        use lopdf::encryption::Permissions;

        let bytes =
            encrypted_blank_document("owner-secret", "user-secret", Permissions::ASSEMBLABLE);
        let mut missing = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            unlock_pdf(&mut missing, None),
            Err(PdfAccessError::PasswordRequired)
        );

        let mut incorrect = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            unlock_pdf(&mut incorrect, Some("wrong")),
            Err(PdfAccessError::IncorrectPassword)
        );

        let mut valid = Document::load_mem(&bytes).unwrap();
        assert_eq!(unlock_pdf(&mut valid, Some("user-secret")), Ok(()));
        assert!(!valid.is_encrypted());
        assert!(valid.was_encrypted());
    }

    #[test]
    fn requires_owner_password_when_page_assembly_is_forbidden() {
        use lopdf::encryption::Permissions;

        let bytes = encrypted_blank_document("owner-secret", "user-secret", Permissions::empty());
        let mut user = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            unlock_pdf(&mut user, Some("user-secret")),
            Err(PdfAccessError::OwnerPasswordRequired)
        );

        let mut owner = Document::load_mem(&bytes).unwrap();
        assert_eq!(unlock_pdf(&mut owner, Some("owner-secret")), Ok(()));
    }

    #[test]
    fn calculates_custom_and_original_image_page_sizes() {
        let custom = ExportSettings {
            image_page_policy: ImagePagePolicy::Custom,
            custom_width_mm: 120.0,
            custom_height_mm: 180.0,
            ..Default::default()
        };
        assert_eq!(image_page_dimensions(&custom, 1000, 500), (120.0, 180.0));

        let original = ExportSettings {
            image_page_policy: ImagePagePolicy::OriginalAtDpi,
            original_dpi: 100.0,
            margin_mm: 10.0,
            ..Default::default()
        };
        let (width, height) = image_page_dimensions(&original, 1000, 500);
        assert!((width - 274.0).abs() < 0.01);
        assert!((height - 147.0).abs() < 0.01);
    }

    #[test]
    fn writes_configured_pdf_metadata() {
        let mut document = Document::with_version("1.7");
        let settings = ExportSettings {
            metadata: crate::export_settings::PdfMetadata {
                title: "Quarterly packet".to_owned(),
                author: "PdfMerger tests".to_owned(),
                subject: "Metadata".to_owned(),
                keywords: "pdf, merge".to_owned(),
            },
            ..Default::default()
        };

        apply_pdf_metadata(&mut document, &settings, None);

        let info_id = document
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .unwrap();
        let info = document.get_dictionary(info_id).unwrap();
        assert_eq!(
            info.get(b"Title").and_then(Object::as_str).unwrap(),
            b"Quarterly packet"
        );
        assert_eq!(
            info.get(b"Author").and_then(Object::as_str).unwrap(),
            b"PdfMerger tests"
        );
    }

    #[test]
    fn recognizes_supported_files_case_insensitively() {
        assert!(is_supported(Path::new("document.PDF")));
        assert!(is_supported(Path::new("photo.JpEg")));
        assert!(!is_supported(Path::new("notes.txt")));
    }

    #[test]
    fn merges_documents_without_flattening_page_trees() {
        fn blank_document() -> Document {
            let mut document = Document::with_version("1.5");
            let pages_id = document.new_object_id();
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
            });
            document.objects.insert(
                pages_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Kids" => vec![Object::Reference(page_id)],
                    "Count" => 1,
                }),
            );
            let catalog_id = document.add_object(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            });
            document.trailer.set("Root", catalog_id);
            document
        }

        let merged = merge_documents(vec![
            PreparedDocument::plain(blank_document()),
            PreparedDocument::plain(blank_document()),
        ])
        .unwrap();
        assert_eq!(merged.get_pages().len(), 2);
    }

    #[test]
    fn applies_clockwise_rotation_to_a_pdf_page() {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);

        rotate_pdf_page(&mut document, PageRotation::Deg90).unwrap();

        let rotation = document
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Rotate")
            .and_then(Object::as_i64)
            .unwrap();
        assert_eq!(rotation, 90);
    }

    #[test]
    fn exports_images_with_the_smaller_file_preset() {
        use crate::{export_settings::ExportPreset, model::Workspace};
        use image::{Rgba, RgbaImage};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = format!("pdf-merger-small-test-{}-{nonce}", std::process::id());
        let image_path = std::env::temp_dir().join(format!("{base}.png"));
        let output_path = std::env::temp_dir().join(format!("{base}.pdf"));
        RgbaImage::from_pixel(640, 320, Rgba([94, 106, 210, 220]))
            .save(&image_path)
            .unwrap();
        let mut workspace = Workspace::default();
        workspace.append(import_file(&image_path).unwrap());
        let mut settings = ExportSettings::default();
        settings.apply_preset(ExportPreset::SmallerFile);
        settings.max_image_dimension = Some(256);

        export_pages_with_settings(workspace.pages(), &output_path, &settings).unwrap();

        assert_eq!(Document::load(&output_path).unwrap().get_pages().len(), 1);
        let _ = fs::remove_file(image_path);
        let _ = fs::remove_file(output_path);
    }

    #[test]
    fn controlled_export_reports_progress_and_cleans_up_on_cancellation() {
        use crate::model::Workspace;
        use image::{Rgb, RgbImage};
        use std::{
            cell::Cell,
            time::{SystemTime, UNIX_EPOCH},
        };

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = format!("pdf-merger-cancel-test-{}-{nonce}", std::process::id());
        let first_path = std::env::temp_dir().join(format!("{base}-1.png"));
        let second_path = std::env::temp_dir().join(format!("{base}-2.png"));
        let output_path = std::env::temp_dir().join(format!("{base}.pdf"));
        RgbImage::from_pixel(16, 16, Rgb([20, 40, 60]))
            .save(&first_path)
            .unwrap();
        RgbImage::from_pixel(16, 16, Rgb([80, 100, 120]))
            .save(&second_path)
            .unwrap();
        let mut workspace = Workspace::default();
        workspace.append(import_file(&first_path).unwrap());
        workspace.append(import_file(&second_path).unwrap());

        let cancel = Cell::new(false);
        let mut observed = Vec::new();
        let result = export_pages_with_settings_and_passwords_controlled(
            workspace.pages(),
            &output_path,
            &ExportSettings::default(),
            &HashMap::new(),
            &mut |completed, total| {
                observed.push((completed, total));
                cancel.set(true);
            },
            &|| cancel.get(),
        );

        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(observed, vec![(1, 2)]);
        assert!(!output_path.exists());
        let _ = fs::remove_file(first_path);
        let _ = fs::remove_file(second_path);
    }
    #[test]
    fn converts_an_image_into_a_readable_pdf() {
        use crate::model::Workspace;
        use image::{Rgb, RgbImage};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = format!("pdf-merger-test-{}-{nonce}", std::process::id());
        let image_path = std::env::temp_dir().join(format!("{base}.png"));
        let output_path = std::env::temp_dir().join(format!("{base}.pdf"));

        RgbImage::from_pixel(32, 16, Rgb([94, 106, 210]))
            .save(&image_path)
            .unwrap();
        let drafts = import_file(&image_path).unwrap();
        let preview = drafts[0].preview.as_ref().unwrap();
        assert_eq!(preview.size, [312, 156]);
        assert_eq!(preview.rgba.len(), 312 * 156 * 4);

        let mut workspace = Workspace::default();
        workspace.append(drafts);

        let report = export_pages(workspace.pages(), &output_path).unwrap();
        assert_eq!(report.page_count, 1);
        assert_eq!(Document::load(&output_path).unwrap().get_pages().len(), 1);

        let pdf_drafts = import_file(&output_path).unwrap();
        let pdf_preview = pdf_drafts[0].preview.as_ref().unwrap();
        assert_eq!(
            pdf_preview.rgba.len(),
            pdf_preview.size[0] * pdf_preview.size[1] * 4
        );
        assert!(pdf_preview.rgba.as_chunks::<4>().0.iter().any(|pixel| {
            pixel[0] > 70 && pixel[0] < 120 && pixel[1] > 80 && pixel[1] < 130 && pixel[2] > 180
        }));

        let _ = fs::remove_file(image_path);
        let _ = fs::remove_file(output_path);
    }

    #[test]
    fn materializes_inherited_page_resources_and_boxes_before_pruning() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let resources_id =
            document.add_object(dictionary! { "ProcSet" => vec![Object::Name(b"PDF".to_vec())] });
        let first_page =
            document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        let second_page =
            document.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(first_page), Object::Reference(second_page)],
                "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "CropBox" => vec![10.into(), 20.into(), 600.into(), 770.into()],
                "Resources" => resources_id,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);

        let retained = retain_single_page(document, 2).unwrap();
        let retained_page = retained.get_dictionary(retained.get_pages()[&1]).unwrap();
        assert!(retained_page.get(b"MediaBox").is_ok());
        assert!(retained_page.get(b"CropBox").is_ok());
        assert_eq!(
            retained_page
                .get(b"Resources")
                .and_then(Object::as_reference)
                .unwrap(),
            resources_id
        );
    }

    #[test]
    fn keeps_external_links_and_disables_unmappable_internal_links() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let second_page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
        });
        let uri_link = document.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Link",
            "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
            "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") },
        });
        let internal_link = document.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Link",
            "Rect" => vec![20.into(), 0.into(), 40.into(), 20.into()],
            "A" => dictionary! {
                "S" => "GoTo",
                "D" => vec![Object::Reference(second_page), Object::Name(b"Fit".to_vec())],
            },
        });
        let first_page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
            "Annots" => vec![Object::Reference(uri_link), Object::Reference(internal_link)],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(first_page), Object::Reference(second_page)],
                "Count" => 2,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);

        assert_eq!(
            disable_unsafe_internal_links(&mut document, first_page).unwrap(),
            1
        );
        assert!(document.get_dictionary(uri_link).unwrap().get(b"A").is_ok());
        assert!(
            document
                .get_dictionary(internal_link)
                .unwrap()
                .get(b"A")
                .is_err()
        );
    }

    #[test]
    fn preserves_source_metadata_unless_export_settings_override_it() {
        let mut document = Document::with_version("1.7");
        let mut source = Dictionary::new();
        source.set("Title", Object::string_literal("Original title"));
        source.set("Creator", Object::string_literal("Source application"));
        source.set("Author", Object::string_literal("Original author"));
        let settings = ExportSettings {
            metadata: crate::export_settings::PdfMetadata {
                author: "New author".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        apply_pdf_metadata(&mut document, &settings, Some(&source));

        let info_id = document
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .unwrap();
        let info = document.get_dictionary(info_id).unwrap();
        assert_eq!(
            info.get(b"Title").and_then(Object::as_str).unwrap(),
            b"Original title"
        );
        assert_eq!(
            info.get(b"Creator").and_then(Object::as_str).unwrap(),
            b"Source application"
        );
        assert_eq!(
            info.get(b"Author").and_then(Object::as_str).unwrap(),
            b"New author"
        );
    }

    #[test]
    fn rebuilds_bookmarks_for_remapped_output_pages_and_warns_about_catalog_data() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
            "AcroForm" => dictionary! {}, "PageLabels" => dictionary! {},
        });
        document.trailer.set("Root", catalog_id);
        document.add_bookmark(
            Bookmark::new("Chapter one".to_owned(), [0.2, 0.3, 0.4], 2, page_id),
            None,
        );
        let outlines_id = document.build_outline().unwrap();
        document.catalog_mut().unwrap().set("Outlines", outlines_id);
        let warnings = catalog_structure_warnings(&document, Path::new("structured.pdf"));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("form fields"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("page labels"))
        );
        assert!(warnings.iter().any(|warning| warning.contains("bookmarks")));

        let mut preparation_warnings = Vec::new();
        let prepared = prepare_pdf_page(
            document,
            1,
            Path::new("structured.pdf"),
            &mut preparation_warnings,
        )
        .unwrap();
        assert_eq!(prepared.bookmarks, vec!["Chapter one"]);
        let merged = merge_documents(vec![prepared]).unwrap();
        let output_page_id = merged.get_pages()[&1];
        assert_eq!(
            extract_bookmarks_for_page(&merged, output_page_id).unwrap(),
            vec!["Chapter one"]
        );
    }
}
