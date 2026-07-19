import argparse
import sys
from pathlib import Path

from app.config import load_config
from app.backup import run_backup
from app.audit import audit_all
from app.scanner import build_project_tree, flatten
from app.restore import restore


def main():
    parser = argparse.ArgumentParser(prog="backup-system")
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("cli", help="interactive menu (default)")
    backup_p = sub.add_parser("backup", help="run a backup and exit")
    backup_p.add_argument("--due", action="store_true",
                           help="only back up projects whose own schedule is due right now "
                                "(this is what the container's cron trigger uses)")
    sub.add_parser("audit", help="print audit report and exit")
    restore_p = sub.add_parser("restore", help="download+unzip an archive")
    restore_p.add_argument("key")
    restore_p.add_argument("--dest", default=None)

    args = parser.parse_args()
    cfg = load_config()

    if args.command == "backup":
        for line in run_backup(cfg, only_due=args.due):
            print(line)
    elif args.command == "audit":
        for report in audit_all(build_project_tree(cfg.backup_root, cfg), cfg):
            print("/".join(report.project.chain()), report.included_size, "unmarked:", report.unmarked_count)
    elif args.command == "restore":
        dest = Path(args.dest) if args.dest else None
        path = restore(cfg, args.key, dest)
        print(f"Restored to {path}")
    else:
        from app.cli import main_menu
        main_menu()


if __name__ == "__main__":
    sys.exit(main() or 0)
