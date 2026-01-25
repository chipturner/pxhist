use std::path::PathBuf;

use bstr::BString;
use nucleo::{Config, Matcher, Utf32Str, pattern::Pattern};
use rusqlite::Connection;

use super::command::FilterMode;

/// A history entry with its metadata
#[derive(Debug, Clone)]
pub struct HistoryEntry {
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
    matcher: Matcher,
    result_limit: usize,
}

impl SearchEngine {
    pub fn new(conn: Connection, working_directory: PathBuf, result_limit: usize) -> Self {
        SearchEngine {
            conn,
            working_directory,
            matcher: Matcher::new(Config::DEFAULT),
            result_limit,
        }
    }

    /// Load history entries from the database, optionally filtered by a search query
    pub fn load_entries(
        &self,
        filter_mode: FilterMode,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let entries = match filter_mode {
            FilterMode::Directory => self.load_entries_for_directory(query)?,
            FilterMode::Global => self.load_all_entries(query)?,
        };
        Ok(entries)
    }

    fn load_all_entries(
        &self,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        // Build WHERE clause for search filter
        let (where_clause, search_param) = match query {
            Some(q) if !q.is_empty() => {
                // Use LIKE with % wildcards for case-insensitive substring match
                // The LIKE is applied in the subquery to filter before grouping
                (
                    "WHERE full_command LIKE '%' || ? || '%' COLLATE NOCASE".to_string(),
                    Some(q.to_string()),
                )
            }
            _ => (String::new(), None),
        };

        // Get full metadata from the most recent execution of each unique command
        let sql = format!(
            r#"
SELECT c.full_command, c.start_unix_timestamp, c.working_directory,
       c.hostname, c.exit_status,
       CASE WHEN c.end_unix_timestamp IS NOT NULL
            THEN c.end_unix_timestamp - c.start_unix_timestamp
            ELSE NULL END as duration
  FROM command_history c
 INNER JOIN (
     SELECT full_command, MAX(start_unix_timestamp) as max_ts
       FROM command_history
      {where_clause}
      GROUP BY full_command
 ) latest ON c.full_command = latest.full_command
         AND c.start_unix_timestamp = latest.max_ts
 ORDER BY c.start_unix_timestamp DESC
 LIMIT {}
"#,
            self.result_limit
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let entries: Vec<HistoryEntry> = if let Some(ref param) = search_param {
            stmt.query_map([param], |row| self.row_to_entry(row))?.collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| self.row_to_entry(row))?.collect::<Result<Vec<_>, _>>()?
        };

        Ok(entries)
    }

    fn row_to_entry(&self, row: &rusqlite::Row) -> rusqlite::Result<HistoryEntry> {
        let command: Vec<u8> = row.get(0)?;
        let timestamp: Option<i64> = row.get(1)?;
        let working_directory: Option<Vec<u8>> = row.get(2)?;
        let hostname: Option<Vec<u8>> = row.get(3)?;
        let exit_status: Option<i32> = row.get(4)?;
        let duration_secs: Option<i64> = row.get(5)?;
        Ok(HistoryEntry {
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
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        // Build additional WHERE clause for search filter
        let (search_clause, search_param) = match query {
            Some(q) if !q.is_empty() => (
                "AND full_command LIKE '%' || ? || '%' COLLATE NOCASE".to_string(),
                Some(q.to_string()),
            ),
            _ => (String::new(), None),
        };

        // Get full metadata from the most recent execution of each unique command in this directory
        let sql = format!(
            r#"
SELECT c.full_command, c.start_unix_timestamp, c.working_directory,
       c.hostname, c.exit_status,
       CASE WHEN c.end_unix_timestamp IS NOT NULL
            THEN c.end_unix_timestamp - c.start_unix_timestamp
            ELSE NULL END as duration
  FROM command_history c
 INNER JOIN (
     SELECT full_command, MAX(start_unix_timestamp) as max_ts
       FROM command_history
      WHERE working_directory = CAST(? as blob)
      {search_clause}
      GROUP BY full_command
 ) latest ON c.full_command = latest.full_command
         AND c.start_unix_timestamp = latest.max_ts
 ORDER BY c.start_unix_timestamp DESC
 LIMIT {}
"#,
            self.result_limit
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let dir_str = self.working_directory.to_string_lossy().to_string();

        let entries: Vec<HistoryEntry> = if let Some(ref param) = search_param {
            stmt.query_map(rusqlite::params![dir_str, param], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([&dir_str], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?
        };

        Ok(entries)
    }

    /// Get the working directory for display
    pub fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }

    /// Filter entries using nucleo fuzzy matching
    /// Returns entries with their match scores, sorted by score (best first)
    pub fn filter_entries<'a>(
        &mut self,
        entries: &'a [HistoryEntry],
        query: &str,
    ) -> Vec<(&'a HistoryEntry, Vec<u32>)> {
        if query.is_empty() {
            // No query - return all entries without match positions
            return entries.iter().map(|e| (e, Vec::new())).collect();
        }

        // Parse the pattern
        let pattern = Pattern::parse(
            query,
            nucleo::pattern::CaseMatching::Smart,
            nucleo::pattern::Normalization::Smart,
        );

        let mut results: Vec<(&HistoryEntry, Vec<u32>)> = Vec::new();
        let mut buf = Vec::new();

        for entry in entries {
            buf.clear();
            let haystack = Utf32Str::new(&entry.command, &mut buf);

            if pattern.score(haystack, &mut self.matcher).is_some() {
                // Get match indices for highlighting
                let mut indices = Vec::new();
                buf.clear();
                let haystack = Utf32Str::new(&entry.command, &mut buf);
                pattern.indices(haystack, &mut self.matcher, &mut indices);
                results.push((entry, indices));
            }
        }

        // Keep original order (sorted by recency from SQL query)
        // Fuzzy matching is used as a filter, not for ranking
        results
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
    use super::*;

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
