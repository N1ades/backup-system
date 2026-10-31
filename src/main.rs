mod archive;
mod config;
mod naming;
mod project;
mod snapshot;
mod storage;
mod telegram;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use humansize::{format_size, DECIMAL};
use project::{build_gitignore, discover_projects, plan_backup};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "backup-system")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Scan,
    Audit,
    /// Pack the whole server into one snapshot archive and upload it
    Run {
        /// Build the archive into DOWNLOAD_DIR and skip upload/retention/notification
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the snapshot tree (what is included and what is skipped)
    Tree,
    Tui,
}

fn print_scan(projects: &[project::Project]) {
    for p in projects.iter() {
        let depth = p.chain(projects).len() - 1;
        println!("{}{} -> {}", "  ".repeat(depth), p.name, p.root.display());
    }
}

fn run_audit(cfg: &Config, projects: &[project::Project]) -> Result<()> {
    for (idx, p) in projects.iter().enumerate() {
        let gi = build_gitignore(p, projects)?;
        let plan = plan_backup(idx, projects, &gi, cfg);
        let chain = p.chain(projects).join(" / ");
        println!("\n== {chain} ==");
        println!(
            "  files: {}  total size: {}",
            plan.files.len(),
            format_size(plan.total_size, DECIMAL)
        );
        for w in &plan.warnings {
            println!("  ! {w}");
        }

        let mut biggest: Vec<&project::PlannedFile> = plan.files.iter().collect();
        biggest.sort_by(|a, b| b.size.cmp(&a.size));
        if !biggest.is_empty() {
            println!("  largest files:");
            for pf in biggest.into_iter().take(5) {
                println!(
                    "    {} ({})",
                    pf.relative.display(),
                    format_size(pf.size, DECIMAL)
                );
            }
        }
    }
    Ok(())
}

async fn run_backup(cfg: &Config, projects: &[project::Project], dry_run: bool) -> Result<()> {
    let started = Instant::now();
    let snap = snapshot::collect(cfg, projects)?;
    if snap.files.is_empty() {
        println!("nothing to back up");
        return Ok(());
    }

    let filename = naming::archive_filename(&cfg.server_name, &[], cfg.timezone);
    let tree = snapshot::render_tree(cfg, &snap, &filename);
    let archive_path = if dry_run {
        cfg.download_dir.join(&filename)
    } else {
        std::env::temp_dir().join(&filename)
    };
    let archive_warnings = archive::write_zip(
        &snap.files,
        &[("_snapshot/TREE.txt", tree.as_bytes())],
        &archive_path,
    )?;
    let zip_size = std::fs::metadata(&archive_path)?.len();

    let skipped = snapshot::skipped_by_category(&snap);
    let skipped_total: u64 = skipped.values().sum();
    let size = format_size(snap.total_size, DECIMAL);
    let packed = format_size(zip_size, DECIMAL);
    println!(
        "snapshot {filename}: {} files, {size} ({packed} packed), {skipped_total} entries skipped",
        snap.files.len()
    );
    for (category, n) in &skipped {
        println!("  ! {n} x {category}");
    }
    println!("  (full tree with skipped entries is in _snapshot/TREE.txt inside the archive)");
    for w in &archive_warnings {
        println!("  ! {w}");
    }

    if dry_run {
        println!("dry run: kept {} locally, nothing uploaded", archive_path.display());
        return Ok(());
    }

    let bucket = storage::bucket(cfg)?;
    storage::upload_file(&bucket, &archive_path, &filename).await?;
    let _ = std::fs::remove_file(&archive_path);
    println!("uploaded {filename} in {:.1}s", started.elapsed().as_secs_f32());

    apply_retention(cfg, &bucket).await?;

    let used = storage::used_bytes(&bucket).await.unwrap_or(0);
    let text = format!(
        "Server snapshot: {filename}\n{} files, {size} ({packed} packed), {skipped_total} entries skipped\n\nstorage used: {}",
        snap.files.len(),
        format_size(used, DECIMAL)
    );
    telegram::notify(cfg, &text).await?;

    Ok(())
}

async fn apply_retention(cfg: &Config, bucket: &s3::bucket::Bucket) -> Result<()> {
    use std::collections::HashMap;

    let objects = storage::list_all(bucket).await?;
    let mut by_chain: HashMap<(String, Vec<String>), Vec<(String, String)>> = HashMap::new();
    for obj in objects {
        if let Some(parsed) = naming::parse_archive_filename(&obj.key) {
            by_chain
                .entry((parsed.server, parsed.chain))
                .or_default()
                .push((parsed.timestamp, obj.key));
        }
    }

    for (_, mut group) in by_chain {
        // timestamp format is fixed-width (YYYY-MM-DD-HHMM-TZ), so string
        // order matches chronological order without needing to parse it.
        group.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, stale_key) in group.into_iter().skip(cfg.retention_keep) {
            storage::delete_object(bucket, &stale_key).await?;
            println!("removed old backup {}", stale_key);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    let projects = discover_projects(&cfg.backup_root);

    if projects.is_empty() && !matches!(cli.command, Command::Tui) {
        println!(
            "no .backup markers found under {}",
            cfg.backup_root.display()
        );
        return Ok(());
    }

    match cli.command {
        Command::Scan => print_scan(&projects),
        Command::Audit => run_audit(&cfg, &projects)?,
        Command::Run { dry_run } => run_backup(&cfg, &projects, dry_run).await?,
        Command::Tree => {
            let snap = snapshot::collect(&cfg, &projects)?;
            print!("{}", snapshot::render_tree(&cfg, &snap, "(preview)"));
        }
        Command::Tui => tui::run(&cfg).await?,
    }

    Ok(())
}