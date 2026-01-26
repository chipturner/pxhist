use std::path::PathBuf;

use bstr::BString;
use nucleo::{Config, Matcher, Utf32Str, pattern::Pattern};
use rusqlite::Connection;

use super::command::{FilterMode, HostFilter};

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
    current_hostname: BString,
    matcher: Matcher,
    result_limit: usize,
}

impl SearchEngine {
    pub fn new(
        conn: Connection,
        working_directory: PathBuf,
        current_hostname: BString,
        result_limit: usize,
    ) -> Self {
        SearchEngine {
            conn,
            working_directory,
            current_hostname,
            matcher: Matcher::new(Config::DEFAULT),
            result_limit,
        }
    }

    /// Get the current hostname
    pub fn current_hostname(&self) -> &BString {
        &self.current_hostname
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
        // Build WHERE clauses
        let mut where_conditions = Vec::new();
        let mut params: Vec<String> = Vec::new();

        // Host filter
        if host_filter == HostFilter::ThisHost {
            where_conditions.push("hostname = CAST(? as blob)".to_string());
            params.push(self.current_hostname.to_string());
        }

        // Search filter
        if let Some(q) = query
            && !q.is_empty()
        {
            where_conditions.push("full_command LIKE '%' || ? || '%' COLLATE NOCASE".to_string());
            params.push(q.to_string());
        }

        let where_clause = if where_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_conditions.join(" AND "))
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

        let entries: Vec<HistoryEntry> = match params.len() {
            0 => {
                stmt.query_map([], |row| self.row_to_entry(row))?.collect::<Result<Vec<_>, _>>()?
            }
            1 => stmt
                .query_map([&params[0]], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?,
            2 => stmt
                .query_map([&params[0], &params[1]], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?,
            _ => unreachable!(),
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
        host_filter: HostFilter,
        query: Option<&str>,
    ) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        // Build WHERE clauses - directory is always filtered
        let mut where_conditions = vec!["working_directory = CAST(? as blob)".to_string()];
        let dir_str = self.working_directory.to_string_lossy().to_string();
        let mut params: Vec<String> = vec![dir_str];

        // Host filter
        if host_filter == HostFilter::ThisHost {
            where_conditions.push("hostname = CAST(? as blob)".to_string());
            params.push(self.current_hostname.to_string());
        }

        // Search filter
        if let Some(q) = query
            && !q.is_empty()
        {
            where_conditions.push("full_command LIKE '%' || ? || '%' COLLATE NOCASE".to_string());
            params.push(q.to_string());
        }

        let where_clause = format!("WHERE {}", where_conditions.join(" AND "));

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

        let entries: Vec<HistoryEntry> = match params.len() {
            1 => stmt
                .query_map([&params[0]], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?,
            2 => stmt
                .query_map([&params[0], &params[1]], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?,
            3 => stmt
                .query_map([&params[0], &params[1], &params[2]], |row| self.row_to_entry(row))?
                .collect::<Result<Vec<_>, _>>()?,
            _ => unreachable!(),
        };

        Ok(entries)
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
        // We normalize dashes to spaces for scoring so `--release` and `release` rank equally,
        // but use the original query for highlighting so `--release` shows highlighted dashes.
        let normalized_query: String =
            query.chars().map(|c| if c == '-' { ' ' } else { c }).collect();
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
            // Normalize command for scoring (- → space)
            normalized_cmd.clear();
            normalized_cmd.extend(entry.command.chars().map(|c| if c == '-' { ' ' } else { c }));
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
