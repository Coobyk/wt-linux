use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::*;

const TIMEOUT: Duration = Duration::from_secs(10);

fn sh(script: &str) -> PtySpec {
    PtySpec::new("/bin/sh").arg("-c").arg(script)
}

/// Pump events until `pred` holds on the accumulated output, or panic after TIMEOUT.
fn read_until(
    ev: &PtyEvents,
    out: &mut Vec<u8>,
    pred: impl Fn(&str) -> bool,
) -> Option<ExitStatus> {
    let deadline = Instant::now() + TIMEOUT;
    let mut exited = None;
    while !pred(&String::from_utf8_lossy(out)) {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "timed out; output so far: {:?}",
            String::from_utf8_lossy(out)
        );
        match ev.recv_timeout(left) {
            Some(PtyEvent::Output(b)) => out.extend_from_slice(&b),
            Some(PtyEvent::Exited(s)) => exited = Some(s),
            None if ev.is_closed() => {
                panic!("pump closed; output: {:?}", String::from_utf8_lossy(out))
            }
            None => {}
        }
    }
    exited
}

/// Drain until `Exited`, returning (all output, status).
fn read_to_exit(ev: &PtyEvents) -> (String, ExitStatus) {
    let deadline = Instant::now() + TIMEOUT;
    let mut out = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "no Exited; output: {:?}",
            String::from_utf8_lossy(&out)
        );
        match ev.recv_timeout(left) {
            Some(PtyEvent::Output(b)) => out.extend_from_slice(&b),
            Some(PtyEvent::Exited(s)) => return (String::from_utf8_lossy(&out).into_owned(), s),
            None => {}
        }
    }
}

#[test]
fn pty_is_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Pty>();
    fn send<T: Send>() {}
    send::<PtyEvents>();
}

#[test]
fn cat_echoes_written_bytes() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let ev = pty.pump(None).unwrap();
    let mut w = pty.writer();
    w.write_all(b"hello wt\n").unwrap();
    w.flush().unwrap();
    let mut out = Vec::new();
    // Once from the tty's echo, once from cat itself.
    read_until(&ev, &mut out, |s| s.matches("hello wt").count() >= 2);
    assert!(pty.child_exited().is_none(), "cat should still be running");
}

#[test]
fn writer_handles_share_one_stream() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let ev = pty.pump(None).unwrap();
    let mut a = pty.writer();
    let mut b = pty.writer();
    let t = std::thread::spawn(move || b.write_all(b"from-thread\n").unwrap());
    a.write_all(b"from-main\n").unwrap();
    t.join().unwrap();
    let mut out = Vec::new();
    read_until(&ev, &mut out, |s| {
        s.contains("from-thread") && s.contains("from-main")
    });
}

#[test]
fn raw_reader_reads_output() {
    let pty = Pty::spawn(sh("printf 'abc-raw'")).unwrap();
    let mut r = pty.reader();
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    let deadline = Instant::now() + TIMEOUT;
    while !String::from_utf8_lossy(&out).contains("abc-raw") {
        assert!(Instant::now() < deadline);
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    assert!(String::from_utf8_lossy(&out).contains("abc-raw"), "{out:?}");
}

#[test]
fn resize_reaches_the_child() {
    let pty = Pty::spawn(PtySpec::new("/bin/sh").size(80, 24)).unwrap();
    assert_eq!(pty.size().unwrap(), (80, 24));
    let ev = pty.pump(None).unwrap();
    pty.resize(132, 43).unwrap();
    assert_eq!(pty.size().unwrap(), (132, 43));
    pty.writer().write_all(b"stty size\n").unwrap();
    let mut out = Vec::new();
    read_until(&ev, &mut out, |s| s.contains("43 132"));
}

#[test]
fn initial_size_is_applied() {
    let pty = Pty::spawn(sh("stty size").size(100, 30)).unwrap();
    let ev = pty.pump(None).unwrap();
    let (out, status) = read_to_exit(&ev);
    assert!(out.contains("30 100"), "{out:?}");
    assert!(status.success());
}

#[test]
fn exit_status_is_detected() {
    let pty = Pty::spawn(sh("echo bye; exit 3")).unwrap();
    let ev = pty.pump(None).unwrap();
    let (out, status) = read_to_exit(&ev);
    assert!(
        out.contains("bye"),
        "output before exit must arrive before Exited: {out:?}"
    );
    assert_eq!(status.exit_code(), 3);
    assert!(!status.success());
    assert_eq!(pty.child_exited().map(|s| s.exit_code()), Some(3));
    // Both pump threads end after Exited.
    let deadline = Instant::now() + TIMEOUT;
    while !ev.is_closed() {
        assert!(Instant::now() < deadline, "pump threads did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn child_exited_without_pump() {
    let pty = Pty::spawn(sh("exit 0")).unwrap();
    let status = pty.wait();
    assert!(status.success());
    assert!(pty.child_exited().unwrap().success());
}

#[test]
fn eof_on_stdin_ends_cat() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let ev = pty.pump(None).unwrap();
    pty.writer().write_all(b"\x04").unwrap(); // ^D at line start
    let (_, status) = read_to_exit(&ev);
    assert!(status.success());
}

#[test]
fn kill_is_reported_as_failure() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let ev = pty.pump(None).unwrap();
    assert!(pty.pid().is_some());
    pty.kill().unwrap();
    let (_, status) = read_to_exit(&ev);
    assert!(!status.success());
    // Killing an exited child is a no-op.
    pty.kill().unwrap();
}

#[test]
fn background_grandchild_does_not_hold_up_exited() {
    // `sleep` keeps the slave open long after the shell exits.
    let pty = Pty::spawn(sh("sleep 30 & echo started; exit 7")).unwrap();
    let ev = pty.pump(None).unwrap();
    let t0 = Instant::now();
    let (out, status) = read_to_exit(&ev);
    assert!(out.contains("started"), "{out:?}");
    assert_eq!(status.exit_code(), 7);
    assert!(t0.elapsed() < Duration::from_secs(5));
}

#[test]
fn env_cwd_and_term_defaults() {
    let dir = std::env::temp_dir().canonicalize().unwrap();
    let spec = sh("echo \"foo=$WT_FOO term=$TERM\"; pwd")
        .env("WT_FOO", "bar")
        .cwd(&dir);
    let pty = Pty::spawn(spec).unwrap();
    let ev = pty.pump(None).unwrap();
    let (out, _) = read_to_exit(&ev);
    assert!(out.contains("foo=bar term=xterm-256color"), "{out:?}");
    assert!(out.contains(dir.to_str().unwrap()), "{out:?}");
}

#[test]
fn spec_env_overrides_term() {
    let pty = Pty::spawn(sh("echo \"term=$TERM\"").env("TERM", "dumb")).unwrap();
    let (out, _) = read_to_exit(&pty.pump(None).unwrap());
    assert!(out.contains("term=dumb"), "{out:?}");
}

#[test]
fn default_program_is_a_shell() {
    let pty = Pty::spawn(PtySpec::default()).unwrap();
    let ev = pty.pump(None).unwrap();
    pty.writer().write_all(b"echo $((6*7))\nexit 5\n").unwrap();
    let (out, status) = read_to_exit(&ev);
    assert!(out.contains("42"), "{out:?}");
    assert_eq!(status.exit_code(), 5);
}

#[test]
fn spawn_failure_is_an_error() {
    assert!(Pty::spawn(PtySpec::new("/definitely/not/a/program")).is_err());
}

#[test]
fn pump_twice_is_an_error() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let _ev = pty.pump(None).unwrap();
    assert!(pty.pump(None).is_err());
}

#[test]
fn waker_fires_and_coalesces() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let waker: Waker = Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });
    let pty = Pty::spawn(sh("for i in 1 2 3 4 5 6 7 8; do echo line$i; done")).unwrap();
    let ev = pty.pump(Some(waker)).unwrap();
    // Don't drain until the child has exited: every event after the first must coalesce.
    pty.wait();
    let deadline = Instant::now() + TIMEOUT;
    while wakes.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        1,
        "undrained events should share one wake"
    );
    let mut out = Vec::new();
    let mut exited = ev.drain_into(&mut out);
    if exited.is_none() {
        exited = read_until(&ev, &mut out, |s| s.contains("line8"));
    }
    assert!(String::from_utf8_lossy(&out).contains("line8"));
    assert!(exited.is_some() || matches!(ev.recv_timeout(TIMEOUT), Some(PtyEvent::Exited(_))));
}

#[test]
fn drop_kills_child() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let pid = pty.pid().unwrap() as i32;
    let exit = pty.exit.clone();
    drop(pty);
    let deadline = Instant::now() + TIMEOUT;
    while exit.get().is_none() {
        assert!(Instant::now() < deadline, "child {pid} survived Pty drop");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn is_closed_never_loses_events() {
    let pty = Pty::spawn(sh("printf peek-me; exit 4")).unwrap();
    let ev = pty.pump(None).unwrap();
    let mut out = Vec::new();
    let mut status = None;
    let deadline = Instant::now() + TIMEOUT;
    // Interleave is_closed with receives; every byte and the exit must still arrive.
    while !ev.is_closed() {
        assert!(Instant::now() < deadline);
        match ev.try_recv() {
            Some(PtyEvent::Output(b)) => out.extend_from_slice(&b),
            Some(PtyEvent::Exited(s)) => status = Some(s),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert!(String::from_utf8_lossy(&out).contains("peek-me"), "{out:?}");
    assert_eq!(status.map(|s| s.exit_code()), Some(4));
}

#[test]
fn cwd_defaults_to_ours() {
    let ours = std::env::current_dir().unwrap();
    let pty = Pty::spawn(PtySpec::new("/bin/pwd")).unwrap();
    let (out, _) = read_to_exit(&pty.pump(None).unwrap());
    assert!(out.contains(ours.to_str().unwrap()), "{out:?} vs {ours:?}");
}

#[test]
fn bad_cwd_is_an_error() {
    let err = Pty::spawn(PtySpec::new("/bin/pwd").cwd("/definitely/not/a/dir"))
        .err()
        .unwrap();
    assert!(err.to_string().contains("not a directory"), "{err:#}");
}

#[test]
fn hup_ignoring_child_is_killed() {
    // Doesn't read stdin, so only a signal can end it.
    let pty = Pty::spawn(sh("trap '' HUP; while :; do sleep 0.05; done")).unwrap();
    // Give the trap time to install before we signal.
    std::thread::sleep(Duration::from_millis(200));
    pty.kill().unwrap();
    let exit = pty.exit.clone();
    drop(pty);
    let t0 = Instant::now();
    assert!(
        exit.wait_timeout(Duration::from_millis(500)).is_none(),
        "SIGHUP should be ignored"
    );
    let status = exit
        .wait_timeout(TIMEOUT)
        .expect("child survived SIGHUP + SIGKILL");
    assert!(!status.success(), "{status:?}");
    assert!(t0.elapsed() < KILL_GRACE + Duration::from_secs(1));
}

#[test]
fn drop_releases_pump_despite_grandchild() {
    let pty = Pty::spawn(sh("sleep 30 & exit 0")).unwrap();
    let ev = pty.pump(None).unwrap();
    let _ = read_to_exit(&ev);
    drop(pty);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ev.is_closed() {
        assert!(Instant::now() < deadline, "read thread still holds the pty");
        let _ = ev.recv_timeout(Duration::from_millis(20));
    }
}

#[test]
fn writes_never_block() {
    // The child never reads, so a blocking writer would stall once the tty queue fills.
    let pty = Pty::spawn(sh("sleep 30")).unwrap();
    let mut w = pty.writer();
    let t0 = Instant::now();
    w.write_all(&vec![b'x'; 1 << 20]).unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "write blocked for {:?}",
        t0.elapsed()
    );
}

#[test]
fn writer_fails_after_drop() {
    let pty = Pty::spawn(PtySpec::new("/bin/cat")).unwrap();
    let mut w = pty.writer();
    drop(pty);
    let deadline = Instant::now() + TIMEOUT;
    while w.write_all(b"x").is_ok() {
        assert!(
            Instant::now() < deadline,
            "writes still accepted after drop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn drop_types_nothing_into_the_shell() {
    let marker = std::env::temp_dir().join(format!("wt-backend-submit-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!("trap '' HUP; read l && echo \"$l\" > '{}'", marker.display());
    let pty = Pty::spawn(sh(&script)).unwrap();
    let exit = pty.exit.clone();
    // Hold a master fd so the tty doesn't hang up (and discard input) when the Pty goes.
    let _keep = pty.reader();
    std::thread::sleep(Duration::from_millis(200));
    pty.writer().write_all(b"half-typed").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(pty);
    exit.wait_timeout(TIMEOUT).expect("child not killed");
    assert!(!marker.exists(), "dropping the pty submitted the pending line");
}

#[test]
fn drop_releases_a_stalled_writer() {
    // A HUP-immune grandchild keeps the slave open and never reads, so the 1 MiB paste
    // stalls. Once every master fd closes the tty hangs up, `stty` fails and it leaves.
    let marker = std::env::temp_dir().join(format!("wt-backend-hangup-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        "stty -icanon -echo; exec 3<&0; (trap '' HUP; while stty size <&3 >/dev/null 2>&1; \
         do sleep 0.05; done; touch '{}') & sleep 30",
        marker.display()
    );
    let pty = Pty::spawn(sh(&script)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    pty.writer().write_all(&vec![b'x'; 1 << 20]).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!marker.exists(), "grandchild lost the tty before drop");
    drop(pty);
    let deadline = Instant::now() + TIMEOUT;
    while !marker.exists() {
        assert!(Instant::now() < deadline, "pty master still open after drop");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&marker);
}
