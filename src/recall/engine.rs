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

    /// Build the LIKE patterns for the DB prefilter: one per whitespace-separated
    /// query atom, since nucleo matches atoms in any order. fzf-style operators
    /// understood by nucleo's `Pattern::parse` (`^`/`'` prefix, `$` suffix) are
    /// stripped so the prefilter remains a superset of what the fuzzy stage
    /// accepts; negated atoms (`!foo`) are dropped entirely because LIKE can only
    /// require presence -- the fuzzy stage enforces absence.
    fn fuzzy_like_patterns(query: &str) -> Vec<String> {
        query
            .split_whitespace()
            .filter_map(|atom| {
                if atom.starts_with('!') {
                    return None;
                }
                let atom = atom.strip_prefix(['^', '\'']).unwrap_or(atom);
                let atom = atom.strip_suffix('$').unwrap_or(atom);
                (!atom.is_empty()).then(|| Self::fuzzy_like_pattern(atom))
            })
            .collect()
    }

    /// Whether a query produces any DB-level prefilter conditions. Queries of
    /// only negated/operator atoms don't, and load identically to no query.
    pub fn query_has_prefilter(query: &str) -> bool {
        !Self::fuzzy_like_patterns(query).is_empty()
    }

    /// Append one LIKE condition per prefilter pattern of `query`.
    fn push_query_conditions(
        query: Option<&str>,
        where_conditions: &mut Vec<String>,
        params: &mut Vec<String>,
    ) {
        for pattern in query.map(Self::fuzzy_like_patterns).unwrap_or_default() {
            where_conditions
                .push("CAST(full_command AS text) LIKE ? ESCAPE '\\' COLLATE NOCASE".to_string());
            params.push(pattern);
        }
    }

    /// Build a LIKE pattern that matches a fuzzy subsequence of one atom.
    /// "gcm" becomes "%g%c%m%" so it matches "git commit -m".
    fn fuzzy_like_pattern(atom: &str) -> String {
        let mut pattern = String::with_capacity(atom.len() * 2 + 1);
        pattern.push('%');
        for ch in atom.chars() {
            match ch {
                // Escape LIKE special characters
                '%' | '_' | '\\' => {
                    pattern.push('\\');
                    pattern.push(ch);
                }
                // Recall-separator chars become wildcards so the prefilter
                // mirrors `normalize_recall_char()` in scoring (see below).
                c if is_recall_separator(c) => pattern.push('%'),
                _ => pattern.push(ch),
            }
            pattern.push('%');
        }
        pattern
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

        Self::push_query_conditions(query, &mut where_conditions, &mut params);

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

        Self::push_query_conditions(query, &mut where_conditions, &mut params);

        let where_clause = format!("WHERE {}", where_conditions.join(" AND "));

        let entries = self.run_recall_query(&where_clause, &params)?;
        Ok(entries)
    }

    /// Shared query logic for loading recall entries. Oversamples by 3x and
    /// relies on the caller's `deduplicate_entries()` for dedup -- avoids the
    /// expensive CTE self-join that caused double table scans at scale.
    fn run_recall_query(
        &self,
        where_clause: &str,
        params: &[String],
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let sql = format!(
            r#"
SELECT id, full_command, start_unix_timestamp, working_directory,
       hostname, exit_status,
       CASE WHEN end_unix_timestamp IS NOT NULL
            THEN end_unix_timestamp - start_unix_timestamp
            ELSE NULL END as duration
  FROM command_history
  {where_clause}
 ORDER BY start_unix_timestamp DESC, id DESC
 LIMIT {}
"#,
            self.result_limit * 3
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let entries: Vec<HistoryEntry> = stmt
            .query_map(param_refs.as_slice(), |row| self.row_to_entry(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Delete all history entries matching a command (trimmed), since the
    /// recall list is deduplicated by command text.  Returns the number of
    /// rows deleted.
    pub fn delete_entries_by_command(
        &self,
        command: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let trimmed = command.trim_end();
        let deleted = self.conn.execute(
            "DELETE FROM command_history WHERE rtrim(CAST(full_command AS text)) = ?",
            [trimmed],
        )?;
        Ok(deleted)
    }

    /// Get the configured result limit
    pub fn result_limit(&self) -> usize {
        self.result_limit
    }

    /// Get the working directory for display
    pub fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }

    /// Filter entries using nucleo fuzzy matching.
    /// Returns (index, highlight_positions) sorted by combined score:
    /// nucleo's fuzzy score (word-boundary matches favored, gaps penalized)
    /// plus a recency boost so freshly-used commands outrank stale ones at
    /// similar match quality. Original-index ascending breaks remaining ties
    /// (more recent first).
    pub fn filter_entries(
        &mut self,
        entries: &[HistoryEntry],
        query: &str,
    ) -> Vec<(usize, Vec<u32>)> {
        if query.is_empty() {
            // No query - return all entries without match positions
            return (0..entries.len()).map(|i| (i, Vec::new())).collect();
        }

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Nucleo distinguishes whitespace boundaries (BONUS_BOUNDARY_WHITE) from
        // delimiter boundaries (BONUS_BOUNDARY_DELIMITER, ~40% smaller), so for query
        // "foobar" the haystack `cat /foobar` scores meaningfully below `foobar plain`
        // -- enough that the frecency boost can't always close the gap. Normalizing
        // `-`, `*`, `/` to spaces in both query and haystack puts these match positions
        // in the BOUNDARY_WHITE tier. (`*` also acts as a word separator in queries.)
        // The original query is used for highlighting so `--release` shows highlighted dashes.
        let normalized_query: String = query.chars().map(normalize_recall_char).collect();
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

        let mut scored_results: Vec<(usize, u32, Vec<u32>)> = Vec::new();
        let mut buf = Vec::new();
        let mut normalized_cmd = String::new();

        for (original_idx, entry) in entries.iter().enumerate() {
            // Normalize command for scoring (-, *, / → space)
            normalized_cmd.clear();
            normalized_cmd.extend(entry.command.chars().map(normalize_recall_char));
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
                let boosted = score + frecency_boost(entry.timestamp, now_secs);
                scored_results.push((original_idx, boosted, indices));
            }
        }

        // Sort by score (descending), then by original index (ascending = more recent first)
        scored_results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        // Return just the indices and highlight positions
        scored_results.into_iter().map(|(idx, _, indices)| (idx, indices)).collect()
    }
}

/// Characters treated as word separators in recall search. `-` covers flag forms
/// (`--release`), `*` covers glob queries, and `/` covers path components so a
/// match after any of them scores like a whitespace boundary rather than nucleo's
/// (lower) delimiter boundary.
fn is_recall_separator(c: char) -> bool {
    matches!(c, '-' | '*' | '/')
}

/// Map separator chars to space so nucleo treats them as word boundaries; pass
/// through everything else.
fn normalize_recall_char(c: char) -> char {
    if is_recall_separator(c) { ' ' } else { c }
}

/// Recency boost added to nucleo's fuzzy score so freshly-used commands outrank
/// stale ones at similar match quality. The step values are tuned to nucleo's
/// natural range (~16-32 for single-char queries): the boost can flip ordering
/// for short queries, but for longer queries -- where nucleo scores grow with
/// each matched char -- it degrades gracefully into a tiebreaker.
fn frecency_boost(timestamp: Option<i64>, now_secs: i64) -> u32 {
    let Some(ts) = timestamp else { return 0 };
    let age_secs = (now_secs - ts).max(0);
    let age_days = age_secs as f64 / 86400.0;
    match age_days {
        d if d < 0.04 => 24, // < ~1 hour
        d if d < 1.0 => 16,
        d if d < 7.0 => 10,
        d if d < 30.0 => 5,
        d if d < 90.0 => 2,
        _ => 0,
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
    fn test_engine_returns_all_rows_ordered_by_time() {
        // load_entries returns raw rows (most recent first); dedup is the caller's job.
        let conn = test_db();
        insert_command(&conn, "ls -la", "host1", "/tmp", 1000);
        insert_command(&conn, "ls -la", "host1", "/tmp", 2000);
        insert_command(&conn, "pwd", "host1", "/tmp", 1500);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "ls -la");
        assert_eq!(entries[0].timestamp, Some(2000));
        assert_eq!(entries[1].command, "pwd");
        assert_eq!(entries[1].timestamp, Some(1500));
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
        assert_eq!(entries[filtered[0].0].command, "cargo build --release");

        let filtered = engine.filter_entries(&entries, "--release");
        assert_eq!(filtered.len(), 1);
        assert_eq!(entries[filtered[0].0].command, "cargo build --release");
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
        assert!(entries[filtered[0].0].command.contains("*.rs"));
    }

    #[test]
    fn test_engine_fuzzy_normalization_slashes() {
        let conn = test_db();
        insert_command(&conn, "cat /foobar", "host1", "/tmp", 1000);
        insert_command(&conn, "echo hello", "host1", "/tmp", 2000);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();

        let filtered = engine.filter_entries(&entries, "foobar");
        assert_eq!(filtered.len(), 1);
        assert_eq!(entries[filtered[0].0].command, "cat /foobar");
    }

    #[test]
    fn test_filter_entries_recent_slash_path_beats_older_plain_word() {
        // The user's reported bug: typing `foobar` should rank a recent
        // `cat /foobar` (path-component match) above an old plain `foobar`.
        // Without `/` normalization, nucleo's delimiter-boundary bonus is
        // smaller than the whitespace-boundary bonus, and the gap can exceed
        // the frecency boost cap.
        let conn = test_db();
        let now = wall_clock_now();

        insert_command(&conn, "foobar --plain", "host1", "/tmp", now - 86_400 * 200);
        insert_command(&conn, "cat /foobar", "host1", "/tmp", now - 60);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        let filtered = engine.filter_entries(&entries, "foobar");

        assert_eq!(filtered.len(), 2);
        assert_eq!(
            entries[filtered[0].0].command, "cat /foobar",
            "recent /foobar should outrank older plain foobar once `/` normalizes to a word boundary"
        );
    }

    #[test]
    fn test_like_filter_normalizes_slash() {
        // Query `/foobar` should match haystacks where `/foobar` doesn't appear
        // literally -- the `/` is a wildcard in the prefilter, consistent with
        // how it's treated as a word separator in scoring.
        let conn = test_db();
        insert_command(&conn, "echo foobar", "host1", "/tmp", 1000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some("/foobar")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "echo foobar");
    }

    #[test]
    fn test_frecency_boost_handles_missing_timestamp() {
        assert_eq!(frecency_boost(None, 1_000_000), 0);
    }

    #[test]
    fn test_frecency_boost_decays_in_steps() {
        let now = 86_400 * 10_000;
        let day = 86_400;
        assert_eq!(frecency_boost(Some(now), now), 24);
        assert_eq!(frecency_boost(Some(now - 1800), now), 24); // 30 min
        assert_eq!(frecency_boost(Some(now - 7200), now), 16); // 2 hours
        assert_eq!(frecency_boost(Some(now - day * 3), now), 10);
        assert_eq!(frecency_boost(Some(now - day * 14), now), 5);
        assert_eq!(frecency_boost(Some(now - day * 60), now), 2);
        assert_eq!(frecency_boost(Some(now - day * 200), now), 0);
    }

    #[test]
    fn test_frecency_boost_clamps_future_timestamps() {
        // Clock skew shouldn't yield a smaller boost than "now".
        let now = 86_400 * 10_000;
        assert_eq!(frecency_boost(Some(now + 86_400), now), 24);
    }

    fn wall_clock_now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
    }

    #[test]
    fn test_filter_entries_recent_beats_older_better_fuzzy_match() {
        // For query "p": "python ..." gets nucleo's first-char bonus, while
        // "cd /p" only gets a word-boundary bonus -- so without frecency the
        // older python entry would win. With frecency, the fresh /p entry
        // should outrank it.
        let conn = test_db();
        let now = wall_clock_now();

        insert_command(&conn, "python script.py", "host1", "/tmp", now - 86_400 * 200);
        insert_command(&conn, "cd /p", "host1", "/tmp", now - 60);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        let filtered = engine.filter_entries(&entries, "p");

        assert!(filtered.len() >= 2);
        assert_eq!(
            entries[filtered[0].0].command, "cd /p",
            "fresh /p should outrank old python under frecency for query 'p'"
        );
    }

    #[test]
    fn test_filter_entries_preserves_fuzzy_quality_when_equally_recent() {
        // Both entries are within the same frecency tier, so the boost cancels
        // and the better fuzzy match (first-char "python") should still win.
        let conn = test_db();
        let now = wall_clock_now();

        insert_command(&conn, "python script.py", "host1", "/tmp", now - 60);
        insert_command(&conn, "cd /p", "host1", "/tmp", now - 120);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        let filtered = engine.filter_entries(&entries, "p");

        assert_eq!(
            entries[filtered[0].0].command, "python script.py",
            "with both recent, fuzzy quality (first-char match) should win"
        );
    }

    #[test]
    fn test_global_result_limit_hides_old_commands() {
        let conn = test_db();
        let result_limit = 5;
        // load_entries oversamples by 3x, so we need > result_limit * 3 newer
        // commands to push the old one beyond the query window.
        let oversample = result_limit * 3;

        // Insert an old "shutdown" command
        insert_command(&conn, "sudo shutdown -h now", "host1", "/home/user", 100);

        // Insert more than oversample newer unique commands to push shutdown out
        for i in 0..(oversample + 1) {
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

        // Global mode without query: shutdown is beyond the oversample window
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), oversample);
        assert!(
            !entries.iter().any(|e| e.command.contains("shutdown")),
            "shutdown should be excluded by oversample limit"
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

        // AllHosts must return at least as many rows as ThisHost
        assert!(
            all_hosts.len() >= this_host.len(),
            "AllHosts ({}) should have >= ThisHost ({}) entries",
            all_hosts.len(),
            this_host.len()
        );

        // Raw rows (dedup is the caller's job):
        // AllHosts: 8 commands * 2 hosts + 2 unique = 18
        // ThisHost: 8 shared + 1 host1-only = 9
        assert_eq!(all_hosts.len(), 18);
        assert_eq!(this_host.len(), 9);
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
    fn test_like_filter_matches_fuzzy_subsequences() {
        let conn = test_db();
        insert_command(&conn, "git commit -m 'test'", "host1", "/tmp", 1000);
        insert_command(&conn, "docker compose up", "host1", "/tmp", 2000);
        insert_command(&conn, "kubectl get pods", "host1", "/tmp", 3000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);

        // "gcm" should match "git commit -m" via subsequence (g...c...m)
        let entries =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some("gcm")).unwrap();
        assert!(
            entries.iter().any(|e| e.command.contains("git commit")),
            "fuzzy query 'gcm' should match 'git commit -m', got: {:?}",
            entries.iter().map(|e| &e.command).collect::<Vec<_>>()
        );

        // "dcu" should match "docker compose up"
        let entries =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some("dcu")).unwrap();
        assert!(
            entries.iter().any(|e| e.command.contains("docker compose")),
            "fuzzy query 'dcu' should match 'docker compose up', got: {:?}",
            entries.iter().map(|e| &e.command).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_like_filter_atoms_match_any_order() {
        // nucleo matches whitespace-separated atoms in any order, so the DB
        // prefilter must too -- one LIKE condition per atom, not one ordered
        // subsequence for the whole query.
        let conn = test_db();
        insert_command(&conn, "git push origin main", "host1", "/tmp", 1000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine
            .load_entries(FilterMode::Global, HostFilter::AllHosts, Some("push git"))
            .unwrap();
        assert_eq!(entries.len(), 1, "reversed word order should still prefilter-match");
        assert_eq!(entries[0].command, "git push origin main");
    }

    #[test]
    fn test_like_filter_strips_fzf_operators() {
        // nucleo's Pattern::parse understands fzf-style operators; the LIKE
        // prefilter must strip them rather than demand them as literal chars.
        let conn = test_db();
        insert_command(&conn, "cargo build --release", "host1", "/tmp", 1000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);

        for query in ["^cargo", "release$", "'build", "^cargo release$"] {
            let entries =
                engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some(query)).unwrap();
            assert_eq!(entries.len(), 1, "query {query:?} should survive the prefilter");
        }
    }

    #[test]
    fn test_like_filter_drops_negated_atoms() {
        // A negated atom can't be prefiltered (LIKE can only require presence),
        // so it must be dropped; the fuzzy stage enforces absence. A query of
        // only negated atoms degrades to the broad (unfiltered) load.
        let conn = test_db();
        insert_command(&conn, "vim notes.txt", "host1", "/tmp", 1000);
        insert_command(&conn, "cargo build", "host1", "/tmp", 2000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);

        let entries =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some("!vim")).unwrap();
        assert_eq!(entries.len(), 2, "pure-negation query should not prefilter at all");

        let entries = engine
            .load_entries(FilterMode::Global, HostFilter::AllHosts, Some("!vim cargo"))
            .unwrap();
        assert_eq!(entries.len(), 1, "positive atom should still prefilter");
        assert_eq!(entries[0].command, "cargo build");
    }

    #[test]
    fn test_filter_entries_negation_and_prefix() {
        // End-to-end: the fuzzy stage honors fzf-style operators the prefilter
        // now lets through.
        let conn = test_db();
        insert_command(&conn, "vim notes.txt", "host1", "/tmp", 1000);
        insert_command(&conn, "cargo build --release", "host1", "/tmp", 2000);
        insert_command(&conn, "echo cargo", "host1", "/tmp", 3000);

        let mut engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);
        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();

        let filtered = engine.filter_entries(&entries, "!vim");
        let commands: Vec<_> = filtered.iter().map(|(i, _)| &entries[*i].command).collect();
        assert!(!commands.iter().any(|c| c.contains("vim")), "negation should exclude vim");
        assert_eq!(commands.len(), 2);

        let filtered = engine.filter_entries(&entries, "^cargo");
        assert_eq!(filtered.len(), 1, "^ should anchor to start of command");
        assert_eq!(entries[filtered[0].0].command, "cargo build --release");
    }

    #[test]
    fn test_query_has_prefilter() {
        assert!(SearchEngine::query_has_prefilter("cargo"));
        assert!(SearchEngine::query_has_prefilter("!vim cargo"));
        assert!(!SearchEngine::query_has_prefilter("!vim"));
        assert!(!SearchEngine::query_has_prefilter("^$"));
        assert!(!SearchEngine::query_has_prefilter("   "));
        assert!(!SearchEngine::query_has_prefilter(""));
    }

    #[test]
    fn test_like_filter_normalizes_dash_and_star() {
        let conn = test_db();
        insert_command(&conn, "git log --oneline", "host1", "/tmp", 1000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);

        // "git-log" should match "git log" because `-` is normalized to wildcard
        let entries =
            engine.load_entries(FilterMode::Global, HostFilter::AllHosts, Some("git-log")).unwrap();
        assert!(
            entries.iter().any(|e| e.command.contains("git log")),
            "query 'git-log' should match 'git log' (dash normalized), got: {:?}",
            entries.iter().map(|e| &e.command).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_delete_entries_by_command_removes_all_duplicates() {
        let conn = test_db();
        // Insert the same command multiple times (different timestamps simulate real usage)
        insert_command(&conn, "git status", "host1", "/tmp", 1000);
        insert_command(&conn, "git status", "host1", "/tmp", 2000);
        insert_command(&conn, "git status", "host1", "/tmp", 3000);
        insert_command(&conn, "other cmd", "host1", "/tmp", 4000);

        let engine =
            SearchEngine::new(conn, PathBuf::from("/tmp"), vec![BString::from("host1")], 100);

        let deleted = engine.delete_entries_by_command("git status").unwrap();
        assert_eq!(deleted, 3, "should delete all rows matching the command");

        let entries = engine.load_entries(FilterMode::Global, HostFilter::AllHosts, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "other cmd");
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
