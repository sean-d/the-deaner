//! `the-deaner upgrade`: find the latest release on GitHub, install it the way this copy
//! was installed (pacman or Homebrew), then restart into it.
//!
//! The sidebar also checks once a day (`update_check`) and shows `↑ <version>` in its
//! footer. It only ever tells you; installing is always `the-deaner upgrade`.

use crate::engine::{Engine, now_unix};
use crate::paths::Paths;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use the_deaner_core::process::output_with_timeout;

/// The version running now.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const DAY: u64 = 24 * 60 * 60;
/// After a failed check (offline), try again in an hour rather than a day.
const RETRY: u64 = 60 * 60;
const API_TIMEOUT: Duration = Duration::from_secs(15);

/// `owner/repo` from the package's repository URL.
fn repo_slug() -> &'static str {
    env!("CARGO_PKG_REPOSITORY")
        .trim_start_matches("https://github.com/")
        .trim_end_matches('/')
}

/// `v0.1.4` or `0.1.4` (anything after `-` or `+` ignored) as comparable numbers.
#[must_use]
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// Whether `candidate` is a newer version than `current`.
#[must_use]
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(r)) => c > r,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Without the leading `v`.
    pub version: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// From GitHub's `releases/latest` JSON.
    ///
    /// # Errors
    /// When the JSON isn't a release.
    pub fn parse(json: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Raw {
            tag_name: String,
            #[serde(default)]
            html_url: String,
            #[serde(default)]
            assets: Vec<RawAsset>,
        }
        #[derive(Deserialize)]
        struct RawAsset {
            name: String,
            browser_download_url: String,
        }
        let raw: Raw = serde_json::from_str(json).context("unexpected answer from GitHub")?;
        Ok(Self {
            version: raw.tag_name.trim_start_matches('v').to_string(),
            page: raw.html_url,
            assets: raw
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    url: a.browser_download_url,
                })
                .collect(),
        })
    }

    /// The Arch package for this machine and its checksum file.
    fn arch_package(&self) -> Option<(&Asset, &Asset)> {
        let suffix = format!("-{}.pkg.tar.zst", std::env::consts::ARCH);
        let pkg = self.assets.iter().find(|a| a.name.ends_with(&suffix))?;
        let sum_name = format!("{}.sha256", pkg.name);
        let sum = self.assets.iter().find(|a| a.name == sum_name)?;
        Some((pkg, sum))
    }
}

/// Ask GitHub for the latest release: through `gh` when it's logged in (its own rate
/// limit), otherwise anonymously with `curl`.
///
/// # Errors
/// When neither can reach GitHub.
pub fn latest() -> Result<Release> {
    let api = format!("repos/{}/releases/latest", repo_slug());
    let gh = output_with_timeout(
        Command::new("gh")
            .args(["api", &api])
            .env("GH_PROMPT_DISABLED", "1"),
        API_TIMEOUT,
    );
    if let Ok(out) = gh
        && out.status.success()
    {
        return Release::parse(&String::from_utf8_lossy(&out.stdout));
    }
    let url = format!("https://api.github.com/{api}");
    let out = output_with_timeout(
        Command::new("curl").args(["-fsSL", "-H", "Accept: application/vnd.github+json", &url]),
        API_TIMEOUT,
    )
    .context("asking GitHub for the latest release (is curl installed?)")?;
    if !out.status.success() {
        bail!(
            "couldn't reach GitHub: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Release::parse(&String::from_utf8_lossy(&out.stdout))
}

// ---- the daily check ----------------------------------------------------------

/// `state/update.toml`: when we last asked GitHub, and what it said.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    pub checked: u64,
    pub latest: Option<String>,
}

fn cache_file(paths: &Paths) -> PathBuf {
    paths.state_dir.join("update.toml")
}

fn read_cache(paths: &Paths) -> Cache {
    std::fs::read_to_string(cache_file(paths))
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(paths: &Paths, cache: &Cache) {
    if let Ok(text) = toml::to_string(cache) {
        let _ = crate::paths::write_atomic(&cache_file(paths), &text);
    }
}

/// Whether a check is due at `now`.
#[must_use]
pub fn due(cache: &Cache, now: u64) -> bool {
    now.saturating_sub(cache.checked) >= DAY
}

/// Ask GitHub if a day has passed since the last time, and return the newer version
/// if there is one. Never fails: offline just means no news (and another try in an hour).
#[must_use]
pub fn check_daily(paths: &Paths) -> Option<String> {
    let mut cache = read_cache(paths);
    let now = now_unix();
    if due(&cache, now) {
        match latest() {
            Ok(r) => {
                cache = Cache {
                    checked: now,
                    latest: Some(r.version),
                };
            }
            Err(_) => cache.checked = now.saturating_sub(DAY - RETRY),
        }
        write_cache(paths, &cache);
    }
    cache.latest.filter(|v| is_newer(v, CURRENT))
}

/// Whether to say "Updated to <CURRENT>" on this start, given the version that ran last.
/// Nothing saved means either a brand-new install (no projects yet: nothing to announce)
/// or an upgrade from before versions were saved (0.1.4).
#[must_use]
pub fn just_upgraded(last: Option<&str>, has_projects: bool) -> bool {
    match last {
        Some(v) => is_newer(CURRENT, v),
        None => has_projects,
    }
}

// ---- installing --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    Pacman,
    Homebrew,
    /// Built from source or copied by hand.
    Other,
}

fn detect(exe: &Path) -> Install {
    let real = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    if real.to_string_lossy().contains("/Cellar/the-deaner/") {
        return Install::Homebrew;
    }
    let owned = output_with_timeout(
        Command::new("pacman").arg("-Qqo").arg(exe),
        Duration::from_secs(10),
    );
    if let Ok(out) = owned
        && out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim() == "the-deaner"
    {
        return Install::Pacman;
    }
    Install::Other
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question} [Y/n] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "" | "y" | "Y" | "yes"))
}

/// Run a command in this terminal (its prompts and output are yours to see).
fn interactive(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed", args.join(" "));
    }
    Ok(())
}

/// The version the binary at `exe` reports now (after an install replaced it).
fn installed_version(exe: &Path) -> Option<String> {
    let out = Command::new(exe).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().map(str::to_string)
}

fn sha256_of(file: &Path) -> Result<String> {
    let out = Command::new("sha256sum")
        .arg(file)
        .output()
        .context("running sha256sum")?;
    if !out.status.success() {
        bail!("sha256sum failed on {}", file.display());
    }
    first_word(&String::from_utf8_lossy(&out.stdout))
}

fn first_word(s: &str) -> Result<String> {
    s.split_whitespace()
        .next()
        .map(str::to_lowercase)
        .context("empty checksum")
}

fn download(url: &str, to: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fL", "--progress-bar", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .context("running curl")?;
    if !status.success() {
        bail!("download failed: {url}");
    }
    Ok(())
}

fn install_pacman(engine: &Engine, release: &Release) -> Result<()> {
    let (pkg, sum) = release.arch_package().with_context(|| {
        format!(
            "v{} has no Arch package for {}",
            release.version,
            std::env::consts::ARCH
        )
    })?;
    let dir = engine.paths.state_dir.join("download");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let result = (|| {
        let pkg_path = dir.join(&pkg.name);
        let sum_path = dir.join(&sum.name);
        println!("downloading {}", pkg.name);
        download(&pkg.url, &pkg_path)?;
        download(&sum.url, &sum_path)?;
        let want = first_word(&std::fs::read_to_string(&sum_path)?)?;
        let got = sha256_of(&pkg_path)?;
        if want != got {
            bail!(
                "checksum mismatch for {}: expected {want}, got {got}",
                pkg.name
            );
        }
        println!("checksum ok; installing with pacman");
        interactive("sudo", &["pacman", "-U", &pkg_path.to_string_lossy()])
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// `the-deaner upgrade [--check] [--yes]`.
///
/// # Errors
/// When GitHub can't be reached or the install fails. Nothing is stopped until the new
/// version is installed and confirmed.
pub fn run(engine: &Engine, check_only: bool, yes: bool) -> Result<()> {
    let release = latest()?;
    write_cache(
        &engine.paths,
        &Cache {
            checked: now_unix(),
            latest: Some(release.version.clone()),
        },
    );
    if !is_newer(&release.version, CURRENT) {
        println!("the-deaner {CURRENT} is the latest");
        return Ok(());
    }
    println!(
        "the-deaner {} is out (you have {CURRENT}): {}",
        release.version, release.page
    );
    if check_only {
        println!("Run `the-deaner upgrade` to install it.");
        return Ok(());
    }

    let exe = crate::frame::exe_path()?;
    let how = detect(&exe);
    if how == Install::Other {
        println!(
            "{} wasn't installed by pacman or Homebrew, so upgrade it the way you installed it, e.g.\n  \
             cargo install --locked --force --git https://github.com/{} the-deaner\n\
             then run `the-deaner restart`.",
            exe.display(),
            repo_slug()
        );
        return Ok(());
    }
    if !yes && !confirm(&format!("Install {} and restart?", release.version))? {
        println!("cancelled");
        return Ok(());
    }
    match how {
        Install::Pacman => install_pacman(engine, &release)?,
        Install::Homebrew => {
            // `brew upgrade` refreshes taps at most once a day by itself, so it may not
            // know about a release from an hour ago yet.
            interactive("brew", &["update"])?;
            interactive("brew", &["upgrade", "the-deaner"])?;
        }
        Install::Other => unreachable!(),
    }

    let now = installed_version(&exe).unwrap_or_default();
    if !is_newer(&now, CURRENT) {
        bail!(
            "{} still reports {}; not restarting",
            exe.display(),
            if now.is_empty() { "nothing" } else { &now }
        );
    }
    println!("installed {now}");
    crate::cli::shutdown(engine, false, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("v0.1.4"), Some((0, 1, 4)));
        assert_eq!(parse_version("0.10.0-rc1"), Some((0, 10, 0)));
        assert_eq!(parse_version("0.1"), None);
        assert_eq!(parse_version("0.1.2.3"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("garbage", "0.1.3"));
    }

    #[test]
    fn a_release_finds_this_machines_arch_package() {
        let arch = std::env::consts::ARCH;
        let json = format!(
            r#"{{"tag_name":"v0.1.4","html_url":"https://github.com/sean-d/the-deaner/releases/tag/v0.1.4",
            "assets":[
              {{"name":"the-deaner-v0.1.4-x86_64-unknown-linux-gnu.tar.gz","browser_download_url":"https://x/a"}},
              {{"name":"the-deaner-0.1.4-1-{arch}.pkg.tar.zst","browser_download_url":"https://x/pkg"}},
              {{"name":"the-deaner-0.1.4-1-{arch}.pkg.tar.zst.sha256","browser_download_url":"https://x/sum"}}
            ]}}"#
        );
        let r = Release::parse(&json).unwrap();
        assert_eq!(r.version, "0.1.4");
        let (pkg, sum) = r.arch_package().unwrap();
        assert_eq!(pkg.url, "https://x/pkg");
        assert_eq!(sum.url, "https://x/sum");

        let none = Release::parse(r#"{"tag_name":"v0.1.4","assets":[]}"#).unwrap();
        assert!(none.arch_package().is_none());
        assert!(Release::parse("{}").is_err());
    }

    #[test]
    fn says_updated_only_after_an_upgrade() {
        assert!(just_upgraded(Some("0.0.1"), true));
        assert!(!just_upgraded(Some(CURRENT), true));
        assert!(
            !just_upgraded(Some("999.0.0"), true),
            "a downgrade isn't news"
        );
        assert!(
            just_upgraded(None, true),
            "from 0.1.4, which saved no version"
        );
        assert!(!just_upgraded(None, false), "a brand-new install");
    }

    #[test]
    fn checks_once_a_day() {
        let c = Cache {
            checked: 1_000_000,
            latest: Some("0.1.4".into()),
        };
        assert!(!due(&c, 1_000_000 + DAY - 1));
        assert!(due(&c, 1_000_000 + DAY));
        assert!(due(&Cache::default(), 1_000_000));
    }

    #[test]
    fn the_cache_round_trips_and_a_bad_one_means_check_now() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        assert_eq!(read_cache(&paths), Cache::default());
        let c = Cache {
            checked: 42,
            latest: Some("0.1.4".into()),
        };
        write_cache(&paths, &c);
        assert_eq!(read_cache(&paths), c);
        std::fs::write(cache_file(&paths), "not toml [").unwrap();
        assert_eq!(read_cache(&paths), Cache::default());
    }

    #[test]
    fn checksum_files_are_hash_then_name() {
        assert_eq!(
            first_word("ABC123  the-deaner-0.1.4-1-x86_64.pkg.tar.zst\n").unwrap(),
            "abc123"
        );
        assert!(first_word("").is_err());
    }
}
