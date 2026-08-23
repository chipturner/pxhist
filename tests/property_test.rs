//! Property-based tests for the byte-level parsing and serialization paths.
//! Commands are BLOBs precisely so arbitrary bytes survive; these properties
//! pin that promise for the import/export formats.

use bstr::BString;
use proptest::prelude::*;
use pxh::{Invocation, join_continuation_lines, unmetafy};

/// Metafy every byte zsh could ever metafy (a superset of its real token set
/// is fine: the decoder must handle any 0x83-escaped byte).
fn metafy_all(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in bytes {
        if b == 0 || b >= 0x80 {
            out.push(0x83);
            out.push(b ^ 0x20);
        } else {
            out.push(b);
        }
    }
    out
}

proptest! {
    #[test]
    fn unmetafy_inverts_metafy(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assert_eq!(unmetafy(&metafy_all(&bytes)), bytes);
    }

    #[test]
    fn unmetafy_is_identity_without_meta_byte(
        bytes in proptest::collection::vec(any::<u8>().prop_filter("no Meta", |b| *b != 0x83), 0..64)
    ) {
        prop_assert_eq!(unmetafy(&bytes), bytes);
    }

    #[test]
    fn unmetafy_never_grows(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assert!(unmetafy(&bytes).len() <= bytes.len());
    }

    /// Lines that do not end in a backslash pass through unchanged (empties
    /// dropped), whatever bytes they contain.
    #[test]
    fn continuation_join_is_identity_without_backslashes(
        lines in proptest::collection::vec(
            proptest::collection::vec(any::<u8>().prop_filter("no nl/backslash", |b| *b != b'\n' && *b != b'\\'), 1..20),
            0..10,
        )
    ) {
        let buf = lines.join(&b'\n');
        prop_assert_eq!(join_continuation_lines(&buf), lines);
    }

    /// A backslash-terminated line is glued to its successor with a newline,
    /// so the logical line count drops by one per continuation.
    #[test]
    fn continuation_join_merges_exactly_the_marked_lines(
        lines in proptest::collection::vec(
            proptest::collection::vec(any::<u8>().prop_filter("no nl/backslash", |b| *b != b'\n' && *b != b'\\'), 1..10),
            2..8,
        ),
        marks in proptest::collection::vec(any::<bool>(), 8),
    ) {
        let n = lines.len();
        let mut physical = Vec::new();
        let mut continued = 0;
        for (i, line) in lines.iter().enumerate() {
            physical.extend_from_slice(line);
            if i + 1 < n && marks[i] {
                physical.push(b'\\');
                continued += 1;
            }
            physical.push(b'\n');
        }
        let logical = join_continuation_lines(&physical);
        prop_assert_eq!(logical.len(), n - continued);
        // No byte is lost: total payload equals the original lines plus one
        // joining newline per continuation.
        let payload: usize = logical.iter().map(Vec::len).sum();
        let expected: usize = lines.iter().map(Vec::len).sum::<usize>() + continued;
        prop_assert_eq!(payload, expected);
    }

    /// The JSON import format must round-trip any bytes, including invalid
    /// UTF-8, in every BLOB-backed field.
    #[test]
    fn invocation_json_roundtrips_arbitrary_bytes(
        command in proptest::collection::vec(any::<u8>(), 1..40),
        dir in proptest::option::of(proptest::collection::vec(any::<u8>(), 0..20)),
        host in proptest::option::of(proptest::collection::vec(any::<u8>(), 0..12)),
        exit_status in proptest::option::of(any::<i64>()),
        ts in proptest::option::of(0i64..4_000_000_000),
        session_id in any::<i64>(),
        machine_id in proptest::option::of(any::<u64>()),
    ) {
        let inv = Invocation {
            command: BString::from(command),
            shellname: "zsh".into(),
            working_directory: dir.map(BString::from),
            hostname: host.map(BString::from),
            username: Some(BString::from("u")),
            exit_status,
            start_unix_timestamp: ts,
            end_unix_timestamp: ts.map(|t| t + 1),
            session_id,
            machine_id,
        };
        let json = serde_json::to_string(&inv).unwrap();
        let back: Invocation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.command, inv.command);
        prop_assert_eq!(back.working_directory, inv.working_directory);
        prop_assert_eq!(back.hostname, inv.hostname);
        prop_assert_eq!(back.exit_status, inv.exit_status);
        prop_assert_eq!(back.start_unix_timestamp, inv.start_unix_timestamp);
        prop_assert_eq!(back.end_unix_timestamp, inv.end_unix_timestamp);
        prop_assert_eq!(back.session_id, inv.session_id);
        prop_assert_eq!(back.machine_id, inv.machine_id);
    }
}
