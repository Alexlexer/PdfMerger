use std::{
    collections::{HashMap, HashSet},
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use crate::model::PageItem;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitMode {
    #[default]
    IndividualPages,
    SourceDocuments,
    Ranges,
}

#[derive(Clone, Debug)]
pub struct SplitGroup {
    pub file_stem: String,
    pub pages: Vec<PageItem>,
}

#[derive(Clone, Debug)]
pub struct PlannedSplit {
    pub path: PathBuf,
    pub pages: Vec<PageItem>,
}

#[derive(Debug)]
pub struct SplitReport {
    pub directory: PathBuf,
    pub written: Vec<PathBuf>,
    pub failures: Vec<String>,
    pub warning_count: usize,
}

pub fn build_groups(
    mode: SplitMode,
    pages: &[PageItem],
    range_spec: &str,
    base_name: &str,
) -> Result<Vec<SplitGroup>> {
    if pages.is_empty() {
        bail!("select at least one page before splitting");
    }
    let base_name = validate_base_name(base_name)?;

    match mode {
        SplitMode::IndividualPages => Ok(pages
            .iter()
            .enumerate()
            .map(|(index, page)| SplitGroup {
                file_stem: format!("{base_name}-page-{:03}", index + 1),
                pages: vec![page.clone()],
            })
            .collect()),
        SplitMode::SourceDocuments => groups_by_source(pages, &base_name),
        SplitMode::Ranges => Ok(parse_ranges(range_spec, pages.len())?
            .into_iter()
            .map(|range| {
                let start = *range.start();
                let end = *range.end();
                let suffix = if start == end {
                    format!("page-{}", start + 1)
                } else {
                    format!("pages-{}-{}", start + 1, end + 1)
                };
                SplitGroup {
                    file_stem: format!("{base_name}-{suffix}"),
                    pages: pages[range].to_vec(),
                }
            })
            .collect()),
    }
}

pub fn plan_outputs(directory: &Path, groups: Vec<SplitGroup>) -> Result<Vec<PlannedSplit>> {
    if !directory.is_dir() {
        bail!("the selected output directory does not exist");
    }
    if groups.is_empty() {
        bail!("there are no split outputs to create");
    }

    let mut planned_paths = HashSet::new();
    let mut collisions = Vec::new();
    let planned = groups
        .into_iter()
        .map(|group| {
            let path = directory.join(format!("{}.pdf", group.file_stem));
            let normalized = path.to_string_lossy().to_lowercase();
            if !planned_paths.insert(normalized) || path.exists() {
                collisions.push(path.clone());
            }
            PlannedSplit {
                path,
                pages: group.pages,
            }
        })
        .collect::<Vec<_>>();

    if !collisions.is_empty() {
        let names = collisions
            .iter()
            .take(3)
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = collisions.len().saturating_sub(3);
        let suffix = if remaining == 0 {
            String::new()
        } else {
            format!(" and {remaining} more")
        };
        bail!("output file(s) already exist or collide: {names}{suffix}");
    }

    Ok(planned)
}

pub fn parse_ranges(spec: &str, page_count: usize) -> Result<Vec<RangeInclusive<usize>>> {
    if spec.trim().is_empty() {
        bail!("enter ranges such as 1-3, 5, 7-9");
    }

    let mut ranges = Vec::new();
    let mut used = HashSet::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            bail!("range entries cannot be empty");
        }
        let (start, end) = if let Some((start, end)) = part.split_once('-') {
            if end.contains('-') {
                bail!("invalid range '{part}'");
            }
            (
                parse_position(start, page_count)?,
                parse_position(end, page_count)?,
            )
        } else {
            let position = parse_position(part, page_count)?;
            (position, position)
        };
        if start > end {
            bail!("range '{part}' runs backwards");
        }
        for index in start..=end {
            if !used.insert(index) {
                bail!("range '{part}' overlaps another range");
            }
        }
        ranges.push(start..=end);
    }
    Ok(ranges)
}

fn parse_position(value: &str, page_count: usize) -> Result<usize> {
    let position = value
        .trim()
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("'{value}' is not a page number"))?;
    if position == 0 || position > page_count {
        bail!("page {position} is outside the selected 1-{page_count} range");
    }
    Ok(position - 1)
}

fn groups_by_source(pages: &[PageItem], base_name: &str) -> Result<Vec<SplitGroup>> {
    let mut source_indexes: HashMap<PathBuf, usize> = HashMap::new();
    let mut groups: Vec<SplitGroup> = Vec::new();

    for page in pages {
        let path = page.source.path().clone();
        let index = if let Some(index) = source_indexes.get(&path) {
            *index
        } else {
            let index = groups.len();
            let source_name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .map(sanitize_generated_stem)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "source".to_owned());
            groups.push(SplitGroup {
                file_stem: format!("{base_name}-{:02}-{source_name}", index + 1),
                pages: Vec::new(),
            });
            source_indexes.insert(path, index);
            index
        };
        groups[index].pages.push(page.clone());
    }
    Ok(groups)
}

fn validate_base_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("base filename cannot be empty");
    }
    if value.ends_with(['.', ' ']) {
        bail!("base filename cannot end with a dot or space");
    }
    if value
        .chars()
        .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        bail!("base filename contains a character that is not allowed in filenames");
    }
    let uppercase = value.to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CLOCK$"];
    let reserved_prefix = ["COM", "LPT"];
    if reserved.contains(&uppercase.as_str())
        || reserved_prefix.iter().any(|prefix| {
            uppercase.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
    {
        bail!("'{value}' is a reserved filename");
    }
    Ok(value.to_owned())
}

fn sanitize_generated_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() || r#"<>:"/\|?*"#.contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim_matches(['.', ' '])
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::model::{PageRotation, PageSource};

    use super::*;

    fn page(id: u64, path: &str) -> PageItem {
        PageItem {
            id,
            group_id: id,
            source: PageSource::Pdf {
                path: PathBuf::from(path),
                page_number: id as u32,
            },
            title: path.to_owned(),
            subtitle: String::new(),
            preview: None,
            rotation: PageRotation::Deg0,
        }
    }

    #[test]
    fn parses_non_overlapping_ranges() {
        let ranges = parse_ranges("1-3, 5, 7-8", 8).unwrap();
        assert_eq!(ranges, vec![0..=2, 4..=4, 6..=7]);
    }

    #[test]
    fn rejects_invalid_or_overlapping_ranges() {
        assert!(parse_ranges("3-1", 5).is_err());
        assert!(parse_ranges("1-3,3-4", 5).is_err());
        assert!(parse_ranges("6", 5).is_err());
    }

    #[test]
    fn groups_pages_by_source_in_first_seen_order() {
        let pages = vec![
            page(1, "alpha.pdf"),
            page(2, "beta.pdf"),
            page(3, "alpha.pdf"),
        ];
        let groups = build_groups(SplitMode::SourceDocuments, &pages, "split", "bundle").unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].file_stem, "bundle-01-alpha");
        assert_eq!(
            groups[0]
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(groups[1].pages[0].id, 2);
    }

    #[test]
    fn detects_existing_output_before_export() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("pdf-merger-split-{nonce}"));
        fs::create_dir(&directory).unwrap();
        let existing = directory.join("bundle-page-001.pdf");
        fs::write(&existing, Arc::<[u8]>::from([])).unwrap();
        let groups = build_groups(
            SplitMode::IndividualPages,
            &[page(1, "alpha.pdf")],
            "",
            "bundle",
        )
        .unwrap();

        assert!(plan_outputs(&directory, groups).is_err());

        fs::remove_file(existing).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
