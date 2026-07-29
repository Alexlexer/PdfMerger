use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageSource {
    Pdf { path: PathBuf, page_number: u32 },
    Image { path: PathBuf },
}

impl PageSource {
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Pdf { path, .. } | Self::Image { path } => path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreviewData {
    pub bytes: Arc<[u8]>,
    pub extension: &'static str,
}

impl PreviewData {
    pub fn new(bytes: Vec<u8>, extension: &'static str) -> Self {
        Self {
            bytes: bytes.into(),
            extension,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PageDraft {
    pub source: PageSource,
    pub title: String,
    pub subtitle: String,
    pub preview: Option<PreviewData>,
}

#[derive(Clone, Debug)]
pub struct PageItem {
    pub id: u64,
    pub source: PageSource,
    pub title: String,
    pub subtitle: String,
    pub preview: Option<PreviewData>,
}

#[derive(Default)]
pub struct Workspace {
    pages: Vec<PageItem>,
    next_id: u64,
}

impl Workspace {
    pub fn pages(&self) -> &[PageItem] {
        &self.pages
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn append(&mut self, drafts: impl IntoIterator<Item = PageDraft>) {
        for draft in drafts {
            self.next_id += 1;
            self.pages.push(PageItem {
                id: self.next_id,
                source: draft.source,
                title: draft.title,
                subtitle: draft.subtitle,
                preview: draft.preview,
            });
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.pages.len() {
            self.pages.remove(index);
        }
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn move_page(&mut self, from: usize, to: usize) {
        if from >= self.pages.len() || to > self.pages.len() || from == to {
            return;
        }

        let page = self.pages.remove(from);
        let adjusted_to = if from < to { to - 1 } else { to };
        self.pages.insert(adjusted_to.min(self.pages.len()), page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str) -> PageDraft {
        PageDraft {
            source: PageSource::Image {
                path: PathBuf::from(format!("{title}.png")),
            },
            title: title.to_owned(),
            subtitle: String::new(),
            preview: None,
        }
    }

    #[test]
    fn moves_pages_in_both_directions() {
        let mut workspace = Workspace::default();
        workspace.append([draft("one"), draft("two"), draft("three")]);

        workspace.move_page(0, 3);
        assert_eq!(
            workspace
                .pages()
                .iter()
                .map(|page| page.title.as_str())
                .collect::<Vec<_>>(),
            ["two", "three", "one"]
        );

        workspace.move_page(2, 0);
        assert_eq!(
            workspace
                .pages()
                .iter()
                .map(|page| page.title.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
    }

    #[test]
    fn ignores_invalid_moves() {
        let mut workspace = Workspace::default();
        workspace.append([draft("one")]);
        workspace.move_page(5, 0);
        workspace.move_page(0, 5);
        assert_eq!(workspace.len(), 1);
    }
}
