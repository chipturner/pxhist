//! Parsed recall query: the single owner of the query→atoms mapping shared
//! by the DB prefilter and the nucleo fuzzy stage.
//!
//! The invariant this module exists to hold in one place: the LIKE
//! conditions from `like_patterns()` must accept every row the nucleo
//! pattern from `scoring_pattern()` can match (the prefilter is a strict
//! superset of the fuzzy stage). Both sides derive from the same
//! normalization and the same atom split, so they cannot drift apart.
//!
//! Known bounded exception: nucleo's `Normalization::Smart` folds accented
//! haystack chars onto ASCII query chars (`cafe` matches `café`); a byte-wise
//! LIKE cannot express that, so a haystack whose only match is via folding
//! can still be prefiltered away. Non-ASCII *query* chars are simply not
//! required by the prefilter (requiring less always preserves the superset).

use nucleo::pattern::{CaseMatching, Normalization, Pattern};

/// Characters treated as word separators in recall search. `-` covers flag
/// forms (`--release`), `*` covers glob queries, and `/` covers path
/// components so a match after any of them scores like a whitespace boundary
/// rather than nucleo's (lower) delimiter boundary.
pub fn is_recall_separator(c: char) -> bool {
    matches!(c, '-' | '*' | '/')
}

/// Map separator chars to space so nucleo treats them as word boundaries;
/// pass through everything else.
pub fn normalize_recall_char(c: char) -> char {
    if is_recall_separator(c) { ' ' } else { c }
}

/// A recall query parsed once into nucleo-style atoms.
///
/// Equality is on the raw text: two queries compare equal iff the user typed
/// the same thing, which is what cache keying wants.
#[derive(Debug, Clone)]
pub struct RecallQuery {
    raw: String,
    normalized: String,
    /// Character runs the DB prefilter requires as ordered subsequences, one
    /// per positive atom (operators stripped, escapes resolved, non-ASCII
    /// dropped). Negated atoms contribute nothing: LIKE can only require
    /// presence, so the fuzzy stage alone enforces absence.
    required: Vec<String>,
}

impl PartialEq for RecallQuery {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl RecallQuery {
    pub fn parse(raw: &str) -> Self {
        let normalized: String = raw.chars().map(normalize_recall_char).collect();
        let required = atomize(&normalized).into_iter().filter_map(required_chars).collect();
        RecallQuery { raw: raw.to_string(), normalized, required }
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Whether this query produces any DB-level prefilter conditions.
    /// Queries of only negated/operator atoms don't, and load identically
    /// to no query.
    pub fn has_prefilter(&self) -> bool {
        !self.required.is_empty()
    }

    /// One LIKE pattern per positive atom. "gcm" becomes "%g%c%m%" so it
    /// matches "git commit -m"; atoms match in any order because each is an
    /// independent AND'd condition, mirroring nucleo's atom semantics.
    pub fn like_patterns(&self) -> Vec<String> {
        self.required.iter().map(|req| like_pattern(req)).collect()
    }

    /// Whether a candidate set loaded with `self`'s prefilter necessarily
    /// contains every row a load with `other`'s prefilter would return:
    /// each of `self`'s required runs must be an (ASCII-case-insensitive,
    /// matching LIKE's NOCASE) subsequence of one of `other`'s runs, so each
    /// of `self`'s conditions is implied by one of `other`'s.
    ///
    /// Truncation caveat, shared with a fresh load: both sets keep only the
    /// most recent rows within the oversampled window, so "covers" is
    /// relative to that window, not the full table.
    pub fn covers(&self, other: &RecallQuery) -> bool {
        self.required.iter().all(|prev| other.required.iter().any(|new| is_subsequence(prev, new)))
    }

    /// Total required chars -- more chars means a narrower candidate set.
    pub fn specificity(&self) -> usize {
        self.required.iter().map(String::len).sum()
    }

    /// The pattern the fuzzy stage scores with: separators normalized to
    /// spaces so matches after them get nucleo's whitespace-boundary bonus.
    pub fn scoring_pattern(&self) -> Pattern {
        Pattern::parse(&self.normalized, CaseMatching::Smart, Normalization::Smart)
    }

    /// The pattern used for highlight indices: the raw query, so
    /// "--release" highlights its dashes in the original command text.
    pub fn highlight_pattern(&self) -> Pattern {
        Pattern::parse(&self.raw, CaseMatching::Smart, Normalization::Smart)
    }
}

/// Split a normalized query into atoms the way nucleo's `Pattern::parse`
/// does: unescaped whitespace separates atoms, `\ ` keeps a space inside
/// one. Backslashes are preserved for `required_chars` to resolve, so a
/// trailing `\$` is still distinguishable from a suffix anchor.
fn atomize(normalized: &str) -> Vec<String> {
    let mut atoms = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for c in normalized.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
        } else if c == '\\' {
            cur.push(c);
            escaped = true;
        } else if c.is_whitespace() {
            if !cur.is_empty() {
                atoms.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        atoms.push(cur);
    }
    atoms
}

/// The chars one positive atom requires the haystack to contain in order,
/// with fzf-style operators stripped (`^`/`'` prefix, unescaped `$` suffix)
/// and escapes resolved. `None` for negated atoms and atoms that reduce to
/// nothing. Non-ASCII chars are dropped: nucleo may satisfy them via Unicode
/// folding that LIKE can't express, and requiring less keeps the superset.
fn required_chars(atom: String) -> Option<String> {
    let mut rest = atom.as_str();
    if rest.starts_with('!') {
        return None;
    }
    rest = rest.strip_prefix(['^', '\'']).unwrap_or(rest);
    if rest.ends_with('$') && !rest.ends_with("\\$") {
        rest = &rest[..rest.len() - 1];
    }

    let mut required = String::new();
    let mut escaped = false;
    for c in rest.chars() {
        if escaped {
            escaped = false;
            if c.is_ascii() {
                required.push(c);
            }
        } else if c == '\\' {
            escaped = true;
        } else if c.is_ascii() {
            required.push(c);
        }
    }
    (!required.is_empty()).then_some(required)
}

/// Build a LIKE pattern matching `required` as an ordered subsequence:
/// "gcm" becomes "%g%c%m%".
fn like_pattern(required: &str) -> String {
    let mut pattern = String::with_capacity(required.len() * 2 + 1);
    pattern.push('%');
    for ch in required.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
        pattern.push('%');
    }
    pattern
}

/// Whether `needle`'s chars appear in `hay` in order, ignoring ASCII case
/// (matching LIKE's NOCASE collation).
fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut hay_chars = hay.chars();
    needle.chars().all(|n| hay_chars.any(|h| h.eq_ignore_ascii_case(&n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(query: &str) -> Vec<String> {
        RecallQuery::parse(query).like_patterns()
    }

    #[test]
    fn test_separators_split_atoms_before_operator_parsing() {
        // The prefilter must see the same atoms nucleo scores with:
        // separators become atom boundaries, not intra-atom wildcards.
        assert_eq!(patterns("rs/main"), ["%r%s%", "%m%a%i%n%"]);
        assert_eq!(patterns("git-log"), ["%g%i%t%", "%l%o%g%"]);
        assert_eq!(patterns("*.rs"), ["%.%r%s%"]);
    }

    #[test]
    fn test_operators_stripped_per_atom() {
        assert_eq!(patterns("^cargo"), ["%c%a%r%g%o%"]);
        assert_eq!(patterns("'build"), ["%b%u%i%l%d%"]);
        assert_eq!(patterns("push$"), ["%p%u%s%h%"]);
        assert_eq!(patterns("^git push$"), ["%g%i%t%", "%p%u%s%h%"]);
    }

    #[test]
    fn test_negated_atoms_contribute_nothing() {
        assert_eq!(patterns("!vim"), Vec::<String>::new());
        assert_eq!(patterns("!vim cargo"), ["%c%a%r%g%o%"]);
        // Normalization can split a negated atom; only its leading piece
        // stays negated, matching what nucleo sees after normalization.
        assert_eq!(patterns("!foo-bar"), ["%b%a%r%"]);
    }

    #[test]
    fn test_escapes_resolved_like_nucleo() {
        // `\ ` keeps the space inside one atom instead of splitting.
        assert_eq!(patterns("foo\\ bar"), ["%f%o%o% %b%a%r%"]);
        // `\$` is a literal dollar, not a suffix anchor.
        assert_eq!(patterns("cost\\$"), ["%c%o%s%t%$%"]);
        // `\!` is a literal bang, not negation.
        assert_eq!(patterns("\\!urgent"), ["%!%u%r%g%e%n%t%"]);
        // A dangling backslash requires nothing extra.
        assert_eq!(patterns("foo\\"), ["%f%o%o%"]);
    }

    #[test]
    fn test_like_metacharacters_escaped() {
        assert_eq!(patterns("50%_x"), ["%5%0%\\%%\\_%x%"]);
    }

    #[test]
    fn test_non_ascii_chars_not_required() {
        // nucleo's Smart normalization may satisfy these via folding; LIKE
        // can't, so the prefilter must not demand them.
        assert_eq!(patterns("café"), ["%c%a%f%"]);
        assert!(!RecallQuery::parse("é").has_prefilter());
    }

    #[test]
    fn test_has_prefilter() {
        assert!(RecallQuery::parse("cargo").has_prefilter());
        assert!(RecallQuery::parse("!vim cargo").has_prefilter());
        assert!(!RecallQuery::parse("!vim").has_prefilter());
        assert!(!RecallQuery::parse("^$").has_prefilter());
        assert!(!RecallQuery::parse("   ").has_prefilter());
        assert!(!RecallQuery::parse("").has_prefilter());
    }

    #[test]
    fn test_covers_extension_and_new_atoms() {
        let covers = |a: &str, b: &str| RecallQuery::parse(a).covers(&RecallQuery::parse(b));
        // Appending chars or atoms only narrows the set.
        assert!(covers("git", "gith"));
        assert!(covers("git", "git push"));
        assert!(covers("rs", "rs/main"), "separator append must stay covered");
        // Atom order doesn't matter: each condition is independent.
        assert!(covers("push", "git push"));
        // LIKE is NOCASE, so coverage is too.
        assert!(covers("GIT", "git push"));
        // Divergent or broader queries are not covered.
        assert!(!covers("abc", "abx"));
        assert!(!covers("git push", "git"));
        // The no-condition query covers everything (modulo the window).
        assert!(covers("!vim", "git"));
        assert!(!covers("git", "!vim"));
    }

    #[test]
    fn test_specificity_orders_narrowness() {
        assert!(
            RecallQuery::parse("git push").specificity() > RecallQuery::parse("git").specificity()
        );
        assert_eq!(RecallQuery::parse("!vim").specificity(), 0);
    }
}
