use crate::config::Config;
use crate::naming::parse_archive_filename;
use crate::storage;
use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::fs::File;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

struct Version {
    key: String,
    size: u64,
    date: DateTime<Utc>,
}

struct ModelNode {
    label: String,
    depth: usize,
    versions: Vec<Version>,
    expanded: bool,
}

struct RenderRow {
    node_idx: usize,
    version_idx: Option<usize>, // None если это сама модель/папка, Some(i) если это конкретная версия в поддереве
}

fn build_tree(objects: Vec<storage::RemoteObject>) -> Vec<ModelNode> {
    use std::collections::BTreeMap;
    use chrono::FixedOffset;
    
    let mut groups: BTreeMap<Vec<String>, Vec<Version>> = BTreeMap::new();
    
    // Московское время (MSK) — это UTC+3
    let msk_offset = FixedOffset::east_opt(3 * 3600).unwrap();

    for o in objects {
        if let Some(p) = parse_archive_filename(&o.key) {
            let mut chain = vec![p.server];
            chain.extend(p.chain);
            
            // Вытаскиваем дату: server1__srv--2026-07-19-1657-MSK.zip
            // Сплитим по "--" и берем вторую часть
            let date = o.key
                .split("--")
                .nth(1)
                .and_then(|part| {
                    // Отрезаем "-MSK.zip" или ".zip", оставляя "2026-07-19-1657"
                    let clean_date = part.split("-MSK").next().unwrap_or(part);
                    let clean_date = clean_date.trim_end_matches(".zip");
                    
                    // Парсим "YYYY-MM-DD-HHMM"
                    chrono::NaiveDateTime::parse_from_str(clean_date, "%Y-%m-%d-%H%M")
                        .map(|naive| {
                            // Привязываем к таймзоне MSK (UTC+3) и переводим в UTC
                            let datetime_msk = naive.and_local_timezone(msk_offset).unwrap();
                            datetime_msk.with_timezone(&Utc)
                        })
                        .ok()
                })
                .unwrap_or_else(Utc::now); // Фолбек на случай непредвиденного формата

            groups.entry(chain).or_default().push(Version {
                key: o.key,
                size: o.size,
                date,
            });
        }
    }

    groups
        .into_iter()
        .map(|(chain, mut versions)| {
            versions.sort_by(|a, b| b.date.cmp(&a.date));
            ModelNode {
                label: chain.last().cloned().unwrap_or_default(),
                depth: chain.len().saturating_sub(1),
                versions,
                expanded: false,
            }
        })
        .collect()
}

// Генерирует плоский список строк для отрисовки на основе состояния раскрытия дерева
fn get_visible_rows(nodes: &[ModelNode]) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    for (n_idx, node) in nodes.iter().enumerate() {
        rows.push(RenderRow { node_idx: n_idx, version_idx: None });
        if node.expanded {
            for v_idx in 0..node.versions.len() {
                rows.push(RenderRow { node_idx: n_idx, version_idx: Some(v_idx) });
            }
        }
    }
    rows
}

fn format_ago(date: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(date);
    if duration.num_weeks() > 0 {
        format!("({} weeks ago)", duration.num_weeks())
    } else if duration.num_days() > 0 {
        format!("({} days ago)", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("({} hours ago)", duration.num_hours())
    } else {
        format!("({} mins ago)", duration.num_minutes().max(0))
    }
}

pub async fn run(cfg: &Config) -> Result<()> {
    let bucket = storage::bucket(cfg)?;
    let objects = storage::list_all(&bucket).await?;
    let mut nodes = build_tree(objects);

    if nodes.is_empty() {
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
    let mut status = String::from("↑/↓ move · Enter open/download · q quit");

    loop {
        let visible_rows = get_visible_rows(&nodes);
        
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);

            let items: Vec<ListItem> = visible_rows
                .iter()
                .map(|r| {
                    let node = &nodes[r.node_idx];
                    if let Some(v_idx) = r.version_idx {
                        let v = &node.versions[v_idx];
                        let indent = "  ".repeat(node.depth + 1);
                        let size = humansize::format_size(v.size, humansize::DECIMAL);
                        let date_str = v.date.format("%Y-%m-%d %H:%M:%S").to_string();
                        let ago = format_ago(v.date);
                        ListItem::new(format!("{indent}└── {date_str} ({size}) {ago}"))
                            .style(Style::default().fg(Color::DarkGray))
                    } else {
                        let indent = "  ".repeat(node.depth);
                        let prefix = if node.expanded { "▼ " } else { "▶ " };
                        ListItem::new(format!("{indent}{prefix}{}", node.label))
                    }
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
                        state.select(Some((i + 1).min(visible_rows.len().saturating_sub(1))));
                    }
                    KeyCode::Up => {
                        let i = state.selected().unwrap_or(0);
                        state.select(Some(i.saturating_sub(1)));
                    }
                    KeyCode::Enter => {
                        if let Some(i) = state.selected() {
                            let r = &visible_rows[i];
                            if let Some(v_idx) = r.version_idx {
                                // Нажата конкретная версия — качаем её
                                let key_to_download = nodes[r.node_idx].versions[v_idx].key.clone();
                                status = format!("downloading {key_to_download}…");
                                terminal.draw(|f| {
                                    let p = Paragraph::new(status.as_str()).block(Block::default().borders(Borders::ALL));
                                    f.render_widget(p, f.area());
                                })?;
                                
                                match download_and_extract(cfg, &bucket, &key_to_download).await {
                                    Ok(dest) => status = format!("extracted to {}", dest.display()),
                                    Err(e) => status = format!("failed: {e}"),
                                }
                            } else {
                                // Нажата модель/папка — инвертируем флаг раскрытия
                                nodes[r.node_idx].expanded = !nodes[r.node_idx].expanded;
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

    let filename = std::path::Path::new(key)
        .file_name()
        .unwrap_or(std::ffi::OsStr::new(key));
    let tmp_zip = std::env::temp_dir().join(filename);
    storage::download_file(bucket, key, &tmp_zip).await?;

    let mut archive = zip::ZipArchive::new(File::open(&tmp_zip)?)?;
    archive.extract(&dest)?;
    let _ = std::fs::remove_file(&tmp_zip);

    Ok(dest)
}