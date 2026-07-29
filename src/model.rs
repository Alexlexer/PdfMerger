use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::Arc,
};

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum PageRotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl PageRotation {
    pub fn degrees(self) -> i64 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    pub fn clockwise(self) -> Self {
        match self {
            Self::Deg0 => Self::Deg90,
            Self::Deg90 => Self::Deg180,
            Self::Deg180 => Self::Deg270,
            Self::Deg270 => Self::Deg0,
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
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
    pub size: [usize; 2],
    pub rgba: Arc<[u8]>,
}

impl PreviewData {
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), width * height * 4);
        Self {
            size: [width, height],
            rgba: rgba.into(),
        }
    }

    pub fn rotated(&self, rotation: PageRotation) -> Self {
        if rotation == PageRotation::Deg0 {
            return self.clone();
        }

        let image = image::RgbaImage::from_raw(
            self.size[0] as u32,
            self.size[1] as u32,
            self.rgba.to_vec(),
        )
        .expect("preview dimensions must match its pixel data");
        let rotated = match rotation {
            PageRotation::Deg0 => image,
            PageRotation::Deg90 => image::imageops::rotate90(&image),
            PageRotation::Deg180 => image::imageops::rotate180(&image),
            PageRotation::Deg270 => image::imageops::rotate270(&image),
        };
        Self::new(
            rotated.width() as usize,
            rotated.height() as usize,
            rotated.into_raw(),
        )
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
    pub rotation: PageRotation,
}

#[derive(Default)]
pub struct Workspace {
    pages: Vec<PageItem>,
    next_id: u64,
    undo_stack: Vec<Vec<PageItem>>,
    redo_stack: Vec<Vec<PageItem>>,
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

    pub fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for page in &self.pages {
            page.source.hash(&mut hasher);
            page.rotation.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn replace_project_pages(
        &mut self,
        pages: impl IntoIterator<Item = (PageDraft, PageRotation)>,
    ) {
        self.pages.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.next_id = 0;
        for (draft, rotation) in pages {
            self.next_id += 1;
            self.pages.push(PageItem {
                id: self.next_id,
                source: draft.source,
                title: draft.title,
                subtitle: draft.subtitle,
                preview: draft.preview,
                rotation,
            });
        }
    }

    pub fn append(&mut self, drafts: impl IntoIterator<Item = PageDraft>) {
        let drafts = drafts.into_iter().collect::<Vec<_>>();
        if drafts.is_empty() {
            return;
        }
        self.record_history();
        for draft in drafts {
            self.next_id += 1;
            self.pages.push(PageItem {
                id: self.next_id,
                source: draft.source,
                title: draft.title,
                subtitle: draft.subtitle,
                preview: draft.preview,
                rotation: PageRotation::default(),
            });
        }
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.pages.len() {
            return false;
        }
        self.record_history();
        self.pages.remove(index);
        true
    }

    pub fn remove_ids(&mut self, ids: &HashSet<u64>) -> usize {
        let removed = self
            .pages
            .iter()
            .filter(|page| ids.contains(&page.id))
            .count();
        if removed == 0 {
            return 0;
        }
        self.record_history();
        self.pages.retain(|page| !ids.contains(&page.id));
        removed
    }

    pub fn clear(&mut self) -> bool {
        if self.pages.is_empty() {
            return false;
        }
        self.record_history();
        self.pages.clear();
        true
    }

    pub fn move_page(&mut self, from: usize, to: usize) -> bool {
        if from >= self.pages.len() || to > self.pages.len() || from == to {
            return false;
        }

        let adjusted_to = if from < to { to - 1 } else { to };
        if adjusted_to == from {
            return false;
        }
        self.record_history();
        let page = self.pages.remove(from);
        self.pages.insert(adjusted_to.min(self.pages.len()), page);
        true
    }

    pub fn rotate_ids_clockwise(&mut self, ids: &HashSet<u64>) -> usize {
        let affected = self
            .pages
            .iter()
            .filter(|page| ids.contains(&page.id))
            .count();
        if affected == 0 {
            return 0;
        }
        self.record_history();
        for page in &mut self.pages {
            if ids.contains(&page.id) {
                page.rotation = page.rotation.clockwise();
            }
        }
        affected
    }

    pub fn move_ids_to_start(&mut self, ids: &HashSet<u64>) -> bool {
        self.move_ids_to_edge(ids, true)
    }

    pub fn move_ids_to_end(&mut self, ids: &HashSet<u64>) -> bool {
        self.move_ids_to_edge(ids, false)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack
            .push(std::mem::replace(&mut self.pages, previous));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.pages, next);
        self.push_undo(current);
        true
    }

    fn move_ids_to_edge(&mut self, ids: &HashSet<u64>, start: bool) -> bool {
        if ids.is_empty() || !self.pages.iter().any(|page| ids.contains(&page.id)) {
            return false;
        }
        let selected = self
            .pages
            .iter()
            .filter(|page| ids.contains(&page.id))
            .cloned()
            .collect::<Vec<_>>();
        let unselected = self
            .pages
            .iter()
            .filter(|page| !ids.contains(&page.id))
            .cloned()
            .collect::<Vec<_>>();
        let reordered: Vec<PageItem> = if start {
            selected.into_iter().chain(unselected).collect()
        } else {
            unselected.into_iter().chain(selected).collect()
        };
        if self
            .pages
            .iter()
            .map(|page| page.id)
            .eq(reordered.iter().map(|page| page.id))
        {
            return false;
        }
        self.record_history();
        self.pages = reordered;
        true
    }

    fn record_history(&mut self) {
        self.push_undo(self.pages.clone());
        self.redo_stack.clear();
    }

    fn push_undo(&mut self, state: Vec<PageItem>) {
        if self.undo_stack.len() == HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(state);
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

    #[test]
    fn undo_and_redo_restore_workspace_changes() {
        let mut workspace = Workspace::default();
        workspace.append([draft("one"), draft("two")]);
        workspace.remove(0);

        assert_eq!(workspace.pages()[0].title, "two");
        assert!(workspace.undo());
        assert_eq!(workspace.len(), 2);
        assert!(workspace.redo());
        assert_eq!(workspace.pages()[0].title, "two");
    }

    #[test]
    fn rotates_and_moves_selected_pages_as_a_group() {
        let mut workspace = Workspace::default();
        workspace.append([draft("one"), draft("two"), draft("three")]);
        let ids = HashSet::from([workspace.pages()[1].id, workspace.pages()[2].id]);

        assert_eq!(workspace.rotate_ids_clockwise(&ids), 2);
        assert_eq!(workspace.pages()[1].rotation, PageRotation::Deg90);
        assert!(workspace.move_ids_to_start(&ids));
        assert_eq!(
            workspace
                .pages()
                .iter()
                .map(|page| page.title.as_str())
                .collect::<Vec<_>>(),
            ["two", "three", "one"]
        );
    }
}
