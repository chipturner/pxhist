CREATE TABLE IF NOT EXISTS command_history (
       id INTEGER PRIMARY KEY AUTOINCREMENT,
       session_id INTEGER NOT NULL,
       full_command BLOB NOT NULL,
       shellname TEXT NOT NULL,
       hostname BLOB,
       username BLOB,
       working_directory BLOB,
       exit_status INTEGER,
       start_unix_timestamp INTEGER,
       end_unix_timestamp INTEGER
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_command_history_unique
ON command_history (
    full_command,
    start_unix_timestamp,
    shellname,
    COALESCE(username, ''),
    COALESCE(hostname, ''),
    COALESCE(working_directory, '')
);

CREATE INDEX IF NOT EXISTS history_session_id ON command_history(session_id);
CREATE INDEX IF NOT EXISTS history_start_time ON command_history(start_unix_timestamp);

CREATE TABLE IF NOT EXISTS settings (
       key TEXT PRIMARY KEY,
       value BLOB
);

ATTACH DATABASE ':memory:' AS memdb;
CREATE TABLE memdb.show_results (
       ch_rowid INTEGER NOT NULL,
       ch_start_unix_timestamp INTEGER,
       ch_id INTEGER NOT NULL
);

CREATE INDEX memdb.result_timestamp ON show_results(ch_start_unix_timestamp, ch_id);
