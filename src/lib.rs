use std::{
    collections::HashMap,
    env,
    fmt::Write as FmtWrite,
    fs::File,
    io,
    io::{BufReader, BufWriter, Read, Write as IoWrite},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    str,
    sync::Arc,
    time::Duration,
};

use bstr::{io::BufReadExt, BString, ByteSlice};
use chrono::prelude::{Local, TimeZone};
use itertools::Itertools;
use regex::bytes::Regex;
use rusqlite::{functions::FunctionFlags, Connection, Error, Result, Row, Transaction};
use serde::{Deserialize, Serialize};

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub fn get_setting(
    conn: &Connection,
    key: &str,
) -> Result<Option<BString>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?")?;
    let mut rows = stmt.query([key])?;

    if let Some(row) = rows.next()? {
        let value: Vec<u8> = row.get(0)?;
        Ok(Some(BString::from(value)))
    } else {
        Ok(None)
    }
}

pub fn set_setting(
    conn: &Connection,
    key: &str,
    value: &BString,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
        (key, value.as_bytes()),
    )?;
    Ok(())
}

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub fn get_hostname() -> BString {
    env::var_os("PXH_HOSTNAME")
        .unwrap_or_else(|| hostname::get().unwrap_or_default())
        .as_bytes()
        .into()
}

pub fn sqlite_connection(path: &Option<PathBuf>) -> Result<Connection, Box<dyn std::error::Error>> {
    let path = path.as_ref().ok_or("Database not defined; use --db or PXH_DB_PATH")?;
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(500))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", "16777216")?;

    let schema = include_str!("base_schema.sql");
    conn.execute_batch(schema)?;

    // From rusqlite::functions example but adapted for non-utf8
    // regexps.
    conn.create_scalar_function("regexp", 2, FunctionFlags::SQLITE_DETERMINISTIC, move |ctx| {
        assert_eq!(ctx.len(), 2, "called with unexpected number of arguments");
        let regexp: Arc<Regex> = ctx
            .get_or_create_aux(0, |vr| -> Result<_, BoxError> { Ok(Regex::new(vr.as_str()?)?) })?;
        let is_match = {
            let text = ctx.get_raw(1).as_bytes().map_err(|e| Error::UserFunctionError(e.into()))?;

            regexp.is_match(text)
        };

        Ok(is_match)
    })?;

    // Check and set the original_hostname if it's not already set
    if let Ok(None) = get_setting(&conn, "original_hostname") {
        let hostname = get_hostname();
        set_setting(&conn, "original_hostname", &hostname)?;
    }

    Ok(conn)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Invocation {
    pub command: BString,
    pub shellname: String,
    pub working_directory: Option<BString>,
    pub hostname: Option<BString>,
    pub username: Option<BString>,
    pub exit_status: Option<i64>,
    pub start_unix_timestamp: Option<i64>,
    pub end_unix_timestamp: Option<i64>,
    pub session_id: i64,
}

impl Invocation {
    fn sameish(&self, other: &Self) -> bool {
        self.command == other.command && self.start_unix_timestamp == other.start_unix_timestamp
    }

    pub fn insert(&self, tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
        tx.execute(
            r#"
INSERT OR IGNORE INTO command_history (
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
                self.session_id,
                self.command.as_slice(),
                self.shellname.clone(),
                self.hostname.as_ref().map(|v| v.to_vec()),
                self.username.as_ref().map(|v| v.to_vec()),
                self.working_directory.as_ref().map(|v| v.to_vec()),
                self.exit_status,
                self.start_unix_timestamp,
                self.end_unix_timestamp,
            ),
        )?;

        Ok(())
    }
}

// Try to generate a "stable" session id based on the file imported.
// If that fails, just create a random one.
fn generate_import_session_id(histfile: &Path) -> i64 {
    if let Ok(st) = std::fs::metadata(histfile) {
        ((st.ino() << 16) | st.dev()) as i64
    } else {
        (rand::random::<u64>() >> 1) as i64
    }
}

pub fn import_zsh_history(
    histfile: &Path,
    hostname: Option<BString>,
    username: Option<BString>,
) -> Result<Vec<Invocation>, Box<dyn std::error::Error>> {
    let mut f = File::open(histfile)?;
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf)?;
    let username = username
        .or_else(|| users::get_current_username().map(|v| BString::from(v.into_vec())))
        .unwrap_or_else(|| BString::from("unknown"));
    let hostname = hostname.unwrap_or_else(get_hostname);
    let buf_iter = buf.split(|&ch| ch == b'\n');

    let mut ret = vec![];
    let session_id = generate_import_session_id(histfile);
    for line in buf_iter {
        let Some((fields, command)) = line.splitn(2, |&ch| ch == b';').collect_tuple() else {
            continue;
        };
        let Some((_skip, start_time, duration_seconds)) =
            fields.splitn(3, |&ch| ch == b':').collect_tuple()
        else {
            continue;
        };
        let start_unix_timestamp = str::from_utf8(&start_time[1..])?.parse::<i64>()?; // 1.. is to skip the leading space!
        let invocation = Invocation {
            command: BString::from(command),
            shellname: "zsh".into(),
            hostname: Some(BString::from(hostname.as_bytes())),
            username: Some(BString::from(username.as_bytes())),
            start_unix_timestamp: Some(start_unix_timestamp),
            end_unix_timestamp: Some(
                start_unix_timestamp + str::from_utf8(duration_seconds)?.parse::<i64>()?,
            ),
            session_id,
            ..Default::default()
        };

        ret.push(invocation);
    }

    Ok(dedup_invocations(ret))
}

pub fn import_bash_history(
    histfile: &Path,
    hostname: Option<BString>,
    username: Option<BString>,
) -> Result<Vec<Invocation>, Box<dyn std::error::Error>> {
    let mut f = File::open(histfile)?;
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf)?;
    let username = username
        .or_else(|| users::get_current_username().map(|v| BString::from(v.as_bytes())))
        .unwrap_or_else(|| BString::from("unknown"));
    let hostname = hostname.unwrap_or_else(get_hostname);
    let buf_iter = buf.split(|&ch| ch == b'\n').filter(|l| !l.is_empty());

    let mut ret = vec![];
    let session_id = generate_import_session_id(histfile);
    let mut last_ts = None;
    for line in buf_iter {
        if line[0] == b'#' {
            if let Ok(ts) = str::parse::<i64>(str::from_utf8(&line[1..]).unwrap_or("0")) {
                if ts > 0 {
                    last_ts = Some(ts);
                }
                continue;
            }
        }
        let invocation = Invocation {
            command: BString::from(line),
            shellname: "bash".into(),
            hostname: Some(BString::from(hostname.as_bytes())),
            username: Some(BString::from(username.as_bytes())),
            start_unix_timestamp: last_ts,
            session_id,
            ..Default::default()
        };

        ret.push(invocation);
    }

    Ok(dedup_invocations(ret))
}

pub fn import_json_history(histfile: &Path) -> Result<Vec<Invocation>, Box<dyn std::error::Error>> {
    let f = File::open(histfile)?;
    let reader = BufReader::new(f);
    Ok(serde_json::from_reader(reader)?)
}

fn dedup_invocations(invocations: Vec<Invocation>) -> Vec<Invocation> {
    let mut it = invocations.into_iter();
    let Some(first) = it.next() else { return vec![] };
    let mut ret = vec![first];
    for elem in it {
        if !elem.sameish(ret.last().unwrap()) {
            ret.push(elem);
        }
    }
    ret
}

impl Invocation {
    pub fn from_row(row: &Row) -> Result<Self, Error> {
        Ok(Invocation {
            session_id: row.get("session_id")?,
            command: BString::from(row.get::<_, Vec<u8>>("full_command")?),
            shellname: row.get("shellname")?,
            working_directory: row
                .get::<_, Option<Vec<u8>>>("working_directory")?
                .map(BString::from),
            hostname: row.get::<_, Option<Vec<u8>>>("hostname")?.map(BString::from),
            username: row.get::<_, Option<Vec<u8>>>("username")?.map(BString::from),
            exit_status: row.get("exit_status")?,
            start_unix_timestamp: row.get("start_unix_timestamp")?,
            end_unix_timestamp: row.get("end_unix_timestamp")?,
        })
    }
}

// Create a pretty export string that gets serialized as an array of
// bytes only if it isn't valid UTF-8; this makes the json export
// prettier.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum PrettyExportString {
    Readable(String),
    Encoded(Vec<u8>),
}

impl From<&[u8]> for PrettyExportString {
    fn from(bytes: &[u8]) -> Self {
        match str::from_utf8(bytes) {
            Ok(v) => Self::Readable(v.to_string()),
            _ => Self::Encoded(bytes.to_vec()),
        }
    }
}

impl From<Option<&Vec<u8>>> for PrettyExportString {
    fn from(bytes: Option<&Vec<u8>>) -> Self {
        match bytes {
            Some(v) => match str::from_utf8(v.as_slice()) {
                Ok(s) => Self::Readable(s.to_string()),
                _ => Self::Encoded(v.to_vec()),
            },
            None => Self::Readable(String::new()),
        }
    }
}

impl Invocation {
    fn to_json_export(&self) -> serde_json::Value {
        serde_json::json!({
            "session_id": self.session_id,
            "command": PrettyExportString::from(self.command.as_slice()),
            "shellname": self.shellname,
            "working_directory": self.working_directory.as_ref().map_or(
                PrettyExportString::Readable(String::new()),
                |b| PrettyExportString::from(b.as_slice())
            ),
            "hostname": self.hostname.as_ref().map_or(
                PrettyExportString::Readable(String::new()),
                |b| PrettyExportString::from(b.as_slice())
            ),
            "username": self.username.as_ref().map_or(
                PrettyExportString::Readable(String::new()),
                |b| PrettyExportString::from(b.as_slice())
            ),
            "exit_status": self.exit_status,
            "start_unix_timestamp": self.start_unix_timestamp,
            "end_unix_timestamp": self.end_unix_timestamp,
        })
    }
}

pub fn json_export(rows: &[Invocation]) -> Result<(), Box<dyn std::error::Error>> {
    let json_values: Vec<serde_json::Value> = rows.iter().map(|r| r.to_json_export()).collect();
    serde_json::to_writer(io::stdout(), &json_values)?;
    Ok(())
}

// column list: command, start, host, shell, cwd, end, duratio, session, ...

struct QueryResultColumnDisplayer {
    header: &'static str,
    style: &'static str,
    displayer: Box<dyn Fn(&Invocation) -> String>,
}

fn time_display_helper(t: Option<i64>) -> String {
    // Chained if-let may make this unpacking of
    // Option/Result/LocalResult cleaner.  Alternative is a closer
    // using `?` chains but that's slightly uglier.
    t.and_then(|t| Local.timestamp_opt(t, 0).single())
        .map(|t| t.format(TIME_FORMAT).to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn binary_display_helper(v: &BString) -> String {
    String::from_utf8_lossy(v.as_slice()).to_string()
}

fn displayers() -> HashMap<&'static str, QueryResultColumnDisplayer> {
    let mut ret = HashMap::new();
    ret.insert(
        "command",
        QueryResultColumnDisplayer {
            header: "Command",
            style: "Fw",
            displayer: Box::new(|row| binary_display_helper(&row.command)),
        },
    );
    ret.insert(
        "start_time",
        QueryResultColumnDisplayer {
            header: "Start",
            style: "Fg",
            displayer: Box::new(|row| time_display_helper(row.start_unix_timestamp)),
        },
    );
    ret.insert(
        "end_time",
        QueryResultColumnDisplayer {
            header: "End",
            style: "Fg",
            displayer: Box::new(|row| time_display_helper(row.end_unix_timestamp)),
        },
    );
    ret.insert(
        "duration",
        QueryResultColumnDisplayer {
            header: "Duration",
            style: "Fm",
            displayer: Box::new(|row| match (row.start_unix_timestamp, row.end_unix_timestamp) {
                (Some(start), Some(end)) => format!("{}s", end - start),
                _ => "n/a".into(),
            }),
        },
    );
    // TODO: Move the style into the displayer (which would return a
    // Cell) to allow for color based on per-column values, like red
    // for non-zero exit statuses.
    ret.insert(
        "status",
        QueryResultColumnDisplayer {
            header: "Status",
            style: "Fr",
            displayer: Box::new(|row| {
                row.exit_status.map_or_else(|| "n/a".into(), |s| s.to_string())
            }),
        },
    );
    // TODO: Make session similar to "context" and just print `.` when
    // it is the current session.
    ret.insert(
        "session",
        QueryResultColumnDisplayer {
            header: "Session",
            style: "Fc",
            displayer: Box::new(|row| format!("{:x}", row.session_id)),
        },
    );
    // Print context specially; the full output is $HOST:$PATH but if
    // $HOST is the current host, the $HOST: is omitted.  If $PATH is
    // the current working directory, it is replaced with `.`.
    ret.insert(
        "context",
        QueryResultColumnDisplayer {
            header: "Context",
            style: "bFb",
            displayer: Box::new(|row| {
                let current_hostname = get_hostname();
                let row_hostname = row.hostname.clone().unwrap_or_default();
                let mut ret = String::new();
                if current_hostname != row_hostname {
                    write!(ret, "{row_hostname}:").unwrap_or_default();
                }
                let current_directory = env::current_dir().unwrap_or_default();
                ret.push_str(&row.working_directory.as_ref().map_or_else(String::new, |v| {
                    let v = String::from_utf8_lossy(v.as_slice()).to_string();
                    if v == current_directory.to_string_lossy() {
                        String::from(".")
                    } else {
                        v
                    }
                }));

                ret
            }),
        },
    );

    ret
}

pub fn present_results_human_readable(
    fields: &[&str],
    rows: &[Invocation],
    suppress_headers: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let displayers = displayers();
    let mut table = prettytable::Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

    if !suppress_headers {
        let mut title_row = prettytable::Row::empty();
        for field in fields {
            let Some(d) = displayers.get(field) else {
                return Err(Box::from(format!("Invalid 'show' field: {field}")));
            };

            title_row.add_cell(prettytable::Cell::new(d.header).style_spec("bFg"));
        }
        table.set_titles(title_row);
    }

    for row in rows.iter() {
        let mut display_row = prettytable::Row::empty();
        for field in fields {
            display_row.add_cell(
                prettytable::Cell::new((displayers[field].displayer)(row).as_str())
                    .style_spec(displayers[field].style),
            );
        }
        table.add_row(display_row);
    }
    table.printstd();
    Ok(())
}

// Rewrite a file with lines matching `contraband` removed.  utf-8
// safe for the file (TODO: I guess make contraband a `BString` too)
pub fn atomically_remove_lines_from_file(
    input_filepath: &PathBuf,
    contraband: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let input_file = File::open(input_filepath)?;
    let mut input_reader = BufReader::new(input_file);

    let output_filepath = input_filepath.with_extension(".new"); // good enough for zsh, good enough for us
    let output_file = File::create(&output_filepath)?;
    let mut output_writer = BufWriter::new(output_file);

    input_reader.for_byte_line_with_terminator(|line| {
        if !line.contains_str(contraband) {
            output_writer.write_all(line)?;
        }
        Ok(true)
    })?;

    output_writer.flush()?;
    std::fs::rename(output_filepath, input_filepath)?;
    Ok(())
}
