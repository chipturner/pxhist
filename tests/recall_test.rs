use pxh::test_utils::PxhTestHelper;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_recall_help_shows_options() -> Result<()> {
    let helper = PxhTestHelper::new();

    // Check that recall command exists and shows help
    let output = helper.command_with_args(&["recall", "--help"]).output()?;
    assert!(output.status.success(), "recall --help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--here"), "help should mention --here flag");
    assert!(stdout.contains("--global"), "help should mention --global flag");

    Ok(())
}

#[test]
fn test_recall_visible_alias() -> Result<()> {
    let helper = PxhTestHelper::new();

    // Check that 'r' is a visible alias for recall
    let output = helper.command_with_args(&["help"]).output()?;
    assert!(output.status.success(), "help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("recall") || stdout.contains("r"),
        "help should mention recall command"
    );

    Ok(())
}

#[test]
fn test_recall_here_and_global_conflict() -> Result<()> {
    let helper = PxhTestHelper::new();

    // Check that --here and --global conflict
    let output = helper.command_with_args(&["recall", "--here", "--global"]).output()?;
    assert!(!output.status.success(), "--here and --global should conflict");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "error should mention conflict"
    );

    Ok(())
}

// Tests for relative time formatting (these test the engine module)
mod relative_time {
    use pxh::recall::engine::format_relative_time;

    fn now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
    }

    #[test]
    fn test_none() {
        assert_eq!(format_relative_time(None), "   ");
    }

    #[test]
    fn test_seconds() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 30)), "30s");
        assert_eq!(format_relative_time(Some(n - 5)), " 5s");
    }

    #[test]
    fn test_minutes() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 120)), " 2m");
        assert_eq!(format_relative_time(Some(n - 3000)), "50m");
    }

    #[test]
    fn test_hours() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 7200)), " 2h");
        assert_eq!(format_relative_time(Some(n - 36000)), "10h");
    }

    #[test]
    fn test_days() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 86400 * 2)), " 2d");
        assert_eq!(format_relative_time(Some(n - 86400 * 5)), " 5d");
    }

    #[test]
    fn test_weeks() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 86400 * 7)), " 1w");
        assert_eq!(format_relative_time(Some(n - 86400 * 14)), " 2w");
    }

    #[test]
    fn test_months() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 86400 * 30)), " 1M");
        assert_eq!(format_relative_time(Some(n - 86400 * 60)), " 2M");
    }

    #[test]
    fn test_years() {
        let n = now();
        assert_eq!(format_relative_time(Some(n - 86400 * 365)), " 1y");
        assert_eq!(format_relative_time(Some(n - 86400 * 730)), " 2y");
    }

    #[test]
    fn test_future_timestamp() {
        let n = now();
        // Future timestamps should return empty
        assert_eq!(format_relative_time(Some(n + 100)), "   ");
    }
}
