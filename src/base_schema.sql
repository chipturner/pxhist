CREATE TABLE IF NOT EXISTS command_history  (
       id INTEGER PRIMARY KEY AUTOINCREMENT,
       session_id INTEGER NOT NULL,
       full_command BLOB NOT NULL,
       shellname TEXT NOT NULL,
       hostname BLOB,
       username BLOB,
       working_directory BLOB,
       exit_status INTEGER,
       start_unix_timestamp INTEGER,
       end_unix_timestamp INTEGER,
       UNIQUE(full_command, start_unix_timestamp, shellname, username, hostname, working_directory) ON CONFLICT IGNORE
);

CREATE INDEX IF NOT EXISTS history_session_id ON command_history(session_id);
CREATE INDEX IF NOT EXISTS history_start_time ON command_history(start_unix_timestamp);

