//! Always-on sanity check that the `portable-pty` plumbing compiles and works in
//! whatever environment runs the suite (the Linux build container, or a host).
//!
//! This is intentionally independent of `claude`: it proves we can open a pty,
//! spawn a process in it, and stream its output back — the exact mechanism the
//! real PtyManager (M1) and the rewind/branch spike (M0) rely on.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn pty_round_trips_command_output() {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg("printf 'PTY_OK\\n'");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn command in pty");

    // Clone a reader off the master; the master itself stays alive in `pair`.
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    // Drop the slave handle so EOF propagates once the child exits.
    drop(pair.slave);

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut acc = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                acc.push_str(&String::from_utf8_lossy(&chunk));
                if acc.contains("PTY_OK") {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.wait();
    assert!(
        acc.contains("PTY_OK"),
        "expected 'PTY_OK' in pty output, got: {acc:?}"
    );
}
