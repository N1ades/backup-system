"""Run with: python test_core.py
Not a pytest suite on purpose - one file, plain asserts, exercises the
actual logic rather than mocking it away."""
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

from app.config import Config
from app.scanner import build_project_tree, flatten, scan_project
from app.naming import build_name, parse_name
from app.sizes import parse as parse_size
from app.backup import is_due, run_retention
from app import backup as backup_module


def make_tree(base: Path):
    proj = base / "myproject"
    proj.mkdir()
    (proj / ".backup").write_text(
        "#remnanode\n"
        "#cron: */5 * * * *\n"
        "#keep_versions: 3\n"
        "#keep_days: 7\n"
        "#max_total_size: 1MB\n"
    )
    (proj / "keep.txt").write_text("x" * 100)
    (proj / ".backupignore").write_text("*.log\nnode_modules/\n")
    (proj / "debug.log").write_text("noise")
    (proj / "node_modules").mkdir()
    (proj / "node_modules" / "pkg.js").write_text("y" * 50)
    (proj / "random_unmarked.bin").write_text("z" * 10)

    # a genuinely huge unmarked dir -> should be skipped + flagged, not by name
    big = proj / "cache_dump"
    big.mkdir()
    (big / "blob.bin").write_bytes(b"0" * (2 * 1024 * 1024))  # 2MB

    # nested project: independent scope, parent's .backupignore must NOT reach in
    nested = proj / "sub"
    nested.mkdir()
    (nested / ".backup").write_text("#child\n")
    (nested / "debug.log").write_text("this SHOULD be kept, parent's ignore doesn't apply here")
    return proj, nested


def test_nesting_and_ignore_precedence():
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        proj_path, nested_path = make_tree(base)
        cfg = Config(audit_max_dir_mb=1)  # 1MB threshold so cache_dump (2MB) trips it

        roots = build_project_tree(base, cfg)
        assert len(roots) == 1
        root = roots[0]
        assert root.name == "remnanode", "should read name from #comment in .backup"
        assert len(root.children) == 1
        assert root.children[0].name == "child"
        assert root.children[0].chain() == ["remnanode", "child"]

        result = scan_project(root, cfg)
        included_names = {f.path.name for f in result.included}
        assert "keep.txt" in included_names
        assert "debug.log" not in included_names, ".log excluded by .backupignore"
        assert "pkg.js" not in included_names, "node_modules/ excluded by .backupignore"
        assert "random_unmarked.bin" in included_names, "unmarked still backed up (not huge)"
        assert any(f.reason == "unmarked" for f in result.included if f.path.name == "random_unmarked.bin")
        assert not any(f.path.name == "blob.bin" for f in result.included), "huge unmarked dir must be skipped"
        assert any(p.name == "cache_dump" for p, _, _ in result.too_big_dirs)

        nested_result = scan_project(root.children[0], cfg)
        nested_included = {f.path.name for f in nested_result.included}
        assert "debug.log" in nested_included, "parent's .backupignore must not reach into nested project"


def test_backup_file_directives():
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        make_tree(base)
        cfg = Config()
        root = build_project_tree(base, cfg)[0]
        s = root.settings
        assert s.cron == "*/5 * * * *"
        assert s.keep_versions == 3
        assert s.keep_days == 7
        assert s.max_total_size_bytes == parse_size("1MB")
        assert root.settings_warnings == []

        # nested project has no directives -> falls back to global config
        child = root.children[0]
        assert child.settings.cron == cfg.backup_cron
        assert child.settings.keep_days == cfg.retention_days
        assert child.settings.max_total_size_bytes is None


def test_bad_directive_falls_back_with_warning():
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        proj = base / "broken"
        proj.mkdir()
        (proj / ".backup").write_text("#cron: not-a-cron\n#keep_days: nope\n")
        cfg = Config(backup_cron="0 3 * * *", retention_days=30)
        root = build_project_tree(base, cfg)[0]
        assert root.settings.cron == "0 3 * * *"
        assert root.settings.keep_days == 30
        assert len(root.settings_warnings) == 2


def test_is_due():
    now = datetime(2026, 7, 19, 3, 0, tzinfo=timezone.utc)
    assert is_due("0 3 * * *", now)
    assert not is_due("0 4 * * *", now)
    assert is_due("*/5 * * * *", now)


def test_naming_roundtrip():
    name = build_name("srv1", ["remnanode", "child"], "UTC")
    assert name.startswith("srv1__remnanode__child_")
    assert name.endswith(".zip")
    parsed = parse_name(name)
    assert parsed["server"] == "srv1"
    assert parsed["chain"] == ["remnanode", "child"]


class _FakeProject:
    def __init__(self, chain, settings):
        self._chain = chain
        self.settings = settings

    def chain(self):
        return self._chain


class _Settings:
    def __init__(self, cron="0 3 * * *", keep_versions=None, keep_days=30, max_total_size_bytes=None):
        self.cron = cron
        self.keep_versions = keep_versions
        self.keep_days = keep_days
        self.max_total_size_bytes = max_total_size_bytes


def test_retention_never_deletes_newest_and_respects_max_size():
    now = datetime.now(timezone.utc)
    server = "srv1"
    chain = ["proj"]
    fake_objects = []
    for i in range(5):  # 5 versions, 1 day apart, newest = i=0
        when = now - timedelta(days=i)
        name = build_name(server, chain, "UTC", when.astimezone(timezone.utc))
        fake_objects.append({"Key": name, "Size": 400_000})  # 400KB each

    deleted = []

    def fake_list_objects(cfg, prefix=""):
        return fake_objects

    def fake_delete(cfg, key):
        deleted.append(key)

    orig_list, orig_delete = backup_module.list_objects, backup_module.delete
    backup_module.list_objects = fake_list_objects
    backup_module.delete = fake_delete
    try:
        cfg = Config(server_name=server)
        # keep_versions=1 AND tiny max size -> would want to delete everything,
        # but the newest must survive regardless
        settings = _Settings(keep_versions=1, keep_days=30, max_total_size_bytes=1)
        removed = run_retention(cfg, [_FakeProject(chain, settings)])
        assert len(removed) == 4, "should drop the 4 older versions"
        newest_key = build_name(server, chain, "UTC", now.astimezone(timezone.utc))
        assert newest_key not in removed, "must never delete the single newest archive"
    finally:
        backup_module.list_objects = orig_list
        backup_module.delete = orig_delete


if __name__ == "__main__":
    test_nesting_and_ignore_precedence()
    test_backup_file_directives()
    test_bad_directive_falls_back_with_warning()
    test_is_due()
    test_naming_roundtrip()
    test_retention_never_deletes_newest_and_respects_max_size()
    print("OK - all self-checks passed")
