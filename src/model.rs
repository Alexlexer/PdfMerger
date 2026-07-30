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
    pub group_id: u64,
    pub source: PageSource,
    pub title: String,
    pub subtitle: String,
    pub preview: Option<PreviewData>,
    pub rotation: PageRotation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageGroup {
    pub id: u64,
    pub start: usize,
    pub end: usize,
    pub source_path: PathBuf,
}

impl PageGroup {
    pub fn page_count(&self) -> usize {
        self.end - self.start
    }
}
#[derive(Default)]
pub struct Workspace {
    pages: Vec<PageItem>,
    next_id: u64,
    next_group_id: u64,
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
            page.group_id.hash(&mut hasher);
            page.source.hash(&mut hasher);
            page.rotation.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn replace_project_pages(
        &mut self,
        pages: impl IntoIterator<Item = (PageDraft, PageRotation)>,
    ) {
        self.replace_project_pages_grouped(
            pages
                .into_iter()
                .map(|(draft, rotation)| (draft, rotation, None)),
        );
    }

    pub fn replace_project_pages_grouped(
        &mut self,
        pages: impl IntoIterator<Item = (PageDraft, PageRotation, Option<u64>)>,
    ) {
        self.pages.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.next_id = 0;
        self.next_group_id = 0;
        let mut legacy_group: Option<(PathBuf, u64)> = None;
        for (draft, rotation, stored_group) in pages {
            let group_id = if let Some(group_id) = stored_group {
                self.next_group_id = self.next_group_id.max(group_id);
                legacy_group = None;
                group_id
            } else if let Some((path, group_id)) = &legacy_group {
                if path == draft.source.path() {
                    *group_id
                } else {
                    self.next_group_id += 1;
                    legacy_group = Some((draft.source.path().clone(), self.next_group_id));
                    self.next_group_id
                }
            } else {
                self.next_group_id += 1;
                legacy_group = Some((draft.source.path().clone(), self.next_group_id));
                self.next_group_id
            };
            self.next_id += 1;
            self.pages.push(PageItem {
                id: self.next_id,
                group_id,
                source: draft.source,
                title: draft.title,
                subtitle: draft.subtitle,
                preview: draft.preview,
                rotation,
            });
        }
        self.normalize_group_runs();
    }

    pub fn append(&mut self, drafts: impl IntoIterator<Item = PageDraft>) {
        let drafts = drafts.into_iter().collect::<Vec<_>>();
        if drafts.is_empty() {
            return;
        }
        self.record_history();
        let mut current_group: Option<(PathBuf, u64)> = None;
        for draft in drafts {
            let group_id = if let Some((path, group_id)) = &current_group {
                if path == draft.source.path() {
                    *group_id
                } else {
                    self.next_group_id += 1;
                    current_group = Some((draft.source.path().clone(), self.next_group_id));
                    self.next_group_id
                }
            } else {
                self.next_group_id += 1;
                current_group = Some((draft.source.path().clone(), self.next_group_id));
                self.next_group_id
            };
            self.next_id += 1;
            self.pages.push(PageItem {
                id: self.next_id,
                group_id,
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
        self.normalize_group_runs();
        true
    }

    pub fn groups(&self) -> Vec<PageGroup> {
        let mut groups = Vec::new();
        let mut start = 0;
        while start < self.pages.len() {
            let group_id = self.pages[start].group_id;
            let mut end = start + 1;
            while end < self.pages.len() && self.pages[end].group_id == group_id {
                end += 1;
            }
            groups.push(PageGroup {
                id: group_id,
                start,
                end,
                source_path: self.pages[start].source.path().clone(),
            });
            start = end;
        }
        groups
    }

    pub fn group_page_ids(&self, group_id: u64) -> HashSet<u64> {
        self.pages
            .iter()
            .filter(|page| page.group_id == group_id)
            .map(|page| page.id)
            .collect()
    }

    pub fn move_page_to_group(&mut self, from: usize, to: usize, target_group_id: u64) -> bool {
        if from >= self.pages.len()
            || to > self.pages.len()
            || !self
                .pages
                .iter()
                .any(|page| page.group_id == target_group_id)
        {
            return false;
        }
        if self.pages[from].group_id == target_group_id {
            return self.move_page(from, to);
        }

        let adjusted_to = if from < to { to - 1 } else { to };
        self.record_history();
        let mut page = self.pages.remove(from);
        page.group_id = target_group_id;
        self.pages.insert(adjusted_to.min(self.pages.len()), page);
        self.normalize_group_runs();
        true
    }

    pub fn move_ids_to_group(&mut self, ids: &HashSet<u64>, target_group_id: u64) -> usize {
        if ids.is_empty()
            || !self
                .pages
                .iter()
                .any(|page| page.group_id == target_group_id)
        {
            return 0;
        }
        let mut moving = self
            .pages
            .iter()
            .filter(|page| ids.contains(&page.id) && page.group_id != target_group_id)
            .cloned()
            .collect::<Vec<_>>();
        if moving.is_empty() {
            return 0;
        }

        self.record_history();
        self.pages
            .retain(|page| !ids.contains(&page.id) || page.group_id == target_group_id);
        for page in &mut moving {
            page.group_id = target_group_id;
        }
        let insert_at = self
            .pages
            .iter()
            .rposition(|page| page.group_id == target_group_id)
            .map_or(self.pages.len(), |index| index + 1);
        let moved = moving.len();
        self.pages.splice(insert_at..insert_at, moving);
        self.normalize_group_runs();
        moved
    }
    pub fn move_group(&mut self, from: usize, to: usize) -> bool {
        let groups = self.groups();
        if from >= groups.len() || to > groups.len() || from == to {
            return false;
        }
        let adjusted_to = if from < to { to - 1 } else { to };
        if adjusted_to == from {
            return false;
        }
        let mut chunks = groups
            .iter()
            .map(|group| self.pages[group.start..group.end].to_vec())
            .collect::<Vec<_>>();
        let moved = chunks.remove(from);
        chunks.insert(adjusted_to.min(chunks.len()), moved);
        self.record_history();
        self.pages = chunks.into_iter().flatten().collect();
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
        self.normalize_group_runs();
        true
    }

    fn normalize_group_runs(&mut self) {
        let mut seen = HashSet::new();
        let mut start = 0;
        while start < self.pages.len() {
            let group_id = self.pages[start].group_id;
            let mut end = start + 1;
            while end < self.pages.len() && self.pages[end].group_id == group_id {
                end += 1;
            }
            if !seen.insert(group_id) {
                self.next_group_id += 1;
                for page in &mut self.pages[start..end] {
                    page.group_id = self.next_group_id;
                }
            }
            start = end;
        }
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
    #[test]
    fn creates_distinct_source_groups_and_reorders_them_as_units() {
        fn sourced(path: &str, title: &str) -> PageDraft {
            PageDraft {
                source: PageSource::Image {
                    path: PathBuf::from(path),
                },
                title: title.to_owned(),
                subtitle: String::new(),
                preview: None,
            }
        }

        let mut workspace = Workspace::default();
        workspace.append([
            sourced("first.pdf", "first-1"),
            sourced("first.pdf", "first-2"),
            sourced("second.pdf", "second-1"),
        ]);
        let initial_groups = workspace.groups();
        assert_eq!(initial_groups.len(), 2);
        assert_eq!(initial_groups[0].page_count(), 2);
        assert_eq!(initial_groups[1].page_count(), 1);

        workspace.append([sourced("first.pdf", "first-again")]);
        let groups = workspace.groups();
        assert_eq!(groups.len(), 3);
        assert_ne!(groups[0].id, groups[2].id);
        assert!(workspace.move_group(2, 0));
        assert_eq!(workspace.pages()[0].title, "first-again");
        assert_eq!(workspace.groups()[1].page_count(), 2);
    }

    #[test]
    fn restores_legacy_pages_as_consecutive_source_groups() {
        let pages = [
            (draft("one"), PageRotation::Deg0),
            (draft("one"), PageRotation::Deg90),
            (draft("two"), PageRotation::Deg0),
        ];
        let mut workspace = Workspace::default();
        workspace.replace_project_pages(pages);

        let groups = workspace.groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].page_count(), 2);
        assert_eq!(groups[1].page_count(), 1);
    }
    #[test]
    fn transfers_pages_between_groups_without_changing_their_sources() {
        let mut workspace = Workspace::default();
        workspace.append([draft("first"), draft("first")]);
        workspace.append([draft("second"), draft("second")]);
        let groups = workspace.groups();
        let target_group = groups[1].id;
        let transferred_source = workspace.pages()[0].source.clone();

        assert!(workspace.move_page_to_group(0, 4, target_group));
        let groups = workspace.groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].page_count(), 1);
        assert_eq!(groups[1].page_count(), 3);
        assert_eq!(workspace.pages()[3].source, transferred_source);
        assert_eq!(workspace.pages()[3].group_id, target_group);

        assert!(workspace.undo());
        assert_eq!(
            workspace
                .groups()
                .iter()
                .map(PageGroup::page_count)
                .collect::<Vec<_>>(),
            [2, 2]
        );
    }

    #[test]
    fn transfers_multiple_selected_pages_as_one_undoable_change() {
        let mut workspace = Workspace::default();
        workspace.append([draft("first"), draft("first")]);
        workspace.append([draft("second"), draft("second")]);
        let groups = workspace.groups();
        let source_ids = workspace.group_page_ids(groups[0].id);
        let target_group = groups[1].id;

        assert_eq!(workspace.move_ids_to_group(&source_ids, target_group), 2);
        assert_eq!(workspace.groups().len(), 1);
        assert_eq!(workspace.groups()[0].page_count(), 4);
        assert_eq!(
            workspace
                .pages()
                .iter()
                .map(|page| page.source.path().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["second.png", "second.png", "first.png", "first.png"]
        );
        assert!(workspace.undo());
        assert_eq!(workspace.groups().len(), 2);
    }
}
