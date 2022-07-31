use std::{
    ffi::OsString,
    io,
    io::Write,
    path::{Path, PathBuf},
    str,
    sync::Arc,
};

use clap::{Parser, Subcommand};
use regex::bytes::Regex;
use rusqlite::{functions::FunctionFlags, params, Connection, Error, Result, Transaction};
type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct PxhArgs {
    #[clap(long, parse(from_os_str), env = "PXH_DB_PATH")]
    db: Option<PathBuf>,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Insert {
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
    },
    Import {
        #[clap(long)]
        histfile: PathBuf,
        #[clap(long)]
        shellname: String,
        #[clap(long)]
        hostname: Option<OsString>,
        #[clap(long)]
        username: Option<OsString>,
    },
    Export {},
    Seal {
        #[clap(long)]
        session_id: i64,
        #[clap(long)]
        exit_status: i32,
        #[clap(long)]
        end_unix_timestamp: i64,
    },
    ShellConfig {
        shellname: String,
    },
    #[clap(visible_alias = "s")]
    Show {
        #[clap(long, default_value_t = 50)]
        limit: i32,
        #[clap(short, long)]
        verbose: bool,
        substring: Option<String>,
    },
}

fn sqlite_connection(path: &Option<PathBuf>) -> Result<Connection, Box<dyn std::error::Error>> {
    let path = path
        .as_ref()
        .ok_or("Database not defined; use --db or PXH_DB_PATH")?;
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", "16777216")?;

    let schema = include_str!("base_schema.sql");
    conn.execute_batch(schema)?;

    // From rusqlite::functions example but adapted for non-utf8
    // regexps.
    conn.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            assert_eq!(ctx.len(), 2, "called with unexpected number of arguments");
            let regexp: Arc<Regex> = ctx.get_or_create_aux(0, |vr| -> Result<_, BoxError> {
                Ok(Regex::new(vr.as_str()?)?)
            })?;
            let is_match = {
                let text = ctx
                    .get_raw(1)
                    .as_bytes()
                    .map_err(|e| Error::UserFunctionError(e.into()))?;

                regexp.is_match(text)
            };

            Ok(is_match)
        },
    )?;

    Ok(conn)
}

fn insert_invocations(
    conn: &mut Connection,
    invocations: Vec<pxh::Invocation>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    for invocation in invocations.into_iter() {
        insert_subcommand(&tx, &invocation)?;
    }
    tx.commit()?;
    Ok(())
}

fn import_subcommand(
    histfile: &Path,
    shellname: &str,
    hostname: &Option<OsString>,
    username: &Option<OsString>,
) -> Result<Vec<pxh::Invocation>, Box<dyn std::error::Error>> {
    match shellname {
        "zsh" => pxh::import_zsh_history(histfile, hostname.as_ref(), username.as_ref()),
        "bash" => pxh::import_bash_history(histfile, hostname.as_ref(), username.as_ref()),
        "json" => pxh::import_json_history(histfile),
        _ => Err(Box::from(format!(
            "Unsupported shell: {} (PRs welcome!)",
            shellname
        ))),
    }
}

fn insert_subcommand(
    tx: &Transaction,
    invocation: &pxh::Invocation,
) -> Result<(), Box<dyn std::error::Error>> {
    let command_bytes: Vec<u8> = invocation.command.to_bytes();
    let username_bytes = invocation
        .username
        .as_ref()
        .map_or_else(Vec::new, |v| v.to_bytes());
    let hostname_bytes = invocation
        .hostname
        .as_ref()
        .map_or_else(Vec::new, |v| v.to_bytes());
    let working_directory_bytes = invocation
        .working_directory
        .as_ref()
        .map_or_else(Vec::new, |v| v.to_bytes());

    let _ = tx.execute(
        r#"
INSERT INTO command_history (
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
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        (
            invocation.session_id,
            command_bytes.as_slice(),
            &invocation.shellname,
            hostname_bytes,
            username_bytes,
            working_directory_bytes,
            invocation.exit_status,
            invocation.start_unix_timestamp,
            invocation.end_unix_timestamp,
        ),
    );

    Ok(())
}

fn shell_config_subcommand(shellname: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Todo: bash and other shell formats
    let contents = match shellname {
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
                shellname
            )))
        }
    };
    io::stdout().write_all(contents.as_bytes())?;
    Ok(())
}

fn seal_subcommand(
    conn: &mut Connection,
    session_id: i64,
    exit_status: i32,
    end_unix_timestamp: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = conn.execute(
        r#"
UPDATE command_history SET exit_status = ?, end_unix_timestamp = ?
 WHERE exit_status is NULL
   AND end_unix_timestamp IS NULL
   AND id = (SELECT MAX(id) FROM command_history hi WHERE hi.session_id = ?)"#,
        (exit_status, end_unix_timestamp, session_id),
    )?;
    Ok(())
}

fn export_subcommand(conn: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
ORDER BY id"#,
    )?;
    let rows: Result<Vec<pxh::InvocationExport>, _> = stmt
        .query_map([], |row| {
            Ok(pxh::InvocationExport {
                session_id: row.get(0)?,
                full_command: row.get(1)?,
                shellname: row.get(2)?,
                working_directory: row.get(3)?,
                hostname: row.get(4)?,
                username: row.get(5)?,
                exit_status: row.get(6)?,
                start_unix_timestamp: row.get(7)?,
                end_unix_timestamp: row.get(8)?,
            })
        })?
        .collect();
    let rows = rows?;
    pxh::json_export(&rows)?;
    Ok(())
}

fn show_subcommand(
    conn: &mut Connection,
    verbose: bool,
    mut limit: i32,
    substring: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let substring = substring.unwrap_or_default();
    if limit <= 0 {
        limit = i32::MAX;
    }
    let mut stmt = conn.prepare(
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
 WHERE full_command REGEXP ?
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
    )?;

    let rows: Result<Vec<pxh::InvocationExport>, _> = stmt
        .query_map(params![substring, limit], |row| {
            Ok(pxh::InvocationExport {
                session_id: row.get(0)?,
                full_command: row.get(1)?,
                shellname: row.get(2)?,
                working_directory: row.get(3)?,
                hostname: row.get(4)?,
                username: row.get(5)?,
                exit_status: row.get(6)?,
                start_unix_timestamp: row.get(7)?,
                end_unix_timestamp: row.get(8)?,
            })
        })?
        .collect();
    let mut rows = rows?;
    rows.reverse();
    if verbose {
        pxh::show_subcommand_human_readable(
            &["start_time", "duration", "session", "context", "command"],
            &rows,
        )?;
    } else {
        pxh::show_subcommand_human_readable(&["start_time", "command"], &rows)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = PxhArgs::parse();

    match &args.command {
        Commands::Insert {
            shellname,
            hostname,
            username,
            working_directory,
            command,
            exit_status,
            start_unix_timestamp,
            end_unix_timestamp,
            session_id,
        } => {
            let mut conn = sqlite_connection(&args.db)?;
            let tx = conn.transaction()?;
            let invocation = pxh::Invocation {
                command: pxh::BinaryStringHelper::from(pxh::command_as_bytes(command).as_slice()),
                shellname: shellname.into(),
                working_directory: working_directory
                    .as_ref()
                    .map(pxh::BinaryStringHelper::from),
                hostname: Some(pxh::BinaryStringHelper::from(hostname)),
                username: Some(pxh::BinaryStringHelper::from(username)),
                exit_status: *exit_status,
                start_unix_timestamp: *start_unix_timestamp,
                end_unix_timestamp: *end_unix_timestamp,
                session_id: *session_id,
            };
            insert_subcommand(&tx, &invocation)?;
            tx.commit()?;
        }
        Commands::Import {
            histfile,
            shellname,
            hostname,
            username,
        } => {
            let invocations = import_subcommand(histfile, shellname, hostname, username)?;
            let mut conn = sqlite_connection(&args.db)?;
            insert_invocations(&mut conn, invocations)?;
        }
        Commands::Export {} => {
            let mut conn = sqlite_connection(&args.db)?;
            export_subcommand(&mut conn)?;
        }
        Commands::Show {
            limit,
            substring,
            verbose,
        } => {
            let mut conn = sqlite_connection(&args.db)?;
            show_subcommand(&mut conn, *verbose, *limit, substring.clone())?;
        }
        Commands::Seal {
            session_id,
            exit_status,
            end_unix_timestamp,
        } => {
            let mut conn = sqlite_connection(&args.db)?;
            seal_subcommand(&mut conn, *session_id, *exit_status, *end_unix_timestamp)?;
        }
        Commands::ShellConfig { shellname } => {
            shell_config_subcommand(shellname)?;
        }
    }
    Ok(())
}
