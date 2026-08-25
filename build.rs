use std::{env, fs, path::Path};

use serde::Deserialize;

const CRITICAL_PATTERN_NAMES: &[&str] = &[
    // AWS - actual key formats
    "AWS API Key",             // AKIA[0-9A-Z]{16}
    "AWS Access Key ID Value", // (AKIA|ASIA|...)[A-Z0-9]{16}
    "AWS MWS key",             // amzn.mws.uuid format
    "AWS AppSync GraphQL Key", // da2-[a-z0-9]{26}
    // GitHub - token prefixes
    "Github App Token",             // (ghu|ghs)_[0-9a-zA-Z]{36}
    "Github OAuth Access Token",    // gho_[0-9a-zA-Z]{36}
    "Github Personal Access Token", // ghp_[0-9a-zA-Z]{36}
    "Github Refresh Token",         // ghr_[0-9a-zA-Z]{76}
    // Slack
    "Slack Token",   // xox[baprs]-
    "Slack Webhook", // hooks.slack.com/services/
    // Stripe - live/test keys
    "Stripe API Key - 1",        // sk_live_
    "Stripe Secret Live Key",    // sk_live_ pattern
    "Stripe Restricted API Key", // rk_live_
    // Google
    "Google API Key",               // AIza[0-9A-Za-z-_]{35}
    "Google (GCP) Service Account", // type.*service_account
    // Generic high-confidence
    "Asymmetric Private Key", // -----BEGIN.*PRIVATE KEY-----
    "Bearer token",           // [Bb]earer\s+...
    // Other services with distinctive formats
    "Twilio API Key",    // SK[0-9a-fA-F]{32}
    "SendGrid API Key",  // SG\.[a-zA-Z0-9-_]{22}\.[...]
    "PyPI upload token", // pypi-[A-Za-z0-9-_]{100,}
    "Alibaba - 2",       // LTAI[a-zA-Z0-9]{17,21}
];

#[derive(Debug, Deserialize)]
struct PatternsFile {
    patterns: Vec<PatternEntry>,
}

#[derive(Debug, Deserialize)]
struct PatternEntry {
    pattern: Pattern,
}

#[derive(Debug, Deserialize)]
struct Pattern {
    name: String,
    regex: String,
    confidence: String,
}

fn escape_string(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    result
}

/// `PXH_GIT_HASH`: short HEAD hash plus `-dirty` in a checkout; `vX.Y.Z`
/// otherwise (a crates.io tarball ships no `.git`), so every build carries
/// a distinguishable identifier.
fn emit_build_id() {
    use std::process::Command;
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let build_id = match git_hash {
        Some(hash) => {
            let dirty = Command::new("git")
                .args(["diff", "--quiet", "HEAD"])
                .status()
                .map(|s| !s.success())
                .unwrap_or(false);
            if dirty { format!("{hash}-dirty") } else { hash }
        }
        None => format!("v{}", env::var("CARGO_PKG_VERSION").unwrap_or_default()),
    };
    println!("cargo:rustc-env=PXH_GIT_HASH={build_id}");

    // In a linked worktree `.git` is a file pointing at the main repo's
    // gitdir, so the real HEAD/index live elsewhere; ask git for the actual
    // paths and fall back to the plain ones if git is unavailable.
    for file in ["HEAD", "index"] {
        let path = Command::new("git")
            .args(["rev-parse", "--git-path", file])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!(".git/{file}"));
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    emit_build_id();

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("secrets_patterns_generated.rs");

    let yaml_path = Path::new("src/vendor/rules-stable.yml");

    if !yaml_path.exists() {
        panic!(
            "src/vendor/rules-stable.yml not found. \
             Run: just vendor-update"
        );
    }

    let yaml_content = fs::read_to_string(yaml_path).expect("Failed to read YAML file");
    let patterns_file: PatternsFile =
        serde_yaml_ng::from_str(&yaml_content).expect("Failed to parse YAML");

    let mut critical_patterns = Vec::new();
    let mut high_patterns = Vec::new();
    let mut low_patterns = Vec::new();

    for entry in patterns_file.patterns {
        let name = escape_string(&entry.pattern.name);
        let regex = escape_string(&entry.pattern.regex);
        let tuple = format!("(\"{}\", \"{}\")", name, regex);

        if CRITICAL_PATTERN_NAMES.contains(&entry.pattern.name.as_str()) {
            critical_patterns.push(tuple);
            continue;
        }

        match entry.pattern.confidence.as_str() {
            "high" => high_patterns.push(tuple),
            "low" => low_patterns.push(tuple),
            other => eprintln!("cargo:warning=Unknown confidence level: {}", other),
        }
    }

    let code = format!(
        r#"// Auto-generated by build.rs - do not edit
pub const PATTERNS_CRITICAL: &[(&str, &str)] = &[
    {}
];

pub const PATTERNS_HIGH: &[(&str, &str)] = &[
    {}
];

pub const PATTERNS_LOW: &[(&str, &str)] = &[
    {}
];
"#,
        critical_patterns.join(",\n    "),
        high_patterns.join(",\n    "),
        low_patterns.join(",\n    ")
    );

    fs::write(&dest_path, code).unwrap();

    println!("cargo:rerun-if-changed=src/vendor/rules-stable.yml");
    println!("cargo:rerun-if-changed=build.rs");
}
