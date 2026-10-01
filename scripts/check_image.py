#!/usr/bin/env python3
"""Verify a built image using disposable volumes and a fictional publication."""

import argparse
import hashlib
import http.cookiejar
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import urllib.request
import uuid
import zipfile


PASSWORD = "isolated-image-test-123"
MIGRATIONS = Path(__file__).resolve().parents[1] / "migrations"


def docker(*args):
    return subprocess.check_output(["docker", *args], stderr=subprocess.STDOUT).decode().strip()


def address(name):
    info = json.loads(docker("inspect", name))[0]
    assert info["Config"]["User"] not in ("", "0", "root"), "image must run without root"
    return "http://127.0.0.1:" + info["NetworkSettings"]["Ports"]["8080/tcp"][0]["HostPort"]


def request(client, base, path, body=None):
    headers = {"Content-Type": "application/json"} if body is not None else {}
    req = urllib.request.Request(base + path, data=json.dumps(body).encode() if body is not None else None, headers=headers)
    with client.open(req, timeout=15) as reply:
        return reply.read()


def wait_ready(client, base, path="/healthz"):
    for _ in range(60):
        try:
            request(client, base, path)
            return
        except (OSError, urllib.error.URLError):
            time.sleep(0.5)
    raise RuntimeError("image did not become ready: " + path)


def verify_migrations(db, directory=MIGRATIONS):
    # SQLx hashes the migration's original UTF-8 SQL with SHA-384. Compare the
    # actual versions and checksums, so an image missing new migrations fails
    # without hard-coding the migration count for every future schema change.
    expected = sorted(
        (int(path.name.split("_", 1)[0]), 1, hashlib.sha384(path.read_bytes()).digest())
        for path in directory.glob("*.sql")
    )
    assert expected, "no source migrations found"
    actual = db.execute(
        "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version"
    ).fetchall()
    assert actual == expected, "image migrations do not match the source versions, successful status and checksums"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image")
    args = parser.parse_args()
    containers, volumes = [], []
    try:
        with tempfile.TemporaryDirectory(prefix="bokhylle-image-check-") as temporary:
            root = Path(temporary)
            fixture = root / "The Lantern Archive - Mira Vale.epub"
            with zipfile.ZipFile(fixture, "w", compression=zipfile.ZIP_DEFLATED) as archive:
                archive.writestr("mimetype", "application/epub+zip")
                archive.writestr("META-INF/container.xml", '<container><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>')
                # Filename-only metadata exercises the watch-folder fallback.
                archive.writestr("book.opf", '<package><metadata/><manifest/><spine/></package>')
            digest = hashlib.sha256(fixture.read_bytes()).hexdigest()

            def create():
                name = "bokhylle-image-check-" + uuid.uuid4().hex[:12]
                owned = [name + "-" + part for part in ("config", "library", "downloads")]
                command = ["create", "--name", name, "--publish", "127.0.0.1::8080", "--env", "BOKHYLLE_ADMIN_PASSWORD=" + PASSWORD, "--env", "BOKHYLLE_SCAN_ON_STARTUP=false", "--env", "BOKHYLLE_UPDATE_CHECKS=false"]
                for volume, mount in zip(owned, ("/config", "/library", "/downloads")):
                    command += ["--mount", f"type=volume,src={volume},dst={mount}"]
                docker(*command, args.image)
                containers.append(name)
                volumes.extend(owned)
                return name

            name = create()
            docker("start", name)
            base = address(name)
            jar = http.cookiejar.CookieJar()
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
            wait_ready(client, base)
            request(client, base, "/api/auth/login", {"username": "admin", "password": PASSWORD})
            request(client, base, "/api/auth/me")
            request(client, base, "/api/health")
            assert b"/assets/" in request(client, base, "/"), "built frontend is missing"
            server_status = json.loads(request(client, base, "/api/admin/server"))
            assert server_status["build"]["installation"] == "docker", "image build identity is missing"
            assert server_status["build"]["builtAt"] > 0
            assert server_status["databaseOk"] and not server_status["restartRequired"]
            updates = json.loads(request(client, base, "/api/admin/server/updates"))
            assert updates["automaticChecks"] is False and updates["state"] == "not_checked"
            diagnostics = request(client, base, "/api/admin/server/diagnostics")
            assert json.loads(diagnostics)["formatVersion"] == 1
            assert PASSWORD.encode() not in diagnostics
            assert not any(private in diagnostics for private in (b'"path":', b'"username":', b'"url":'))
            docker("exec", name, "mkdir", "-p", "/config/ingest")
            docker("cp", str(fixture), name + ":/config/ingest/" + fixture.name)
            docker("exec", "--user", "0", name, "chown", "10001:10001", "/config/ingest/" + fixture.name)
            docker("exec", name, "touch", "-d", "2 minutes ago", "/config/ingest/" + fixture.name)
            # Settings writes use PUT rather than POST.
            req = urllib.request.Request(base + "/api/admin/settings/imports.watch_enabled", data=b'{"value":true}', headers={"Content-Type": "application/json"}, method="PUT")
            with client.open(req, timeout=15):
                pass
            docker("restart", name)
            base = address(name)
            wait_ready(client, base, "/api/auth/me")
            for _ in range(100):
                books = json.loads(request(client, base, "/api/books?scope=household"))["items"]
                if books:
                    break
                time.sleep(0.2)
            assert len(books) == 1 and books[0]["title"] == "The Lantern Archive", books
            assert books[0]["authors"] == ["Mira Vale"], books
            status = json.loads(request(client, base, "/api/admin/maintenance/watch"))
            assert status["pending"] == 0 and status["cleanupPending"] == 0, status
            backup = root / "backup.db"
            backup.write_bytes(request(client, base, "/api/admin/backup"))
            with sqlite3.connect(backup) as db:
                assert db.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
                assert db.execute("PRAGMA foreign_key_check").fetchall() == []
                verify_migrations(db)
                saved_path, saved_digest = db.execute("SELECT path, sha256 FROM book_files").fetchone()
                assert saved_digest == digest
            library = root / "library"
            library.mkdir()
            docker("stop", "--time", "10", name)
            docker("cp", name + ":/library/.", str(library))
            restored = create()
            docker("cp", str(backup), restored + ":/config/bokhylle.db")
            docker("cp", str(library) + "/.", restored + ":/library")
            docker("run", "--rm", "--user", "0", "--entrypoint", "chown",
                   "--mount", "type=volume,src=" + restored + "-config,dst=/config",
                   "--mount", "type=volume,src=" + restored + "-library,dst=/library",
                   args.image, "-R", "10001:10001", "/config", "/library")
            docker("start", restored)
            restored_base = address(restored)
            restored_client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            wait_ready(restored_client, restored_base)
            request(restored_client, restored_base, "/api/auth/login", {"username": "admin", "password": PASSWORD})
            restored_books = json.loads(request(restored_client, restored_base, "/api/books?scope=household"))["items"]
            assert restored_books[0]["title"] == books[0]["title"]
            retrieved = root / "restored.epub"
            docker("cp", restored + ":" + saved_path, str(retrieved))
            assert hashlib.sha256(retrieved.read_bytes()).hexdigest() == digest
            request(restored_client, restored_base, "/api/health")
            print("Image verified: fresh install, assets, server administration, diagnostics, filename import, restart, database/library restore, non-root runtime")
    except Exception:
        for name in containers:
            print(docker("logs", name)[-8000:])
        raise
    finally:
        for name in containers:
            subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for volume in volumes:
            subprocess.run(["docker", "volume", "rm", volume], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
