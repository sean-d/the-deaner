//! Integration tests against a real, isolated tmux server.
#![cfg(feature = "test-support")]

use deaner_core::AgentState;
use deaner_core::pane::{OPT_PATH, OPT_STATE};
use deaner_tmux::{Scope, TestServer, session_target};
use std::path::Path;
use std::time::{Duration, Instant};

fn wait_for(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn sessions_windows_and_options() {
    let srv = TestServer::new();
    let t = &srv.tmux;
    assert!(!t.is_running());

    let tmp = std::env::temp_dir();
    let first = t.new_session("api/feat.x", &tmp, "claude").unwrap();
    assert!(first.pane_id.starts_with('%'));
    assert!(t.has_session("api/feat.x") || t.has_session("api/feat_x"));
    let name = t.sessions().pop().unwrap();

    let second = t.new_window(&name, &tmp, "nvim").unwrap();
    assert_eq!(second.window_index, 1);
    let split = t.split(&second.pane_id, &tmp).unwrap();
    assert_eq!(split.window_index, 1);

    t.set_option(Scope::Session(&name), OPT_PATH, "/work/api")
        .unwrap();
    t.set_option(Scope::Pane(&first.pane_id), OPT_STATE, "busy")
        .unwrap();
    assert_eq!(
        t.show_option(Scope::Session(&name), OPT_PATH).as_deref(),
        Some("/work/api")
    );

    let panes = t.panes();
    assert_eq!(panes.len(), 3);
    let p0 = panes.iter().find(|p| p.pane_id == first.pane_id).unwrap();
    assert_eq!(p0.state, AgentState::Busy);
    assert_eq!(p0.window_name, "claude");
    // session options resolve on every pane of the session
    assert!(
        panes
            .iter()
            .all(|p| p.workspace_path.as_deref() == Some(Path::new("/work/api")))
    );

    t.unset_option(Scope::Pane(&first.pane_id), OPT_STATE)
        .unwrap();
    assert_eq!(t.show_option(Scope::Pane(&first.pane_id), OPT_STATE), None);

    t.kill_session(&name).unwrap();
    assert!(!t.has_session(&name));
}

#[test]
fn a_test_server_leaves_no_socket_file_behind() {
    let srv = TestServer::new();
    srv.tmux
        .new_session("s", &std::env::temp_dir(), "w")
        .unwrap();
    let socket = srv.tmux.display(None, "#{socket_path}").unwrap();
    assert!(Path::new(&socket).exists());
    drop(srv);
    assert!(
        wait_for(|| !Path::new(&socket).exists()),
        "{socket} still there"
    );
}

#[test]
fn send_text_reaches_the_pane() {
    let srv = TestServer::new();
    let t = &srv.tmux;
    let dir = tempdir();
    let created = t.new_session("send", &dir, "sh").unwrap();
    let marker = dir.join("marker");
    t.send_text(
        &created.pane_id,
        &format!("echo hi > '{}'", marker.display()),
        true,
    )
    .unwrap();
    assert!(
        wait_for(|| marker.exists()),
        "command typed into pane should run"
    );
    assert_eq!(session_target("send"), "=send:");
}

fn tempdir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deaner-tmux-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}
