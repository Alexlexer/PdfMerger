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
pub enum SummaryLanguage {
    SameAsDocument,
    English,
    French,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryRequest {
    pub document: ExtractedDocument,
    pub length: SummaryLength,
    pub audience: SummaryAudience,
    pub language: SummaryLanguage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryResult {
    pub text: String,
    pub cited_pages: Vec<u32>,
    pub warnings: Vec<String>,
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
    Extracting,
    Ocr,
    Synthesizing,
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
        (Ok(mut summary), Ok(())) => {
            if is_cancelled() {
                bail!("summarization cancelled");
            }
            let allowed = request
                .document
                .pages
                .iter()
                .filter(|p| p.has_searchable_text)
                .map(|p| p.page_number)
                .collect::<Vec<_>>();
            let (valid, invalid) = crate::summary_pipeline::citations(&summary.text, &allowed);
            // Backend validation may know a stricter set (e.g. final synthesis input).
            summary.cited_pages.retain(|page| valid.contains(page));
            if summary.cited_pages.is_empty() {
                summary
                    .warnings
                    .push("No validated source references; verify the summary manually.".into());
            }
            if !invalid.is_empty() {
                summary
                    .warnings
                    .push(format!("Invalid citations: {}", invalid.join(", ")));
            }
            if request.document.truncated {
                summary
                    .warnings
                    .push("Partial summary: extraction limits truncated source text.".into());
            }
            Ok((summary, diagnostics))
        }
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

#[derive(Clone, Debug, Default)]
pub struct ExtractionReport {
    pub processed: Vec<u32>,
    pub skipped: Vec<u32>,
    pub failed: Vec<u32>,
    pub truncated: Vec<u32>,
    pub ocr: Vec<u32>,
    pub vision: Vec<u32>,
    pub warnings: Vec<String>,
}
impl ExtractionReport {
    pub fn partial(&self) -> bool {
        !self.skipped.is_empty() || !self.failed.is_empty() || !self.truncated.is_empty()
    }
}
pub trait PageOcr {
    fn kind(&self) -> &'static str {
        "Tesseract OCR"
    }
    fn recognize(&mut self, page: u32, cancelled: &dyn Fn() -> bool) -> Result<String>;
}

pub fn extract_pdf_text(
    path: &Path,
    password: Option<&str>,
    requested_pages: Option<&[u32]>,
    limits: ExtractionLimits,
) -> Result<ExtractedDocument> {
    extract_pdf_text_controlled(
        path,
        password,
        requested_pages,
        limits,
        &|| false,
        &mut |_| {},
        None,
    )
    .map(|(document, _)| document)
}

pub fn extract_pdf_text_controlled(
    path: &Path,
    password: Option<&str>,
    requested_pages: Option<&[u32]>,
    limits: ExtractionLimits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(SummaryProgress),
    mut ocr: Option<&mut dyn PageOcr>,
) -> Result<(ExtractedDocument, ExtractionReport)> {
    if cancelled() {
        bail!("extraction cancelled");
    }
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
    let mut report = ExtractionReport::default();
    let total_pages = page_numbers.len();

    for (index, page_number) in page_numbers.into_iter().enumerate() {
        if cancelled() {
            bail!("extraction cancelled");
        }
        progress(SummaryProgress {
            phase: SummaryPhase::Extracting,
            completed: index,
            total: total_pages,
        });
        let remaining = limits
            .max_characters_per_document
            .saturating_sub(total_characters);
        if remaining == 0 {
            document_truncated = true;
            report.truncated.push(page_number);
            report.skipped.push(page_number);
            pages.push(ExtractedPage {
                page_number,
                text: String::new(),
                has_searchable_text: false,
                truncated: true,
            });
            continue;
        }

        let raw = document
            .extract_text_with_limit(&[page_number], limits.max_decompressed_bytes_per_page);
        let extraction_failed = raw.is_err();
        let mut normalized = normalize_text(&raw.unwrap_or_default());
        // Isolate fallback decoding to this page and enforce the same decompression cap.
        if !usable_text(&normalized, limits) && !extraction_failed {
            let mut isolated = document.clone();
            let other_pages = available_pages
                .keys()
                .copied()
                .filter(|p| *p != page_number)
                .collect::<Vec<_>>();
            isolated.delete_pages(&other_pages);
            let mut page_bytes = Vec::new();
            if isolated.save_to(&mut page_bytes).is_ok() {
                // No whole-document fallback: only the requested page is decoded.
                if let Ok(fallback) = pdf_extract::extract_text_from_mem(&page_bytes) {
                    let fallback = normalize_text(&fallback);
                    if extraction_is_better(&fallback, &normalized) {
                        normalized = fallback;
                    }
                }
            }
        }
        if cancelled() {
            bail!("extraction cancelled");
        }
        let mut ocr_failed = false;
        if !usable_text(&normalized, limits)
            && let Some(engine) = ocr.as_deref_mut()
        {
            progress(SummaryProgress {
                phase: SummaryPhase::Ocr,
                completed: index,
                total: total_pages,
            });
            match engine.recognize(page_number, cancelled) {
                Ok(text) => {
                    normalized = normalize_text(&text);
                    if engine.kind() == "AI vision" {
                        report.vision.push(page_number);
                    } else {
                        report.ocr.push(page_number);
                    }
                }
                Err(_) if cancelled() => bail!("OCR cancelled"),
                Err(error) => {
                    ocr_failed = true;
                    report.warnings.push(format!(
                        "{} failed on page {page_number}: {error}",
                        engine.kind()
                    ));
                }
            }
        }
        let allowed = remaining.min(limits.max_characters_per_page);
        let (text, truncated) = truncate_characters(normalized, allowed);
        let searchable_characters = searchable_character_count(&text);
        let has_searchable_text = searchable_characters >= limits.minimum_searchable_characters
            && !looks_like_decoding_garbage(&text);
        total_characters += text.chars().count();
        document_truncated |= truncated;
        if truncated {
            report.truncated.push(page_number);
        }
        if has_searchable_text {
            report.processed.push(page_number);
        } else if extraction_failed || ocr_failed {
            report.failed.push(page_number);
        } else {
            report.skipped.push(page_number);
        }
        pages.push(ExtractedPage {
            page_number,
            text,
            has_searchable_text,
            truncated,
        });
    }

    if cancelled() {
        bail!("extraction cancelled");
    }
    progress(SummaryProgress {
        phase: SummaryPhase::Extracting,
        completed: total_pages,
        total: total_pages,
    });
    Ok((
        ExtractedDocument {
            pages,
            total_characters,
            truncated: document_truncated,
        },
        report,
    ))
}

fn usable_text(text: &str, limits: ExtractionLimits) -> bool {
    searchable_character_count(text) >= limits.minimum_searchable_characters
        && !looks_like_decoding_garbage(text)
}

fn searchable_character_count(text: &str) -> usize {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .count()
}

fn extraction_is_better(candidate: &str, current: &str) -> bool {
    match (
        looks_like_decoding_garbage(candidate),
        looks_like_decoding_garbage(current),
    ) {
        (false, true) => true,
        (true, false) => false,
        _ => searchable_character_count(candidate) > searchable_character_count(current),
    }
}

fn looks_like_decoding_garbage(text: &str) -> bool {
    let mut total = 0usize;
    let mut uncommon_symbols = 0usize;
    let mut current_run = 0usize;
    let mut longest_run = 0usize;

    for character in text.chars() {
        total += 1;
        if character == '\u{fffd}' {
            uncommon_symbols += 1;
        }
        if character.is_whitespace() {
            longest_run = longest_run.max(current_run);
            current_run = 0;
            continue;
        }
        // Long unbroken runs are suspicious only for ASCII text. Many scripts
        // legitimately omit spaces or use combining vowel/tone marks.
        if character.is_ascii() {
            current_run += 1;
        } else {
            longest_run = longest_run.max(current_run);
            current_run = 0;
        }
        if character.is_ascii()
            && !character.is_alphanumeric()
            && !matches!(
                character,
                '.' | ','
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '\''
                    | '’'
                    | '"'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '-'
                    | '/'
                    | '\\'
                    | '+'
                    | '%'
                    | '€'
                    | '$'
                    | '&'
            )
        {
            uncommon_symbols += 1;
        }
    }
    longest_run = longest_run.max(current_run);

    total >= 20 && (longest_run > 120 || uncommon_symbols.saturating_mul(5) > total)
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
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect::<String>()
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
        SummaryAudience, SummaryLanguage, SummaryLength, SummaryPhase, SummaryProgress,
        SummaryRequest, SummaryResult, extract_pdf_text, run_summary_job,
    };

    #[test]
    fn strips_pdf_control_bytes_before_tokenization() {
        assert_eq!(
            super::normalize_text("alpha\0beta\r\ngamma\tend"),
            "alphabeta\ngamma\tend"
        );
    }

    #[test]
    fn detects_broken_custom_font_decoding() {
        assert!(super::looks_like_decoding_garbage(
            "cddefghgcdidgjkgdlgmnopqrstumomorstvuuvwsxvypsruoz{|rs}~psysupsuwvuv~su~}ursups}uros}vyysquwsut~sxuvyysquvyysquwsut~sxaXilZK"
        ));
        assert!(!super::looks_like_decoding_garbage(
            "AVIS D'ÉCHÉANCE - LOYER\nPériode du 01/06/2026 au 30/06/2026\nSOLDE À PAYER : 2 116,40 €"
        ));
    }

    #[test]
    fn accepts_multilingual_text_without_spaces_or_ascii_punctuation() {
        for text in [
            "这是一份可以阅读的中文文件，其中包含日期和金额。".repeat(20),
            "เอกสารนี้มีข้อมูลที่อ่านได้และมีวันที่กับจำนวนเงิน".repeat(20),
            "यह दस्तावेज़ पढ़ने योग्य है और इसमें दिनांक और राशि हैं।".repeat(20),
        ] {
            assert!(!super::looks_like_decoding_garbage(&text));
            assert!(super::usable_text(&text, ExtractionLimits::default()));
        }
    }

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
                warnings: Vec::new(),
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
            language: SummaryLanguage::SameAsDocument,
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

    struct FakeOcr {
        calls: Vec<u32>,
        missing: bool,
    }
    impl super::PageOcr for FakeOcr {
        fn recognize(&mut self, page: u32, cancelled: &dyn Fn() -> bool) -> anyhow::Result<String> {
            self.calls.push(page);
            if cancelled() {
                anyhow::bail!("cancelled");
            }
            if self.missing {
                anyhow::bail!("OCR missing");
            }
            Ok("Local OCR recovered enough searchable text from this scanned page.".into())
        }
    }
    #[test]
    fn mixed_text_and_scanned_pages_only_ocr_unusable_text() {
        let path = write_pdf(&[
            "This original text page needs no OCR and is searchable.",
            "",
        ]);
        let mut ocr = FakeOcr {
            calls: Vec::new(),
            missing: false,
        };
        let (document, report) = super::extract_pdf_text_controlled(
            &path,
            None,
            None,
            ExtractionLimits::default(),
            &|| false,
            &mut |_| {},
            Some(&mut ocr),
        )
        .unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(ocr.calls, vec![2]);
        assert_eq!(report.ocr, vec![2]);
        assert_eq!(report.processed, vec![1, 2]);
        assert!(!report.partial());
        assert_eq!(document.pages[1].page_number, 2);
    }
    #[test]
    fn unavailable_ocr_still_returns_text_pages_and_partial_coverage() {
        let path = write_pdf(&["This original text page has enough searchable content.", ""]);
        let mut ocr = FakeOcr {
            calls: Vec::new(),
            missing: true,
        };
        let (_, report) = super::extract_pdf_text_controlled(
            &path,
            None,
            None,
            ExtractionLimits::default(),
            &|| false,
            &mut |_| {},
            Some(&mut ocr),
        )
        .unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(report.processed, vec![1]);
        assert_eq!(report.failed, vec![2]);
        assert!(report.partial());
    }
    #[test]
    fn document_limit_reports_every_unread_page_as_truncated() {
        let path = write_pdf(&[
            "First page text fills the limit.",
            "Second page",
            "Third page",
        ]);
        let (document, report) = super::extract_pdf_text_controlled(
            &path,
            None,
            None,
            ExtractionLimits {
                max_characters_per_document: 5,
                ..ExtractionLimits::default()
            },
            &|| false,
            &mut |_| {},
            None,
        )
        .unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(document.pages.len(), 3);
        assert_eq!(report.truncated, vec![1, 2, 3]);
        assert!(report.partial());
    }
    #[test]
    fn extraction_can_cancel_between_pages() {
        let path = write_pdf(&[
            "First page content is sufficiently long.",
            "Second page content is sufficiently long.",
        ]);
        let cancelled = std::cell::Cell::new(false);
        let result = super::extract_pdf_text_controlled(
            &path,
            None,
            None,
            ExtractionLimits::default(),
            &|| cancelled.get(),
            &mut |p| {
                if p.completed == 1 {
                    cancelled.set(true);
                }
            },
            None,
        );
        fs::remove_file(path).unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
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

        assert!(result.cited_pages.is_empty());
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
