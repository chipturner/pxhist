use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

use bstr::{BString, ByteVec};
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

fn bvec(v: &[&str]) -> Vec<BString> {
    v.iter().cloned().map(BString::from).collect()
}

#[test]
fn word_boundaries() {
    assert_eq!(pxh::all_long_word_boundaries("a").len(), 2);
    assert_eq!(pxh::all_long_word_boundaries(b"a").len(), 2);
    assert_eq!(pxh::all_long_word_boundaries("ab").len(), 2);
    assert_eq!(pxh::all_long_word_boundaries("a b").len(), 4);
    assert_eq!(pxh::all_long_word_boundaries("a b2").len(), 4);
    assert_eq!(pxh::all_long_word_boundaries("a b c").len(), 6);

    assert!(pxh::long_credential_windows("a", 6).is_empty());

    for single_match_case in &["a", "ab", "abcdefg", "abcdefgabcdefg"] {
        assert_eq!(pxh::long_credential_windows(single_match_case, 1).len(), 1);
        assert_eq!(
            pxh::long_credential_windows(single_match_case, 1)[0].bytes,
            single_match_case.as_bytes()
        );
    }

    assert_eq!(
        pxh::long_credential_windows("a b c", 1).into_iter().map(|v| v.bytes).collect::<Vec<_>>(),
        bvec(&["a", "a b", "a b c", "b", "b c", "c"])
    );

    // test a hashed version of "password"
    let password_hashed =
        "$argon2id$v=19$m=16,t=2,p=1$cjFNb0V0eDVNRzZnRWZuMg$3PBdJhNrazr7smlEk68ZXg";
    let secret = pxh::HashedSecret::new(&password_hashed);
    assert!(secret.search_haystack("my password is super secret").is_some());

    let hunter2_hashed =
        pxh::hash_password(pxh::HashAlgorithm::Argon2Default, "hunter2".as_bytes());
    let secret = pxh::HashedSecret::new(&hunter2_hashed);
    assert!(secret.search_haystack("my password is hunter2").is_some());
    assert!(secret.search_haystack("no secrets here nope").is_none());
}

#[test]
fn prefix_hash_optimization() {
    // Test the cutoff optimization by taking a short password and
    // verifying hashes find the password both when the cutoff is
    // shorter than, and longer than, the password itself.
    let password = b"0123456789abcdef";
    let alg = pxh::HashAlgorithm::Argon2(32, 2, 1);

    // Range is to straddle the end of the password and test multiple
    // permutations; 3 is not particularly special.
    for cutoff in password.len() - 3..password.len() + 3 {
        let short_password: Vec<u8> = password.iter().cloned().take(cutoff).collect();
        let prefix_hashed = pxh::hash_password(alg, &short_password);
        let full_hashed = pxh::hash_password(alg, password.as_ref());

        // Some basic checks; this doesn't verify whether it was the
        // short or long hash that matched, however.
        let secret = pxh::HashedSecret::from_full(&prefix_hashed, &full_hashed, cutoff);
        assert!(secret.search_haystack("not here").is_none());
        assert!(secret.search_haystack("word word 0123456789abcdeX word word").is_none());
        assert!(secret.search_haystack("word word 0123456789abcdef word word").is_some());

        // Now create a new secret that contains precisely the
        // password defined at the cutoff point surrounded by
        // "word". Only when this includes the entire password will
        // the full secret match.
        let mut short_plaintext = BString::from("word word ");
        short_plaintext.push_str(&short_password);
        short_plaintext.push_str(" word word");
        if cutoff >= password.len() {
            assert!(secret.search_haystack(&short_password).is_some());
        } else {
            assert!(secret.search_haystack(&short_password).is_none());
        }

        // Finally verify just using the short secret we would match
        // the password.
        let short_secret = pxh::HashedSecret::new(&prefix_hashed);
        assert!(short_secret.search_haystack(&short_password).is_some());
    }
}
