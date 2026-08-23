//! Experimental, local-only document summarization primitives.
//!
//! This module currently contains bounded text extraction only. It deliberately has no model or
//! network dependency.

use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use lopdf::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryLength {
    Short,
    Standard,
    Detailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryAudience {
    General,
    Technical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryRequest {
    pub document: ExtractedDocument,
    pub length: SummaryLength,
    pub audience: SummaryAudience,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryResult {
    pub text: String,
    pub cited_pages: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelConfig {
    pub id: String,
    pub path: std::path::PathBuf,
    pub context_size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendDiagnostics {
    pub runtime: String,
    pub accelerator: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryPhase {
    LoadingModel,
    Generating,
    UnloadingModel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryProgress {
    pub phase: SummaryPhase,
    pub completed: usize,
    pub total: usize,
}

pub trait SummarizationBackend {
    fn load(&mut self, model: &ModelConfig) -> Result<BackendDiagnostics>;

    fn summarize(
        &mut self,
        request: &SummaryRequest,
        is_cancelled: &dyn Fn() -> bool,
        report_progress: &mut dyn FnMut(SummaryProgress),
    ) -> Result<SummaryResult>;

    fn unload(&mut self) -> Result<()>;
}

pub fn run_summary_job(
    backend: &mut dyn SummarizationBackend,
    model: &ModelConfig,
    request: &SummaryRequest,
    is_cancelled: &dyn Fn() -> bool,
    report_progress: &mut dyn FnMut(SummaryProgress),
) -> Result<(SummaryResult, BackendDiagnostics)> {
    if is_cancelled() {
        bail!("summarization cancelled before model loading");
    }
    report_progress(SummaryProgress {
        phase: SummaryPhase::LoadingModel,
        completed: 0,
        total: 1,
    });
    let diagnostics = backend.load(model)?;
    report_progress(SummaryProgress {
        phase: SummaryPhase::LoadingModel,
        completed: 1,
        total: 1,
    });

    let generation = if is_cancelled() {
        Err(anyhow!("summarization cancelled after model loading"))
    } else {
        backend.summarize(request, is_cancelled, report_progress)
    };

    report_progress(SummaryProgress {
        phase: SummaryPhase::UnloadingModel,
        completed: 0,
        total: 1,
    });
    let unloading = backend.unload();
    report_progress(SummaryProgress {
        phase: SummaryPhase::UnloadingModel,
        completed: 1,
        total: 1,
    });

    match (generation, unloading) {
        (Ok(summary), Ok(())) => Ok((summary, diagnostics)),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("summary completed but model unloading failed"),
        (Err(generation_error), Err(unload_error)) => {
            Err(generation_error).context(format!("model unloading also failed: {unload_error:#}"))
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExtractionLimits {
    pub max_file_bytes: u64,
    pub max_decompressed_bytes_per_page: usize,
    pub max_characters_per_page: usize,
    pub max_characters_per_document: usize,
    pub minimum_searchable_characters: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 512 * 1024 * 1024,
            max_decompressed_bytes_per_page: 16 * 1024 * 1024,
            max_characters_per_page: 100_000,
            max_characters_per_document: 2_000_000,
            minimum_searchable_characters: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedPage {
    pub page_number: u32,
    pub text: String,
    pub has_searchable_text: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedDocument {
    pub pages: Vec<ExtractedPage>,
    pub total_characters: usize,
    pub truncated: bool,
}

pub fn extract_pdf_text(
    path: &Path,
    password: Option<&str>,
    requested_pages: Option<&[u32]>,
    limits: ExtractionLimits,
) -> Result<ExtractedDocument> {
    validate_limits(limits)?;
    let metadata =
        fs::metadata(path).with_context(|| format!("could not inspect {}", path.display()))?;
    if metadata.len() > limits.max_file_bytes {
        bail!(
            "{} is too large for summarization ({} bytes; limit is {} bytes)",
            path.display(),
            metadata.len(),
            limits.max_file_bytes
        );
    }

    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let mut document = Document::load_mem(&bytes)
        .with_context(|| format!("could not parse {}", path.display()))?;
    if document.is_encrypted() {
        let password = password.ok_or_else(|| anyhow!("this PDF requires a password"))?;
        document
            .decrypt(password)
            .map_err(|_| anyhow!("the PDF password is incorrect or encryption is unsupported"))?;
    }

    let available_pages = document.get_pages();
    let page_numbers = match requested_pages {
        Some(pages) => {
            if pages.is_empty() {
                bail!("at least one page must be selected");
            }
            let mut pages = pages.to_vec();
            pages.sort_unstable();
            pages.dedup();
            if let Some(page) = pages
                .iter()
                .find(|page| !available_pages.contains_key(page))
            {
                bail!("page {page} does not exist in {}", path.display());
            }
            pages
        }
        None => available_pages.keys().copied().collect(),
    };

    let mut pages = Vec::with_capacity(page_numbers.len());
    let mut total_characters = 0usize;
    let mut document_truncated = false;

    for page_number in page_numbers {
        let remaining = limits
            .max_characters_per_document
            .saturating_sub(total_characters);
        if remaining == 0 {
            document_truncated = true;
            break;
        }

        let raw = document
            .extract_text_with_limit(&[page_number], limits.max_decompressed_bytes_per_page)
            .with_context(|| format!("could not extract text from page {page_number}"))?;
        let normalized = normalize_text(&raw);
        let allowed = remaining.min(limits.max_characters_per_page);
        let (text, truncated) = truncate_characters(normalized, allowed);
        let searchable_characters = text
            .chars()
            .filter(|character| character.is_alphanumeric())
            .count();
        total_characters += text.chars().count();
        document_truncated |= truncated;
        pages.push(ExtractedPage {
            page_number,
            text,
            has_searchable_text: searchable_characters >= limits.minimum_searchable_characters,
            truncated,
        });
    }

    Ok(ExtractedDocument {
        pages,
        total_characters,
        truncated: document_truncated,
    })
}

fn validate_limits(limits: ExtractionLimits) -> Result<()> {
    if limits.max_file_bytes == 0
        || limits.max_decompressed_bytes_per_page == 0
        || limits.max_characters_per_page == 0
        || limits.max_characters_per_document == 0
    {
        bail!("text extraction limits must be greater than zero");
    }
    Ok(())
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

fn truncate_characters(text: String, limit: usize) -> (String, bool) {
    let mut boundaries = text.char_indices();
    let cutoff = boundaries.nth(limit).map(|(index, _)| index);
    match cutoff {
        Some(index) => (text[..index].to_owned(), true),
        None => (text, false),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use lopdf::{
        Document, Object, Stream,
        content::{Content, Operation},
        dictionary,
    };

    use super::{
        BackendDiagnostics, ExtractedDocument, ExtractionLimits, ModelConfig, SummarizationBackend,
        SummaryAudience, SummaryLength, SummaryPhase, SummaryProgress, SummaryRequest,
        SummaryResult, extract_pdf_text, run_summary_job,
    };

    #[derive(Default)]
    struct MockBackend {
        loaded: bool,
        load_count: usize,
        unload_count: usize,
        fail_generation: bool,
    }

    impl SummarizationBackend for MockBackend {
        fn load(&mut self, _model: &ModelConfig) -> anyhow::Result<BackendDiagnostics> {
            assert!(!self.loaded);
            self.loaded = true;
            self.load_count += 1;
            Ok(BackendDiagnostics {
                runtime: "deterministic mock".to_owned(),
                accelerator: "none".to_owned(),
            })
        }

        fn summarize(
            &mut self,
            request: &SummaryRequest,
            is_cancelled: &dyn Fn() -> bool,
            report_progress: &mut dyn FnMut(SummaryProgress),
        ) -> anyhow::Result<SummaryResult> {
            assert!(self.loaded);
            if self.fail_generation {
                anyhow::bail!("mock generation failure");
            }
            let searchable = request
                .document
                .pages
                .iter()
                .filter(|page| page.has_searchable_text)
                .collect::<Vec<_>>();
            for (index, _) in searchable.iter().enumerate() {
                if is_cancelled() {
                    anyhow::bail!("summarization cancelled during generation");
                }
                report_progress(SummaryProgress {
                    phase: SummaryPhase::Generating,
                    completed: index + 1,
                    total: searchable.len(),
                });
            }
            Ok(SummaryResult {
                text: format!("Mock summary of {} searchable page(s).", searchable.len()),
                cited_pages: searchable.iter().map(|page| page.page_number).collect(),
            })
        }

        fn unload(&mut self) -> anyhow::Result<()> {
            assert!(self.loaded);
            self.loaded = false;
            self.unload_count += 1;
            Ok(())
        }
    }

    fn mock_request() -> SummaryRequest {
        SummaryRequest {
            document: ExtractedDocument {
                pages: vec![super::ExtractedPage {
                    page_number: 3,
                    text: "Searchable test content for a deterministic summary.".to_owned(),
                    has_searchable_text: true,
                    truncated: false,
                }],
                total_characters: 52,
                truncated: false,
            },
            length: SummaryLength::Standard,
            audience: SummaryAudience::General,
        }
    }

    fn mock_model() -> ModelConfig {
        ModelConfig {
            id: "test/mock".to_owned(),
            path: PathBuf::from("unused.gguf"),
            context_size: 4096,
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pdf-merger-{name}-{}-{nonce}.pdf",
            std::process::id()
        ))
    }

    fn write_pdf(page_texts: &[&str]) -> PathBuf {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let mut page_ids = Vec::new();

        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                    Operation::new("Td", vec![24.into(), 100.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                "Contents" => content_id,
            });
            page_ids.push(page_id);
        }

        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => page_ids.len() as i64,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);

        let path = fixture_path("text");
        document.save(&path).unwrap();
        path
    }

    #[test]
    fn extracts_requested_pages_with_page_numbers() {
        let path = write_pdf(&[
            "First page contains enough searchable text.",
            "Second page contains different searchable text.",
        ]);
        let result =
            extract_pdf_text(&path, None, Some(&[2]), ExtractionLimits::default()).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(result.pages.len(), 1);
        assert_eq!(result.pages[0].page_number, 2);
        assert!(result.pages[0].text.contains("Second page"));
        assert!(result.pages[0].has_searchable_text);
    }

    #[test]
    fn marks_blank_pages_as_not_searchable() {
        let path = write_pdf(&[""]);
        let result = extract_pdf_text(&path, None, None, ExtractionLimits::default()).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(result.pages.len(), 1);
        assert!(!result.pages[0].has_searchable_text);
    }

    #[test]
    fn enforces_character_limits_without_splitting_utf8() {
        let path = write_pdf(&["ééééé long searchable text"]);
        let limits = ExtractionLimits {
            max_characters_per_page: 5,
            max_characters_per_document: 5,
            minimum_searchable_characters: 1,
            ..ExtractionLimits::default()
        };
        let result = extract_pdf_text(&path, None, None, limits).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(result.pages[0].text.chars().count(), 5);
        assert!(result.pages[0].truncated);
        assert!(result.truncated);
    }

    #[test]
    fn rejects_missing_requested_pages() {
        let path = write_pdf(&["A page with enough searchable text for the test."]);
        let error = extract_pdf_text(&path, None, Some(&[2]), ExtractionLimits::default())
            .unwrap_err()
            .to_string();
        fs::remove_file(path).unwrap();

        assert!(error.contains("page 2 does not exist"));
    }

    #[test]
    fn summary_job_loads_generates_and_unloads() {
        let mut backend = MockBackend::default();
        let mut phases = Vec::new();
        let (result, diagnostics) = run_summary_job(
            &mut backend,
            &mock_model(),
            &mock_request(),
            &|| false,
            &mut |progress| phases.push(progress.phase),
        )
        .unwrap();

        assert_eq!(result.cited_pages, vec![3]);
        assert_eq!(diagnostics.runtime, "deterministic mock");
        assert!(!backend.loaded);
        assert_eq!(backend.load_count, 1);
        assert_eq!(backend.unload_count, 1);
        assert!(phases.contains(&SummaryPhase::Generating));
        assert_eq!(phases.last(), Some(&SummaryPhase::UnloadingModel));
    }

    #[test]
    fn summary_job_unloads_after_generation_failure() {
        let mut backend = MockBackend {
            fail_generation: true,
            ..MockBackend::default()
        };
        let error = run_summary_job(
            &mut backend,
            &mock_model(),
            &mock_request(),
            &|| false,
            &mut |_| {},
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("mock generation failure"));
        assert!(!backend.loaded);
        assert_eq!(backend.unload_count, 1);
    }

    #[test]
    fn summary_job_does_not_load_when_already_cancelled() {
        let mut backend = MockBackend::default();
        let error = run_summary_job(
            &mut backend,
            &mock_model(),
            &mock_request(),
            &|| true,
            &mut |_| {},
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("cancelled before model loading"));
        assert_eq!(backend.load_count, 0);
        assert_eq!(backend.unload_count, 0);
    }
}
