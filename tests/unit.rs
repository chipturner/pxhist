use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

use bstr::{BString, ByteVec};
use tempfile::NamedTempFile;

fn verify_file_matches(path: &PathBuf, expected_contents: &str) {
    let fh = File::open(&path).unwrap();

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
