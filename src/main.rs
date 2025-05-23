use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    fs::{File, OpenOptions},
    io,
    io::{BufRead, BufReader, Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    str,
};

use bstr::{BString, ByteSlice};
use clap::{Parser, Subcommand};
use regex::bytes::Regex;
use rusqlite::{Connection, Result, TransactionBehavior};
use tempfile::NamedTempFile;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct PxhArgs {
    #[clap(long, env = "PXH_DB_PATH")]
    db: Option<PathBuf>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(visible_alias = "s", about = "search for and display history entries")]
    Show(ShowCommand),
    #[clap(about = "install pxh helpers by modifying your shell rc file")]
    Install(InstallCommand),
    #[clap(about = "import history entries from your existing shell history or from an export")]
    Import(ImportCommand),
    #[clap(about = "export full history as JSON")]
    Export(ExportCommand),
    #[clap(about = "synchronize to and from a directory of other pxh history databases")]
    Sync(SyncCommand),
    #[clap(about = "scrub (remove) history entries matching the prompted-for string")]
    Scrub(ScrubCommand),
    #[clap(about = "(internal) invoked by the shell to insert a history entry")]
    Insert(InsertCommand),
    #[clap(about = "(internal) seal the previous inserted command to mark status, timing, etc")]
    Seal(SealCommand),
    #[clap(about = "(internal) shell configuration suitable for `source`'ing to enable pxh")]
    ShellConfig(ShellConfigCommand),
    #[clap(
        about = "perform ANALYZE and VACUUM on the specified database files to optimize performance and reclaim space"
    )]
    Maintenance(MaintenanceCommand),
}

#[derive(Parser, Debug)]
struct InstallCommand {
    #[clap(help = "shell to install helpers into")]
    shellname: String,
}

#[derive(Parser, Debug)]
struct ShowCommand {
    #[clap(short = 'i', long, help = "perform case-insensitive matching", default_value_t = false)]
    ignore_case: bool,
    #[clap(
        short,
        long,
        default_value_t = 50,
        help = "display at most this many entries; 0 for unlimited"
    )]
    limit: usize,
    #[clap(short, long, help = "display extra fields in the output")]
    verbose: bool,
    #[clap(long, help = "suppress headers")]
    suppress_headers: bool,
    #[clap(long, help = "show entries that were populated while in the current working directory")]
    here: bool,
    #[clap(
        long,
        help = "alters --here; instead of the current working directory, use the specified directory"
    )]
    working_directory: Option<PathBuf>,
    #[clap(
        long,
        help = "display only commands from the specified session (use $PXH_SESSION_ID for this session)"
    )]
    session: Option<String>,
    #[clap(
        long,
        help = "if specified, list of patterns can be matched in any order against command lines"
    )]
    loosen: bool,
    #[clap(
        help = "one or more regular expressions to search through history entries; multiple values joined by `.*\\s.*`"
    )]
    patterns: Vec<String>,
}

#[derive(Parser, Debug)]
struct ImportCommand {
    #[clap(long, help = "path to history file to import")]
    histfile: PathBuf,
    #[clap(long, help = "type of shell history specified by --histfile")]
    shellname: String,
    #[clap(long, help = "hostname to tag imported entries with (defaults to current hostname)")]
    hostname: Option<OsString>,
    #[clap(long, help = "username to tag importen entries with (defaults to current user)")]
    username: Option<OsString>,
}

#[derive(Parser, Debug)]
struct SyncCommand {
    #[clap(help = "Directory for sync operations (required for directory-based sync)")]
    dirname: Option<PathBuf>,
    #[clap(
        long,
        help = "Only export the current database; do not read other databases",
        default_value_t = false
    )]
    export_only: bool,
    #[clap(long, help = "Remote host to sync with via SSH")]
    remote: Option<String>,
    #[clap(
        long,
        help = "Only send database to remote (no receive)",
        conflicts_with = "receive_only"
    )]
    send_only: bool,
    #[clap(
        long,
        help = "Only receive database from remote (no send)",
        conflicts_with = "send_only"
    )]
    receive_only: bool,
    #[clap(long, help = "Remote database path")]
    remote_db: Option<PathBuf>,
    #[clap(
        short = 'e',
        long,
        default_value = "ssh",
        help = "SSH command to use for connection (like rsync's -e option)"
    )]
    ssh_cmd: String,
    #[clap(long, default_value = "pxh", help = "Path to pxh binary on the remote host")]
    remote_pxh: String,
    #[clap(long, help = "Internal: run in server mode")]
    server: bool,
    #[clap(long, help = "Only sync commands from the last N days", value_name = "DAYS")]
    since: Option<u32>,
    #[clap(long, help = "Use stdin/stdout for sync instead of SSH (for testing)")]
    stdin_stdout: bool,
}

#[derive(Parser, Debug)]
struct ScrubCommand {
    #[clap(long, help = "If specified, also remove lines from this file; typically $HISTFILE")]
    histfile: Option<PathBuf>,
    #[clap(
        short = 'n',
        long,
        help = "Dry-run mode (only display the rows, don't actually scrub)",
        default_value_t = false
    )]
    dry_run: bool,
    #[clap(
        help = "The string to scrub.  Avoid this parameter and prefer being prompted for the value to be provided interactively."
    )]
    contraband: Option<String>,
}

#[derive(Parser, Debug)]
struct InsertCommand {
    #[clap(long)]
    shellname: String,
    #[clap(long)]
    hostname: OsString,
    #[clap(long)]
    username: OsString,
    #[clap(long)]
    working_directory: Option<PathBuf>, // option because importing may lack working dir
    #[clap(long)]
    exit_status: Option<i64>,
    #[clap(long)]
    session_id: i64,
    #[clap(long)]
    start_unix_timestamp: Option<i64>, // similar to above
    #[clap(long)]
    end_unix_timestamp: Option<i64>,
    command: Vec<OsString>,
}

#[derive(Parser, Debug)]
struct SealCommand {
    #[clap(long)]
    session_id: i64,
    #[clap(long)]
    exit_status: i32,
    #[clap(long)]
    end_unix_timestamp: i64,
}

#[derive(Parser, Debug)]
struct ShellConfigCommand {
    shellname: String,
}

#[derive(Parser, Debug)]
struct ExportCommand {}

#[derive(Parser, Debug)]
struct MaintenanceCommand {
    #[clap(
        help = "Path(s) to SQLite database files to maintain (if not specified, maintains the current database)"
    )]
    files: Vec<PathBuf>,
}

impl ImportCommand {
    fn go(&self, mut conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        let invocations = match self.shellname.as_ref() {
            "zsh" => pxh::import_zsh_history(
                &self.histfile,
                self.hostname.as_ref().map(|v| v.as_bytes().into()),
                self.username.as_ref().map(|v| v.as_bytes().into()),
            ),
            "bash" => pxh::import_bash_history(
                &self.histfile,
                self.hostname.as_ref().map(|v| v.as_bytes().into()),
                self.username.as_ref().map(|v| v.as_bytes().into()),
            ),
            "json" => pxh::import_json_history(&self.histfile),
            _ => Err(Box::from(format!("Unsupported shell: {} (PRs welcome!)", self.shellname))),
        }?;
        let tx = conn.transaction()?;
        for invocation in invocations {
            invocation.insert(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl ShellConfigCommand {
    fn go(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Todo: bash and other shell formats
        let contents = match self.shellname.as_str() {
            "zsh" => String::from(include_str!("shell_configs/pxh.zsh")),
            "bash" => {
                let mut contents = String::new();
                contents.push_str(include_str!("shell_configs/bash-preexec/bash-preexec.sh"));
                contents.push_str(include_str!("shell_configs/pxh.bash"));
                contents
            }
            _ => {
                return Err(Box::from(format!(
                    "Unsupported shell: {} (PRs welcome!)",
                    self.shellname
                )));
            }
        };
        io::stdout().write_all(contents.as_bytes())?;
        io::stdout().flush()?;
        Ok(())
    }
}

impl InstallCommand {
    fn go(&self) -> Result<(), Box<dyn std::error::Error>> {
        let shellname = self.shellname.as_ref();
        let rc_file = match shellname {
            "zsh" => ".zshrc",
            "bash" => ".bashrc",
            _ => return Err(Box::from(format!("Unsupported shell: {shellname} (PRs welcome!)"))),
        };

        let mut pb = home::home_dir().ok_or("Unable to determine your homedir")?;
        pb.push(rc_file);

        // Skip installationif "pxh shell-config" is present in the
        // current RC file.
        let file = File::open(&pb)?;
        let reader = BufReader::new(&file);
        for line in reader.lines() {
            let line = line.unwrap();
            if line.contains("pxh shell-config") {
                println!("Shell config already present in {}; taking no action.", pb.display());
                return Ok(());
            }
        }

        let mut file = OpenOptions::new().append(true).open(&pb)?;

        write!(file, "\n# Install the pxh shell helpers to add interactive history realtime.")?;
        writeln!(
            file,
            r#"
if command -v pxh &> /dev/null; then
    source <(pxh shell-config {shellname})
fi"#
        )?;
        println!("Shell config successfully added to {}.", pb.display());
        println!(
            "pxh will be active for all new shell sessions.  To activate for this session, run:"
        );
        println!("  source <(pxh shell-config {shellname})");
        Ok(())
    }
}

impl SealCommand {
    fn go(&self, conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        conn.execute(
            r#"
UPDATE command_history SET exit_status = ?, end_unix_timestamp = ?
 WHERE exit_status is NULL
   AND end_unix_timestamp IS NULL
   AND id = (SELECT MAX(id) FROM command_history hi WHERE hi.session_id = ?)"#,
            (self.exit_status, self.end_unix_timestamp, self.session_id),
        )?;
        Ok(())
    }
}

impl ExportCommand {
    fn go(&self, conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        let mut stmt = conn.prepare(
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
ORDER BY id"#,
    )?;
        let rows: Result<Vec<pxh::Invocation>, _> =
            stmt.query_map([], pxh::Invocation::from_row)?.collect();
        let rows = rows?;
        pxh::json_export(&rows)?;
        Ok(())
    }
}

impl MaintenanceCommand {
    fn go(&self, default_conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Helper function to get database size and other stats
        fn get_db_info(conn: &Connection) -> Result<(i64, i64, i64), Box<dyn std::error::Error>> {
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            let freelist_count: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
            Ok((page_count, page_size, freelist_count))
        }

        // Helper function to run maintenance on a single database connection
        fn maintain_database(
            conn: &Connection,
            db_name: &str,
        ) -> Result<(), Box<dyn std::error::Error>> {
            // Show database information before maintenance
            let (page_count, page_size, freelist_count) = get_db_info(conn)?;
            let total_size = page_count * page_size;
            let freelist_size = freelist_count * page_size;

            println!("Database '{db_name}' information before maintenance:");
            println!("  Total size: {:.2} MB", total_size as f64 / 1024.0 / 1024.0);
            println!("  Free space: {:.2} MB", freelist_size as f64 / 1024.0 / 1024.0);
            println!("  Page count: {page_count}");
            println!("  Page size: {page_size} bytes");
            println!("  Freelist count: {freelist_count}");

            // Show row counts for main tables
            let command_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0))
                .unwrap_or_default(); // Handle case where table might not exist
            println!("  Command history entries: {command_count}");
            println!();

            // Remove non-standard tables (except those prefixed with KEEP_)
            println!("Looking for non-standard tables to clean up...");
            let mut cleanup_count = 0;

            // Define the standard tables (excluding memory database tables)
            let standard_tables = ["command_history", "settings", "sqlite_sequence"];

            // Get all tables from the database
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'
                                        EXCEPT SELECT name FROM sqlite_master WHERE name IN (?1, ?2, ?3)")?;

            let non_standard_tables: Vec<String> = stmt
                .query_map(
                    [&standard_tables[0], &standard_tables[1], &standard_tables[2]],
                    |row| row.get(0),
                )?
                .collect::<Result<Vec<String>, _>>()?;

            for table_name in non_standard_tables {
                if table_name.starts_with("KEEP_") {
                    println!("  Keeping user table: {table_name}");
                    continue;
                }

                println!("  Dropping non-standard table: {table_name}");
                conn.execute(&format!("DROP TABLE IF EXISTS {table_name}"), [])?;
                cleanup_count += 1;
            }

            if cleanup_count > 0 {
                println!("Cleaned up {cleanup_count} non-standard tables");
            } else {
                println!("No non-standard tables found to clean up");
            }

            // Clean up non-standard indexes (except those prefixed with KEEP_)
            println!("Looking for non-standard indexes to clean up...");
            let standard_indexes =
                ["idx_command_history_unique", "history_session_id", "history_start_time"];

            // Exclude system indexes (sqlite_autoindex_*) and the standard indexes.
            // Also exclude indexes that relate to PRIMARY KEY or UNIQUE constraints to avoid errors
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='index' AND 
                                        name NOT LIKE 'sqlite_autoindex_%' AND 
                                        tbl_name NOT LIKE 'sqlite_%' AND 
                                        name NOT IN (?1, ?2, ?3)",
            )?;

            let non_standard_indexes: Vec<String> = stmt
                .query_map(
                    [&standard_indexes[0], &standard_indexes[1], &standard_indexes[2]],
                    |row| row.get(0),
                )?
                .collect::<Result<Vec<String>, _>>()?;

            cleanup_count = 0;
            for index_name in non_standard_indexes {
                if index_name.starts_with("KEEP_") {
                    println!("  Keeping user index: {index_name}");
                    continue;
                }

                // Try to drop the index, but don't fail if it can't be dropped
                // (might be a PRIMARY KEY or UNIQUE constraint)
                match conn.execute(&format!("DROP INDEX IF EXISTS {index_name}"), []) {
                    Ok(_) => {
                        println!("  Dropping non-standard index: {index_name}");
                        cleanup_count += 1;
                    }
                    Err(e) => {
                        println!("  Skipping index {index_name}: {e}");
                    }
                }
            }

            if cleanup_count > 0 {
                println!("Cleaned up {cleanup_count} non-standard indexes");
            } else {
                println!("No non-standard indexes found to clean up");
            }

            // Run ANALYZE to update statistics
            println!("Running ANALYZE...");
            conn.execute("ANALYZE", [])?;
            println!("ANALYZE completed successfully.");

            // Run VACUUM to reclaim space
            println!("Running VACUUM...");
            conn.execute("VACUUM", [])?;
            println!("VACUUM completed successfully.");

            // Show database information after maintenance
            let (page_count, page_size, freelist_count) = get_db_info(conn)?;
            let total_size = page_count * page_size;
            let freelist_size = freelist_count * page_size;

            println!("\nDatabase '{db_name}' information after maintenance:");
            println!("  Total size: {:.2} MB", total_size as f64 / 1024.0 / 1024.0);
            println!("  Free space: {:.2} MB", freelist_size as f64 / 1024.0 / 1024.0);
            println!("  Page count: {page_count}");
            println!("  Page size: {page_size} bytes");
            println!("  Freelist count: {freelist_count}");

            println!("\nDatabase '{db_name}' maintenance completed.");
            Ok(())
        }

        // If no files specified, use the default connection
        if self.files.is_empty() {
            return maintain_database(&default_conn, "default");
        }

        // Otherwise, process each file
        let mut success = true;
        for file_path in &self.files {
            let file_str = file_path.to_string_lossy();
            println!("\nPerforming maintenance on: {file_str}");

            // Open the database connection for this file
            match Connection::open(file_path) {
                Ok(conn) => {
                    if let Err(err) = maintain_database(&conn, &file_str) {
                        println!("Error maintaining database '{file_str}': {err}");
                        success = false;
                    }
                }
                Err(err) => {
                    println!("Error opening database '{file_str}': {err}");
                    success = false;
                }
            }
        }

        if success {
            println!("\nAll database maintenance operations completed successfully.");
            Ok(())
        } else {
            Err("One or more database maintenance operations failed".into())
        }
    }
}

// Merge all (hopefully) pxh files ending in .db in the specified path
// into the current database, then write an output with our hostname.

impl SyncCommand {
    /// Create a temporary database file with optional --since filtering
    fn create_filtered_db_copy(
        &self,
        conn: &mut Connection,
    ) -> Result<NamedTempFile, Box<dyn std::error::Error>> {
        // Create a temporary file with the database
        let temp_file = NamedTempFile::new()?;

        // Use VACUUM INTO to create a complete copy
        conn.execute("VACUUM INTO ?", (temp_file.path().to_str(),))?;

        if let Some(days) = self.since {
            // Open the temp database and delete old records
            let temp_conn = Connection::open(temp_file.path())?;

            // Calculate timestamp threshold
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let threshold = now - (days as i64 * 86400);

            // Delete records older than the threshold
            temp_conn.execute(
                "DELETE FROM command_history WHERE start_unix_timestamp <= ?",
                [threshold],
            )?;

            // VACUUM to reclaim space
            temp_conn.execute("VACUUM", ())?;
            drop(temp_conn); // Close the connection to flush all changes
        }

        Ok(temp_file)
    }
    fn go(&self, mut conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Validate that --send-only and --receive-only are only used with remote sync
        if (self.send_only || self.receive_only) && self.remote.is_none() && !self.stdin_stdout {
            return Err(Box::from(
                "--send-only and --receive-only flags require --remote or --stdin-stdout to be specified",
            ));
        }

        // Validate that --remote and directory are not used together
        if self.remote.is_some() && self.dirname.is_some() {
            return Err(Box::from("Cannot specify both --remote and a directory path"));
        }

        // If in server mode, handle sync protocol
        if self.server {
            return self.handle_server_mode(&mut conn);
        }

        // Handle remote sync if specified (either SSH or stdin/stdout)
        if self.stdin_stdout || self.remote.is_some() {
            return self.handle_remote_sync(&mut conn);
        }

        // Original directory-based sync behavior requires dirname
        let dirname =
            self.dirname.as_ref().ok_or("Directory path is required for directory-based sync")?;

        if !dirname.exists() {
            fs::create_dir(dirname)?;
        }
        let mut output_path = dirname.clone();
        let original_hostname =
            pxh::get_setting(&conn, "original_hostname")?.unwrap_or_else(pxh::get_hostname);
        output_path.push(original_hostname.to_path_lossy());
        output_path.set_extension("db");
        // TODO: vacuum seems to want a plain text string path, unlike
        // ATTACH below which takes an os_str as bytes, so we can't
        // use BString to get a vec<u8>.  Look into why this is and if
        // there is a workaround.
        let output_path_str =
            output_path.to_str().ok_or("Unable to represent output filename as a string")?;

        if !self.export_only {
            let entries = fs::read_dir(dirname)?;
            let db_extension = OsStr::new("db");
            for entry in entries {
                let path = entry?.path();
                if path.extension() == Some(db_extension) && output_path != path {
                    print!("Syncing from {}...", path.to_string_lossy());
                    let (other_count, after_count) =
                        Self::merge_database_from_file(&mut conn, path)?;
                    println!("done, considered {other_count} rows and added {after_count}");
                }
            }
        }

        // Create database copy without filtering (--since only applies to remote sync)
        let temp_file = NamedTempFile::new_in(dirname.as_path())?;
        conn.execute("VACUUM INTO ?", (temp_file.path().to_str(),))?;
        temp_file.persist(output_path_str)?;
        if self.export_only {
            println!("Backed-up database to {output_path_str}");
        } else {
            println!("Saved merged database to {output_path_str}");
        }

        Ok(())
    }

    fn handle_remote_sync(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Determine sync mode
        let mode = if self.send_only {
            "send"
        } else if self.receive_only {
            "receive"
        } else {
            "bidirectional"
        };

        let mut child = if self.stdin_stdout {
            // For stdin/stdout mode (testing), we're already connected
            // Create a dummy child process that uses stdin/stdout
            None
        } else {
            // SSH mode
            let host = self.remote.as_ref().ok_or("Remote host required for SSH sync")?;
            println!("Syncing with {host}...");

            // Parse SSH command and arguments
            let (ssh_cmd, ssh_args) = pxh::helpers::parse_ssh_command(&self.ssh_cmd);

            let remote_db_path =
                self.remote_db.clone().unwrap_or_else(|| PathBuf::from("~/.pxh/pxh.db"));

            // Intelligently determine remote pxh path if not specified
            let remote_pxh = pxh::helpers::determine_remote_pxh_path(&self.remote_pxh);

            // Start SSH connection to remote with server mode
            let mut remote_command =
                format!("{} --db {} sync --server", remote_pxh, remote_db_path.display());
            if let Some(days) = self.since {
                remote_command.push_str(&format!(" --since {days}"));
            }

            let mut cmd = std::process::Command::new(&ssh_cmd);
            cmd.args(&ssh_args)
                .arg(host)
                .arg(&remote_command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::inherit()); // Map stderr to our stderr

            Some(cmd.spawn().map_err(|e| format!("Failed to spawn SSH command: {e}"))?)
        };

        // Handle stdin/stdout directly or through SSH child process
        let (mut stdin_writer, mut stdout_reader) = if self.stdin_stdout {
            // Use actual stdin/stdout
            (
                Box::new(std::io::stdout()) as Box<dyn Write>,
                Box::new(std::io::stdin()) as Box<dyn Read>,
            )
        } else {
            // Use SSH child process pipes
            if let Some(ref mut child) = child {
                let stdin = child.stdin.take().ok_or("Failed to get stdin from SSH process")?;
                let stdout = child.stdout.take().ok_or("Failed to get stdout from SSH process")?;
                (Box::new(stdin) as Box<dyn Write>, Box::new(stdout) as Box<dyn Read>)
            } else {
                return Err(Box::from("No child process available"));
            }
        };

        // Send mode to server
        stdin_writer.write_all(mode.as_bytes())?;
        stdin_writer.write_all(b"\n")?;
        stdin_writer.flush()?;

        // Execute the appropriate sync operations
        match mode {
            "send" => {
                self.send_database(&mut stdin_writer, conn)?;
                drop(stdin_writer);
            }
            "receive" => {
                // For receive-only, we need to close stdin
                drop(stdin_writer);
                self.receive_database(&mut stdout_reader, conn)?;
            }
            "bidirectional" => {
                self.send_database(&mut stdin_writer, conn)?;
                // Close stdin to signal we're done sending
                drop(stdin_writer);
                self.receive_database(&mut stdout_reader, conn)?;
            }
            _ => unreachable!(),
        }

        // Wait for child process if using SSH
        if let Some(mut child) = child {
            let status = child.wait()?;
            if !status.success() {
                return Err(Box::from("Remote sync failed"));
            }
        }

        if !self.stdin_stdout {
            println!("Sync completed successfully");
        }

        Ok(())
    }

    // Merge history from the database file at `path` into the current database.
    fn merge_database_from_file(
        conn: &mut Connection,
        path: PathBuf,
    ) -> Result<(u64, u64), Box<dyn std::error::Error>> {
        let tx = conn.transaction()?;
        let before_count: u64 =
            tx.prepare("SELECT COUNT(*) FROM main.command_history")?.query_row((), |r| r.get(0))?;
        tx.execute("ATTACH DATABASE ? AS other", (path.as_os_str().as_bytes(),))?;

        // Merge all records from the other database
        tx.execute(
            r#"
INSERT OR IGNORE INTO main.command_history (
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
SELECT session_id,
    full_command,
    shellname,
    hostname,
    username,
    working_directory,
    exit_status,
    start_unix_timestamp,
    end_unix_timestamp
FROM other.command_history
"#,
            (),
        )?;

        // Count records after the merge
        let after_count: u64 =
            tx.prepare("SELECT COUNT(*) FROM main.command_history")?.query_row((), |r| r.get(0))?;

        // Count how many records were in the other database
        let other_count: u64 = tx
            .prepare("SELECT COUNT(*) FROM other.command_history")?
            .query_row((), |r| r.get(0))?;

        tx.commit()?;
        conn.execute("DETACH DATABASE other", ())?;
        Ok((other_count, after_count - before_count))
    }

    fn handle_server_mode(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Read mode from stdin
        let mut mode = String::new();
        std::io::stdin().read_line(&mut mode)?;

        if mode.is_empty() {
            return Err(Box::from("No sync mode received"));
        }

        let mode = mode.trim();

        match mode {
            "send" => {
                // Server receives database from client
                self.receive_database(&mut std::io::stdin(), conn)?;
            }
            "receive" => {
                // Server sends database to client
                self.send_database(&mut std::io::stdout(), conn)?;
            }
            "bidirectional" => {
                // Server receives then sends
                self.receive_database(&mut std::io::stdin(), conn)?;
                self.send_database(&mut std::io::stdout(), conn)?;
            }
            _ => return Err(Box::from(format!("Unknown sync mode: {mode}"))),
        }

        Ok(())
    }

    fn send_database<W: Write>(
        &self,
        writer: &mut W,
        conn: &mut Connection,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create filtered database copy
        let temp_file = self.create_filtered_db_copy(conn)?;

        // Get file size
        let metadata = std::fs::metadata(temp_file.path())?;
        let size = metadata.len();

        // Send size and database
        writer.write_all(&size.to_le_bytes())?;

        let mut file = File::open(temp_file.path())?;
        io::copy(&mut file, writer)?;
        writer.flush()?;

        Ok(())
    }

    fn receive_database<R: Read>(
        &self,
        reader: &mut R,
        conn: &mut Connection,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Receive database size (8 bytes)
        let mut size_bytes = [0u8; 8];
        reader.read_exact(&mut size_bytes)?;
        let size = u64::from_le_bytes(size_bytes);

        // Receive database data
        let mut data = vec![0u8; size as usize];
        reader.read_exact(&mut data)?;

        // Create temporary file for the received database
        let temp_file = tempfile::NamedTempFile::new()?;
        std::fs::write(temp_file.path(), &data)?;

        // Use the existing merge function to merge directly into the main database
        let (other_count, added_count) =
            Self::merge_database_from_file(conn, temp_file.path().to_path_buf())?;

        // Get current hostname and database path
        let current_hostname = pxh::get_hostname();
        let current_db_path =
            conn.path().map(|p| p.to_string()).unwrap_or_else(|| "in-memory".to_string());

        eprintln!(
            "{current_hostname}: Merged into {current_db_path} considered {other_count} entries, added {added_count} entries"
        );
        Ok(())
    }
}

// Helper trait for any command that may want to render a list of
// commands during execution.
trait PrintableCommand {
    fn verbose(&self) -> bool;
    fn suppress_headers(&self) -> bool;
    fn display_limit(&self) -> usize;

    fn extra_filter_step(
        &self,
        rows: Vec<pxh::Invocation>,
    ) -> Result<Vec<pxh::Invocation>, Box<dyn std::error::Error>>;

    fn present_results(&self, conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Now that we have the relevant rows, just present the output
        let mut stmt = conn.prepare(
	r#"
SELECT session_id, full_command, shellname, working_directory, hostname, username, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM memdb.show_results sr, command_history h
 WHERE sr.ch_rowid = h.rowid
ORDER BY ch_start_unix_timestamp DESC, ch_id DESC
"#)?;

        let rows: Result<Vec<pxh::Invocation>, _> =
            stmt.query_map([], pxh::Invocation::from_row)?.collect();
        let rows = self.extra_filter_step(rows?)?;
        if self.verbose() {
            pxh::present_results_human_readable(
                &["start_time", "duration", "session", "context", "status", "command"],
                &rows,
                self.suppress_headers(),
            )?;
        } else {
            pxh::present_results_human_readable(
                &["start_time", "command"],
                &rows,
                self.suppress_headers(),
            )?;
        }
        Ok(())
    }
}

impl PrintableCommand for ScrubCommand {
    fn verbose(&self) -> bool {
        false
    }

    fn suppress_headers(&self) -> bool {
        false
    }

    fn display_limit(&self) -> usize {
        0
    }

    fn extra_filter_step(
        &self,
        rows: Vec<pxh::Invocation>,
    ) -> Result<Vec<pxh::Invocation>, Box<dyn std::error::Error>> {
        Ok(rows)
    }
}

impl ScrubCommand {
    fn go(
        &self,
        mut conn: Connection,
        histfile: &Option<PathBuf>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contraband = match &self.contraband {
            Some(value) => {
                println!(
                    "WARNING: specifying the contraband on the command line is inherently risky; prefer not specifying it\n"
                );
                value.clone()
            }
            None => {
                let mut input = String::new();
                print!("String to scrub: ");
                std::io::stdout().flush()?;
                io::stdin().read_line(&mut input)?;
                input.trim_end().into()
            }
        };

        if contraband.is_empty() {
            println!(); // from the input prompt in case the user hit ctrl-D
            return Err(String::from("String to scrub must be non-empty; aborting.").into());
        }

        conn.execute("DELETE FROM memdb.show_results", ())?;
        conn.execute(
            r#"
INSERT INTO memdb.show_results (ch_rowid, ch_start_unix_timestamp, ch_id)
SELECT rowid, start_unix_timestamp, id
  FROM command_history h
 WHERE INSTR(full_command, ?) > 0
ORDER BY start_unix_timestamp DESC, id DESC"#,
            (&contraband,),
        )?;
        println!("Entries to scrub from pxh database...\n");
        self.present_results(&conn)?;

        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM command_history WHERE rowid IN (SELECT ch_rowid FROM memdb.show_results)",
            (),
        )?;
        if self.dry_run {
            tx.rollback()?;
            println!("\nDry-run, no entries scrubbed.");
        } else {
            tx.commit()?;
            if let Some(histfile) = histfile {
                pxh::atomically_remove_lines_from_file(histfile, &contraband)?;
                println!("\nEntries scrubbed from database and {}.", &histfile.display());
            } else {
                println!("\nEntries scrubbed from database.");
            }
        }
        Ok(())
    }
}

// Show history in two steps:
//   1. Populate memdb.show_results with the relevant rows to show
//   2. Invoke present_results to actually show them.
//
// This two step dance is preparatory work for `--here` and `--around`
// queries where we need some arbitrary subset of selected entries.
// Rather than round-trip the rowid's and create a complex query, we
// just use a temp memory table.
impl PrintableCommand for ShowCommand {
    fn extra_filter_step(
        &self,
        rows: Vec<pxh::Invocation>,
    ) -> Result<Vec<pxh::Invocation>, Box<dyn std::error::Error>> {
        let regexes: Result<Vec<Regex>, _> =
            self.patterns.iter().skip(1).map(|s| Regex::new(s.as_str())).collect();
        let regexes = regexes?;
        Ok(rows
            .into_iter()
            .filter(|row| match_all_regexes(row, &regexes))
            .rev()
            .take(self.display_limit())
            .collect())
    }

    fn verbose(&self) -> bool {
        self.verbose
    }

    fn suppress_headers(&self) -> bool {
        self.suppress_headers
    }
    fn display_limit(&self) -> usize {
        self.limit
    }
}

impl ShowCommand {
    fn go(&self, conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
        // If we are loosening then just use the first string for the
        // sqlite query.  This requires fetching all matches, however,
        // to properly limit the final count.
        let pattern = if self.loosen {
            self.patterns.first().map_or_else(String::default, String::clone)
        } else {
            self.patterns.join(".*\\s.*")
        };

        // Add case-insensitive modifier and convert pattern to lowercase if needed
        let pattern =
            if self.ignore_case { format!("(?i){}", pattern.to_lowercase()) } else { pattern };

        conn.execute("DELETE FROM memdb.show_results", ())?;

        let working_directory = self.working_directory.as_ref().map_or_else(
            || {
                env::var_os("PWD")
                    .map(PathBuf::from)
                    .or_else(|| env::current_dir().ok())
                    .unwrap_or_default()
            },
            |v| v.clone(),
        );

        if let Some(ref maybe_session_hex) = self.session {
            let session_id = i64::from_str_radix(maybe_session_hex, 16)?;

            conn.execute(
                r#"
INSERT INTO memdb.show_results (ch_rowid, ch_start_unix_timestamp, ch_id)
SELECT rowid, start_unix_timestamp, id
  FROM command_history h
 WHERE full_command REGEXP ? AND session_id = ?
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
                (pattern, session_id, self.display_limit()),
            )?;
        } else if self.here {
            conn.execute(
                r#"
INSERT INTO memdb.show_results (ch_rowid, ch_start_unix_timestamp, ch_id)
SELECT rowid, start_unix_timestamp, id
  FROM command_history h
 WHERE working_directory = CAST(? as blob)
   AND full_command REGEXP ?
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
                (working_directory.to_string_lossy(), pattern, self.display_limit()),
            )?;
        } else {
            conn.execute(
                r#"
INSERT INTO memdb.show_results (ch_rowid, ch_start_unix_timestamp, ch_id)
SELECT rowid, start_unix_timestamp, id
  FROM command_history h
 WHERE full_command REGEXP ?
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
                (pattern, self.display_limit()),
            )?;
        }

        self.present_results(&conn)
    }
}

fn match_all_regexes(row: &pxh::Invocation, regexes: &[Regex]) -> bool {
    regexes.iter().all(|regex| regex.is_match(row.command.as_slice()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Check if binary was invoked as "pxhs", which is a shorthand for "pxh show"
    let args_vec = env::args().collect::<Vec<_>>();

    // Check if the executable name contains "pxhs" (handles both direct calls and symlinks)
    let is_pxhs = pxh::helpers::determine_is_pxhs(&args_vec);

    let mut args = if is_pxhs {
        // When invoked as pxhs, always transform to "pxh show ..."
        let program = args_vec[0].clone(); // Get program name
        let rest = args_vec.iter().skip(1).cloned(); // Get remaining args

        // Build new args with "show" inserted after program name
        let combined_args = std::iter::once(program)
            .chain(std::iter::once(String::from("show")))
            .chain(rest)
            .collect::<Vec<_>>();

        PxhArgs::parse_from(combined_args)
    } else {
        PxhArgs::parse()
    };

    let make_conn = || pxh::sqlite_connection(&args.db);
    match &mut args.command {
        Commands::ShellConfig(cmd) => {
            cmd.go()?;
        }
        Commands::Install(cmd) => {
            cmd.go()?;
        }
        Commands::Import(cmd) => {
            cmd.go(make_conn()?)?;
        }
        Commands::Export(cmd) => {
            cmd.go(make_conn()?)?;
        }
        Commands::Show(cmd) => {
            let actual_limit =
                if cmd.limit == 0 || cmd.loosen { i32::MAX as usize } else { cmd.limit };
            cmd.limit = actual_limit;
            cmd.go(make_conn()?)?;
        }
        Commands::Scrub(cmd) => {
            cmd.go(make_conn()?, &cmd.histfile)?;
        }
        Commands::Seal(cmd) => {
            cmd.go(make_conn()?)?;
        }
        Commands::Sync(cmd) => {
            cmd.go(make_conn()?)?;
        }
        Commands::Maintenance(cmd) => {
            cmd.go(make_conn()?)?;
        }
        Commands::Insert(cmd) => {
            let mut conn = make_conn()?;
            let invocation = pxh::Invocation {
                command: cmd.command.join(OsStr::new(" ")).as_bytes().into(),
                shellname: cmd.shellname.clone(),
                working_directory: cmd
                    .working_directory
                    .as_ref()
                    .map(|v| BString::from(v.as_path().as_os_str().as_bytes())),
                hostname: Some(BString::from(cmd.hostname.clone().into_vec())),
                username: Some(BString::from(cmd.username.clone().into_vec())),
                exit_status: cmd.exit_status,
                start_unix_timestamp: cmd.start_unix_timestamp,
                end_unix_timestamp: cmd.end_unix_timestamp,
                session_id: cmd.session_id,
            };
            let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
            invocation.insert(&tx)?;
            tx.commit()?;
        }
    }
    Ok(())
}
