use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

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
