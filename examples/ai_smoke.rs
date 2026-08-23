use std::path::PathBuf;

use anyhow::{Context, Result};
use pdf_merger::{
    llama_backend::LlamaCppBackend,
    summarization::{
        ExtractedDocument, ExtractedPage, ModelConfig, SummaryAudience, SummaryLength,
        SummaryRequest, run_summary_job,
    },
};

fn main() -> Result<()> {
    let model_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: cargo run --release --example ai_smoke -- MODEL.gguf")?;
    let request = SummaryRequest {
        document: ExtractedDocument {
            pages: vec![ExtractedPage {
                page_number: 1,
                text: "PdfMerger is a private offline desktop application. It combines and arranges PDF pages and images. The experimental local AI feature summarizes searchable PDF text without uploading the document.".to_owned(),
                has_searchable_text: true,
                truncated: false,
            }],
            total_characters: 181,
            truncated: false,
        },
        length: SummaryLength::Short,
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
