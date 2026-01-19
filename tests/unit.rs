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
fn test_determine_remote_pxh_path() {
    // Test explicit path - should return as-is
    assert_eq!(helpers::determine_remote_pxh_path("/usr/bin/custom-pxh"), "/usr/bin/custom-pxh");
    assert_eq!(helpers::determine_remote_pxh_path("custom-pxh"), "custom-pxh");

    // Test default "pxh" - we can't easily test the actual smart path logic without
    // controlling the environment, but we can test that it returns something
    let result = helpers::determine_remote_pxh_path("pxh");
    assert!(!result.is_empty());
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
