use fortinet_vpn_status::{
    monitor::{Monitor, Rates},
    Status,
};
use std::time::Duration;

fn sample(sent: &str, received: &str) -> Status {
    Status {
        state: "connected".into(),
        name: "Example".into(),
        interface: "fctvpn123".into(),
        ip: "192.0.2.1".into(),
        sent: sent.into(),
        received: received.into(),
        duration: "00:01:00".into(),
        error: String::new(),
    }
}

fn between(before: Status, after: Status, now: u64) -> Option<Rates> {
    let mut monitor = Monitor::default();
    assert!(monitor
        .observe(&before, Duration::from_secs(10), Duration::from_secs(30))
        .is_none());
    monitor.observe(&after, Duration::from_secs(now), Duration::from_secs(30))
}

#[test]
fn speed_uses_actual_elapsed_time() {
    assert_eq!(
        between(sample("100", "200"), sample("400", "800"), 13),
        Some(Rates {
            sent: 100.0,
            received: 200.0
        })
    );
}

#[test]
fn unchanged_counters_mean_idle_connection() {
    assert_eq!(
        between(sample("100", "200"), sample("100", "200"), 15),
        Some(Rates {
            sent: 0.0,
            received: 0.0
        })
    );
}

#[test]
fn changed_connection_discards_baseline() {
    for field in ["state", "name", "interface", "ip"] {
        let before = sample("100", "200");
        let mut after = sample("400", "800");
        match field {
            "state" => after.state = "disconnected".into(),
            "name" => after.name = "Other".into(),
            "interface" => after.interface = "fctvpn456".into(),
            _ => after.ip = "192.0.2.2".into(),
        }
        assert!(between(before, after, 15).is_none());
    }
}

#[test]
fn counter_and_session_resets_discard_baseline() {
    assert!(between(sample("100", "200"), sample("50", "800"), 15).is_none());
    assert!(between(sample("100", "200"), sample("400", "50"), 15).is_none());
    let mut after = sample("400", "800");
    after.duration = "00:00:01".into();
    assert!(between(sample("100", "200"), after, 15).is_none());
}

#[test]
fn clock_regression_zero_interval_and_long_gaps_discard_baseline() {
    for now in [0, 9, 10, 41] {
        assert!(between(sample("100", "200"), sample("400", "800"), now).is_none());
    }
}

#[test]
fn invalid_counters_are_not_rates() {
    for counter in [
        "",
        "unknown",
        "12 KiB",
        "-1",
        "+1",
        "1.5",
        "18446744073709551616",
    ] {
        assert!(between(sample("100", "200"), sample(counter, "800"), 15).is_none());
    }
}

#[test]
fn large_integer_counters_preserve_small_deltas() {
    assert_eq!(
        between(
            sample("18446744073709551614", "9007199254740992"),
            sample("18446744073709551615", "9007199254740993"),
            11
        ),
        Some(Rates {
            sent: 1.0,
            received: 1.0
        })
    );
}

#[test]
fn failure_and_explicit_reset_require_two_new_samples() {
    let mut monitor = Monitor::default();
    let gap = Duration::from_secs(30);
    assert!(monitor
        .observe(&sample("100", "200"), Duration::ZERO, gap)
        .is_none());
    assert!(monitor
        .observe(
            &Status::unavailable("forticlient_timeout"),
            Duration::from_secs(5),
            gap
        )
        .is_none());
    assert!(monitor
        .observe(&sample("400", "800"), Duration::from_secs(10), gap)
        .is_none());
    assert!(monitor
        .observe(&sample("500", "900"), Duration::from_secs(15), gap)
        .is_some());
    monitor.reset();
    assert!(monitor
        .observe(&sample("600", "1000"), Duration::from_secs(20), gap)
        .is_none());
}
