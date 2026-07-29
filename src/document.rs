use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use lopdf::{Document, Object, dictionary};
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

#[derive(Debug)]
pub struct ExportReport {
    pub path: PathBuf,
    pub page_count: usize,
    pub warnings: Vec<String>,
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
    if !path.is_file() {
        bail!("{} is not a file", path.display());
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "pdf" {
        import_pdf(path)
    } else if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        import_image(path)
    } else {
        bail!("unsupported file type: {}", path.display());
    }
}

fn import_pdf(path: &Path) -> Result<Vec<PageDraft>> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let source = Document::load_mem(&bytes)
        .with_context(|| format!("could not parse {}", path.display()))?;
    let page_count = source.get_pages().len();
    if page_count == 0 {
        bail!("{} contains no pages", path.display());
    }

    // Hayro rasterizes the original PDF directly. It is pure Rust and embeds
    // its standard fonts/CMaps, so previews remain native and offline.
    let preview_pdf = hayro::hayro_syntax::Pdf::new(bytes).ok();
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
    if pages.is_empty() {
        bail!("add at least one page before exporting");
    }
    settings.validate()?;

    let mut pdf_cache: HashMap<PathBuf, Document> = HashMap::new();
    let mut documents = Vec::with_capacity(pages.len());
    let mut warnings = Vec::new();

    for page in pages {
        let document = match &page.source {
            PageSource::Pdf { path, page_number } => {
                if !pdf_cache.contains_key(path) {
                    let loaded = Document::load(path)
                        .with_context(|| format!("could not load {}", path.display()))?;
                    pdf_cache.insert(path.clone(), loaded);
                }
                let source = pdf_cache
                    .get(path)
                    .expect("a PDF inserted into the cache must be available")
                    .clone();
                let mut document = retain_single_page(source, *page_number)?;
                rotate_pdf_page(&mut document, page.rotation)?;
                document
            }
            PageSource::Image { path } => {
                image_as_pdf(path, page.rotation, settings, &mut warnings)?
            }
        };
        documents.push(document);
    }

    let mut merged = merge_documents(documents)?;
    apply_pdf_metadata(&mut merged, settings);
    merged.compress();
    merged
        .save(output_path)
        .with_context(|| format!("could not save {}", output_path.display()))?;

    Ok(ExportReport {
        path: output_path.to_path_buf(),
        page_count: pages.len(),
        warnings,
    })
}
fn retain_single_page(mut document: Document, page_number: u32) -> Result<Document> {
    let pages = document.get_pages();
    if !pages.contains_key(&page_number) {
        bail!("PDF page {page_number} no longer exists");
    }

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

fn apply_pdf_metadata(document: &mut Document, settings: &ExportSettings) {
    let metadata = &settings.metadata;
    let mut info = lopdf::Dictionary::new();
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
fn merge_documents(documents: Vec<Document>) -> Result<Document> {
    if documents.is_empty() {
        bail!("there are no documents to merge");
    }

    let mut output = Document::with_version("1.7");
    let mut next_object_id = 1;
    let mut page_tree_roots = Vec::with_capacity(documents.len());
    let mut total_pages = 0_u32;

    for mut source in documents {
        source.renumber_objects_with(next_object_id);
        next_object_id = source.max_id + 1;
        total_pages += source.get_pages().len() as u32;

        let catalog_id = source
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .context("source PDF has no catalog")?;
        let pages_id = source
            .get_dictionary(catalog_id)
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
            .context("source PDF has no page tree")?;
        page_tree_roots.push(pages_id);

        output.objects.extend(source.objects);
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
    let catalog_id = output.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLayout" => "OneColumn",
    });
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

        apply_pdf_metadata(&mut document, &settings);

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

        let merged = merge_documents(vec![blank_document(), blank_document()]).unwrap();
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
        assert!(pdf_preview.rgba.chunks_exact(4).any(|pixel| {
            pixel[0] > 70 && pixel[0] < 120 && pixel[1] > 80 && pixel[1] < 130 && pixel[2] > 180
        }));

        let _ = fs::remove_file(image_path);
        let _ = fs::remove_file(output_path);
    }
}
