mod archive;
mod config;
mod naming;
mod project;
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
    Run,
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

async fn run_backup(cfg: &Config, projects: &[project::Project]) -> Result<()> {
    let bucket = storage::bucket(cfg)?;
    let mut messages = Vec::new();

    for idx in 0..projects.len() {
        let p = &projects[idx];
        let gi = build_gitignore(p, projects)?;
        let plan = plan_backup(idx, projects, &gi, cfg);
        if plan.files.is_empty() {
            continue;
        }

        let chain = p.chain(projects);
        let filename = naming::archive_filename(&cfg.server_name, &chain, cfg.timezone);
        let tmp_path = std::env::temp_dir().join(&filename);

        let started = Instant::now();
        let archive_warnings = archive::write_zip(&plan, &tmp_path)?;
        storage::upload_file(&bucket, &tmp_path, &filename).await?;
        let _ = std::fs::remove_file(&tmp_path);

        let size = format_size(plan.total_size, DECIMAL);
        println!(
            "backed up {} -> {filename} ({size}, {:.1}s)",
            chain.join("/"),
            started.elapsed().as_secs_f32()
        );
        messages.push(format!("{filename}: {size}"));
        for w in &plan.warnings {
            println!("  ! {w}");
        }
        for w in &archive_warnings {
            println!("  ! {w}");
        }
    }

    apply_retention(cfg, &bucket).await?;

    if !messages.is_empty() {
        let used = storage::used_bytes(&bucket).await.unwrap_or(0);
        let text = format!(
            "Backup complete:\n{}\n\nstorage used: {}",
            messages.join("\n"),
            format_size(used, DECIMAL)
        );
        telegram::notify(cfg, &text).await?;
    }

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
        Command::Run => run_backup(&cfg, &projects).await?,
        Command::Tui => tui::run(&cfg).await?,
    }

    Ok(())
}