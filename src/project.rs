use crate::config::Config;
use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub parent: Option<usize>,
}

impl Project {
    pub fn chain(&self, all: &[Project]) -> Vec<String> {
        let mut chain = Vec::new();
        let mut cur = Some(self);
        let mut stack = Vec::new();
        while let Some(p) = cur {
            stack.push(p.name.clone());
            cur = p.parent.map(|i| &all[i]);
        }
        stack.reverse();
        chain.extend(stack);
        chain
    }
}

fn read_project_name(marker: &Path, dir: &Path) -> String {
    if let Ok(content) = fs::read_to_string(marker) {
        if let Some(first_line) = content.lines().next() {
            let trimmed = first_line.trim();
            if let Some(name) = trimmed.strip_prefix('#') {
                let name = name.trim();
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".to_string())
}

pub fn discover_projects(root: &Path) -> Vec<Project> {
    let mut marker_dirs: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.file_name() == ".backup")
        .filter_map(|e| e.path().parent().map(|p| p.to_path_buf()))
        .collect();
    marker_dirs.sort_by_key(|p| p.components().count());

    let mut projects: Vec<Project> = Vec::new();
    for dir in marker_dirs {
        let parent = projects
            .iter()
            .enumerate()
            .filter(|(_, p)| dir.starts_with(&p.root) && dir != p.root)
            .max_by_key(|(_, p)| p.root.components().count())
            .map(|(i, _)| i);
        let name = read_project_name(&dir.join(".backup"), &dir);
        projects.push(Project {
            root: dir,
            name,
            parent,
        });
    }
    projects
}

pub fn build_gitignore(project: &Project, all: &[Project]) -> Result<Gitignore> {
    let nested_roots: Vec<&Path> = all
        .iter()
        .enumerate()
        .filter(|(_, p)| p.root != project.root && p.parent.is_some())
        .filter(|(_, p)| p.root.starts_with(&project.root))
        .map(|(_, p)| p.root.as_path())
        .collect();

    let mut builder = GitignoreBuilder::new(&project.root);
    for entry in WalkDir::new(&project.root)
        .into_iter()
        .filter_entry(|e| e.path() == project.root || !nested_roots.contains(&e.path()))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == ".backupignore" {
            builder.add(entry.path());
        }
    }
    builder.build().map_err(|e| anyhow::anyhow!(e))
}

pub struct PlannedFile {
    pub absolute: PathBuf,
    pub relative: PathBuf,
    pub size: u64,
}

pub struct BackupPlan {
    pub files: Vec<PlannedFile>,
    pub warnings: Vec<String>,
    // (path relative to the project root, "category (details)") for everything left out
    pub skipped: Vec<(PathBuf, String)>,
    pub total_size: u64,
}

// Bounded-ish scan: keeps going until both limits are confirmed exceeded,
// but does not exhaustively walk further than needed to find a handful of
// the biggest contributors, so a giant ignored-heavy tree still resolves
// fast. Ignored paths (per .backupignore) and nested project roots are
// skipped so already-excluded files never count toward the parent
// directory's size/count — this is what proves the ignore is honored.
struct DirUsage {
    size: u64,
    count: u64,
    top_files: Vec<(PathBuf, u64)>,
}

fn dir_usage(
    path: &Path,
    project_root: &Path,
    gi: &Gitignore,
    nested_roots: &[&Path],
) -> DirUsage {
    let mut size = 0u64;
    let mut count = 0u64;
    let mut top_files: Vec<(PathBuf, u64)> = Vec::new();
    let walker = WalkDir::new(path).into_iter().filter_entry(|e| {
        if nested_roots.contains(&e.path()) {
            return false;
        }
        let rel = e.path().strip_prefix(project_root).unwrap();
        let is_dir = e.file_type().is_dir();
        !gi.matched_path_or_any_parents(rel, is_dir).is_ignore()
    });
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            count += 1;
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            size += file_size;
            top_files.push((entry.path().to_path_buf(), file_size));
        }
    }
    top_files.sort_by(|a, b| b.1.cmp(&a.1));
    top_files.truncate(5);
    DirUsage {
        size,
        count,
        top_files,
    }
}

fn dir_exceeds_limits(
    path: &Path,
    project_root: &Path,
    gi: &Gitignore,
    nested_roots: &[&Path],
    size_limit: u64,
    count_limit: u64,
) -> Option<DirUsage> {
    let usage = dir_usage(path, project_root, gi, nested_roots);
    if usage.size > size_limit || usage.count > count_limit {
        Some(usage)
    } else {
        None
    }
}

pub fn plan_backup(
    idx: usize,
    projects: &[Project],
    gi: &Gitignore,
    cfg: &Config,
) -> BackupPlan {
    let project = &projects[idx];
    let nested_roots: Vec<&Path> = projects
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .filter(|(_, p)| p.parent == Some(idx))
        .map(|(_, p)| p.root.as_path())
        .collect();

    let size_limit = cfg.max_file_size_bytes();
    let count_limit = cfg.max_dir_file_count;

    let mut files = Vec::new();
    let total_size = std::cell::Cell::new(0u64);
    let warnings = std::cell::RefCell::new(Vec::new());
    let skipped = std::cell::RefCell::new(Vec::new());

    let walker = WalkDir::new(&project.root).into_iter().filter_entry(|e| {
        if e.path() == project.root {
            return true;
        }
        if nested_roots.contains(&e.path()) {
            return false;
        }
        let rel = e.path().strip_prefix(&project.root).unwrap();
        let is_dir = e.file_type().is_dir();
        let m = gi.matched_path_or_any_parents(rel, is_dir);
        if m.is_ignore() {
            skipped
                .borrow_mut()
                .push((rel.to_path_buf(), "ignored by .backupignore".to_string()));
            return false;
        }
        if is_dir && m.is_none() {
            if let Some(usage) = dir_exceeds_limits(
                e.path(),
                &project.root,
                gi,
                &nested_roots,
                size_limit,
                count_limit,
            ) {
                let contributors = usage
                    .top_files
                    .iter()
                    .map(|(p, s)| {
                        format!(
                            "{} ({})",
                            p.strip_prefix(&project.root).unwrap_or(p).display(),
                            humansize::format_size(*s, humansize::DECIMAL)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                skipped.borrow_mut().push((
                    rel.to_path_buf(),
                    format!(
                        "oversized directory ({}, {} files)",
                        humansize::format_size(usage.size, humansize::DECIMAL),
                        usage.count
                    ),
                ));
                warnings.borrow_mut().push(format!(
                    "skipped oversized directory {} ({}, {} files after ignore rules; biggest: {}) — mark with ! in .backupignore to force-include",
                    rel.display(),
                    humansize::format_size(usage.size, humansize::DECIMAL),
                    usage.count,
                    contributors
                ));
                return false;
            }
        }
        true
    });

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path == project.root || entry.file_type().is_dir() {
            continue;
        }
        let rel = path.strip_prefix(&project.root).unwrap().to_path_buf();
        let m = gi.matched_path_or_any_parents(&rel, false);
        if m.is_ignore() {
            continue;
        }
        if !entry.file_type().is_file() {
            // Symlinks, sockets, FIFOs, device nodes, etc. — not backupable
            // content. Opening a socket for reading fails with ENXIO, so
            // these must be excluded here rather than left to fail later.
            skipped.borrow_mut().push((
                rel.clone(),
                "non-regular file (socket/fifo/device/symlink)".to_string(),
            ));
            warnings.borrow_mut().push(format!(
                "skipped non-regular file {} (socket/fifo/device/symlink; not backed up)",
                rel.display()
            ));
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if m.is_none() && size > size_limit {
            skipped.borrow_mut().push((
                rel.clone(),
                format!("oversized file ({})", humansize::format_size(size, humansize::DECIMAL)),
            ));
            warnings.borrow_mut().push(format!(
                "skipped oversized file {} ({} bytes, mark with ! in .backupignore to force-include)",
                rel.display(),
                size
            ));
            continue;
        }
        total_size.set(total_size.get() + size);
        files.push(PlannedFile {
            absolute: path.to_path_buf(),
            relative: rel,
            size,
        });
    }

    BackupPlan {
        files,
        warnings: warnings.into_inner(),
        skipped: skipped.into_inner(),
        total_size: total_size.get(),
    }
}