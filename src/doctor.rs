use std::path::PathBuf;

use clap::Parser;
use rusqlite::Connection;

#[derive(Parser, Debug)]
pub struct DoctorCommand {
    #[clap(long, help = "Attempt to fix issues automatically (prompts first)")]
    pub fix: bool,
    #[clap(long, help = "Output a GitHub-issue-ready diagnostic report")]
    pub report: bool,
    #[clap(short, long, help = "Show all checks, including passing ones")]
    pub verbose: bool,
    #[clap(short = 'y', long, help = "Skip confirmation prompts for --fix")]
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub label: String,
    pub status: Status,
    pub message: Option<String>,
    pub fixable: bool,
}

impl CheckResult {
    fn ok(label: impl Into<String>) -> Self {
        Self { label: label.into(), status: Status::Ok, message: None, fixable: false }
    }

    fn warn(label: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Warn,
            message: Some(msg.into()),
            fixable: false,
        }
    }

    fn fail(label: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Fail,
            message: Some(msg.into()),
            fixable: false,
        }
    }

    fn with_fix(mut self) -> Self {
        self.fixable = true;
        self
    }
}

/// Current expected schema version -- must match run_schema_migrations in lib.rs.
const CURRENT_SCHEMA_VERSION: i32 = 1;

impl DoctorCommand {
    pub fn go(
        &self,
        conn: Option<Connection>,
        db_path: &Option<PathBuf>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Run migrate_host_settings (idempotent)
        if let Some(c) = &conn {
            pxh::migrate_host_settings(c);
        }

        let all_checks: Vec<(&str, Vec<CheckResult>)> = vec![
            ("Binary & Version", self.check_binary()),
            ("Database", self.check_database(&conn, db_path)),
            ("Shell Integration", self.check_shell_integration(db_path)),
            ("Config", self.check_config()),
            ("Path", self.check_path_ambiguity()),
            ("Secrets", self.check_secrets(&conn)),
        ];

        if self.report {
            self.print_report(&all_checks, db_path, &conn);
        } else {
            self.print_human(&all_checks);
        }

        if self.fix {
            self.run_fixes(&all_checks, &conn, db_path)?;
        }

        let has_fail =
            all_checks.iter().any(|(_, checks)| checks.iter().any(|c| c.status == Status::Fail));
        if has_fail {
            std::process::exit(1);
        }

        Ok(())
    }

    // ── Checks ──────────────────────────────────────────────────────────

    fn check_binary(&self) -> Vec<CheckResult> {
        let mut results = Vec::new();

        results.push(CheckResult::ok(format!(
            "pxh {} (SQLite {}, schema v{CURRENT_SCHEMA_VERSION})",
            env!("CARGO_PKG_VERSION"),
            rusqlite::version(),
        )));

        if let Ok(current_exe) = std::env::current_exe() {
            results.push(CheckResult::ok(format!("Binary: {}", current_exe.display())));

            // Check if the PATH version matches the running binary
            if let Some(path_exe) = Self::which_pxh() {
                let current_canon =
                    std::fs::canonicalize(&current_exe).unwrap_or(current_exe.clone());
                let path_canon = std::fs::canonicalize(&path_exe).unwrap_or(path_exe.clone());
                if current_canon != path_canon {
                    results.push(CheckResult::warn(
                        format!("PATH has different pxh: {}", path_exe.display()),
                        "You may have a stale install; the running binary differs from the one on PATH",
                    ));
                }
            }
        }

        results
    }

    fn check_database(
        &self,
        conn: &Option<Connection>,
        db_path: &Option<PathBuf>,
    ) -> Vec<CheckResult> {
        let mut results = Vec::new();

        let Some(path) = db_path else {
            results.push(CheckResult::fail(
                "No database path configured",
                "Set PXH_DB_PATH or use --db",
            ));
            return results;
        };

        if path.to_string_lossy() == ":memory:" {
            results.push(CheckResult::ok("Database: in-memory"));
            return results;
        }

        if !path.exists() {
            results.push(CheckResult::fail(
                format!("Database not found: {}", path.display()),
                "Run any pxh command to create it, or check PXH_DB_PATH",
            ));
            return results;
        }

        // Size and row count
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let size_str = if size > 1_000_000 {
            format!("{:.1} MB", size as f64 / 1_000_000.0)
        } else {
            format!("{:.0} KB", size as f64 / 1_000.0)
        };
        let row_count = conn.as_ref().and_then(|c| {
            c.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get::<_, i64>(0)).ok()
        });
        results.push(CheckResult::ok(format!(
            "Database: {} ({}, {} commands)",
            path.display(),
            size_str,
            row_count.unwrap_or(0),
        )));

        // Permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(path) {
                let mode = meta.permissions().mode() & 0o777;
                if mode == 0o600 {
                    results.push(CheckResult::ok("Permissions 0600"));
                } else {
                    results.push(
                        CheckResult::warn(
                            format!("Permissions {mode:04o} (should be 0600)"),
                            "Database is readable by other users",
                        )
                        .with_fix(),
                    );
                }
            }
        }

        if let Some(c) = conn {
            // Schema version
            let version: i32 =
                c.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap_or(0);
            if version >= CURRENT_SCHEMA_VERSION {
                results.push(CheckResult::ok(format!("Schema version {version} (current)")));
            } else {
                results.push(
                    CheckResult::warn(
                        format!("Schema version {version} (expected {CURRENT_SCHEMA_VERSION})"),
                        "Run migrations to update",
                    )
                    .with_fix(),
                );
            }

            // WAL mode
            let journal: String =
                c.pragma_query_value(None, "journal_mode", |row| row.get(0)).unwrap_or_default();
            if journal == "wal" {
                results.push(CheckResult::ok("WAL mode enabled"));
            } else {
                results.push(CheckResult::warn(
                    format!("Journal mode is '{journal}' (expected 'wal')"),
                    "WAL mode provides better concurrent access",
                ));
            }

            // Staleness
            let last_ts: Option<i64> = c
                .query_row("SELECT MAX(start_unix_timestamp) FROM command_history", [], |r| {
                    r.get(0)
                })
                .unwrap_or(None);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            match last_ts {
                Some(ts) if now - ts > 3600 => {
                    let days = (now - ts) / 86400;
                    let msg = if days > 0 {
                        format!(
                            "Last command recorded {days} day{} ago -- shell hooks may not be active",
                            if days == 1 { "" } else { "s" }
                        )
                    } else {
                        let hours = (now - ts) / 3600;
                        format!(
                            "Last command recorded {hours} hour{} ago -- shell hooks may not be active",
                            if hours == 1 { "" } else { "s" }
                        )
                    };
                    results.push(CheckResult::warn(
                        msg,
                        "Check that your shell rc file sources pxh shell-config",
                    ));
                }
                Some(_) => {
                    results.push(CheckResult::ok("Shell hooks active (recent commands recorded)"));
                }
                None => {
                    results.push(CheckResult::warn(
                        "No commands recorded yet",
                        "Run 'pxh install <shell>' to set up shell hooks",
                    ));
                }
            }
        } else {
            results.push(CheckResult::fail(
                "Could not open database",
                "Check file permissions and path",
            ));
        }

        results
    }

    fn check_shell_integration(&self, db_path: &Option<PathBuf>) -> Vec<CheckResult> {
        let mut results = Vec::new();

        let shell = std::env::var("SHELL").unwrap_or_default();
        let shell_name =
            std::path::Path::new(&shell).file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        results.push(CheckResult::ok(format!("Current shell: {shell_name}")));

        let rc_file = match shell_name {
            "zsh" => Some(".zshrc"),
            "bash" => Some(".bashrc"),
            _ => None,
        };

        if let (Some(rc), Some(home)) = (rc_file, home::home_dir()) {
            let rc_path = home.join(rc);
            if rc_path.exists() {
                let contents = std::fs::read_to_string(&rc_path).unwrap_or_default();
                if contents.contains("pxh shell-config") {
                    results.push(CheckResult::ok(format!("~/{rc} contains pxh shell-config")));
                } else {
                    results.push(
                        CheckResult::warn(
                            format!("~/{rc} does not contain pxh shell-config"),
                            format!("Run: pxh install {shell_name}"),
                        )
                        .with_fix(),
                    );
                }
            } else {
                results.push(CheckResult::warn(
                    format!("~/{rc} not found"),
                    format!("Create it and run: pxh install {shell_name}"),
                ));
            }
        }

        if std::env::var("PXH_SESSION_ID").is_ok() {
            results.push(CheckResult::ok("PXH_SESSION_ID is set (hooks active in this session)"));
        } else {
            results.push(CheckResult::warn(
                "PXH_SESSION_ID not set",
                "Shell hooks may not be active; run 'source <(pxh shell-config <shell>)'",
            ));
        }

        if let (Ok(env_path), Some(actual)) = (std::env::var("PXH_DB_PATH"), db_path) {
            let env_pb = PathBuf::from(&env_path);
            if env_pb != *actual {
                results.push(CheckResult::warn(
                    format!(
                        "PXH_DB_PATH ({}) differs from actual DB ({})",
                        env_pb.display(),
                        actual.display()
                    ),
                    "Commands may be recorded to different databases",
                ));
            }
        }

        results
    }

    fn check_config(&self) -> Vec<CheckResult> {
        let mut results = Vec::new();

        let config = pxh::recall::config::Config::load();
        let config_dir = pxh::pxh_config_dir();

        if let Some(dir) = &config_dir {
            let config_path = dir.join("config.toml");
            if config_path.exists() {
                let contents = std::fs::read_to_string(&config_path).unwrap_or_default();
                if contents.parse::<toml_edit::DocumentMut>().is_ok() {
                    results.push(CheckResult::ok(format!(
                        "Config: {} (valid TOML)",
                        config_path.display()
                    )));
                } else {
                    results.push(CheckResult::fail(
                        format!("Config: {} (invalid TOML)", config_path.display()),
                        "Edit or delete the config file to fix parse errors",
                    ));
                }
            } else {
                results.push(CheckResult::ok(format!(
                    "Config: {} (not yet created -- defaults used)",
                    config_path.display()
                )));
            }
        }

        if config.host.machine_id.is_some() {
            results.push(CheckResult::ok("machine_id present"));
        } else {
            results.push(
                CheckResult::warn(
                    "No machine_id in config",
                    "Will be generated on next install/config run",
                )
                .with_fix(),
            );
        }

        if config.host.hostname.is_some() {
            results.push(CheckResult::ok("hostname present in config"));
        } else {
            results.push(CheckResult::warn(
                "No hostname set in config",
                "Will be detected live (may vary on DHCP)",
            ));
        }

        // Ambiguous config dirs
        if let Some(home) = home::home_dir() {
            let legacy_config = home.join(".pxh").join("config.toml");
            let xdg_config = config_dir.as_ref().map(|d| d.join("config.toml"));
            if legacy_config.exists()
                && xdg_config.as_ref().is_some_and(|p| p.exists() && *p != legacy_config)
            {
                results.push(CheckResult::warn(
                    "Both legacy ~/.pxh/config.toml and XDG config exist",
                    "Remove the one you don't want to avoid confusion",
                ));
            }
        }

        results
    }

    fn check_path_ambiguity(&self) -> Vec<CheckResult> {
        let mut results = Vec::new();

        let home = match home::home_dir() {
            Some(h) => h,
            None => return results,
        };

        let legacy_db = home.join(".pxh").join("pxh.db");
        let xdg_data = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local").join("share"));
        let xdg_db = xdg_data.join("pxh").join("pxh.db");

        if legacy_db.exists() && xdg_db.exists() && legacy_db != xdg_db {
            let legacy_size = std::fs::metadata(&legacy_db).map(|m| m.len()).unwrap_or(0);
            let xdg_size = std::fs::metadata(&xdg_db).map(|m| m.len()).unwrap_or(0);

            let legacy_rows = Connection::open(&legacy_db)
                .and_then(|c| {
                    c.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get::<_, i64>(0))
                })
                .unwrap_or(0);
            let xdg_rows = Connection::open(&xdg_db)
                .and_then(|c| {
                    c.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get::<_, i64>(0))
                })
                .unwrap_or(0);

            results.push(
                CheckResult::warn(
                    "Both legacy and XDG databases exist",
                    format!(
                        "Legacy ~/.pxh/pxh.db ({:.1} MB, {} rows) and {} ({:.1} MB, {} rows)",
                        legacy_size as f64 / 1_000_000.0,
                        legacy_rows,
                        xdg_db.display(),
                        xdg_size as f64 / 1_000_000.0,
                        xdg_rows,
                    ),
                )
                .with_fix(),
            );
        }

        results
    }

    fn check_secrets(&self, conn: &Option<Connection>) -> Vec<CheckResult> {
        let Some(c) = conn else {
            return vec![];
        };

        let Ok((patterns, regex_set)) = crate::build_secret_patterns("critical") else {
            return vec![];
        };

        let mut matches = Vec::new();
        if crate::scan_database(c, &regex_set, &patterns, &mut matches, 100).is_err() {
            return vec![];
        }

        if matches.is_empty() {
            vec![CheckResult::ok("No secrets detected at critical confidence")]
        } else {
            vec![CheckResult::warn(
                format!(
                    "{} potential secret{} found in history",
                    matches.len(),
                    if matches.len() == 1 { "" } else { "s" }
                ),
                "Run 'pxh scan' for details, 'pxh scrub --scan' to remove",
            )]
        }
    }

    // ── Output ──────────────────────────────────────────────────────────

    fn print_human(&self, sections: &[(&str, Vec<CheckResult>)]) {
        for (header, checks) in sections {
            // Skip empty sections in non-verbose mode if all ok
            if !self.verbose && checks.iter().all(|c| c.status == Status::Ok) {
                continue;
            }
            println!("{header}:");
            for check in checks {
                match check.status {
                    Status::Ok => {
                        if self.verbose {
                            println!("  ok  {}", check.label);
                        }
                    }
                    Status::Warn => {
                        println!("  !!  {}", check.label);
                        if let Some(ref msg) = check.message {
                            println!("      {msg}");
                        }
                        if check.fixable && !self.fix {
                            println!("      -> Run: pxh doctor --fix");
                        }
                    }
                    Status::Fail => {
                        println!("  XX  {}", check.label);
                        if let Some(ref msg) = check.message {
                            println!("      {msg}");
                        }
                        if check.fixable && !self.fix {
                            println!("      -> Run: pxh doctor --fix");
                        }
                    }
                }
            }
        }

        let issues: usize = sections
            .iter()
            .flat_map(|(_, checks)| checks.iter())
            .filter(|c| c.status != Status::Ok)
            .count();
        if issues == 0 {
            println!("\nNo issues found.");
        } else {
            println!("\n{issues} issue{} found.", if issues == 1 { "" } else { "s" });
        }
    }

    fn print_report(
        &self,
        sections: &[(&str, Vec<CheckResult>)],
        db_path: &Option<PathBuf>,
        conn: &Option<Connection>,
    ) {
        println!("<details>");
        println!("<summary>pxh doctor report</summary>\n");
        println!("```");
        println!("pxh version:     {}", env!("CARGO_PKG_VERSION"));

        let os_info = Self::get_os_info();
        println!("OS:              {os_info}");

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "(unknown)".to_string());
        println!("Shell:           {shell}");

        println!("SQLite:          {} (bundled)", rusqlite::version());

        let schema_ver = conn
            .as_ref()
            .map(|c| {
                c.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0)).unwrap_or(-1)
            })
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(no db)".to_string());
        println!("Schema version:  {schema_ver}");

        if let Some(path) = db_path {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let rows = conn.as_ref().and_then(|c| {
                c.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get::<_, i64>(0)).ok()
            });
            let size_str = if size > 1_000_000 {
                format!("{:.1} MB", size as f64 / 1_000_000.0)
            } else {
                format!("{:.0} KB", size as f64 / 1_000.0)
            };
            println!(
                "Database:        {} ({}, {} rows)",
                path.display(),
                size_str,
                rows.unwrap_or(0)
            );
        } else {
            println!("Database:        (not configured)");
        }

        let config_path = pxh::pxh_config_dir().map(|d| d.join("config.toml"));
        let config_valid = config_path
            .as_ref()
            .map(|p| if p.exists() { "valid" } else { "not found" })
            .unwrap_or("(no config dir)");
        println!(
            "Config:          {} ({config_valid})",
            config_path.map(|p| p.display().to_string()).unwrap_or_else(|| "(unknown)".to_string())
        );

        let db_env = std::env::var("PXH_DB_PATH").unwrap_or_else(|_| "(not set)".to_string());
        println!("PXH_DB_PATH:     {db_env}");

        if let Some(c) = conn {
            let last_ts: Option<i64> = c
                .query_row("SELECT MAX(start_unix_timestamp) FROM command_history", [], |r| {
                    r.get(0)
                })
                .unwrap_or(None);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let hooks_str = match last_ts {
                Some(ts) if now - ts < 3600 => {
                    let ago = now - ts;
                    format!("yes (last command {ago}s ago)")
                }
                Some(ts) => {
                    let days = (now - ts) / 86400;
                    format!("possibly not (last command {days}d ago)")
                }
                None => "no history recorded".to_string(),
            };
            println!("Hooks active:    {hooks_str}");
        }

        let legacy = home::home_dir().map(|h| h.join(".pxh"));
        let legacy_str = match legacy {
            Some(ref p) if p.exists() => "present",
            _ => "not present",
        };
        println!("Legacy ~/.pxh:   {legacy_str}");

        let issues: Vec<&CheckResult> = sections
            .iter()
            .flat_map(|(_, checks)| checks.iter())
            .filter(|c| c.status != Status::Ok)
            .collect();
        if issues.is_empty() {
            println!("\nIssues: none");
        } else {
            println!("\nIssues:");
            for issue in &issues {
                let prefix = match issue.status {
                    Status::Warn => "!!",
                    Status::Fail => "XX",
                    Status::Ok => "ok",
                };
                println!("  {prefix}  {}", issue.label);
            }
        }

        println!("```\n");
        println!("</details>");
    }

    // ── Fixes ───────────────────────────────────────────────────────────

    fn run_fixes(
        &self,
        sections: &[(&str, Vec<CheckResult>)],
        conn: &Option<Connection>,
        db_path: &Option<PathBuf>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fixable: Vec<&CheckResult> = sections
            .iter()
            .flat_map(|(_, checks)| checks.iter())
            .filter(|c| c.fixable && c.status != Status::Ok)
            .collect();

        if fixable.is_empty() {
            return Ok(());
        }

        println!("\n--- Fixes ---");

        for check in &fixable {
            let label = &check.label;

            if label.contains("Permissions")
                && label.contains("should be 0600")
                && let Some(path) = db_path
            {
                let path = path.clone();
                self.apply_fix("Fix database permissions to 0600", || {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                    }
                    Ok(())
                })?;
            }

            if label.contains("Schema version")
                && label.contains("expected")
                && let Some(c) = conn
            {
                self.apply_fix("Run schema migrations", || {
                    pxh::run_schema_migrations(c)?;
                    Ok(())
                })?;
            }

            if label.contains("machine_id")
                && let Some(c) = conn
            {
                self.apply_fix("Generate machine_id and migrate host settings", || {
                    pxh::migrate_host_settings(c);
                    Ok(())
                })?;
            }

            if label.contains("does not contain pxh shell-config") {
                let shell = std::env::var("SHELL").unwrap_or_default();
                let shell_name = std::path::Path::new(&shell)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("bash")
                    .to_string();
                self.apply_fix_with_prompt(
                    &format!("Install pxh shell hooks into ~/.{shell_name}rc"),
                    || {
                        let status = std::process::Command::new(std::env::current_exe()?)
                            .args(["install", &shell_name])
                            .status()?;
                        if status.success() { Ok(()) } else { Err("pxh install failed".into()) }
                    },
                )?;
            }

            if label.contains("Both legacy and XDG databases exist") {
                self.apply_fix_with_prompt(
                    "Merge legacy ~/.pxh/pxh.db into XDG database and back up ~/.pxh",
                    || {
                        let home = home::home_dir().ok_or("Cannot determine home directory")?;
                        let legacy_db = home.join(".pxh").join("pxh.db");
                        let xdg_data = std::env::var("XDG_DATA_HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| home.join(".local").join("share"));
                        let xdg_db = xdg_data.join("pxh").join("pxh.db");

                        let mut xdg_conn = Connection::open(&xdg_db)?;
                        let tx = xdg_conn.transaction()?;
                        tx.execute(
                            "ATTACH DATABASE ? AS legacy",
                            [legacy_db.to_str().ok_or("Invalid path")?],
                        )?;
                        tx.execute(
                            r#"INSERT OR IGNORE INTO main.command_history
                               (session_id, full_command, shellname, hostname, username,
                                working_directory, exit_status, start_unix_timestamp, end_unix_timestamp)
                               SELECT session_id, full_command, shellname, hostname, username,
                                      working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
                               FROM legacy.command_history"#,
                            [],
                        )?;
                        let added: i64 = tx.query_row("SELECT changes()", [], |r| r.get(0))?;
                        tx.execute("DETACH DATABASE legacy", [])?;
                        tx.commit()?;

                        let backup = home.join(".pxh.backup");
                        std::fs::rename(home.join(".pxh"), &backup)?;

                        println!(
                            "    Merged {added} new commands. Legacy dir moved to ~/.pxh.backup"
                        );
                        Ok(())
                    },
                )?;
            }
        }

        Ok(())
    }

    fn apply_fix(
        &self,
        description: &str,
        action: impl FnOnce() -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        print!("  Fixing: {description}...");
        match action() {
            Ok(()) => {
                println!(" done.");
                Ok(())
            }
            Err(e) => {
                println!(" FAILED: {e}");
                Ok(()) // Don't abort other fixes
            }
        }
    }

    fn apply_fix_with_prompt(
        &self,
        description: &str,
        action: impl FnOnce() -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.yes {
            print!("  {description}? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut input = String::new();
            std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("  Skipped.");
                return Ok(());
            }
        }
        self.apply_fix(description, action)
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn get_os_info() -> String {
        let arch = std::env::consts::ARCH;
        #[cfg(target_os = "linux")]
        {
            if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
                for line in contents.lines() {
                    if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
                        let name = name.trim_matches('"');
                        return format!("{name} ({arch})");
                    }
                }
            }
            format!("Linux ({arch})")
        }
        #[cfg(target_os = "macos")]
        {
            let version = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();
            format!("macOS {} ({arch})", version.trim())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            format!("{} ({arch})", std::env::consts::OS)
        }
    }

    fn which_pxh() -> Option<PathBuf> {
        std::process::Command::new("which")
            .arg("pxh")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| PathBuf::from(s.trim()))
    }
}
