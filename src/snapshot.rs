use crate::config::Config;
use crate::project::{build_gitignore, plan_backup, PlannedFile, Project};
use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub enum Entry {
    File(u64),
    // "category (details)": the category is what the tree summary counts.
    Skipped(String),
    // Marks a directory that is a project root; carries the project chain.
    Project(String),
}

pub struct Snapshot {
    pub files: Vec<PlannedFile>,
    // Keyed by path relative to the backup root.
    pub entries: BTreeMap<PathBuf, Entry>,
    pub total_size: u64,
}

const NOT_A_PROJECT: &str = "not in any project (no .backup marker)";

pub fn collect(cfg: &Config, projects: &[Project]) -> Result<Snapshot> {
    let mut files = Vec::new();
    let mut entries = BTreeMap::new();
    let mut total_size = 0u64;

    for idx in 0..projects.len() {
        let p = &projects[idx];
        let prefix = p
            .root
            .strip_prefix(&cfg.backup_root)
            .unwrap_or(&p.root)
            .to_path_buf();
        if !prefix.as_os_str().is_empty() {
            entries.insert(prefix.clone(), Entry::Project(p.chain(projects).join("/")));
        }
        let gi = build_gitignore(p, projects)?;
        let plan = plan_backup(idx, projects, &gi, cfg);
        for pf in plan.files {
            let relative = prefix.join(&pf.relative);
            entries.insert(relative.clone(), Entry::File(pf.size));
            total_size += pf.size;
            files.push(PlannedFile {
                absolute: pf.absolute,
                relative,
                size: pf.size,
            });
        }
        for (rel, reason) in plan.skipped {
            entries.insert(prefix.join(rel), Entry::Skipped(reason));
        }
    }

    let roots: Vec<&Path> = projects.iter().map(|p| p.root.as_path()).collect();
    mark_unmarked(&cfg.backup_root, &cfg.backup_root, &roots, &mut entries);

    Ok(Snapshot {
        files,
        entries,
        total_size,
    })
}

// Everything under the backup root that no project covers is never walked by
// plan_backup, so it is recorded here: the tree has to say what is absent too.
fn mark_unmarked(
    dir: &Path,
    backup_root: &Path,
    roots: &[&Path],
    entries: &mut BTreeMap<PathBuf, Entry>,
) {
    if roots.iter().any(|r| dir.starts_with(r)) {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for child in read.filter_map(|e| e.ok()) {
        let path = child.path();
        let is_dir = child.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && roots.iter().any(|r| r.starts_with(&path)) {
            mark_unmarked(&path, backup_root, roots, entries);
        } else if !roots.iter().any(|r| path.starts_with(r)) {
            let rel = path.strip_prefix(backup_root).unwrap_or(&path).to_path_buf();
            entries.insert(rel, Entry::Skipped(NOT_A_PROJECT.to_string()));
        }
    }
}

#[derive(Default)]
struct Node<'a> {
    children: BTreeMap<String, Node<'a>>,
    entry: Option<&'a Entry>,
}

fn build<'a>(entries: &'a BTreeMap<PathBuf, Entry>) -> Node<'a> {
    let mut root = Node::default();
    for (path, entry) in entries {
        let mut node = &mut root;
        for comp in path.components() {
            node = node
                .children
                .entry(comp.as_os_str().to_string_lossy().to_string())
                .or_default();
        }
        node.entry = Some(entry);
    }
    root
}

// (files, bytes, skipped entries)
fn stats(node: &Node) -> (u64, u64, u64) {
    let mut acc = match node.entry {
        Some(Entry::File(size)) => (1, *size, 0),
        Some(Entry::Skipped(_)) => (0, 0, 1),
        _ => (0, 0, 0),
    };
    for child in node.children.values() {
        let (f, s, k) = stats(child);
        acc = (acc.0 + f, acc.1 + s, acc.2 + k);
    }
    acc
}

fn render(node: &Node, prefix: &str, out: &mut String) {
    let count = node.children.len();
    for (i, (name, child)) in node.children.iter().enumerate() {
        let last = i + 1 == count;
        let (files, size, skipped) = stats(child);
        let totals = format!("{files} files, {}", format_size(size, DECIMAL));
        let skipped_note = if skipped > 0 {
            format!(", {skipped} skipped")
        } else {
            String::new()
        };
        let label = match child.entry {
            Some(Entry::File(size)) => format!("{name}  ({})", format_size(*size, DECIMAL)),
            Some(Entry::Skipped(reason)) => format!("{name}  [SKIPPED: {reason}]"),
            Some(Entry::Project(chain)) => {
                format!("{name}/  [project {chain}]  {totals}{skipped_note}")
            }
            None => format!("{name}/  {totals}{skipped_note}"),
        };
        out.push_str(&format!(
            "{prefix}{}{label}\n",
            if last { "└── " } else { "├── " }
        ));
        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
        render(child, &child_prefix, out);
    }
}

// Skipped reasons grouped by category, for the header and the run summary.
pub fn skipped_by_category(snap: &Snapshot) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for entry in snap.entries.values() {
        if let Entry::Skipped(reason) = entry {
            let category = reason.split(" (").next().unwrap_or(reason).to_string();
            let category = if reason == NOT_A_PROJECT {
                reason.clone()
            } else {
                category
            };
            *counts.entry(category).or_insert(0) += 1;
        }
    }
    counts
}

pub fn render_tree(cfg: &Config, snap: &Snapshot, archive_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("Server snapshot: {archive_name}\n"));
    out.push_str(&format!("Server:          {}\n", cfg.server_name));
    out.push_str(&format!("Backup root:     {}\n", cfg.backup_root.display()));
    out.push_str(&format!(
        "Contents:        {} files, {} (uncompressed)\n",
        snap.files.len(),
        format_size(snap.total_size, DECIMAL)
    ));
    let skipped = skipped_by_category(snap);
    if skipped.is_empty() {
        out.push_str("Skipped:         nothing\n");
    } else {
        out.push_str("Skipped:\n");
        for (category, n) in &skipped {
            out.push_str(&format!("  {n:>6}  {category}\n"));
        }
    }
    out.push_str(
        "\nPaths are relative to the backup root and match the paths inside this archive.\n\
         Entries marked [SKIPPED: ...] are NOT in the archive; everything else is.\n\n",
    );
    out.push_str(&format!("{}/\n", cfg.backup_root.display()));
    render(&build(&snap.entries), "", &mut out);
    out
}
