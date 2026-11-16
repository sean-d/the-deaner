//! What's new: `CHANGELOG.md`, built into the binary. One `## <version>` section per release,
//! newest first. After an upgrade the sidebar opens it with what changed since the version you
//! had at the top.

use crate::upgrade::is_newer;

/// The changelog as written.
pub const TEXT: &str = include_str!("../../../CHANGELOG.md");

/// One release's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release<'a> {
    pub version: &'a str,
    /// The heading after `## `, e.g. `0.1.14 · 2026-11-15`.
    pub heading: &'a str,
    pub body: &'a str,
}

/// The text before the first release, and the releases in file order (newest first).
#[must_use]
pub fn parse(text: &str) -> (&str, Vec<Release<'_>>) {
    let mut parts = text.split("\n## ");
    let intro = parts.next().unwrap_or_default();
    let releases = parts
        .map(|part| {
            let (heading, body) = part.split_once('\n').unwrap_or((part, ""));
            let heading = heading.trim();
            let version = heading.split_whitespace().next().unwrap_or_default();
            Release {
                version,
                heading,
                body: body.trim_matches('\n'),
            }
        })
        .collect();
    (intro.trim_end(), releases)
}

/// The page to show: everything, or, given the version you upgraded from, what's new since
/// then first and every release below it.
#[must_use]
pub fn page(text: &str, since: Option<&str>) -> String {
    let (intro, releases) = parse(text);
    let newer: Vec<&Release<'_>> = since
        .map(|s| releases.iter().filter(|r| is_newer(r.version, s)).collect())
        .unwrap_or_default();
    let mut out = format!("{intro}\n");
    let section = |out: &mut String, r: &Release<'_>, level: &str| {
        out.push_str(&format!("\n{level} {}\n\n{}\n", r.heading, r.body));
    };
    match since {
        Some(s) if !newer.is_empty() => {
            out.push_str(&format!("\n## New since {s}\n"));
            for r in &newer {
                section(&mut out, r, "###");
            }
            out.push_str("\n## Every version\n");
            for r in &releases {
                section(&mut out, r, "###");
            }
        }
        _ => {
            for r in &releases {
                section(&mut out, r, "##");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::{CURRENT, parse_version};

    #[test]
    fn every_release_has_notes_including_this_one() {
        let (_, releases) = parse(TEXT);
        assert!(
            releases.iter().any(|r| r.version == CURRENT),
            "CHANGELOG.md needs a `## {CURRENT}` section before this version is released"
        );
        for r in &releases {
            assert!(
                parse_version(r.version).is_some(),
                "bad heading: {}",
                r.heading
            );
            assert!(!r.body.trim().is_empty(), "{} has no notes", r.version);
        }
        let versions: Vec<_> = releases.iter().map(|r| parse_version(r.version)).collect();
        let mut sorted = versions.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(versions, sorted, "newest first");
    }

    const SAMPLE: &str = "# What's new\n\nIntro.\n\n## 0.1.3 · d3\n\n- three\n\n## 0.1.2 · d2\n\n- two\n\n## 0.1.1 · d1\n\n- one\n";

    #[test]
    fn new_since_comes_first_then_everything() {
        let p = page(SAMPLE, Some("0.1.1"));
        let new_at = p.find("## New since 0.1.1").unwrap();
        let all_at = p.find("## Every version").unwrap();
        assert!(new_at < all_at);
        let new = &p[new_at..all_at];
        assert!(new.contains("### 0.1.3 · d3") && new.contains("### 0.1.2 · d2"));
        assert!(
            !new.contains("### 0.1.1"),
            "not the version you already had: {new}"
        );
        assert!(p[all_at..].contains("### 0.1.1 · d1"));
        assert!(p.starts_with("# What's new\n\nIntro."));
    }

    #[test]
    fn without_since_or_nothing_new_it_is_just_the_list() {
        for since in [None, Some("0.1.3")] {
            let p = page(SAMPLE, since);
            assert!(!p.contains("New since"));
            assert!(p.contains("## 0.1.3 · d3") && p.contains("## 0.1.1 · d1"));
        }
    }
}
