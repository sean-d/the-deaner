//! Generated docs must match the code. Regenerate with:
//! `UPDATE_DOCS=1 cargo test -p deaner --test docs`

use deaner::cli::docs_page;
use deaner_core::config::Config;
use std::path::PathBuf;

fn docs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/src")
}

fn check(page: &str) {
    let path = docs_dir().join(format!("{page}.md"));
    let generated = docs_page(page).unwrap();
    if std::env::var_os("UPDATE_DOCS").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == generated,
        "{} is stale. Run: UPDATE_DOCS=1 cargo test -p deaner --test docs",
        path.display()
    );
}

#[test]
fn keybindings_page_is_current() {
    check("keybindings");
}

#[test]
fn cli_page_is_current() {
    check("cli");
}

#[test]
fn configuration_example_matches_defaults() {
    let text = std::fs::read_to_string(docs_dir().join("configuration.md")).unwrap();
    let block = text
        .split("```toml\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("configuration.md has a toml block");
    let parsed = Config::from_toml(block).expect("documented config parses");
    assert_eq!(
        parsed,
        Config::default(),
        "documented config.toml must equal the defaults"
    );
}
