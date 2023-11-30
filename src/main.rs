use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    fs::{File, OpenOptions},
    io,
    io::{BufRead, BufReader, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    str,
};

use bstr::{BString, ByteSlice};
use clap::{Parser, Subcommand};
use regex::bytes::Regex;
use rusqlite::{Connection, Result};

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
}

#[derive(Parser, Debug)]
struct InstallCommand {
    #[clap(help = "shell to install helpers into")]
    shellname: String,
}

#[derive(Parser, Debug)]
struct ShowCommand {
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
    dirname: PathBuf,
    #[clap(
        long,
        help = "Only export the current database; do not read other databases",
        default_value_t = false
    )]
    export_only: bool,
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
    working_directory: Option<PathBuf>,
    #[clap(long)]
    exit_status: Option<i64>,
    #[clap(long)]
    session_id: i64,
    #[clap(long)]
    start_unix_timestamp: Option<i64>,
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

impl ImportCommand {
    fn go(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
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
                )))
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
    fn go(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
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
    fn go(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        let mut stmt = conn.prepare(
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
ORDER BY id"#,
    )?;
        let rows: Result<Vec<pxh::InvocationDatabaseRow>, _> =
            stmt.query_map([], pxh::InvocationDatabaseRow::from_row)?.collect();
        let rows = rows?;
        pxh::json_export(&rows)?;
        Ok(())
    }
}

// Merge all (hopefully) pxh files ending in .db in the specified path
// into the current database, then write an output with our hostname.

impl SyncCommand {
    fn go(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        if !self.dirname.exists() {
            fs::create_dir(&self.dirname)?;
        }
        let mut output_path = self.dirname.clone();
        output_path.push(pxh::get_hostname().to_path_lossy());
        output_path.set_extension("db");
        // TODO: vacuum seems to want a plain text path, unlike ATTACH
        // above, so we can't use BString to get a vec<u8>.  Look into
        // why this is and if there is a workaround.
        let output_path_str =
            output_path.to_str().ok_or("Unable to represent output filename as a string")?;

        if !self.export_only {
            let entries = fs::read_dir(&self.dirname)?;
            let db_extension = OsStr::new("db");
            for entry in entries {
                let path = entry?.path();
                if path.extension() == Some(db_extension) && output_path != path {
                    print!("Syncing from {}...", path.to_string_lossy());
                    let (other_count, after_count) = Self::merge_into(conn, path)?;
                    println!("done, considered {other_count} rows and added {after_count}");
                }
            }
        }

        // VACUUM wants the output to not exist, so delete it if it does.
        // TODO: save to temp filename, rename over after vacuum succeeds.
        let _unused = fs::remove_file(&output_path);
        conn.execute("VACUUM INTO ?", (output_path_str,))?;
        if self.export_only {
            println!("Backed-up database to {output_path_str}");
        } else {
            println!("Saved merged database to {output_path_str}");
        }

        Ok(())
    }

    // Merge history from the file specified in `path` into the current
    // history database.
    fn merge_into(
        conn: &mut Connection,
        path: PathBuf,
    ) -> Result<(u64, u64), Box<dyn std::error::Error>> {
        let tx = conn.transaction()?;
        let before_count: u64 =
            tx.prepare("SELECT COUNT(*) FROM main.command_history")?.query_row((), |r| r.get(0))?;
        tx.execute("ATTACH DATABASE ? AS other", (path.as_os_str().as_bytes(),))?;
        let other_count: u64 = tx
            .prepare("SELECT COUNT(*) FROM other.command_history")?
            .query_row((), |r| r.get(0))?;
        tx.execute(
            r#"
INSERT INTO main.command_history (
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
        let after_count: u64 =
            tx.prepare("SELECT COUNT(*) FROM main.command_history")?.query_row((), |r| r.get(0))?;
        tx.commit()?;
        conn.execute("DETACH DATABASE other", ())?;
        Ok((other_count, after_count - before_count))
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
        rows: Vec<pxh::InvocationDatabaseRow>,
    ) -> Result<Vec<pxh::InvocationDatabaseRow>, Box<dyn std::error::Error>>;

    fn present_results(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        // Now that we have the relevant rows, just present the output
        let mut stmt = conn.prepare(
	r#"
SELECT session_id, full_command, shellname, working_directory, hostname, username, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM memdb.show_results sr, command_history h
 WHERE sr.ch_rowid = h.rowid
ORDER BY ch_start_unix_timestamp DESC, ch_id DESC
"#)?;

        let rows: Result<Vec<pxh::InvocationDatabaseRow>, _> =
            stmt.query_map([], pxh::InvocationDatabaseRow::from_row)?.collect();
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
        rows: Vec<pxh::InvocationDatabaseRow>,
    ) -> Result<Vec<pxh::InvocationDatabaseRow>, Box<dyn std::error::Error>> {
        Ok(rows)
    }
}

impl ScrubCommand {
    fn go(
        &self,
        conn: &mut Connection,
        histfile: &Option<PathBuf>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contraband = match &self.contraband {
            Some(value) => {
                println!("WARNING: specifying the contraband on the command line is inherently risky; prefer not specifying it\n");
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
        self.present_results(conn)?;

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
        rows: Vec<pxh::InvocationDatabaseRow>,
    ) -> Result<Vec<pxh::InvocationDatabaseRow>, Box<dyn std::error::Error>> {
        let regexes: Result<Vec<Regex>, _> =
            self.patterns.iter().skip(1).map(|s| Regex::new(s.as_str())).collect();
        let regexes = regexes?;
        Ok(rows
            .into_iter()
            .filter(|row| match_all_regexes(row, &regexes))
            .rev()
            .take(self.limit)
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
    fn go(&self, conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
        // If we are loosening then just use the first string for the
        // sqlite query.  This requires fetching all matches, however,
        // to properly limit the final count.
        let pattern = if self.loosen {
            self.patterns.first().map_or_else(String::default, String::clone)
        } else {
            self.patterns.join(".*\\s.*")
        };

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
                (pattern, session_id, self.limit),
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
                (working_directory.to_string_lossy(), pattern, self.limit),
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
                (pattern, self.limit),
            )?;
        }

        self.present_results(conn)
    }
}

fn match_all_regexes(row: &pxh::InvocationDatabaseRow, regexes: &[Regex]) -> bool {
    regexes.iter().all(|regex| regex.is_match(&row.command))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let mut args = PxhArgs::parse();

    // TODO: refactor the sqlite_connection out below cleanly somehow
    match &mut args.command {
        Commands::ShellConfig(cmd) => {
            cmd.go()?;
        }
        Commands::Install(cmd) => {
            cmd.go()?;
        }
        Commands::Import(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            cmd.go(&mut conn)?;
        }
        Commands::Export(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            cmd.go(&mut conn)?;
        }
        Commands::Show(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            let actual_limit =
                if cmd.limit == 0 || cmd.loosen { i32::MAX as usize } else { cmd.limit };
            cmd.limit = actual_limit;
            cmd.go(&mut conn)?;
        }
        Commands::Scrub(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            cmd.go(&mut conn, &cmd.histfile)?;
        }
        Commands::Seal(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            cmd.go(&mut conn)?;
        }
        Commands::Sync(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            cmd.go(&mut conn)?;
        }
        Commands::Insert(cmd) => {
            let mut conn = pxh::sqlite_connection(&args.db)?;
            let tx = conn.transaction()?;
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
            invocation.insert(&tx)?;
            tx.commit()?;
        }
    }
    Ok(())
}
