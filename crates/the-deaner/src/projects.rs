//! The projects file: your projects, on every machine.
//!
//! `~/.config/the-deaner/projects.toml` lists projects by path and git remote. `the-deaner
//! export` writes it from the sidebar; `the-deaner sync` adds what it lists to this machine's
//! sidebar, cloning what's missing. Sync shows its plan first, never runs on its own, and never
//! removes anything.

use crate::engine::Engine;
use crate::paths::expand;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectsFile {
    #[serde(default, rename = "project")]
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Where it lives, `~/…` so one file fits every machine.
    pub path: String,
    /// Its git remote (`origin`). None for a plain folder: nothing to clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl ProjectsFile {
    /// # Errors
    /// When the TOML is invalid.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("projects file isn't valid TOML")
    }

    /// As written by `the-deaner export`, with a line saying what it is.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!(
            "# the-deaner projects: `the-deaner sync` adds these to a sidebar, cloning what's \
             missing.\n# Written by `the-deaner export`. Paths use ~ so the file fits every \
             machine.\n\n{body}"
        )
    }
}

/// A remote reduced to `host/owner/repo`, so the SSH and HTTPS spellings of one repo match:
/// `git@github.com:sean-d/x.git`, `ssh://git@github.com/sean-d/x`, `https://github.com/sean-d/x`.
#[must_use]
pub fn normalize_remote(url: &str) -> String {
    let u = url.trim();
    let u = u
        .strip_prefix("ssh://")
        .or_else(|| u.strip_prefix("https://"))
        .or_else(|| u.strip_prefix("http://"))
        .or_else(|| u.strip_prefix("git://"))
        .unwrap_or(u);
    // Drop a user (`git@`), then turn scp-style `host:owner/repo` into `host/owner/repo`.
    let u = u.split_once('@').map_or(u, |(_, rest)| rest);
    let u = match u.split_once(':') {
        Some((host, rest)) if !rest.starts_with('/') && !host.contains('/') => {
            format!("{host}/{rest}")
        }
        _ => u.to_string(),
    };
    let u = u.trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    match u.split_once('/') {
        Some((host, rest)) => format!("{}/{rest}", host.to_lowercase()),
        None => u.to_string(),
    }
}

fn same_remote(a: &str, b: &str) -> bool {
    normalize_remote(a) == normalize_remote(b)
}

/// One thing sync will do (or deliberately not do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Already in the sidebar with the same path and remote.
    Skip { name: String, why: String },
    /// On disk, not in the sidebar: add it.
    Add { name: String, path: PathBuf },
    /// Not on disk: clone it, then add it.
    Clone {
        name: String,
        remote: String,
        path: PathBuf,
    },
    /// Something else is there; left alone.
    Warn { name: String, why: String },
    /// Nothing to do here, for a reason worth saying.
    Note { name: String, why: String },
}

impl Step {
    /// `  clone   kexp-api   git@… → ~/dev/go/kexp-api`
    #[must_use]
    pub fn line(&self) -> String {
        let (verb, name, what) = match self {
            Self::Skip { name, why } => ("skip", name, why.clone()),
            Self::Add { name, path } => ("add", name, tilde(path)),
            Self::Clone { name, remote, path } => {
                ("clone", name, format!("{remote} → {}", tilde(path)))
            }
            Self::Warn { name, why } => ("warn", name, why.clone()),
            Self::Note { name, why } => ("note", name, why.clone()),
        };
        format!("  {verb:<7} {name:<20} {what}")
    }

    fn does_something(&self) -> bool {
        matches!(self, Self::Add { .. } | Self::Clone { .. })
    }
}

/// What sync would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// The new pin order, when it changes: the file's pinned projects in its order, then pins
    /// already here that the file doesn't mention.
    pub pins: Option<Vec<PathBuf>>,
    /// Lines describing the pin change.
    pub pin_lines: Vec<String>,
}

impl Plan {
    #[must_use]
    pub fn has_work(&self) -> bool {
        self.steps.iter().any(Step::does_something) || self.pins.is_some()
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A path with the home folder written as `~`.
#[must_use]
pub fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|h| path.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Decide what sync would do on this machine. Looks, changes nothing.
#[must_use]
pub fn plan(engine: &Engine, file: &ProjectsFile) -> Plan {
    let state = engine.state();
    let mut steps = Vec::new();
    // Projects that will be in the sidebar afterwards, for pinning.
    let mut present: Vec<PathBuf> = Vec::new();
    for p in &file.projects {
        let path = expand(&p.path);
        let name = name_of(&path);
        let in_sidebar = state.repos.iter().any(|r| r.path == path);
        let is_git = path.is_dir() && the_deaner_git::is_repo(&path);
        let have = is_git.then(|| the_deaner_git::origin_url(&path)).flatten();
        let step = match (&p.remote, in_sidebar, path.is_dir()) {
            (None, true, _) => Step::Skip {
                name,
                why: "already in the sidebar".into(),
            },
            (Some(want), true, _) => match &have {
                Some(h) if same_remote(want, h) => Step::Skip {
                    name,
                    why: "already in the sidebar".into(),
                },
                Some(h) => Step::Warn {
                    name,
                    why: format!("in the sidebar with a different remote ({h}); left alone"),
                },
                None => Step::Warn {
                    name,
                    why: format!(
                        "in the sidebar but has no remote here (the file says {want}); left alone"
                    ),
                },
            },
            (None, false, true) => Step::Add {
                name,
                path: path.clone(),
            },
            (Some(want), false, true) => match (&have, is_git) {
                (Some(h), _) if same_remote(want, h) => Step::Add {
                    name,
                    path: path.clone(),
                },
                (Some(h), _) => Step::Warn {
                    name,
                    why: format!("{} has a different remote ({h}); left alone", tilde(&path)),
                },
                (None, true) => Step::Warn {
                    name,
                    why: format!(
                        "{} has no `origin` remote (the file says {want}); left alone",
                        tilde(&path)
                    ),
                },
                (None, false) => Step::Warn {
                    name,
                    why: format!(
                        "{} isn't a git repo (the file says {want}); left alone",
                        tilde(&path)
                    ),
                },
            },
            (Some(remote), false, false) if !path.exists() => Step::Clone {
                name,
                remote: remote.clone(),
                path: path.clone(),
            },
            (Some(_), false, false) => Step::Warn {
                name,
                why: format!("{} exists but isn't a folder; left alone", tilde(&path)),
            },
            (None, false, false) => Step::Note {
                name,
                why: format!("{} doesn't exist here; nothing to clone", tilde(&path)),
            },
        };
        if matches!(step, Step::Add { .. } | Step::Clone { .. }) || in_sidebar {
            present.push(path);
        }
        steps.push(step);
    }

    // Pins: the file's, in its order, then the ones already here it doesn't mention.
    let wanted: Vec<PathBuf> = file
        .projects
        .iter()
        .filter(|p| p.pinned)
        .map(|p| expand(&p.path))
        .filter(|p| present.contains(p))
        .collect();
    let mut pins = wanted.clone();
    pins.extend(state.pins.iter().filter(|p| !wanted.contains(p)).cloned());
    let mut pin_lines = Vec::new();
    let pins = if pins == state.pins {
        None
    } else {
        for p in &wanted {
            if !state.pins.contains(p) {
                pin_lines.push(format!(
                    "  {:<7} {:<20} pinned, in the file's order",
                    "pin",
                    name_of(p)
                ));
            }
        }
        if pin_lines.is_empty() {
            pin_lines.push(format!(
                "  {:<7} {:<20} put in the file's order",
                "pins", ""
            ));
        }
        Some(pins)
    };
    Plan {
        steps,
        pins,
        pin_lines,
    }
}

/// What happened when the plan ran.
#[derive(Debug, Default)]
pub struct Outcome {
    pub added: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Carry out a plan: clone (git's own progress showing), add, pin. A failed clone doesn't stop
/// the rest.
///
/// # Errors
/// When state can't be written.
pub fn run(engine: &Engine, plan: &Plan) -> Result<Outcome> {
    let mut out = Outcome::default();
    for step in &plan.steps {
        match step {
            Step::Add { name, path } => match engine.add_repo(&path.display().to_string()) {
                Ok(_) => out.added.push(name.clone()),
                Err(e) => out.failed.push((name.clone(), format!("{e:#}"))),
            },
            Step::Clone { name, remote, path } => {
                println!("cloning {name} from {remote}");
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let ok = std::process::Command::new("git")
                    .args(["clone", "--", remote])
                    .arg(path)
                    .status()
                    .is_ok_and(|s| s.success());
                if !ok {
                    out.failed
                        .push((name.clone(), format!("git clone {remote} failed")));
                    continue;
                }
                match engine.add_repo(&path.display().to_string()) {
                    Ok(_) => out.added.push(name.clone()),
                    Err(e) => out.failed.push((name.clone(), format!("{e:#}"))),
                }
            }
            Step::Skip { .. } | Step::Warn { .. } | Step::Note { .. } => {}
        }
    }
    if let Some(pins) = &plan.pins {
        engine.update_state(|s| {
            // Only what actually made it into the sidebar (a clone may have failed).
            let known: Vec<PathBuf> = s.repos.iter().map(|r| r.path.clone()).collect();
            s.pins = pins.iter().filter(|p| known.contains(p)).cloned().collect();
        })?;
    }
    Ok(out)
}

/// This sidebar as a projects file: pinned projects first, in pin order, then the rest by name.
#[must_use]
pub fn export(engine: &Engine) -> ProjectsFile {
    let state = engine.state();
    let mut repos = state.repos.clone();
    let rank = |path: &Path| {
        state
            .pins
            .iter()
            .position(|p| p == path)
            .unwrap_or(usize::MAX)
    };
    repos.sort_by(|a, b| {
        rank(&a.path)
            .cmp(&rank(&b.path))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    ProjectsFile {
        projects: repos
            .iter()
            .map(|r| Project {
                path: tilde(&r.path),
                remote: the_deaner_git::origin_url(&r.path),
                pinned: state.pins.contains(&r.path),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_and_https_spellings_of_a_repo_match() {
        let forms = [
            "git@github.com:sean-d/the-deaner.git",
            "ssh://git@github.com/sean-d/the-deaner",
            "https://github.com/sean-d/the-deaner.git",
            "https://GitHub.com/sean-d/the-deaner/",
        ];
        for f in forms {
            assert_eq!(normalize_remote(f), "github.com/sean-d/the-deaner", "{f}");
        }
        assert!(!same_remote(
            "git@github.com:sean-d/the-deaner.git",
            "git@github.com:someone/the-deaner.git"
        ));
        // Local paths (tests, bare repos on disk) compare as written.
        assert_eq!(normalize_remote("/tmp/x/remote.git"), "/tmp/x/remote");
    }

    #[test]
    fn the_file_round_trips_and_leaves_out_defaults() {
        let file = ProjectsFile {
            projects: vec![
                Project {
                    path: "~/dev/rust/the-deaner".into(),
                    remote: Some("git@github.com:sean-d/the-deaner.git".into()),
                    pinned: true,
                },
                Project {
                    path: "~/temp".into(),
                    remote: None,
                    pinned: false,
                },
            ],
        };
        let text = file.to_toml();
        assert!(text.contains("[[project]]\npath = \"~/temp\"\n"), "{text}");
        assert!(!text.contains("pinned = false"), "{text}");
        assert_eq!(ProjectsFile::parse(&text).unwrap(), file);
    }
}
