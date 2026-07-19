from collections import defaultdict
from pathlib import Path

import questionary
from rich.console import Console
from rich.table import Table
from rich.tree import Tree

from app.config import load_config
from app.naming import parse_name
from app.storage import list_objects
from app.scanner import build_project_tree, flatten, scan_project
from app.audit import audit_project
from app.backup import run_backup
from app.sizes import human
from app.restore import restore

console = Console()


def _group_backups(objects: list[dict]) -> dict:
    """server -> chain-path-tuple -> list of (key, size, last_modified)"""
    tree = defaultdict(lambda: defaultdict(list))
    for obj in objects:
        parsed = parse_name(obj["Key"])
        if not parsed:
            continue
        tree[parsed["server"]][tuple(parsed["chain"])].append(
            (obj["Key"], obj["Size"], parsed["date"])
        )
    return tree


def show_backups(cfg):
    objects = list_objects(cfg)
    if not objects:
        console.print("[yellow]No backups found in storage yet.[/yellow]")
        return
    grouped = _group_backups(objects)
    root = Tree("\U0001F5C4  Backups")
    for server, chains in sorted(grouped.items()):
        server_node = root.add(f"[bold cyan]{server}[/bold cyan]")
        nodes = {(): server_node}
        for chain in sorted(chains):
            for i in range(1, len(chain) + 1):
                sub = chain[:i]
                if sub in nodes:
                    continue
                parent = nodes[sub[:-1]]
                nodes[sub] = parent.add(f"[green]{sub[-1]}[/green]")
        for chain, versions in sorted(chains.items()):
            node = nodes[chain]
            for key, size, date in sorted(versions, key=lambda v: v[2], reverse=True):
                node.add(f"{date}  [dim]{human(size)}[/dim]  {key}")
    console.print(root)


def show_audit(cfg):
    projects = build_project_tree(cfg.backup_root, cfg)
    flat = flatten(projects)
    if not flat:
        console.print("[yellow]No projects (.backup markers) found under[/yellow]", cfg.backup_root)
        return
    table = Table(title="Audit")
    table.add_column("Project")
    table.add_column("To back up", justify="right")
    table.add_column("Unmarked", justify="right")
    table.add_column("Skipped (too big)")
    table.add_column("Large files")
    table.add_column("Schedule")
    table.add_column("Retention")
    table.add_column("Config warnings")
    for project in flat:
        result = scan_project(project, cfg)
        report = audit_project(result, cfg)
        skipped = ", ".join(f"{p.name} ({human(sz)})" for p, sz, _ in report.too_big_dirs) or "-"
        large = ", ".join(f"{p.name} ({human(sz)})" for p, sz in report.large_files) or "-"
        warn = f"[red]{report.unmarked_count}[/red]" if report.unmarked_count else "0"
        s = project.settings
        retention = f"{s.keep_days}d"
        if s.keep_versions is not None:
            retention += f" / {s.keep_versions}v"
        if s.max_total_size_bytes is not None:
            retention += f" / \u2264{human(s.max_total_size_bytes)}"
        warnings = "\n".join(project.settings_warnings) or "-"
        table.add_row("/".join(project.chain()), human(report.included_size), warn,
                      skipped, large, s.cron, retention, warnings)
    console.print(table)


def do_backup_now(cfg):
    console.print("[cyan]Running backup...[/cyan]")
    for line in run_backup(cfg):
        console.print(line)


def do_restore(cfg):
    objects = list_objects(cfg)
    keys = sorted((o["Key"] for o in objects if parse_name(o["Key"])), reverse=True)
    if not keys:
        console.print("[yellow]Nothing to restore.[/yellow]")
        return
    key = questionary.select("Choose an archive to restore:", choices=keys).ask()
    if not key:
        return
    dest = questionary.text("Restore into which base folder?", default=str(cfg.restore_root)).ask()
    if dest is None:
        return
    path = restore(cfg, key, Path(dest))
    console.print(f"[green]Restored to {path}[/green]")


def show_settings(cfg):
    table = Table(title="Config (defaults - see Audit for per-project overrides from .backup)")
    table.add_column("Key")
    table.add_column("Value")
    for k in ("server_name", "timezone", "backup_root", "restore_root", "s3_bucket",
              "audit_max_dir_mb", "audit_max_dir_files", "retention_days", "backup_cron"):
        table.add_row(k, str(getattr(cfg, k)))
    console.print(table)


def main_menu():
    cfg = load_config()
    while True:
        choice = questionary.select(
            "Backup system",
            choices=[
                "Show backups (hierarchy)",
                "Run backup now",
                "Audit",
                "Restore a backup",
                "Settings",
                "Exit",
            ],
        ).ask()
        if choice is None or choice == "Exit":
            break
        console.rule(choice)
        if choice == "Show backups (hierarchy)":
            show_backups(cfg)
        elif choice == "Run backup now":
            do_backup_now(cfg)
        elif choice == "Audit":
            show_audit(cfg)
        elif choice == "Restore a backup":
            do_restore(cfg)
        elif choice == "Settings":
            show_settings(cfg)
        console.print()
