use crate::config::Config;
use crate::naming::parse_archive_filename;
use crate::storage;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::fs::File;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

struct Row {
    key: String,
    size: u64,
    depth: usize,
    label: String,
}

fn build_rows(objects: Vec<storage::RemoteObject>) -> Vec<Row> {
    let mut parsed: Vec<(String, u64, Vec<String>)> = objects
        .into_iter()
        .filter_map(|o| {
            parse_archive_filename(&o.key).map(|p| {
                let mut chain = vec![p.server];
                chain.extend(p.chain);
                (o.key, o.size, chain)
            })
        })
        .collect();
    parsed.sort_by(|a, b| a.2.cmp(&b.2));

    let mut rows = Vec::new();
    for (key, size, chain) in parsed {
        rows.push(Row {
            key,
            size,
            depth: chain.len().saturating_sub(1),
            label: chain.last().cloned().unwrap_or_default(),
        });
    }
    rows
}

pub async fn run(cfg: &Config) -> Result<()> {
    let bucket = storage::bucket(cfg)?;
    let objects = storage::list_all(&bucket).await?;
    let rows = build_rows(objects);

    if rows.is_empty() {
        println!("No backups found in storage yet.");
        return Ok(());
    }

    enable_raw_mode()?;
    let mut out = stdout();
    out.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut state = ListState::default();
    state.select(Some(0));
    let mut status = String::from("↑/↓ move · Enter download+extract · q quit");

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);

            let items: Vec<ListItem> = rows
                .iter()
                .map(|r| {
                    let indent = "  ".repeat(r.depth);
                    let size = humansize::format_size(r.size, humansize::DECIMAL);
                    ListItem::new(format!("{indent}{} ({size})", r.label))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Backups"))
                .highlight_symbol("➤ ")
                .highlight_style(Style::default().add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let status_bar = Paragraph::new(status.as_str())
                .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(status_bar, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down => {
                        let i = state.selected().unwrap_or(0);
                        state.select(Some((i + 1).min(rows.len().saturating_sub(1))));
                    }
                    KeyCode::Up => {
                        let i = state.selected().unwrap_or(0);
                        state.select(Some(i.saturating_sub(1)));
                    }
                    KeyCode::Enter => {
                        if let Some(i) = state.selected() {
                            status = format!("downloading {}…", rows[i].key);
                            terminal.draw(|f| {
                                let area = f.area();
                                let p = Paragraph::new(status.as_str())
                                    .block(Block::default().borders(Borders::ALL));
                                f.render_widget(p, area);
                            })?;
                            match download_and_extract(cfg, &bucket, &rows[i].key).await {
                                Ok(dest) => {
                                    status = format!("extracted to {}", dest.display());
                                }
                                Err(e) => {
                                    status = format!("failed: {e}");
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

async fn download_and_extract(
    cfg: &Config,
    bucket: &s3::bucket::Bucket,
    key: &str,
) -> Result<PathBuf> {
    let parsed = parse_archive_filename(key)
        .ok_or_else(|| anyhow::anyhow!("cannot parse archive name {key}"))?;
    let mut dest = cfg.download_dir.clone();
    dest.push(&parsed.server);
    for part in &parsed.chain {
        dest.push(part);
    }
    std::fs::create_dir_all(&dest)?;

    let tmp_zip = std::env::temp_dir().join(key);
    storage::download_file(bucket, key, &tmp_zip).await?;

    let mut archive = zip::ZipArchive::new(File::open(&tmp_zip)?)?;
    archive.extract(&dest)?;
    let _ = std::fs::remove_file(&tmp_zip);

    Ok(dest)
}
