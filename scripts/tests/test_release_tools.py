"""Exercise release tools with disposable repositories and website outputs."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ReleaseTools(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.repo = self.base / "working"
        self.repo.mkdir()
        self.run_git("init", "-b", "main")
        self.run_git("config", "user.name", "Release Test")
        self.run_git("config", "user.email", "release-test@example.com")
        (self.repo / "migrations").mkdir()
        (self.repo / "migrations/0001.sql").write_text("CREATE TABLE books(id INTEGER);\n")
        (self.repo / ".gitignore").write_text(".env\n/data\n")
        self.run_git("add", ".")
        self.run_git("commit", "-m", "fixture")
        self.commit = self.run_git("rev-parse", "HEAD").stdout.strip()

    def run_git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, check=True,
                              capture_output=True, text=True)

    def migration_check(self, base):
        return subprocess.run([sys.executable, str(ROOT / "scripts/check_migrations.py"), base],
                              cwd=self.repo, capture_output=True, text=True)

    def test_initial_push_without_a_previous_commit(self):
        result = self.migration_check("0" * 40)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Initial push", result.stdout)

    def test_additions_allowed_but_existing_migrations_immutable(self):
        (self.repo / "migrations/0002.sql").write_text("CREATE TABLE shelves(id INTEGER);\n")
        self.assertEqual(self.migration_check(self.commit).returncode, 0)
        (self.repo / "migrations/0001.sql").write_text("DROP TABLE books;\n")
        result = self.migration_check(self.commit)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("migrations/0001.sql", result.stderr)

    def test_deleted_migration_and_invalid_base_fail(self):
        (self.repo / "migrations/0001.sql").unlink()
        self.assertNotEqual(self.migration_check(self.commit).returncode, 0)
        self.assertNotEqual(self.migration_check("not-a-commit").returncode, 0)

    def export(self):
        scripts = self.repo / "scripts"
        scripts.mkdir(exist_ok=True)
        shutil.copy2(ROOT / "scripts/export_public_repo.py", scripts / "export_public_repo.py")
        return subprocess.run([sys.executable, str(scripts / "export_public_repo.py"),
                               str(self.base / "bokhylle")], capture_output=True, text=True)

    def test_export_preserves_current_source_without_history_or_local_data(self):
        (self.repo / "migrations/0001.sql").write_text("current working source\n")
        (self.repo / "new-source.txt").write_text("uncommitted source\n")
        (self.repo / ".env").write_text("LOCAL_SETTING=fixture\n")
        (self.repo / "data").mkdir()
        (self.repo / "data/private.txt").write_text("private fixture\n")
        result = self.export()
        self.assertEqual(result.returncode, 0, result.stderr)
        public = self.base / "bokhylle"
        self.assertEqual((public / "migrations/0001.sql").read_text(), "current working source\n")
        self.assertTrue((public / "new-source.txt").is_file())
        for name in (".git", ".env", "data"):
            self.assertFalse((public / name).exists())

    def test_export_refuses_visible_private_configuration(self):
        (self.repo / ".env.production").write_text("LOCAL_SETTING=fixture\n")
        result = self.export()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("private or generated file", result.stderr)
        self.assertFalse((self.base / "bokhylle").exists())

    def test_website_requires_https_and_exports_only_public_assets(self):
        output = self.base / "website"
        command = [sys.executable, str(ROOT / "website/build.py"), "--output", str(output)]
        invalid = subprocess.run(command + ["--demo-url", "http://demo.example.com"],
                                 capture_output=True, text=True)
        self.assertNotEqual(invalid.returncode, 0)
        self.assertFalse(output.exists())
        valid = subprocess.run(command + ["--demo-url", "https://demo.example.com"],
                               capture_output=True, text=True)
        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertIn('content="https://demo.example.com"', (output / "index.html").read_text())
        self.assertTrue((output / "assets/bokhylle-icon.svg").is_file())
        self.assertFalse((output / "README.md").exists())
        self.assertFalse((output / "build.py").exists())


if __name__ == "__main__":
    unittest.main()
