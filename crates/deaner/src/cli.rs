//! Command-line interface.

use crate::engine::Engine;
use crate::{browse, cheatsheet, frame, hooks, sidebar};
use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use deaner_core::agent::{AgentState, HookEvent};
use deaner_core::config::{Config, TabSpec};
use deaner_core::sidebar::WsStatus;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "deaner",
    version,
    about = "A tmux-backed command center for running coding agents side by side",
    long_about = "A tmux-backed command center for running coding agents side by side.\n\n\
                  Run `deaner` with no arguments to open the sidebar and terminal frame."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Run the sidebar in the current terminal (the frame runs this for you)
    Sidebar {
        /// Allow `q` to quit (implied outside the frame)
        #[arg(long)]
        standalone: bool,
    },
    /// Keep a workspace client attached in the frame's terminal pane (internal)
    #[command(hide = true)]
    Term,
    /// Add repos or folders to the sidebar
    Add {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Forget a repo; its sessions keep running and files are untouched
    Forget { target: String },
    /// Create a worktree for BRANCH in REPO and start a session for it
    New { repo: String, branch: String },
    /// Start (or unpark) a workspace and switch to it. A folder that isn't added yet is added first
    Open { target: String },
    /// Add a tab to a workspace, e.g. `deaner tab api claude` or `deaner tab api logs -- tail -f app.log`
    Tab {
        target: String,
        name: String,
        /// Command to run in the tab (defaults to the agent of the same name, or a shell)
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Remove a worktree and kill its session
    Archive {
        target: String,
        /// Remove even with uncommitted changes
        #[arg(long)]
        force: bool,
    },
    /// List workspaces, tabs and agent states
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Type a message into another agent's tab and press Enter
    Send {
        /// `<session>` or `<session>:<tab>` (see `deaner ls`)
        addr: String,
        #[arg(required = true)]
        text: Vec<String>,
        /// Type the text without pressing Enter
        #[arg(long)]
        no_enter: bool,
    },
    /// Print this pane's address, for other agents to `deaner send` to
    Whoami,
    /// Snapshot all workspaces now
    Save,
    /// Save a workspace and stop its session; open it again to bring it back
    Park { target: String },
    /// Rebuild workspaces from the latest snapshot
    Restore {
        /// Restore from this snapshot file instead
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Done for the day: save everything, then stop all sessions
    Shutdown {
        /// Don't ask about agents that are still working
        #[arg(long)]
        force: bool,
    },
    /// Install deaner's status hooks into an agent's settings
    InstallHooks {
        #[arg(default_value = "claude")]
        agent: String,
        /// Remove deaner's hooks instead
        #[arg(long)]
        uninstall: bool,
        /// Settings file (default: ~/.claude/settings.json)
        #[arg(long)]
        settings: Option<PathBuf>,
        /// How hooks should invoke deaner
        #[arg(long, default_value = "deaner")]
        command: String,
    },
    /// Called by agent hooks to report status (internal)
    #[command(hide = true)]
    Hook { agent: String, event: String },
    /// Browse folders to pick a project, add it, and open it (`a` in the sidebar)
    Browse {
        /// Folder to start in (default: where your most recent project lives)
        dir: Option<String>,
    },
    /// Open the searchable shortcut cheat sheet (Alt-/ inside deaner)
    Cheatsheet,
    /// Show keybindings
    Keys {
        /// Print the generated docs page
        #[arg(long)]
        markdown: bool,
    },
    /// Show where config and state live, or print the default config
    Config {
        #[arg(long)]
        default: bool,
    },
    /// Generate documentation pages (internal)
    #[command(hide = true)]
    Docs { page: String },
}

/// Run the CLI.
///
/// # Errors
/// Propagates command failures for `main` to print.
pub fn run(cli: Cli) -> Result<()> {
    let command = cli.command;
    // The hook path must be as fast and forgiving as possible: no config load.
    if let Some(Cmd::Hook { agent, event }) = &command {
        if let Ok(event) = event.parse::<HookEvent>() {
            hooks::run(agent, event);
        }
        return Ok(());
    }
    if let Some(Cmd::Docs { page }) = &command {
        print!("{}", docs_page(page)?);
        return Ok(());
    }
    if let Some(Cmd::Keys { markdown }) = &command {
        print_keys(*markdown);
        return Ok(());
    }

    let engine = Engine::from_env()?;
    match command {
        None => frame::open(&engine),
        Some(Cmd::Sidebar { standalone }) => sidebar::run(engine, standalone),
        Some(Cmd::Cheatsheet) => cheatsheet::run(&engine),
        Some(Cmd::Browse { dir }) => browse::run(&engine, dir.as_deref()),
        Some(Cmd::Term) => frame::term(&engine),
        Some(Cmd::Add { paths }) => {
            for p in paths {
                let repo = engine.add_repo(&p)?;
                println!("added {} ({})", repo.name, repo.path.display());
            }
            Ok(())
        }
        Some(Cmd::Forget { target }) => {
            let ws = engine.find(&target)?;
            if engine.forget_repo(&ws.repo)? {
                println!("forgot {}", ws.repo.display());
            }
            Ok(())
        }
        Some(Cmd::New { repo, branch }) => {
            let repo_path = engine
                .find(&repo)
                .map(|w| w.repo)
                .unwrap_or_else(|_| crate::paths::expand(&repo));
            let name = engine.new_worktree(&repo_path, &branch)?;
            println!("created {name}");
            show(&engine, &name)
        }
        Some(Cmd::Open { target }) => {
            let name = match engine.find(&target) {
                Ok(ws) => engine.ensure_session(&ws)?,
                Err(not_found) => {
                    let path = crate::paths::expand(&target);
                    if !path.is_dir() {
                        return Err(not_found);
                    }
                    browse::open_project(&engine, &path)?
                }
            };
            show(&engine, &name)
        }
        Some(Cmd::Tab {
            target,
            name,
            command,
        }) => {
            let ws = engine.find(&target)?;
            let session = engine.ensure_session(&ws)?;
            let spec = if command.is_empty() {
                engine
                    .tab_choices(&ws.repo)
                    .into_iter()
                    .find(|t| t.name == name)
                    .unwrap_or_else(|| TabSpec::new(&name, None))
            } else {
                TabSpec::new(&name, Some(&shell_words::join(&command)))
            };
            engine.new_tab(&session, &ws.path, &spec)?;
            println!("added tab {name} to {session}");
            Ok(())
        }
        Some(Cmd::Archive { target, force }) => {
            let ws = engine.find(&target)?;
            engine.archive(&ws, force)?;
            println!("archived {}", ws.name);
            Ok(())
        }
        Some(Cmd::Ls { json }) => ls(&engine, json),
        Some(Cmd::Send {
            addr,
            text,
            no_enter,
        }) => {
            let to = engine.send(&addr, &text.join(" "), !no_enter)?;
            println!("sent to {to}");
            Ok(())
        }
        Some(Cmd::Whoami) => {
            let pane = std::env::var("TMUX_PANE").context("not inside tmux")?;
            let addr = deaner_tmux::Tmux::inherit()
                .display(Some(&pane), "#{session_name}:#{window_name}")
                .context("could not ask tmux for this pane")?;
            println!("{addr}");
            Ok(())
        }
        Some(Cmd::Save) => {
            let snap = engine.snapshot();
            let path = engine.save_snapshot(&snap)?;
            println!(
                "saved {} workspace(s) to {}",
                snap.workspaces.len(),
                path.display()
            );
            Ok(())
        }
        Some(Cmd::Park { target }) => {
            let ws = engine.find(&target)?;
            if ws.status != WsStatus::Live {
                bail!("{} is not running", ws.name);
            }
            engine.park(&ws.name)?;
            println!("parked {}", ws.name);
            Ok(())
        }
        Some(Cmd::Restore { from }) => {
            let snap = match from {
                Some(p) => deaner_core::Snapshot::from_toml(&std::fs::read_to_string(&p)?)
                    .with_context(|| format!("invalid snapshot {}", p.display()))?,
                None => engine
                    .latest_snapshot()
                    .context("no snapshot saved yet (see `deaner save`)")?,
            };
            let report = engine.restore(&snap)?;
            for name in &report.restored {
                println!("restored {name}");
            }
            for (name, why) in &report.skipped {
                println!("skipped {name}: {why}");
            }
            Ok(())
        }
        Some(Cmd::Shutdown { force }) => shutdown(&engine, force),
        Some(Cmd::InstallHooks {
            agent,
            uninstall,
            settings,
            command,
        }) => {
            if agent != "claude" {
                bail!(
                    "only `claude` hooks are built in; other agents get status from tmux activity"
                );
            }
            let path = settings.unwrap_or_else(hooks::claude_settings_path);
            let r = hooks::install(&path, &command, uninstall)?;
            if uninstall {
                println!(
                    "removed {} deaner hook(s) from {}",
                    r.removed,
                    r.path.display()
                );
            } else {
                println!("installed deaner hooks in {}", r.path.display());
            }
            if let Some(b) = r.backup {
                println!("backup: {}", b.display());
            }
            Ok(())
        }
        Some(Cmd::Config { default }) => {
            if default {
                print!("{}", toml::to_string_pretty(&Config::default())?);
            } else {
                println!("config: {}", engine.paths.config_file().display());
                println!("state:  {}", engine.paths.state_file().display());
                println!("snapshots: {}", engine.paths.snapshots_dir().display());
            }
            Ok(())
        }
        Some(Cmd::Hook { .. } | Cmd::Docs { .. } | Cmd::Keys { .. }) => {
            unreachable!("handled above")
        }
    }
}

fn show(engine: &Engine, session: &str) -> Result<()> {
    if let Some(hint) = frame::Switcher::detect(engine).show(engine, session)? {
        println!("{hint}");
    }
    Ok(())
}

fn ls(engine: &Engine, json: bool) -> Result<()> {
    let groups = engine.groups(engine.workspaces());
    if json {
        println!("{}", serde_json::to_string_pretty(&groups)?);
        return Ok(());
    }
    for g in &groups {
        println!("{}{}", if g.pinned { "★ " } else { "" }, g.name);
        for w in &g.workspaces {
            let status = match w.status {
                WsStatus::Live => "live",
                WsStatus::Parked => "parked",
                WsStatus::Stopped => "-",
            };
            let tabs: Vec<String> = w
                .tabs
                .iter()
                .map(|t| match t.state {
                    AgentState::None => t.address.clone(),
                    s => format!("{} ({s})", t.address),
                })
                .collect();
            println!(
                "  {:<24} {:<7} {}",
                w.branch.as_deref().unwrap_or(&w.name),
                status,
                tabs.join("  ")
            );
        }
    }
    Ok(())
}

fn shutdown(engine: &Engine, force: bool) -> Result<()> {
    let mut active = engine.active_agents();
    if !force && !active.is_empty() {
        println!("Agents still active:");
        for p in &active {
            println!("  {}  {}", p.address(), p.state);
        }
        print!("[w]ait for busy agents, [i]nterrupt them, [s]ave anyway, [c]ancel? ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer)?;
        match answer.trim() {
            "w" | "wait" => loop {
                active = engine.active_agents();
                if !active
                    .iter()
                    .any(|p| matches!(p.state, AgentState::Busy | AgentState::Compacting))
                {
                    break;
                }
                print!(".");
                std::io::stdout().flush()?;
                std::thread::sleep(Duration::from_secs(2));
            },
            "i" | "interrupt" => {
                for p in &active {
                    let _ = engine.tmux.send_keys(&p.pane_id, &["Escape"]);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            "s" | "save" => {}
            _ => {
                println!("cancelled");
                return Ok(());
            }
        }
    }
    let snap = engine.snapshot();
    let path = engine.save_snapshot(&snap)?;
    println!(
        "saved {} workspace(s) to {}",
        snap.workspaces.len(),
        path.display()
    );
    println!("stopping deaner. Run `deaner` to pick up where you left off.");
    engine.tmux.kill_server();
    frame::frame_tmux(engine).kill_server();
    Ok(())
}

fn print_keys(markdown: bool) {
    if markdown {
        print!("{}", deaner_ui::keymap::markdown());
        return;
    }
    let keys = deaner_core::config::FrameKeys::default();
    println!("Frame (anywhere):");
    for b in deaner_ui::keymap::frame_bindings(&keys) {
        println!("  {:<10} {}", b.key, b.help);
    }
    for section in deaner_ui::keymap::sections() {
        println!("\n{section}:");
        for b in deaner_ui::keymap::BINDINGS
            .iter()
            .filter(|b| b.section == section)
        {
            println!("  {:<10} {}", deaner_ui::keymap::keys_label(b), b.help);
        }
    }
}

/// Generated documentation pages: `cli` or `keybindings`.
///
/// # Errors
/// For unknown page names.
pub fn docs_page(page: &str) -> Result<String> {
    match page {
        "keybindings" => Ok(deaner_ui::keymap::markdown()),
        "cli" => Ok(cli_markdown()),
        other => bail!("unknown docs page `{other}` (expected cli or keybindings)"),
    }
}

fn cli_markdown() -> String {
    let mut root = Cli::command().term_width(100);
    let mut s = String::from(
        "<!-- Generated by `deaner docs cli`. Do not edit by hand; CI checks it is current. -->\n\n# CLI reference\n\n```text\n",
    );
    s.push_str(&root.render_long_help().to_string());
    s.push_str("```\n");
    let names: Vec<String> = root
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .map(|c| c.get_name().to_string())
        .collect();
    for name in names {
        let sub = root.find_subcommand_mut(&name).expect("listed");
        let mut sub = sub.clone().bin_name(format!("deaner {name}"));
        s.push_str(&format!(
            "\n## deaner {name}\n\n```text\n{}```\n",
            sub.render_long_help()
        ));
    }
    s
}
