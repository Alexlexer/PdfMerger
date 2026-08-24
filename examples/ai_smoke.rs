use std::path::PathBuf;

use anyhow::{Context, Result};
use pdf_merger::{
    llama_backend::LlamaCppBackend,
    summarization::{
        ExtractedDocument, ExtractedPage, ExtractionLimits, ModelConfig, SummaryAudience,
        SummaryLength, SummaryRequest, extract_pdf_text, run_summary_job,
    },
};

fn main() -> Result<()> {
    let model_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: cargo run --release --example ai_smoke -- MODEL.gguf")?;
    let document_path = std::env::args_os().nth(2).map(PathBuf::from);
    let source_text = "PdfMerger is a private offline desktop application. It combines and arranges PDF pages and images. The experimental local AI feature summarizes searchable PDF text without uploading the document.";
    let repeat = std::env::var("PDF_MERGER_AI_SMOKE_REPEAT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let source_text = source_text.repeat(repeat);
    let uses_pdf = document_path.is_some();
    let document = match document_path {
        Some(path) => extract_pdf_text(&path, None, None, ExtractionLimits::default())?,
        None => ExtractedDocument {
            pages: vec![ExtractedPage {
                page_number: 1,
                text: source_text.clone(),
                has_searchable_text: true,
                truncated: false,
            }],
            total_characters: source_text.chars().count(),
            truncated: false,
        },
    };
    let request = SummaryRequest {
        document,
        length: if uses_pdf {
            SummaryLength::Standard
        } else {
            SummaryLength::Short
        },
        audience: SummaryAudience::General,
    };
    let model = ModelConfig {
        id: "smoke-test".to_owned(),
        path: model_path,
        context_size: 8192,
    };
    let mut backend = LlamaCppBackend::new();
    let (summary, diagnostics) =
        run_summary_job(&mut backend, &model, &request, &|| false, &mut |_| {})?;
    anyhow::ensure!(
        !summary.text.trim().is_empty(),
        "model returned an empty summary"
    );
    println!("{} / {}", diagnostics.runtime, diagnostics.accelerator);
    println!("{}", summary.text);
    Ok(())
}
