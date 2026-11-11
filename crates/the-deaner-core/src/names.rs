//! tmux session naming.
//!
//! tmux rewrites `.` and `:` in session names (they are target separators), so
//! the-deaner sanitizes up front and always looks sessions up by the name it chose.

/// Make `raw` safe to use as (part of) a tmux session name.
#[must_use]
pub fn sanitize(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| match c {
            '.' | ':' => '_',
            c if c.is_whitespace() => '-',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

/// Session name for a workspace: `<repo>` for the main checkout,
/// `<repo>/<branch>` for a worktree.
#[must_use]
pub fn session_name(repo_name: &str, worktree_branch: Option<&str>) -> String {
    match worktree_branch {
        Some(branch) => format!("{}/{}", sanitize(repo_name), sanitize(branch)),
        None => sanitize(repo_name),
    }
}

/// Pick `base`, or `base-2`, `base-3`, … if `taken` already contains it.
#[must_use]
pub fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.iter().any(|t| t == candidate))
        .expect("an unused suffix always exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_tmux_separators() {
        assert_eq!(sanitize("my.repo"), "my_repo");
        assert_eq!(sanitize("a:b c"), "a_b-c");
        assert_eq!(sanitize("  "), "_");
    }

    #[test]
    fn worktree_names_nest_under_repo() {
        assert_eq!(session_name("shop-api", None), "shop-api");
        assert_eq!(
            session_name("shop-api", Some("feat/x.y")),
            "shop-api/feat/x_y"
        );
    }

    #[test]
    fn unique_appends_suffix() {
        let taken = vec!["api".to_string(), "api-2".to_string()];
        assert_eq!(unique("api", &taken), "api-3");
        assert_eq!(unique("web", &taken), "web");
    }
}
