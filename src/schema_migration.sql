DROP INDEX idx_command_history_unique;

CREATE TABLE command_history_new (
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

-- Create a separate unique index that handles NULLs properly
CREATE UNIQUE INDEX idx_command_history_unique 
ON command_history_new(
    full_command,
    start_unix_timestamp,
    shellname,
    COALESCE(username, ''),
    COALESCE(hostname, ''),
    COALESCE(working_directory, '')
);

INSERT INTO command_history_new (
    session_id,
    full_command,
    shellname,
    hostname,
    username,
    working_directory,
    exit_status,
    start_unix_timestamp,
    end_unix_timestamp
)
SELECT 
    session_id,
    full_command,
    shellname,
    hostname,
    username,
    working_directory,
    exit_status,
    start_unix_timestamp,
    end_unix_timestamp
FROM (
    SELECT *,
        ROW_NUMBER() OVER (
            PARTITION BY 
                full_command,
                start_unix_timestamp,
                shellname,
                COALESCE(username, ''),
                COALESCE(hostname, ''),
                COALESCE(working_directory, '')
            ORDER BY id
        ) as rn
    FROM command_history
)
WHERE rn = 1;

DROP TABLE command_history;
ALTER TABLE command_history_new RENAME TO command_history;

VACUUM;
