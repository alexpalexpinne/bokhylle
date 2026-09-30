"""The image gate must follow source migrations and reject stale image schemas."""

import hashlib
import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "check_image", Path(__file__).resolve().parents[1] / "check_image.py"
)
CHECK_IMAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK_IMAGE)


class ImageMigrations(unittest.TestCase):
    def test_new_migrations_are_required_without_changing_the_checker(self):
        with tempfile.TemporaryDirectory() as temporary, sqlite3.connect(":memory:") as db:
            directory = Path(temporary)
            db.execute("CREATE TABLE _sqlx_migrations (version INTEGER, success INTEGER, checksum BLOB)")
            for version in (1, 2, 3):
                sql = f"CREATE TABLE fixture_{version} (id INTEGER);\n".encode()
                (directory / f"{version:04}_fixture.sql").write_bytes(sql)
                db.execute("INSERT INTO _sqlx_migrations VALUES (?, 1, ?)",
                           (version, hashlib.sha384(sql).digest()))
                CHECK_IMAGE.verify_migrations(db, directory)
            (directory / "0004_more.sql").write_text("SELECT 1;\n")
            with self.assertRaisesRegex(AssertionError, "do not match"):
                CHECK_IMAGE.verify_migrations(db, directory)

    def test_incomplete_unexpected_and_mismatched_records_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            sql = b"CREATE TABLE books (id INTEGER);\n"
            (directory / "0001_books.sql").write_bytes(sql)
            checksum = hashlib.sha384(sql).digest()
            cases = {
                "missing": [],
                "failed": [(1, 0, checksum)],
                "changed SQL": [(1, 1, b"wrong checksum")],
                "unexpected": [(1, 1, checksum), (2, 1, checksum)],
                "wrong version": [(2, 1, checksum)],
            }
            for name, rows in cases.items():
                with self.subTest(name=name), sqlite3.connect(":memory:") as db:
                    db.execute("CREATE TABLE _sqlx_migrations (version INTEGER, success INTEGER, checksum BLOB)")
                    db.executemany("INSERT INTO _sqlx_migrations VALUES (?, ?, ?)", rows)
                    with self.assertRaisesRegex(AssertionError, "do not match"):
                        CHECK_IMAGE.verify_migrations(db, directory)


if __name__ == "__main__":
    unittest.main()
