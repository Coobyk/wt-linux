use super::*;
use crate::config::{Config, command_override};
use std::time::{Duration, Instant};

fn profile(cmd: &[&str]) -> ResolvedProfile {
    let mut p = Config::default().resolve_profile(None, &|_| None).unwrap();
    command_override(
        &mut p,
        &cmd.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    p
}

fn wait_finished(s: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(s.is_finished() && s.exit_status().is_some()) {
        assert!(Instant::now() < deadline, "child never finished");
        s.drain(DRAIN_BUDGET);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(not(feature = "real-vt"))]
fn screen(s: &Session) -> Vec<String> {
    let g = s.term.grid();
    (0..g.rows()).map(|i| g.line_text(i)).collect()
}

#[test]
#[cfg(not(feature = "real-vt"))]
fn pump_feeds_terminal_and_notifies() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    let notify: Notify = Arc::new(move || {
        h.fetch_add(1, Ordering::Relaxed);
    });
    let mut s = Session::spawn(
        profile(&["printf \"hello\\nworld $WT_PROFILE\""]),
        20,
        4,
        100,
        notify,
    )
    .unwrap();
    wait_finished(&mut s);
    assert_eq!(screen(&s)[..2], ["hello", "world Shell"]);
    assert!(s.exit_status().unwrap().success());
    assert!(hits.load(Ordering::Relaxed) >= 1);
}

#[test]
#[cfg(not(feature = "real-vt"))]
fn writes_reach_the_child() {
    let mut s = Session::spawn(profile(&["head", "-n1"]), 20, 4, 100, Arc::new(|| {})).unwrap();
    s.write(b"ping\r");
    wait_finished(&mut s);
    assert_eq!(screen(&s)[0], "ping");
}

#[test]
fn drain_respects_budget() {
    let mut s = Session::spawn(
        profile(&["head -c 300000 /dev/zero | tr '\\0' x"]),
        80,
        24,
        0,
        Arc::new(|| {}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_more = false;
    while !s.is_finished() {
        assert!(Instant::now() < deadline);
        saw_more |= s.drain(1000).more;
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(saw_more, "a 300 kB burst should exceed a 1 kB budget");
}

#[test]
fn spawn_failure_is_an_error() {
    assert!(
        Session::spawn(
            profile(&["/definitely/not/a/program"]),
            80,
            24,
            0,
            Arc::new(|| {})
        )
        .is_err()
    );
}

#[test]
fn tabs_select_cycle_close() {
    let mut t = Tabs::default();
    for _ in 0..3 {
        t.push(Session::spawn(profile(&["true"]), 10, 2, 0, Arc::new(|| {})).unwrap());
    }
    assert_eq!(t.active, 2);
    t.cycle(true);
    assert_eq!(t.active, 0);
    t.cycle(false);
    assert_eq!(t.active, 2);
    t.select(1);
    t.close(0);
    assert_eq!((t.sessions.len(), t.active), (2, 0));
    t.close(0);
    t.close(0);
    assert!(t.is_empty() && t.active().is_none());
}
