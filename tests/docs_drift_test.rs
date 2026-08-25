//! The docs that restate config keys must agree with the code. With
//! `deny_unknown_fields`, a README example using a renamed or removed key
//! fails here instead of silently misleading a reader.

const README: &str = include_str!("../README.md");

/// Every fenced ```toml block in the README, in order.
fn toml_blocks(md: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in md.lines() {
        match (&mut current, line.trim_end()) {
            (None, "```toml") => current = Some(String::new()),
            (Some(buf), "```") => blocks.push(std::mem::take(buf)),
            (Some(buf), _) => {
                buf.push_str(line);
                buf.push('\n');
            }
            (None, _) => {}
        }
        if line.trim_end() == "```" {
            current = None;
        }
    }
    blocks
}

#[test]
fn readme_toml_examples_are_valid_config() {
    let blocks = toml_blocks(README);
    assert!(blocks.len() >= 2, "expected the README to have TOML examples, found {}", blocks.len());
    for (i, block) in blocks.iter().enumerate() {
        toml::from_str::<pxh::config::Config>(block).unwrap_or_else(|e| {
            panic!("README toml block #{i} is not a valid pxh config: {e}\n{block}")
        });
    }
}
