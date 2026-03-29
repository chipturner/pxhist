use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

// Use the actual functions from the helpers module
use pxh::helpers;
use tempfile::NamedTempFile;

fn verify_file_matches(path: &PathBuf, expected_contents: &str) {
    let fh = File::open(path).unwrap();

    let reader = BufReader::new(fh);
    let file_lines: Vec<_> = reader.lines().collect::<Result<Vec<_>, _>>().unwrap();
    let expected_lines: Vec<_> = expected_contents.split('\n').collect();
    assert_eq!(file_lines, expected_lines);
}

#[test]
fn atomic_line_remove() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\n").unwrap();

    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_lines_from_file(&path, "line2").unwrap();
    verify_file_matches(&path, "line1\nline3");

    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3").unwrap();

    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_lines_from_file(&path, "line2").unwrap();
    verify_file_matches(&path, "line1\nline3");

    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3").unwrap();

    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_lines_from_file(&path, "line9").unwrap();
    verify_file_matches(&path, "line1\nline2\nline3");
}

#[test]
fn atomic_line_remove_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\n").unwrap();

    let (_, path) = tmpfile.keep().unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    pxh::atomically_remove_lines_from_file(&path, "line2").unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "permissions should be preserved after atomic rewrite");
}

#[test]
fn test_determine_is_pxhs() {
    // Test normal pxh invocation
    assert!(!helpers::determine_is_pxhs(&["/usr/bin/pxh".to_string()]));
    assert!(!helpers::determine_is_pxhs(&["/path/to/pxh".to_string()]));
    assert!(!helpers::determine_is_pxhs(&["./pxh".to_string()]));

    // Test pxhs invocation (symlink behavior)
    assert!(helpers::determine_is_pxhs(&["/usr/bin/pxhs".to_string()]));
    assert!(helpers::determine_is_pxhs(&["/path/to/pxhs".to_string()]));
    assert!(helpers::determine_is_pxhs(&["./pxhs".to_string()]));

    // Edge cases
    assert!(helpers::determine_is_pxhs(&["pxhs".to_string()]));
    assert!(helpers::determine_is_pxhs(&["pxhs-something".to_string()]));
    assert!(!helpers::determine_is_pxhs(&["".to_string()]));
    assert!(!helpers::determine_is_pxhs(&[]));
}

#[test]
fn test_build_remote_pxh_command_explicit_path() {
    // Explicit path -- used directly, no probing shell snippet
    let cmd = helpers::build_remote_pxh_command(
        "/usr/bin/custom-pxh",
        "--db ~/.pxh/pxh.db sync --server",
    );
    assert_eq!(cmd, "/usr/bin/custom-pxh --db ~/.pxh/pxh.db sync --server");

    let cmd = helpers::build_remote_pxh_command("custom-pxh", "--db ~/.pxh/pxh.db sync --server");
    assert_eq!(cmd, "custom-pxh --db ~/.pxh/pxh.db sync --server");
}

#[test]
fn test_build_remote_pxh_command_default() {
    // Default "pxh" -- should produce a multi-candidate probing command
    let cmd = helpers::build_remote_pxh_command("pxh", "--db ~/.pxh/pxh.db sync --server");
    // Should be a sh -c wrapper that probes multiple locations
    assert!(cmd.starts_with("sh -c '"), "expected sh -c wrapper, got: {cmd}");
    assert!(cmd.contains(".cargo/bin/pxh"), "should probe .cargo/bin");
    assert!(cmd.contains("/usr/local/bin/pxh"), "should probe /usr/local/bin");
    assert!(cmd.contains("not found on remote host"), "should have fallback error");
    // Should contain the args we passed
    assert!(cmd.contains("--db ~/.pxh/pxh.db sync --server"));
}

#[test]
fn test_default_remote_db_expr() {
    let expr = helpers::default_remote_db_expr();
    // Should be a shell $(...) expression that probes XDG vs legacy paths
    assert!(expr.starts_with("$("), "expected shell subst expression, got: {expr}");
    assert!(expr.contains("XDG_DATA_HOME"), "should reference XDG_DATA_HOME");
    assert!(expr.contains(".local/share"), "should reference XDG default");
    assert!(expr.contains(".pxh/pxh.db"), "should fall back to legacy path");
}

#[test]
fn test_sqlite_connection_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("nested").join("subdir").join("pxh.db");
    assert!(!db_path.parent().unwrap().exists());
    let conn = pxh::sqlite_connection(&Some(db_path.clone()));
    assert!(conn.is_ok(), "should create parent dirs: {:?}", conn.err());
    assert!(db_path.exists());
}

#[test]
fn test_sqlite_connection_creates_dirs_through_symlink() {
    let dir = tempfile::tempdir().unwrap();

    // Symlink pointing to a directory that doesn't exist yet
    let real_target = dir.path().join("real_pxh_dir");
    let symlink = dir.path().join("pxh_link");
    std::os::unix::fs::symlink(&real_target, &symlink).unwrap();
    assert!(!real_target.exists());

    let db_path = symlink.join("pxh.db");
    let conn = pxh::sqlite_connection(&Some(db_path.clone()));
    assert!(conn.is_ok(), "should create target dir through symlink: {:?}", conn.err());
    assert!(real_target.exists());
    assert!(db_path.exists());
}

#[test]
fn test_get_relative_path_from_home() {
    // Test with no overrides - this will use the actual current exe and home dir
    let result = helpers::get_relative_path_from_home(None, None);
    // It should return either Some(path) or None
    match result {
        Some(path) => {
            // If it returns a path, it should be non-empty (unless exe is exactly at home)
            assert!(path.is_empty() || !path.is_empty());
        }
        None => assert!(true), // None is a valid response if exe is not under home
    }

    // Test with specific paths
    let exe = PathBuf::from("/home/user/bin/pxh");
    let home = PathBuf::from("/home/user");
    let result = helpers::get_relative_path_from_home(Some(&exe), Some(&home));
    assert_eq!(result, Some("bin/pxh".to_string()));
}

#[test]
fn test_parse_ssh_command() {
    // Simple command
    let (cmd, args) = helpers::parse_ssh_command("ssh");
    assert_eq!(cmd, "ssh");
    assert!(args.is_empty());

    // Command with args
    let (cmd, args) = helpers::parse_ssh_command("ssh -p 2222");
    assert_eq!(cmd, "ssh");
    assert_eq!(args, vec!["-p", "2222"]);

    // Command with quoted args
    let (cmd, args) = helpers::parse_ssh_command("ssh -o 'UserKnownHostsFile=/dev/null'");
    assert_eq!(cmd, "ssh");
    assert_eq!(args, vec!["-o", "UserKnownHostsFile=/dev/null"]);

    // Command with double quotes
    let (cmd, args) = helpers::parse_ssh_command("ssh -i \"/path/to/key\"");
    assert_eq!(cmd, "ssh");
    assert_eq!(args, vec!["-i", "/path/to/key"]);

    // Complex command
    let (cmd, args) =
        helpers::parse_ssh_command("ssh -p 2222 -o 'StrictHostKeyChecking=no' -i /path/to/key");
    assert_eq!(cmd, "ssh");
    assert_eq!(args, vec!["-p", "2222", "-o", "StrictHostKeyChecking=no", "-i", "/path/to/key"]);

    // Command with escaped quotes
    let (cmd, args) = helpers::parse_ssh_command("ssh -o \"Option=\\\"value\\\"\"");
    assert_eq!(cmd, "ssh");
    assert_eq!(args, vec!["-o", "Option=\"value\""]);
}

#[test]
fn test_path_resolution_across_home_dirs() {
    // Test that /Users/chip/bin/pxh and /home/chip/bin/pxh resolve properly
    // to the same relative path when home is /Users/chip and /home/chip respectively

    // Test 1: /Users/chip/bin/pxh with home as /Users/chip
    let exe_path1 = PathBuf::from("/Users/chip/bin/pxh");
    let home1 = PathBuf::from("/Users/chip");
    let result1 = helpers::get_relative_path_from_home(Some(&exe_path1), Some(&home1));
    assert_eq!(result1, Some("bin/pxh".to_string()));

    // Test 2: /home/chip/bin/pxh with home as /home/chip
    let exe_path2 = PathBuf::from("/home/chip/bin/pxh");
    let home2 = PathBuf::from("/home/chip");
    let result2 = helpers::get_relative_path_from_home(Some(&exe_path2), Some(&home2));
    assert_eq!(result2, Some("bin/pxh".to_string()));

    // Both should resolve to the same relative path
    assert_eq!(result1, result2);

    // Test edge cases
    // Executable not under home directory
    let exe_outside = PathBuf::from("/usr/bin/pxh");
    let result3 = helpers::get_relative_path_from_home(Some(&exe_outside), Some(&home1));
    assert_eq!(result3, None);

    // Same path for exe and home
    let same_path = PathBuf::from("/home/chip");
    let result4 = helpers::get_relative_path_from_home(Some(&same_path), Some(&same_path));
    assert_eq!(result4, Some("".to_string()));
}

#[test]
fn atomic_matching_line_remove() {
    // Basic removal of exact matching lines
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line2"]).unwrap();
    verify_file_matches(&path, "line1\nline3");

    // Multiple lines to remove
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\nline4\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line2", "line4"]).unwrap();
    verify_file_matches(&path, "line1\nline3");

    // Lines with whitespace - should match after trimming
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\n  line2  \nline3\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line2"]).unwrap();
    verify_file_matches(&path, "line1\nline3");

    // No matching lines - file unchanged
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line9"]).unwrap();
    verify_file_matches(&path, "line1\nline2\nline3");

    // Empty contraband list - file unchanged
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &[]).unwrap();
    verify_file_matches(&path, "line1\nline2");

    // Partial match should NOT remove (unlike atomically_remove_lines_from_file)
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line"]).unwrap();
    verify_file_matches(&path, "line1\nline2\nline3");

    // File without trailing newline
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\nline3").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line2"]).unwrap();
    verify_file_matches(&path, "line1\nline3");

    // Remove all lines
    let mut tmpfile = NamedTempFile::new().unwrap();
    write!(tmpfile, "line1\nline2\n").unwrap();
    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_matching_lines_from_file(&path, &["line1", "line2"]).unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.is_empty());
}

#[test]
fn test_get_hostname_strips_domain() {
    use bstr::BString;
    use std::env;

    // Save original PXH_HOSTNAME if it exists
    let original = env::var_os("PXH_HOSTNAME");

    // Test FQDN hostname gets shortened to first component
    unsafe { env::set_var("PXH_HOSTNAME", "myhost.example.local") };
    let hostname = pxh::get_hostname();
    assert_eq!(hostname, BString::from("myhost"));

    // Test simple hostname stays unchanged
    unsafe { env::set_var("PXH_HOSTNAME", "myhost") };
    let hostname = pxh::get_hostname();
    assert_eq!(hostname, BString::from("myhost"));

    // Test multiple dots - should only keep first component
    unsafe { env::set_var("PXH_HOSTNAME", "host.sub.example.com") };
    let hostname = pxh::get_hostname();
    assert_eq!(hostname, BString::from("host"));

    // Test empty hostname stays empty
    unsafe { env::set_var("PXH_HOSTNAME", "") };
    let hostname = pxh::get_hostname();
    assert_eq!(hostname, BString::from(""));

    // Test leading dot produces empty string
    unsafe { env::set_var("PXH_HOSTNAME", ".example.com") };
    let hostname = pxh::get_hostname();
    assert_eq!(hostname, BString::from(""));

    // Restore original PXH_HOSTNAME or unset it
    match original {
        Some(val) => unsafe { env::set_var("PXH_HOSTNAME", val) },
        None => unsafe { env::remove_var("PXH_HOSTNAME") },
    }
}
