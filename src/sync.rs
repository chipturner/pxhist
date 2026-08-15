//! Database merge engine for sync.
//!
//! This is the shared core of every sync mode (directory, SSH, stdin/stdout):
//! merge one pxh database file into the live connection with deduplication,
//! optional secret filtering, an optional incremental watermark, and
//! unsealed-row upgrades. Callers own transport and presentation; this module
//! owns the merge semantics and reports what happened via [`MergeStats`].

use std::path::Path;
use std::time::Duration;

use regex::bytes::RegexSet;
use rusqlite::Connection;

/// What a merge did, for callers to present or assert on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeStats {
    /// Rows we actually scanned in the source (above the watermark, if any).
    pub considered: i64,
    /// Rows newly inserted into main.
    pub added: i64,
    /// Rows skipped due to secret-pattern filtering.
    pub filtered: i64,
    /// `MAX(id)` from the source -- the next watermark for it.
    pub new_max_id: Option<i64>,
}

/// Generous budget: sync is background work, so wait out long writers
/// rather than failing the merge.
const WRITE_RETRY_BUDGET: Duration = Duration::from_secs(30);
const CHUNK_SIZE: i64 = 5000;

/// Read the incremental-sync watermark recorded for a source machine, i.e.
/// the highest source `id` a previous merge fully processed.
pub fn sync_watermark(conn: &Connection, machine_id: u64) -> Option<i64> {
    crate::get_setting(conn, &watermark_key(machine_id))
        .ok()
        .flatten()
        .and_then(|bs| std::str::from_utf8(bs.as_slice()).ok()?.parse::<i64>().ok())
}

/// Persist the watermark for a source machine. Call only after a fully
/// successful merge: an interrupted merge must re-consider its rows.
pub fn set_sync_watermark(
    conn: &Connection,
    machine_id: u64,
    new_max: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let bs = bstr::BString::from(new_max.to_string());
    crate::set_setting(conn, &watermark_key(machine_id), &bs)
}

fn watermark_key(machine_id: u64) -> String {
    format!("sync_watermark_{machine_id}")
}

/// Merge history from a database file into `conn`, with an optional secret
/// filter (rows whose command matches are skipped and counted) and an
/// optional incremental-sync watermark (skip rows in the source whose
/// `id <= watermark`). The unsealed-row update always scans the whole source
/// since seal info may sit at any id, watermark or not.
///
/// The source file is schema-migrated in place before ATTACHing, so older
/// databases (e.g. pre-machine_id) don't fail on missing columns.
pub fn merge_database_from_file(
    conn: &mut Connection,
    path: &Path,
    secret_filter: Option<&RegexSet>,
    watermark: Option<i64>,
) -> Result<MergeStats, Box<dyn std::error::Error>> {
    {
        let other = Connection::open(path)?;
        crate::initialize_base_schema(&other)?;
        crate::run_schema_migrations(&other)?;
    }

    use std::os::unix::ffi::OsStrExt;
    conn.execute("ATTACH DATABASE ? AS other", (path.as_os_str().as_bytes(),))?;
    let result = merge_attached(conn, secret_filter, watermark);
    conn.execute("DETACH DATABASE other", ())?;
    result
}

/// Merge `other.command_history` (already ATTACHed) into main.
///
/// Structured to minimize write-lock hold time so concurrent shell hooks
/// (insert/seal, which only retry for ~1s) don't hit "database is locked"
/// during a sync: all reads and regex filtering happen outside any write
/// transaction (WAL readers never block writers), and inserts run in
/// id-ordered chunks, each a short BEGIN IMMEDIATE transaction via
/// `with_write_retry`. The merge is therefore not atomic, but INSERT OR
/// IGNORE is idempotent and the caller only advances the watermark after
/// full success, so an interrupted merge simply re-considers rows next sync.
fn merge_attached(
    conn: &mut Connection,
    secret_filter: Option<&RegexSet>,
    watermark: Option<i64>,
) -> Result<MergeStats, Box<dyn std::error::Error>> {
    // -1 sentinel matches all rows (id is AUTOINCREMENT, so always >= 1).
    let lo = watermark.unwrap_or(-1);

    // Read-only pre-pass: source stats, taken before the merge so
    // `considered` reflects what we scan below.
    let considered: i64 = conn
        .prepare("SELECT COUNT(*) FROM other.command_history WHERE id > ?")?
        .query_row([lo], |r| r.get(0))?;

    // Highest id in source -- caller persists as the next watermark.
    let new_max_id: Option<i64> =
        conn.prepare("SELECT MAX(id) FROM other.command_history")?.query_row((), |r| r.get(0)).ok();

    let mut added: usize = 0;
    let mut filtered_count: i64 = 0;
    let mut cursor = lo;
    loop {
        // Upper id bound of the next chunk (NULL once the source is drained).
        let hi: Option<i64> = conn
            .prepare(
                "SELECT MAX(id) FROM (SELECT id FROM other.command_history
                  WHERE id > ? ORDER BY id LIMIT ?)",
            )?
            .query_row((cursor, CHUNK_SIZE), |r| r.get(0))?;
        let Some(hi) = hi else { break };

        if let Some(regex_set) = secret_filter {
            // Read and regex-filter the chunk before taking the write
            // lock; pattern matching is the expensive part of the merge.
            type SourceRow = (
                i64,
                Vec<u8>,
                String,
                Option<Vec<u8>>,
                Option<Vec<u8>>,
                Option<Vec<u8>>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
            );
            let rows: Vec<SourceRow> = conn
                .prepare(
                    r#"
SELECT session_id, full_command, shellname, hostname, username,
       working_directory, exit_status, start_unix_timestamp, end_unix_timestamp, machine_id
FROM other.command_history
WHERE id > ? AND id <= ?
"#,
                )?
                .query_map((cursor, hi), |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;

            let total = rows.len();
            let keep: Vec<SourceRow> =
                rows.into_iter().filter(|row| !regex_set.is_match(&row.1)).collect();
            filtered_count += (total - keep.len()) as i64;

            added += crate::with_write_retry(conn, WRITE_RETRY_BUDGET, |tx| {
                let mut inserted = 0;
                for row in &keep {
                    inserted += tx.execute(
                        r#"
INSERT OR IGNORE INTO main.command_history (
    session_id, full_command, shellname, hostname, username,
    working_directory, exit_status, start_unix_timestamp, end_unix_timestamp, machine_id
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
                        rusqlite::params![
                            row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9
                        ],
                    )?;
                }
                Ok(inserted)
            })?;
        } else {
            // No filtering, bulk-copy the chunk in SQL.
            added += crate::with_write_retry(conn, WRITE_RETRY_BUDGET, |tx| {
                tx.execute(
                    r#"
INSERT OR IGNORE INTO main.command_history (
    session_id, full_command, shellname, hostname, username,
    working_directory, exit_status, start_unix_timestamp, end_unix_timestamp, machine_id
)
SELECT session_id, full_command, shellname, hostname, username,
    working_directory, exit_status, start_unix_timestamp, end_unix_timestamp, machine_id
FROM other.command_history
WHERE id > ? AND id <= ?
"#,
                    (cursor, hi),
                )
            })?;
        }
        cursor = hi;
    }

    // Upgrade unsealed rows: if a command was synced while still running
    // (exit_status/end_unix_timestamp NULL), fill in the sealed values from
    // the other database where available. Scans all of `other` regardless
    // of watermark -- a seal can land at any id. Find candidates with a
    // read-only join, then apply targeted updates in one short transaction.
    let seal_updates: Vec<(i64, i64, Option<i64>)> = conn
        .prepare(
            r#"
SELECT m.id, o.exit_status, o.end_unix_timestamp
  FROM main.command_history m
  JOIN other.command_history o
    ON m.full_command = o.full_command
   AND m.start_unix_timestamp IS o.start_unix_timestamp
   AND m.shellname = o.shellname
   AND COALESCE(m.hostname, '') = COALESCE(o.hostname, '')
 WHERE m.exit_status IS NULL
   AND o.exit_status IS NOT NULL
"#,
        )?
        .query_map((), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;

    if !seal_updates.is_empty() {
        crate::with_write_retry(conn, WRITE_RETRY_BUDGET, |tx| {
            for (id, exit_status, end_ts) in &seal_updates {
                // Re-check exit_status IS NULL: a local seal may have
                // landed since the read above.
                tx.execute(
                    "UPDATE command_history SET exit_status = ?, end_unix_timestamp = ?
                      WHERE id = ? AND exit_status IS NULL",
                    (exit_status, end_ts, id),
                )?;
            }
            Ok(())
        })?;
    }

    Ok(MergeStats { considered, added: added as i64, filtered: filtered_count, new_max_id })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn schema_db(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        crate::initialize_base_schema(&conn).unwrap();
        crate::run_schema_migrations(&conn).unwrap();
        conn
    }

    fn insert_row(conn: &Connection, cmd: &str, ts: i64, exit_status: Option<i32>) {
        conn.execute(
            "INSERT INTO command_history (session_id, full_command, shellname, hostname,
                                          start_unix_timestamp, exit_status)
             VALUES (1, CAST(? AS blob), 'zsh', CAST('host1' AS blob), ?, ?)",
            rusqlite::params![cmd, ts, exit_status],
        )
        .unwrap();
    }

    fn count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM command_history", (), |r| r.get(0)).unwrap()
    }

    /// A target DB plus a source DB file, both schema-initialized.
    fn merge_fixture() -> (tempfile::TempDir, Connection, PathBuf, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let target_path = dir.path().join("target.db");
        let source_path = dir.path().join("source.db");
        let target = schema_db(&target_path);
        let source = schema_db(&source_path);
        (dir, target, source_path, source)
    }

    #[test]
    fn test_merge_reports_stats_and_is_idempotent() {
        let (_dir, mut target, source_path, source) = merge_fixture();
        for i in 0..3 {
            insert_row(&source, &format!("cmd-{i}"), 1000 + i, Some(0));
        }
        drop(source);

        let stats = merge_database_from_file(&mut target, &source_path, None, None).unwrap();
        assert_eq!(stats, MergeStats { considered: 3, added: 3, filtered: 0, new_max_id: Some(3) });
        assert_eq!(count(&target), 3);

        // INSERT OR IGNORE makes a re-merge a no-op.
        let stats = merge_database_from_file(&mut target, &source_path, None, None).unwrap();
        assert_eq!(stats, MergeStats { considered: 3, added: 0, filtered: 0, new_max_id: Some(3) });
        assert_eq!(count(&target), 3);
    }

    #[test]
    fn test_merge_crosses_chunk_boundaries() {
        // More source rows than one chunk: the id-ordered chunk loop must
        // walk every chunk, not just the first.
        let (_dir, mut target, source_path, source) = merge_fixture();
        let total = CHUNK_SIZE + 2;
        {
            let tx = source.unchecked_transaction().unwrap();
            for i in 0..total {
                tx.execute(
                    "INSERT INTO command_history (session_id, full_command, shellname,
                                                  start_unix_timestamp)
                     VALUES (1, CAST(? AS blob), 'zsh', ?)",
                    rusqlite::params![format!("cmd-{i}"), 1000 + i],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        drop(source);

        let stats = merge_database_from_file(&mut target, &source_path, None, None).unwrap();
        assert_eq!(stats.considered, total);
        assert_eq!(stats.added, total);
        assert_eq!(stats.new_max_id, Some(total));
        assert_eq!(count(&target), total);
    }

    #[test]
    fn test_merge_watermark_skips_already_merged_ids() {
        let (_dir, mut target, source_path, source) = merge_fixture();
        for i in 0..5 {
            insert_row(&source, &format!("cmd-{i}"), 1000 + i, Some(0));
        }
        drop(source);

        let stats = merge_database_from_file(&mut target, &source_path, None, Some(2)).unwrap();
        assert_eq!(stats, MergeStats { considered: 3, added: 3, filtered: 0, new_max_id: Some(5) });
        // Only rows with id > 2 (cmd-2 .. cmd-4) came over.
        assert_eq!(count(&target), 3);
        let has_cmd0: i64 = target
            .query_row(
                "SELECT COUNT(*) FROM command_history WHERE full_command = CAST('cmd-0' AS blob)",
                (),
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_cmd0, 0, "row below the watermark must be skipped");
    }

    #[test]
    fn test_merge_secret_filter_drops_and_counts() {
        let (_dir, mut target, source_path, source) = merge_fixture();
        insert_row(&source, "export API_KEY=hunter2", 1000, Some(0));
        insert_row(&source, "ls -la", 1001, Some(0));
        drop(source);

        let filter = RegexSet::new(["API_KEY="]).unwrap();
        let stats =
            merge_database_from_file(&mut target, &source_path, Some(&filter), None).unwrap();
        assert_eq!(stats, MergeStats { considered: 2, added: 1, filtered: 1, new_max_id: Some(2) });
        assert_eq!(count(&target), 1);
    }

    #[test]
    fn test_merge_upgrades_unsealed_rows() {
        // A row synced while still running (no exit status) gets its sealed
        // values filled in from a source that has them -- even when the row
        // itself is below the watermark.
        let (_dir, mut target, source_path, source) = merge_fixture();
        insert_row(&target, "long-running", 1000, None);
        insert_row(&source, "long-running", 1000, Some(7));
        drop(source);

        let stats = merge_database_from_file(&mut target, &source_path, None, Some(1000)).unwrap();
        assert_eq!(stats.added, 0, "watermark excludes the row from insertion");

        let exit_status: Option<i32> = target
            .query_row(
                "SELECT exit_status FROM command_history WHERE full_command = CAST('long-running' AS blob)",
                (),
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exit_status, Some(7), "seal info must be copied onto the unsealed row");
    }

    #[test]
    fn test_merge_reports_max_id_regression_via_stats() {
        // A source restored from backup can have max(id) below our watermark;
        // the caller detects that from new_max_id and resets the watermark.
        let (_dir, mut target, source_path, source) = merge_fixture();
        insert_row(&source, "cmd", 1000, Some(0));
        drop(source);

        let stats = merge_database_from_file(&mut target, &source_path, None, Some(100)).unwrap();
        assert_eq!(stats, MergeStats { considered: 0, added: 0, filtered: 0, new_max_id: Some(1) });
    }

    #[test]
    fn test_watermark_roundtrip_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let conn = schema_db(&dir.path().join("db.db"));
        assert_eq!(sync_watermark(&conn, 42), None);
        set_sync_watermark(&conn, 42, 1234).unwrap();
        assert_eq!(sync_watermark(&conn, 42), Some(1234));
        set_sync_watermark(&conn, 42, 5678).unwrap();
        assert_eq!(sync_watermark(&conn, 42), Some(5678));
        assert_eq!(sync_watermark(&conn, 43), None, "keys are per machine_id");
    }
}
