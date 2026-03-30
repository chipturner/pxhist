use std::path::PathBuf;

use bstr::BString;
use nucleo::{Config, Matcher, Utf32Str, pattern::Pattern};
use rusqlite::Connection;

use super::command::{FilterMode, HostFilter};

/// A history entry with its metadata
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub command: String,
    pub timestamp: Option<i64>,
    pub working_directory: Option<BString>,
    pub hostname: Option<BString>,
    pub exit_status: Option<i32>,
    pub duration_secs: Option<i64>,
}

/// Search engine that combines SQLite queries with nucleo fuzzy matching
pub struct SearchEngine {
    conn: Connection,
    working_directory: PathBuf,
    host_set: Vec<BString>,
    matcher: Matcher,
    result_limit: usize,
}

impl SearchEngine {
    pub fn new(
        conn: Connection,
        working_directory: PathBuf,
        host_set: Vec<BString>,
        result_limit: usize,
    ) -> Self {
        SearchEngine {
            conn,
            working_directory,
            host_set,
            matcher: Matcher::new(Config::DEFAULT),
            result_limit,
        }
    }

    /// Get the primary (current live) hostname -- used for display
    pub fn primary_hostname(&self) -> &BString {
        &self.host_set[0]
    }

    /// Check if a hostname is in this host's set (current + aliases)
    pub fn is_this_host(&self, hostname: &BString) -> bool {
        self.host_set.contains(hostname)
    }

    /// Load history entries from the database, optionally filtered by a search query
    pub fn load_entries(
        &self,
        filter_mode: FilterMode,
        host_filter: HostFilter,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let entries = match filter_mode {
            FilterMode::Directory => self.load_entries_for_directory(host_filter, query)?,
            FilterMode::Global => self.load_all_entries(host_filter, query)?,
        };
        Ok(entries)
    }

    fn load_all_entries(
        &self,
        host_filter: HostFilter,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let mut where_conditions = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if host_filter == HostFilter::ThisHost {
            let placeholders: String =
                self.host_set.iter().map(|_| "CAST(? as blob)").collect::<Vec<_>>().join(", ");
            where_conditions.push(format!("hostname IN ({placeholders})"));
            for h in &self.host_set {
                params.push(h.to_string());
            }
        }

        if let Some(q) = query
            && !q.is_empty()
        {
            where_conditions
                .push("CAST(full_command AS text) LIKE '%' || ? || '%' COLLATE NOCASE".to_string());
            params.push(q.to_string());
        }

        let where_clause = if where_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_conditions.join(" AND "))
        };

        let entries = self.run_recall_query(&where_clause, &params)?;
        Ok(entries)
    }

    fn row_to_entry(&self, row: &rusqlite::Row) -> rusqlite::Result<HistoryEntry> {
        let id: i64 = row.get(0)?;
        let command: Vec<u8> = row.get(1)?;
        let timestamp: Option<i64> = row.get(2)?;
        let working_directory: Option<Vec<u8>> = row.get(3)?;
        let hostname: Option<Vec<u8>> = row.get(4)?;
        let exit_status: Option<i32> = row.get(5)?;
        let duration_secs: Option<i64> = row.get(6)?;
        Ok(HistoryEntry {
            id,
            command: String::from_utf8_lossy(&command).to_string(),
            timestamp,
            working_directory: working_directory.map(BString::from),
            hostname: hostname.map(BString::from),
            exit_status,
            duration_secs,
        })
    }

    fn load_entries_for_directory(
        &self,
        host_filter: HostFilter,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let mut where_conditions = vec!["working_directory = CAST(? as blob)".to_string()];
        let dir_str = self.working_directory.to_string_lossy().to_string();
        let mut params: Vec<String> = vec![dir_str];

        if host_filter == HostFilter::ThisHost {
            let placeholders: String =
                self.host_set.iter().map(|_| "CAST(? as blob)").collect::<Vec<_>>().join(", ");
            where_conditions.push(format!("hostname IN ({placeholders})"));
            for h in &self.host_set {
                params.push(h.to_string());
            }
        }

        if let Some(q) = query
            && !q.is_empty()
        {
            where_conditions
                .push("CAST(full_command AS text) LIKE '%' || ? || '%' COLLATE NOCASE".to_string());
            params.push(q.to_string());
        }

        let where_clause = format!("WHERE {}", where_conditions.join(" AND "));

        let entries = self.run_recall_query(&where_clause, &params)?;
        Ok(entries)
    }

    /// Shared query logic for loading recall entries. Uses a CTE so the
    /// WHERE clause applies to both the GROUP BY aggregation and the outer
    /// select -- preventing cross-host/directory row duplication in the JOIN.
    fn run_recall_query(
        &self,
        where_clause: &str,
        params: &[String],
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let sql = format!(
            r#"
WITH filtered AS (
    SELECT * FROM command_history
    {where_clause}
)
SELECT f.id, f.full_command, f.start_unix_timestamp, f.working_directory,
       f.hostname, f.exit_status,
       CASE WHEN f.end_unix_timestamp IS NOT NULL
            THEN f.end_unix_timestamp - f.start_unix_timestamp
            ELSE NULL END as duration
  FROM filtered f
 INNER JOIN (
     SELECT full_command, MAX(start_unix_timestamp) as max_ts
       FROM filtered
      GROUP BY full_command
 ) latest ON f.full_command = latest.full_command
         AND f.start_unix_timestamp = latest.max_ts
 ORDER BY f.start_unix_timestamp DESC
 LIMIT {}
"#,
            self.result_limit
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let entries: Vec<HistoryEntry> = stmt
            .query_map(param_refs.as_slice(), |row| self.row_to_entry(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Delete a single history entry by its database ID.
    pub fn delete_entry(&self, id: i64) -> Result<usize, Box<dyn std::error::Error>> {
        let deleted = self.conn.execute("DELETE FROM command_history WHERE id = ?", [id])?;
        Ok(deleted)
    }

    /// Get the working directory for display
    pub fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }

    /// Filter entries using nucleo fuzzy matching.
    /// Returns entries sorted by match score (word boundary matches favored, gaps penalized),
    /// with recency as a tiebreaker for equal scores.
    pub fn filter_entries<'a>(
        &mut self,
        entries: &'a [HistoryEntry],
        query: &str,
    ) -> Vec<(&'a HistoryEntry, Vec<u32>)> {
        if query.is_empty() {
            // No query - return all entries without match positions
            return entries.iter().map(|e| (e, Vec::new())).collect();
        }

        // Nucleo's fuzzy matcher gives word-boundary bonuses, treating `-` as a separator.
        // This causes `--release` to score poorly (empty segments before "release").
        // We normalize dashes and asterisks to spaces for scoring so `--release` and `release`
        // rank equally, and `*` acts as a word separator in queries.
        // The original query is used for highlighting so `--release` shows highlighted dashes.
        let normalized_query: String =
            query.chars().map(|c| if c == '-' || c == '*' { ' ' } else { c }).collect();
        let scoring_pattern = Pattern::parse(
            &normalized_query,
            nucleo::pattern::CaseMatching::Smart,
            nucleo::pattern::Normalization::Smart,
        );
        let highlight_pattern = Pattern::parse(
            query,
            nucleo::pattern::CaseMatching::Smart,
            nucleo::pattern::Normalization::Smart,
        );

        let mut scored_results: Vec<(usize, u32, &HistoryEntry, Vec<u32>)> = Vec::new();
        let mut buf = Vec::new();
        let mut normalized_cmd = String::new();

        for (original_idx, entry) in entries.iter().enumerate() {
            // Normalize command for scoring (- and * → space)
            normalized_cmd.clear();
            normalized_cmd
                .extend(entry.command.chars().map(|c| if c == '-' || c == '*' { ' ' } else { c }));
            buf.clear();
            let haystack = Utf32Str::new(&normalized_cmd, &mut buf);

            if let Some(score) = scoring_pattern.score(haystack, &mut self.matcher) {
                // Get highlight indices from original command/query, with fallback to scoring
                // pattern if original query doesn't match (e.g., query "--release" vs cmd "release")
                let mut indices = Vec::new();
                buf.clear();
                let haystack = Utf32Str::new(&entry.command, &mut buf);
                highlight_pattern.indices(haystack, &mut self.matcher, &mut indices);
                if indices.is_empty() {
                    buf.clear();
                    let haystack = Utf32Str::new(&entry.command, &mut buf);
                    scoring_pattern.indices(haystack, &mut self.matcher, &mut indices);
                }
                scored_results.push((original_idx, score, entry, indices));
            }
        }

        // Sort by score (descending), then by original index (ascending = more recent first)
        scored_results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        // Return just the entries and indices
        scored_results.into_iter().map(|(_, _, entry, indices)| (entry, indices)).collect()
    }
}

/// Format a timestamp as a relative time string (e.g., "2m", "3h", "2d")
pub fn format_relative_time(timestamp: Option<i64>) -> String {
    let Some(ts) = timestamp else {
        return "   ".to_string();
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let diff = now - ts;
    if diff < 0 {
        return "   ".to_string();
    }

    let diff = diff as u64;
    if diff < 60 {
        format!("{:>2}s", diff)
    } else if diff < 3600 {
        format!("{:>2}m", diff / 60)
    } else if diff < 86400 {
        format!("{:>2}h", diff / 3600)
    } else if diff < 86400 * 7 {
        format!("{:>2}d", diff / 86400)
    } else if diff < 86400 * 30 {
        format!("{:>2}w", diff / (86400 * 7))
    } else if diff < 86400 * 365 {
        format!("{:>2}M", diff / (86400 * 30))
    } else {
        format!("{:>2}y", diff / (86400 * 365))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bstr::BString;
    use rusqlite::Connection;

    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_base_schema(&conn).unwrap();
        crate::run_schema_migrations(&conn).unwrap();
        conn
    }

    fn insert_command(conn: &Connection, cmd: &str, hostname: &str, dir: &str, ts: i64) {
        conn.execute(
            "INSERT INTO command_history (session_id, full_command, shellname, hostname, working_directory, start_unix_timestamp)
             VALUES (1, CAST(? AS blob), 'bash', CAST(? AS blob), CAST(? AS blob), ?)",
            rusqlite::params![cmd, hostname, dir, ts],
        )
        .unwrap();
    }

    #[test]
    fn test_engine_host_filter() {
        let conn = test_db();
        insert_command(&conn, "alpha-cmd", "alpha", "/tmp", 1000);
        insert_command(&conn, "beta-cmd", "beta", "/tmp", 2000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("alpha")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::ThisHost, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "alpha-cmd");

        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_engine_directory_filter() {
        let conn = test_db();
        insert_command(&conn, "in-project", "host1", "/home/user/project", 1000);
        insert_command(&conn, "in-other", "host1", "/home/user/other", 2000);

        let engine = SearchEngine::new(
            conn,
            PathBuf::from("/home/user/project"),
            vec![BString::from("host1")],
            100,
        );
        let entries =
            engine.load_entries(FilterMode::Directory, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "in-project");
    }

    #[test]
    fn test_engine_dedup_keeps_latest() {
        let conn = test_db();
        insert_command(&conn, "ls -la", "host1", "/tmp", 1000);
        insert_command(&conn, "ls -la", "host1", "/tmp", 2000);
        insert_command(&conn, "pwd", "host1", "/tmp", 1500);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), 2);

        let ls_entry = entries.iter().find(|e| e.command == "ls -la").unwrap();
        assert_eq!(ls_entry.timestamp, Some(2000));
    }

    #[test]
    fn test_engine_fuzzy_normalization_dashes() {
        let conn = test_db();
        insert_command(&conn, "cargo build --release", "host1", "/tmp", 1000);
        insert_command(&conn, "echo hello", "host1", "/tmp", 2000);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();

        let filtered = engine.filter_entries(&entries, "release");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0.command, "cargo build --release");

        let filtered = engine.filter_entries(&entries, "--release");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0.command, "cargo build --release");
    }

    #[test]
    fn test_engine_fuzzy_normalization_asterisks() {
        let conn = test_db();
        insert_command(&conn, "find . -name '*.rs'", "host1", "/tmp", 1000);
        insert_command(&conn, "echo hello", "host1", "/tmp", 2000);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();

        let filtered = engine.filter_entries(&entries, "*.rs");
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].0.command.contains("*.rs"));
    }

    #[test]
    fn test_global_result_limit_hides_old_commands() {
        let conn = test_db();
        let result_limit = 5;

        // Insert an old "shutdown" command
        insert_command(&conn, "sudo shutdown -h now", "host1", "/home/user", 100);

        // Insert more than result_limit newer unique commands to push shutdown out
        for i in 0..(result_limit + 1) {
            insert_command(
                &conn,
                &format!("unique-cmd-{i}"),
                "host1",
                "/home/user",
                1000 + i as i64,
            );
        }

        let engine = SearchEngine::new(
            conn,
            PathBuf::from("/home/user"),
            vec![BString::from("host1")],
            result_limit,
        );

        // Global mode without query: shutdown is beyond the result_limit window
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), result_limit);
        assert!(
            !entries.iter().any(|e| e.command.contains("shutdown")),
            "shutdown should be excluded by result_limit"
        );

        // Global mode WITH query: LIKE filter narrows before LIMIT, so shutdown is found
        let entries = engine
            .load_entries(FilterMode::Global, HostFilter::AllHosts, Some("shutdown"))
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "sudo shutdown -h now");
    }

    #[test]
    fn test_all_hosts_returns_superset_of_this_host() {
        let conn = test_db();
        let result_limit = 10;

        // Same commands on both hosts, many with identical timestamps (from sync)
        for i in 0..8 {
            insert_command(&conn, &format!("shared-cmd-{i}"), "host1", "/tmp", 1000 + i);
            insert_command(&conn, &format!("shared-cmd-{i}"), "host2", "/tmp", 1000 + i);
        }
        // Commands unique to each host
        insert_command(&conn, "host1-only", "host1", "/tmp", 900);
        insert_command(&conn, "host2-only", "host2", "/tmp", 901);

        let engine = SearchEngine::new(
            conn,
            PathBuf::from("/tmp"),
            vec![BString::from("host1")],
            result_limit,
        );

        let this_host =
            engine.load_entries(FilterMode::Global, HostFilter::ThisHost, None).unwrap();
        let all_hosts =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();

        // AllHosts must return at least as many unique commands as ThisHost
        assert!(
            all_hosts.len() >= this_host.len(),
            "AllHosts ({}) should have >= ThisHost ({}) entries",
            all_hosts.len(),
            this_host.len()
        );

        // AllHosts should include commands from both hosts
        assert_eq!(all_hosts.len(), 10); // 8 shared + 2 unique
        assert_eq!(this_host.len(), 9); // 8 shared + 1 host1-only
    }

    #[test]
    fn test_format_relative_time_none() {
        assert_eq!(format_relative_time(None), "   ");
    }

    #[test]
    fn test_format_relative_time_seconds() {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                as i64;
        assert_eq!(format_relative_time(Some(now - 30)), "30s");
        assert_eq!(format_relative_time(Some(now - 5)), " 5s");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                as i64;
        assert_eq!(format_relative_time(Some(now - 120)), " 2m");
        assert_eq!(format_relative_time(Some(now - 3000)), "50m");
    }

    #[test]
    fn test_format_relative_time_hours() {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                as i64;
        assert_eq!(format_relative_time(Some(now - 7200)), " 2h");
        assert_eq!(format_relative_time(Some(now - 36000)), "10h");
    }

    #[test]
    fn test_format_relative_time_days() {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                as i64;
        assert_eq!(format_relative_time(Some(now - 86400 * 2)), " 2d");
        assert_eq!(format_relative_time(Some(now - 86400 * 5)), " 5d");
    }
}
