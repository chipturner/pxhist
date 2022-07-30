use std::{
    ffi::OsString,
    io,
    io::Write,
    path::{Path, PathBuf},
    str,
    str::FromStr,
};

use clap::{Parser, Subcommand};
use sqlx::{
    sqlite::SqliteConnectOptions, ConnectOptions, Connection, SqliteConnection, Transaction,
};

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
    #[clap(visible_alias="s")]
    Show {
        #[clap(long, default_value_t = 50)]
        limit: i32,
        #[clap(short, long)]
        verbose: bool,
        substring: Option<String>,
    },
}

async fn sqlite_connection(
    path: &Option<PathBuf>,
) -> Result<SqliteConnection, Box<dyn std::error::Error>> {
    let path = path
        .as_ref()
        .ok_or("Database not defined; use --db or PXH_DB_PATH")?;
    let database_url = format!("sqlite://{}", path.to_string_lossy());
    let mut conn = SqliteConnectOptions::from_str(&database_url)?
        .create_if_missing(true)
        .pragma("journal_mode", "WAL")
        .pragma("temp_store", "MEMORY")
        .pragma("cache_size", "16777216")
        .connect()
        .await?;
    sqlx::migrate!("src/migrations").run(&mut conn).await?;

    Ok(conn)
}

async fn insert_invocations(
    conn: &mut SqliteConnection,
    invocations: Vec<pxh::Invocation>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tx = conn.begin().await?;
    for invocation in invocations.into_iter() {
        insert_subcommand(&mut tx, &invocation).await?;
    }
    tx.commit().await?;
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

async fn insert_subcommand(
    tx: &mut Transaction<'_, sqlx::Sqlite>,
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

    let _ = sqlx::query!(
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
        invocation.session_id,
        command_bytes,
        invocation.shellname,
        hostname_bytes,
        username_bytes,
        working_directory_bytes,
        invocation.exit_status,
        invocation.start_unix_timestamp,
        invocation.end_unix_timestamp,
    )
    .execute(tx)
    .await?;

    Ok(())
}

fn shell_config_subcommand(shellname: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Todo: bash and other shell formats
    let contents = match shellname {
        "zsh" => include_str!("shell_configs/pxh.zsh"),
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

async fn seal_subcommand(
    conn: &mut SqliteConnection,
    session_id: i64,
    exit_status: i32,
    end_unix_timestamp: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = sqlx::query!(
        r#"
UPDATE command_history SET exit_status = ?, end_unix_timestamp = ?
 WHERE exit_status is NULL
   AND end_unix_timestamp IS NULL
   AND id = (SELECT MAX(id) FROM command_history hi WHERE hi.session_id = ?)"#,
        exit_status,
        end_unix_timestamp,
        session_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn export_subcommand(conn: &mut SqliteConnection) -> Result<(), Box<dyn std::error::Error>> {
    let rows = sqlx::query_as!(
	pxh::InvocationExport,
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
ORDER BY id"#,
    )
    .fetch_all(conn)
	.await?;
    pxh::json_export(&rows)?;
    Ok(())
}

async fn show_subcommand(
    conn: &mut SqliteConnection,
    verbose: bool,
    mut limit: i32,
    substring: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let substring = substring.unwrap_or_default();
    if limit <= 0 {
        limit = i32::MAX;
    }
    let mut rows = sqlx::query_as!(
	pxh::InvocationExport,
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
  FROM command_history h
 WHERE INSTR(full_command, ?)
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
        substring, limit
    )
    .fetch_all(conn)
	.await?;
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

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
            let mut conn = sqlite_connection(&args.db).await?;
            let mut tx = conn.begin().await?;
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
            insert_subcommand(&mut tx, &invocation).await?;
            tx.commit().await?;
        }
        Commands::Import {
            histfile,
            shellname,
            hostname,
            username,
        } => {
            let invocations = import_subcommand(histfile, shellname, hostname, username)?;
            let mut conn = sqlite_connection(&args.db).await?;
            insert_invocations(&mut conn, invocations).await?;
        }
        Commands::Export {} => {
            let mut conn = sqlite_connection(&args.db).await?;
            export_subcommand(&mut conn).await?;
        }
        Commands::Show {
            limit,
            substring,
            verbose,
        } => {
            let mut conn = sqlite_connection(&args.db).await?;
            show_subcommand(&mut conn, *verbose, *limit, substring.clone()).await?;
        }
        Commands::Seal {
            session_id,
            exit_status,
            end_unix_timestamp,
        } => {
            let mut conn = sqlite_connection(&args.db).await?;
            seal_subcommand(&mut conn, *session_id, *exit_status, *end_unix_timestamp).await?;
        }
        Commands::ShellConfig { shellname } => {
            shell_config_subcommand(shellname)?;
        }
    }
    Ok(())
}
