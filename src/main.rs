#[macro_use]
extern crate prettytable;

use std::{
    ffi::OsString,
    io,
    io::Write,
    path::{Path, PathBuf},
    str,
    str::FromStr,
};

use chrono::prelude::{Local, TimeZone};
use clap::{Parser, Subcommand};
use prettytable::Table;
use sqlx::{
    sqlite::SqliteConnectOptions, ConnectOptions, Connection, SqliteConnection, Transaction,
};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

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
    Show {
        #[clap(long, default_value_t = 50)]
        limit: i32,
        #[clap(long, default_value = "human")]
        output_format: String,
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

async fn show_subcommand(
    conn: &mut SqliteConnection,
    output_format: &str,
    mut limit: i32,
    substring: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let substring = substring.unwrap_or_default();
    if output_format == "json" {
        limit = i32::MAX;
    }
    let mut rows = sqlx::query!(
        r#"
SELECT session_id, full_command, shellname, hostname, username, working_directory, exit_status, start_unix_timestamp, end_unix_timestamp, end_unix_timestamp - start_unix_timestamp as duration
  FROM command_history h
 WHERE INSTR(full_command, ?)
ORDER BY start_unix_timestamp DESC, id DESC
LIMIT ?"#,
        substring, limit
    )
    .fetch_all(conn)
	.await?;
    rows.reverse();
    if output_format == "json" {
        let invocations: Vec<pxh::Invocation> = rows
            .iter()
            .map(|row| pxh::Invocation {
                command: pxh::BinaryStringHelper::from(row.full_command.as_slice()),
                shellname: row.shellname.clone(),
                hostname: row
                    .hostname
                    .as_ref()
                    .map(|v| pxh::BinaryStringHelper::from(v.as_slice())),
                username: row
                    .username
                    .as_ref()
                    .map(|v| pxh::BinaryStringHelper::from(v.as_slice())),
                working_directory: row
                    .working_directory
                    .as_ref()
                    .map(|v| pxh::BinaryStringHelper::from(v.as_slice())),
                exit_status: row.exit_status,
                start_unix_timestamp: row.start_unix_timestamp,
                end_unix_timestamp: row.end_unix_timestamp,
                session_id: row.session_id,
            })
            .collect();
        serde_json::to_writer(io::stdout(), &invocations)?
    } else {
        let mut table = Table::new();
        table.set_format(*prettytable::format::consts::FORMAT_CLEAN);
        table.set_titles(row![
            "Time", "Duration", "Session", "Status", "cwd", "command"
        ]);
        for row in rows {
            let start: Option<i64> = row.start_unix_timestamp;
            let duration: Option<i64> = row.duration;
            let start_time_display = start.map_or_else(
                || "n/a".into(),
                |t| Local.timestamp(t, 0).format(TIME_FORMAT).to_string(),
            );
            let exit_status_display = row
                .exit_status
                .map_or_else(|| "n/a".into(), |s| s.to_string());
            let duration_display = duration.map_or_else(|| "n/a".into(), |t| format!("{}s", t));
            table.add_row(row![
                start_time_display,
                duration_display,
                format!("{:x}", row.session_id),
                exit_status_display,
                row.working_directory
                    .as_ref()
                    .map_or_else(String::new, |v| String::from_utf8_lossy(v).to_string()),
                String::from_utf8_lossy(&row.full_command)
            ]);
        }
        table.printstd();
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
        Commands::Show {
            output_format,
            limit,
            substring,
        } => {
            let mut conn = sqlite_connection(&args.db).await?;
            show_subcommand(&mut conn, output_format, *limit, substring.clone()).await?;
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
