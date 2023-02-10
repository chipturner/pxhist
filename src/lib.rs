use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fmt::Write,
    fs::File,
    io,
    io::{BufReader, Read},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    str,
    sync::Arc,
    time::Duration,
};

use chrono::prelude::{Local, TimeZone};
use itertools::Itertools;
use regex::bytes::Regex;
use rusqlite::{functions::FunctionFlags, Connection, Error, Result, Transaction};
use serde::{Deserialize, Serialize};

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub fn get_hostname() -> OsString {
    hostname::get().unwrap_or_else(|_| OsString::new())
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum BinaryStringHelper {
    Readable(String),
    Encoded(Vec<u8>),
}

impl BinaryStringHelper {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Encoded(b) => b.clone(),
            Self::Readable(s) => s.as_bytes().to_vec(),
        }
    }

    pub fn to_string_lossy(&self) -> String {
        match self {
            Self::Encoded(b) => String::from_utf8_lossy(b).to_string(),
            Self::Readable(s) => s.clone(),
        }
    }

    pub fn to_os_str(&self) -> OsString {
        match self {
            Self::Encoded(b) => OsString::from_vec(b.to_vec()),
            Self::Readable(s) => OsString::from(s),
        }
    }
}

impl From<&[u8]> for BinaryStringHelper {
    fn from(bytes: &[u8]) -> Self {
        match str::from_utf8(bytes) {
            Ok(v) => Self::Readable(v.to_string()),
            _ => Self::Encoded(bytes.to_vec()),
        }
    }
}

impl From<&Vec<u8>> for BinaryStringHelper {
    fn from(v: &Vec<u8>) -> Self {
        Self::from(v.as_slice())
    }
}

impl From<&OsString> for BinaryStringHelper {
    fn from(osstr: &OsString) -> Self {
        Self::from(osstr.as_bytes())
    }
}

impl From<&OsStr> for BinaryStringHelper {
    fn from(osstr: &OsStr) -> Self {
        Self::from(osstr.as_bytes())
    }
}

impl From<&PathBuf> for BinaryStringHelper {
    fn from(pb: &PathBuf) -> Self {
        Self::from(pb.as_path().as_os_str())
    }
}

impl<T: From<T>> From<Option<T>> for BinaryStringHelper
where
    BinaryStringHelper: From<T>,
{
    fn from(t: Option<T>) -> Self {
        t.map_or_else(Self::default, Self::from)
    }
}

impl Default for BinaryStringHelper {
    fn default() -> Self {
        Self::Readable("".to_string())
    }
}

pub fn sqlite_connection(path: &Option<PathBuf>) -> Result<Connection, Box<dyn std::error::Error>> {
    let path = path.as_ref().ok_or("Database not defined; use --db or PXH_DB_PATH")?;
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(100))?;
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

    Ok(conn)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Invocation {
    pub command: BinaryStringHelper,
    pub shellname: String,
    pub working_directory: Option<BinaryStringHelper>,
    pub hostname: Option<BinaryStringHelper>,
    pub username: Option<BinaryStringHelper>,
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
        let command_bytes: Vec<u8> = self.command.to_bytes();
        let username_bytes = self.username.as_ref().map_or_else(Vec::new, |v| v.to_bytes());
        let hostname_bytes = self.hostname.as_ref().map_or_else(Vec::new, |v| v.to_bytes());
        let working_directory_bytes =
            self.working_directory.as_ref().map_or_else(Vec::new, |v| v.to_bytes());

        tx.execute(
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
                self.session_id,
                command_bytes.as_slice(),
                &self.shellname,
                hostname_bytes,
                username_bytes,
                working_directory_bytes,
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
        (st.ino() << 16 | st.dev()) as i64
    } else {
        (rand::random::<u64>() >> 1) as i64
    }
}

pub fn import_zsh_history(
    histfile: &Path,
    hostname: Option<&OsString>,
    username: Option<&OsString>,
) -> Result<Vec<Invocation>, Box<dyn std::error::Error>> {
    let mut f = File::open(histfile)?;
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf)?;
    let username = username
        .cloned()
        .or_else(users::get_current_username)
        .unwrap_or_else(|| OsString::from("unknown"));
    let hostname = hostname.cloned().unwrap_or_else(get_hostname);
    let buf_iter = buf.split(|&ch| ch == b'\n');

    let mut ret = vec![];
    let session_id = generate_import_session_id(histfile);
    for line in buf_iter {
        if let Some((fields, command)) = line.splitn(2, |&ch| ch == b';').collect_tuple() {
            if let Some((_skip, start_time, duration_seconds)) =
                fields.splitn(3, |&ch| ch == b':').collect_tuple()
            {
                let start_unix_timestamp = str::from_utf8(&start_time[1..])?.parse::<i64>()?; // 1.. is to skip the leading space!
                let invocation = Invocation {
                    command: BinaryStringHelper::from(command),
                    shellname: "zsh".into(),
                    hostname: Some(BinaryStringHelper::from(&hostname)),
                    username: Some(BinaryStringHelper::from(&username)),
                    start_unix_timestamp: Some(start_unix_timestamp),
                    end_unix_timestamp: Some(
                        start_unix_timestamp + str::from_utf8(duration_seconds)?.parse::<i64>()?,
                    ),
                    session_id,
                    ..Default::default()
                };

                ret.push(invocation);
            }
        }
    }

    Ok(dedup_invocations(ret))
}

pub fn import_bash_history(
    histfile: &Path,
    hostname: Option<&OsString>,
    username: Option<&OsString>,
) -> Result<Vec<Invocation>, Box<dyn std::error::Error>> {
    let mut f = File::open(histfile)?;
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf)?;
    let username = username
        .cloned()
        .or_else(users::get_current_username)
        .unwrap_or_else(|| OsString::from("unknown"));
    let hostname = hostname.cloned().unwrap_or_else(get_hostname);
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
            command: BinaryStringHelper::from(line),
            shellname: "bash".into(),
            hostname: Some(BinaryStringHelper::from(&hostname)),
            username: Some(BinaryStringHelper::from(&username)),
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
    match it.next() {
        Some(first) => {
            let mut ret = vec![first];
            for elem in it {
                if !elem.sameish(ret.last().unwrap()) {
                    ret.push(elem);
                }
            }
            ret
        }
        _ => vec![],
    }
}

pub struct InvocationExport {
    pub session_id: i64,
    pub full_command: Vec<u8>,
    pub shellname: String,
    pub working_directory: Option<Vec<u8>>,
    pub hostname: Option<Vec<u8>>,
    pub username: Option<Vec<u8>>,
    pub exit_status: Option<i64>,
    pub start_unix_timestamp: Option<i64>,
    pub end_unix_timestamp: Option<i64>,
}

pub fn json_export(rows: &[InvocationExport]) -> Result<(), Box<dyn std::error::Error>> {
    let invocations: Vec<Invocation> = rows
        .iter()
        .map(|row| Invocation {
            command: BinaryStringHelper::from(&row.full_command),
            shellname: row.shellname.clone(),
            hostname: row.hostname.as_ref().map(BinaryStringHelper::from),
            username: row.username.as_ref().map(BinaryStringHelper::from),
            working_directory: row.working_directory.as_ref().map(BinaryStringHelper::from),
            exit_status: row.exit_status,
            start_unix_timestamp: row.start_unix_timestamp,
            end_unix_timestamp: row.end_unix_timestamp,
            session_id: row.session_id,
        })
        .collect();
    serde_json::to_writer(io::stdout(), &invocations)?;
    Ok(())
}

// column list: command, start, host, shell, cwd, end, duratio, session, ...

struct QueryResultColumnDisplayer {
    header: &'static str,
    displayer: Box<dyn Fn(&InvocationExport) -> String>,
}

fn time_display_helper(t: Option<i64>) -> String {
    // Chained if-let may make this unpacking of
    // Option/Result/LocalResult cleaner.  Alternative is a closer
    // using `?` chains but that's slightly uglier.
    t.and_then(|t| Local.timestamp_opt(t, 0).single())
        .map(|t| t.format(TIME_FORMAT).to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn binary_display_helper(v: &[u8]) -> String {
    String::from_utf8_lossy(v).to_string()
}

fn displayers() -> HashMap<&'static str, QueryResultColumnDisplayer> {
    let mut ret = HashMap::new();
    ret.insert(
        "command",
        QueryResultColumnDisplayer {
            header: "Command",
            displayer: Box::new(|row| binary_display_helper(&row.full_command)),
        },
    );
    ret.insert(
        "start_time",
        QueryResultColumnDisplayer {
            header: "Start",
            displayer: Box::new(|row| time_display_helper(row.start_unix_timestamp)),
        },
    );
    ret.insert(
        "end_time",
        QueryResultColumnDisplayer {
            header: "End",
            displayer: Box::new(|row| time_display_helper(row.end_unix_timestamp)),
        },
    );
    ret.insert(
        "duration",
        QueryResultColumnDisplayer {
            header: "Duration",
            displayer: Box::new(|row| match (row.start_unix_timestamp, row.end_unix_timestamp) {
                (Some(start), Some(end)) => format!("{}s", end - start),
                _ => "n/a".into(),
            }),
        },
    );
    ret.insert(
        "status",
        QueryResultColumnDisplayer {
            header: "Status",
            displayer: Box::new(|row| {
                row.exit_status.map_or_else(|| "n/a".into(), |s| s.to_string())
            }),
        },
    );
    ret.insert(
        "session",
        QueryResultColumnDisplayer {
            header: "Session",
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
            displayer: Box::new(|row| {
                let current_hostname = get_hostname();
                let row_hostname = BinaryStringHelper::from(row.hostname.as_ref());
                let mut ret = String::new();
                if current_hostname != row_hostname.to_os_str() {
                    write!(ret, "{}:", row_hostname.to_string_lossy()).unwrap_or_default();
                }
                let current_directory = env::current_dir().unwrap_or_default();
                ret.push_str(&row.working_directory.as_ref().map_or_else(String::new, |v| {
                    let v = String::from_utf8_lossy(v).to_string();
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
    rows: &[InvocationExport],
    suppress_headers: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let displayers = displayers();
    let mut table = prettytable::Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

    if !suppress_headers {
        let mut title_row = prettytable::Row::empty();
        for field in fields {
            let title = match displayers.get(field) {
                Some(d) => d.header,
                None => return Err(Box::from(format!("Invalid 'show' field: {field}"))),
            };

            title_row.add_cell(prettytable::Cell::new(title));
        }
        table.set_titles(title_row);
    }

    for row in rows.iter() {
        let mut display_row = prettytable::Row::empty();
        for field in fields {
            display_row
                .add_cell(prettytable::Cell::new((displayers[field].displayer)(row).as_str()));
        }
        table.add_row(display_row);
    }
    table.printstd();
    Ok(())
}
