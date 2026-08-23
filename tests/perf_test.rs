//! Recall-latency guard: the interactive hot paths must cost O(window), not
//! O(table). Rather than absolute thresholds (which depend on the machine),
//! each path is timed against a 50k-row and a 500k-row database and must not
//! grow with the table. An O(table) regression shows up as roughly 10x.
//!
//! Ignored by default (builds two large databases, needs `--release`); run
//! with `just perf` or the CI perf job.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use pxh::test_utils::PxhTestHelper;
use rusqlite::Connection;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SMALL: i64 = 50_000;
const LARGE: i64 = 500_000;
const HOST: &str = "perfhost";
/// Generous fixed slack so microsecond-scale noise can never trip the ratio.
const SLACK: Duration = Duration::from_millis(25);
const RUNS: usize = 5;

fn build_db(helper: &PxhTestHelper, rows: i64) -> Result<PathBuf> {
    let path = helper.home_dir().join(format!("perf-{rows}.db"));
    // Let pxh create the schema, then bulk-fill with realistic variety.
    let status = helper
        .command_with_args(&[
            "--db",
            path.to_str().unwrap(),
            "insert",
            "--shellname",
            "zsh",
            "--hostname",
            HOST,
            "--username",
            "u",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            "1600000000",
            "seed",
        ])
        .status()?;
    assert!(status.success());
    let conn = Connection::open(&path)?;
    conn.execute_batch(&format!(
        r#"
        WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < {rows})
        INSERT INTO command_history (session_id, full_command, shellname, hostname, username,
                                     working_directory, exit_status, start_unix_timestamp,
                                     end_unix_timestamp)
        SELECT n % 3000,
               CAST(printf('cmd%d git commit -m "msg %d" && cargo build # %d', n % 5000, n, n % 97) AS BLOB),
               'zsh',
               CAST(CASE WHEN n % 10 = 0 THEN 'otherhost' ELSE '{HOST}' END AS BLOB),
               CAST('u' AS BLOB),
               CAST(printf('/home/u/proj%d', n % 200) AS BLOB),
               n % 7 = 0,
               1600000000 + n * 200,
               1600000000 + n * 200 + n % 30
        FROM seq;
        "#
    ))?;
    Ok(path)
}

/// Parse a `Duration`'s Debug rendering (`88.8µs`, `13.7ms`, `1.2s`).
fn parse_duration(s: &str) -> Duration {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap();
    let (num, unit) = s.split_at(split);
    let v: f64 = num.parse().unwrap();
    match unit {
        "ns" => Duration::from_secs_f64(v / 1e9),
        "µs" | "us" => Duration::from_secs_f64(v / 1e6),
        "ms" => Duration::from_secs_f64(v / 1e3),
        "s" => Duration::from_secs_f64(v),
        _ => panic!("unknown duration unit in {s:?}"),
    }
}

fn timing_field(output: &str, field: &str) -> Duration {
    let line = output
        .lines()
        .find(|l| l.trim_start().starts_with(field))
        .unwrap_or_else(|| panic!("no {field:?} in timing output: {output}"));
    parse_duration(line.split_once(':').unwrap().1)
}

/// Minimum over several runs: we want the cost of the work, not scheduler noise.
fn min_of<F: FnMut() -> Result<Duration>>(mut f: F) -> Result<Duration> {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        best = best.min(f()?);
    }
    Ok(best)
}

fn recall_print_query_time(helper: &PxhTestHelper, db: &Path, extra: &[&str]) -> Result<Duration> {
    min_of(|| {
        let mut args = vec!["--db", db.to_str().unwrap(), "recall", "--print", "--timing"];
        args.extend_from_slice(extra);
        let out = helper.command_with_args(&args).env("PWD", "/home/u/proj7").output()?;
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Ok(timing_field(&String::from_utf8_lossy(&out.stderr), "DB query"))
    })
}

fn tui_init_time(helper: &PxhTestHelper, db: &Path, extra: &[&str]) -> Result<Duration> {
    min_of(|| {
        let mut args =
            vec!["--db", db.to_str().unwrap(), "recall", "--paint-then-exit", "--timing"];
        args.extend_from_slice(extra);
        let mut session =
            rexpect::session::spawn_command(helper.command_with_args(&args), Some(30_000))?;
        let out = session.exp_eof()?;
        Ok(timing_field(&out, "TUI init"))
    })
}

fn wall_time(helper: &PxhTestHelper, args: &[&str]) -> Result<Duration> {
    min_of(|| {
        let start = Instant::now();
        let out = helper.command_with_args(args).output()?;
        let elapsed = start.elapsed();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Ok(elapsed)
    })
}

fn assert_flat(label: &str, small: Duration, large: Duration) {
    let limit = small * 3 + SLACK;
    eprintln!("{label:<40} {SMALL} rows: {small:>10.2?}   {LARGE} rows: {large:>10.2?}");
    assert!(
        large <= limit,
        "{label}: {large:?} at {LARGE} rows vs {small:?} at {SMALL} rows -- scales with table size (limit {limit:?})"
    );
}

#[test]
#[ignore = "perf guard: builds 550k rows; run via `just perf`"]
fn hot_paths_do_not_scale_with_table_size() -> Result<()> {
    let helper = PxhTestHelper::new();
    let small = build_db(&helper, SMALL)?;
    let large = build_db(&helper, LARGE)?;
    let (s, l) = (small.as_path(), large.as_path());
    let db = |p: &Path| p.to_str().unwrap().to_string();

    assert_flat(
        "recall global load (DB query)",
        recall_print_query_time(&helper, s, &[])?,
        recall_print_query_time(&helper, l, &[])?,
    );
    assert_flat(
        "recall directory load (DB query)",
        recall_print_query_time(&helper, s, &["--here"])?,
        recall_print_query_time(&helper, l, &["--here"])?,
    );
    assert_flat(
        "recall prefiltered load (DB query)",
        recall_print_query_time(&helper, s, &["-q", "cargo build"])?,
        recall_print_query_time(&helper, l, &["-q", "cargo build"])?,
    );
    assert_flat(
        "recall TUI init (load + dedup)",
        tui_init_time(&helper, s, &[])?,
        tui_init_time(&helper, l, &[])?,
    );
    assert_flat(
        "insert (wall)",
        wall_time(
            &helper,
            &[
                "--db",
                &db(s),
                "insert",
                "--shellname",
                "zsh",
                "--hostname",
                HOST,
                "--username",
                "u",
                "--session-id",
                "77",
                "--start-unix-timestamp",
                "1700000001",
                "perf insert",
            ],
        )?,
        wall_time(
            &helper,
            &[
                "--db",
                &db(l),
                "insert",
                "--shellname",
                "zsh",
                "--hostname",
                HOST,
                "--username",
                "u",
                "--session-id",
                "77",
                "--start-unix-timestamp",
                "1700000001",
                "perf insert",
            ],
        )?,
    );
    assert_flat(
        "seal (wall)",
        wall_time(
            &helper,
            &[
                "--db",
                &db(s),
                "seal",
                "--session-id",
                "77",
                "--exit-status",
                "0",
                "--end-unix-timestamp",
                "1700000002",
            ],
        )?,
        wall_time(
            &helper,
            &[
                "--db",
                &db(l),
                "seal",
                "--session-id",
                "77",
                "--exit-status",
                "0",
                "--end-unix-timestamp",
                "1700000002",
            ],
        )?,
    );
    assert_flat(
        "autosuggest hit (wall)",
        wall_time(&helper, &["--db", &db(s), "autosuggest", "--", "cmd42 git"])?,
        wall_time(&helper, &["--db", &db(l), "autosuggest", "--", "cmd42 git"])?,
    );
    Ok(())
}

/// Sanity check that the harness can see table size at all: a query matching
/// nothing forces the prefilter to walk every row, and that *should* scale.
#[test]
#[ignore = "perf guard: builds 550k rows; run via `just perf`"]
fn full_table_walk_is_visible_to_the_harness() -> Result<()> {
    let helper = PxhTestHelper::new();
    let small = build_db(&helper, SMALL)?;
    let large = build_db(&helper, LARGE)?;
    let s = recall_print_query_time(&helper, &small, &["-q", "zzznomatch"])?;
    let l = recall_print_query_time(&helper, &large, &["-q", "zzznomatch"])?;
    eprintln!("{:<40} {SMALL} rows: {s:>10.2?}   {LARGE} rows: {l:>10.2?}", "full walk (no match)");
    assert!(l > s * 3, "expected the no-match walk to scale with rows: {s:?} -> {l:?}");
    Ok(())
}
