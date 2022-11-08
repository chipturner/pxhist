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
};

use chrono::prelude::{Local, TimeZone};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn get_hostname() -> OsString {
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
            BinaryStringHelper::Encoded(b) => b.clone(),
            BinaryStringHelper::Readable(s) => s.as_bytes().to_vec(),
        }
    }

    pub fn to_string_lossy(&self) -> String {
        match self {
            BinaryStringHelper::Encoded(b) => String::from_utf8_lossy(b).to_string(),
            BinaryStringHelper::Readable(s) => s.clone(),
        }
    }

    pub fn to_os_str(&self) -> OsString {
        match self {
            BinaryStringHelper::Encoded(b) => OsString::from_vec(b.to_vec()),
            BinaryStringHelper::Readable(s) => OsString::from(s),
        }
    }
}

impl From<&[u8]> for BinaryStringHelper {
    fn from(bytes: &[u8]) -> Self {
        match str::from_utf8(bytes) {
            Ok(v) => BinaryStringHelper::Readable(v.to_string()),
            _ => BinaryStringHelper::Encoded(bytes.to_vec()),
        }
    }
}

impl From<&OsString> for BinaryStringHelper {
    fn from(osstr: &OsString) -> Self {
        BinaryStringHelper::from(osstr.as_bytes())
    }
}

impl From<&OsStr> for BinaryStringHelper {
    fn from(osstr: &OsStr) -> Self {
        BinaryStringHelper::from(osstr.as_bytes())
    }
}

impl From<&PathBuf> for BinaryStringHelper {
    fn from(pb: &PathBuf) -> Self {
        BinaryStringHelper::from(pb.as_path().as_os_str())
    }
}

impl Default for BinaryStringHelper {
    fn default() -> Self {
        BinaryStringHelper::Readable("".to_string())
    }
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
                    hostname: Some(BinaryStringHelper::from(hostname.as_bytes())),
                    username: Some(BinaryStringHelper::from(username.as_bytes())),
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
            hostname: Some(BinaryStringHelper::from(hostname.as_bytes())),
            username: Some(BinaryStringHelper::from(username.as_bytes())),
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

pub fn command_as_bytes(v: &[OsString]) -> Vec<u8> {
    let mut ret = Vec::with_capacity(v.len() * v.iter().map(|elem| elem.len()).sum::<usize>() + 1);
    v.iter().for_each(|elem| {
        ret.extend(elem.as_bytes());
        ret.push(b' ');
    });
    ret.remove(ret.len() - 1); // trim trailing space added in last iteration
    ret
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
            command: BinaryStringHelper::from(row.full_command.as_slice()),
            shellname: row.shellname.clone(),
            hostname: row
                .hostname
                .as_ref()
                .map(|v| BinaryStringHelper::from(v.as_slice())),
            username: row
                .username
                .as_ref()
                .map(|v| BinaryStringHelper::from(v.as_slice())),
            working_directory: row
                .working_directory
                .as_ref()
                .map(|v| BinaryStringHelper::from(v.as_slice())),
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
    t.map_or_else(
        || "n/a".into(),
        |t| Local.timestamp(t, 0).format(TIME_FORMAT).to_string(),
    )
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
            displayer: Box::new(
                |row| match (row.start_unix_timestamp, row.end_unix_timestamp) {
                    (Some(start), Some(end)) => format!("{}s", end - start),
                    _ => "n/a".into(),
                },
            ),
        },
    );
    ret.insert(
        "status",
        QueryResultColumnDisplayer {
            header: "Status",
            displayer: Box::new(|row| {
                row.exit_status
                    .map_or_else(|| "n/a".into(), |s| s.to_string())
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
                let row_hostname = match &row.hostname {
                    None => return String::new(),
                    Some(v) => BinaryStringHelper::from(v.as_slice()),
                };
                let mut ret = String::new();
                if current_hostname != row_hostname.to_os_str() {
                    write!(ret, "{}:", row_hostname.to_string_lossy()).unwrap_or_default();
                }
                let current_directory = env::current_dir().unwrap_or_default();
                ret.push_str(
                    &row.working_directory
                        .as_ref()
                        .map_or_else(String::new, |v| {
                            let v = String::from_utf8_lossy(v).to_string();
                            if v == current_directory.to_string_lossy() {
                                String::from(".")
                            } else {
                                v
                            }
                        }),
                );

                ret
            }),
        },
    );

    ret
}

pub fn show_subcommand_human_readable(
    fields: &[&str],
    rows: &[InvocationExport],
) -> Result<(), Box<dyn std::error::Error>> {
    let displayers = displayers();
    let mut table = prettytable::Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

    let mut title_row = prettytable::Row::empty();
    for field in fields {
        let title = match displayers.get(field) {
            Some(d) => d.header,
            None => return Err(Box::from(format!("Invalid 'show' field: {}", field))),
        };

        title_row.add_cell(prettytable::Cell::new(title));
    }
    table.set_titles(title_row);

    for row in rows.iter() {
        let mut display_row = prettytable::Row::empty();
        for field in fields {
            display_row.add_cell(prettytable::Cell::new(
                (displayers[field].displayer)(row).as_str(),
            ));
        }
        table.add_row(display_row);
    }
    table.printstd();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_as_bytes() {
        assert_eq!(command_as_bytes(&[OsString::from("xyz")]), b"xyz");
        assert_eq!(
            command_as_bytes(&[OsString::from("xyz"), OsString::from("pqr")]),
            b"xyz pqr"
        );
    }
}
