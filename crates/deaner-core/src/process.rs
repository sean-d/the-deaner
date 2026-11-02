//! Running subprocesses with a deadline.
//!
//! The one piece of IO in `deaner-core`, shared by the tmux and git crates: a
//! hung `gh` (network, auth prompt) or `git` (huge repo, lock) must never stall
//! the rest of deaner, so every call gets a timeout and is killed when it expires.

use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Run `cmd` to completion, capturing stdout and stderr, killing it after `timeout`.
///
/// stdin is closed so nothing can wait on a prompt.
///
/// # Errors
/// - [`io::ErrorKind::TimedOut`] if the deadline passes (the process is killed).
/// - Any spawn error (e.g. the program is not installed).
pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    // Drain both pipes concurrently so a chatty process can't block on a full pipe.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );

    let deadline = Instant::now() + timeout;
    let mut nap = Duration::from_millis(1);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // Reader threads finish once the pipes close; don't wait on them
            // (a grandchild could still hold a pipe open).
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out after {}s", timeout.as_secs_f32()),
            ));
        }
        thread::sleep(nap);
        nap = (nap * 2).min(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output() {
        let out = output_with_timeout(
            Command::new("sh").args(["-c", "echo out; echo err >&2"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "out\n");
        assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n");
    }

    #[test]
    fn kills_hung_process_quickly() {
        let start = Instant::now();
        let err = output_with_timeout(Command::new("sleep").arg("30"), Duration::from_millis(200))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn large_output_does_not_deadlock() {
        let out = output_with_timeout(
            Command::new("sh").args([
                "-c",
                "head -c 3000000 /dev/zero; head -c 300000 /dev/zero >&2",
            ]),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stdout.len(), 3_000_000);
        assert_eq!(out.stderr.len(), 300_000);
    }

    #[test]
    fn missing_program_is_an_error() {
        assert!(
            output_with_timeout(
                &mut Command::new("deaner-no-such-program"),
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
}
