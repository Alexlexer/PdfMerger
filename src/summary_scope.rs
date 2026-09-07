//! Stable source identities, explicit scope, and coverage for one summary job.
use crate::{
    model::{PageItem, PageSource},
    summarization::ExtractionReport,
};
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SummaryScope {
    #[default]
    Selected,
    Group,
    Original,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePage {
    pub path: PathBuf,
    pub page: u32,
}
#[derive(Clone, Debug)]
pub struct SourceSelection {
    pub path: PathBuf,
    pub pages: Option<Vec<u32>>,
}
pub fn select_scope(
    pages: &[PageItem],
    selected: &HashSet<u64>,
    scope: SummaryScope,
    group: Option<u64>,
    original: Option<&PathBuf>,
) -> Result<Vec<SourceSelection>> {
    if scope == SummaryScope::Original {
        return Ok(vec![SourceSelection {
            path: original
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Choose an original PDF"))?,
            pages: None,
        }]);
    }
    let mut sources: Vec<SourceSelection> = Vec::new();
    for page in pages.iter().filter(|p| match scope {
        SummaryScope::Selected => selected.contains(&p.id),
        SummaryScope::Group => Some(p.group_id) == group,
        SummaryScope::Original => false,
    }) {
        let PageSource::Pdf { path, page_number } = &page.source else {
            bail!(
                "This scope contains image items. Select PDF pages; scanned pages inside PDFs support optional OCR."
            );
        };
        if let Some(source) = sources.iter_mut().find(|s| s.path == *path) {
            let numbers = source.pages.as_mut().unwrap();
            if !numbers.contains(page_number) {
                numbers.push(*page_number);
            }
        } else {
            sources.push(SourceSelection {
                path: path.clone(),
                pages: Some(vec![*page_number]),
            });
        }
    }
    if sources.is_empty() {
        bail!(
            "This scope contains no PDF pages. Select pages or choose a document group/original PDF explicitly."
        );
    }
    Ok(sources)
}
#[derive(Clone, Debug, Default)]
pub struct SummaryCoverage {
    pub references: BTreeMap<u32, SourcePage>,
    pub sources: Vec<(PathBuf, ExtractionReport)>,
}
impl SummaryCoverage {
    pub fn partial(&self) -> bool {
        self.sources.iter().any(|(_, r)| r.partial())
    }
    pub fn label(&self, id: u32) -> String {
        self.references
            .get(&id)
            .map(|p| format!("{} · p. {}", p.path.display(), p.page))
            .unwrap_or_else(|| format!("Invalid reference {id}"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PageDraft, Workspace};
    #[test]
    fn empty_selection_never_becomes_original_pdf() {
        assert!(
            select_scope(
                &[],
                &HashSet::new(),
                SummaryScope::Selected,
                None,
                Some(&"a.pdf".into())
            )
            .is_err()
        );
    }
    #[test]
    fn scopes_keep_same_number_from_different_sources_distinct() {
        let mut workspace = Workspace::default();
        for path in ["a.pdf", "b.pdf"] {
            workspace.append(vec![PageDraft {
                source: PageSource::Pdf {
                    path: path.into(),
                    page_number: 1,
                },
                title: String::new(),
                subtitle: String::new(),
                preview: None,
            }]);
        }
        let selected = workspace.pages().iter().map(|p| p.id).collect();
        let result = select_scope(
            workspace.pages(),
            &selected,
            SummaryScope::Selected,
            None,
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_ne!(result[0].path, result[1].path);
        assert_eq!(
            select_scope(
                workspace.pages(),
                &selected,
                SummaryScope::Group,
                Some(workspace.pages()[0].group_id),
                None
            )
            .unwrap()
            .len(),
            1
        );
        assert!(
            select_scope(
                workspace.pages(),
                &selected,
                SummaryScope::Original,
                None,
                Some(&"a.pdf".into())
            )
            .unwrap()[0]
                .pages
                .is_none()
        );
    }
}
