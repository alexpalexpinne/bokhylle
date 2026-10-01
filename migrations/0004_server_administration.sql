-- Operational state survives restarts; snapshots still live in config/backups.
CREATE TABLE server_backup_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_attempt_at INTEGER,
    outcome TEXT NOT NULL DEFAULT 'idle' CHECK (outcome IN ('idle', 'running', 'succeeded', 'failed')),
    last_success_at INTEGER,
    last_success_size INTEGER,
    last_failure_at INTEGER,
    last_failure_summary TEXT
);
INSERT INTO server_backup_state (id) VALUES (1);

CREATE TABLE server_release_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    checked_at INTEGER,
    last_success_at INTEGER,
    latest_version TEXT,
    release_url TEXT,
    error TEXT
);
INSERT INTO server_release_state (id) VALUES (1);
