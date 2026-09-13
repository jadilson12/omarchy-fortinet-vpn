use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fortinet-status-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn run(&self) -> Value {
        self.run_with_args(&[])
    }

    fn run_with_args(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_fortinet-vpn-status"))
            .args(args)
            .current_dir(&self.0)
            .env("PATH", &self.0)
            .env("LC_ALL", "POSIX")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let mut result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(result["display"]["label"].is_string());
        assert!(result["display"]["heading"].is_string());
        assert!(result["display"]["error"].is_string());
        assert!(result["checked_at_ms"].is_u64());
        assert!(result["rates"].is_null()); // A single check has no previous sample.
        let fields = result.as_object_mut().unwrap();
        fields.remove("display");
        fields.remove("checked_at_ms");
        fields.remove("rates");
        result
    }

    fn watch(&self) -> Watch {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fortinet-vpn-status"))
            .args(["--watch", "--config", "config.toml"])
            .current_dir(&self.0)
            .env("PATH", &self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Watch { child, lines }
    }
}

struct Watch {
    child: Child,
    lines: Receiver<String>,
}

impl Watch {
    fn next(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(7))
            .expect("watch must flush each JSON line promptly");
        serde_json::from_str(&line).unwrap()
    }

    fn refresh(&mut self) -> Value {
        self.child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"refresh\n")
            .unwrap();
        self.next()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn defaults() -> Value {
    json!({"poll_interval_seconds": 5, "show_rates": false, "show_connection_name": false})
}

fn unknown(error: &str) -> Value {
    json!({"state": "unknown", "name": "Fortinet", "error": error, "config": defaults(),
        "ip": "", "duration": "", "sent": "", "received": "", "interface": ""})
}

#[test]
fn queries_commands_and_emits_only_selected_fields() {
    let fixture = Fixture::new();
    fixture.script("forticlient", r#"
        [ "$#" = 2 ] && [ "$1" = vpn ] && [ "$2" = status ] && [ "$LC_ALL" = C ] || exit 1
        printf 'Status: Connected\nVPN name: Office\nUsername: private\nIP: 10.0.0.1\nDuration: 00:01:00\nSent bytes: 123\nRecv bytes: 456\n'
    "#);
    fixture.script(
        "ip",
        r#"
        [ "$#" = 2 ] && [ "$1" = -j ] && [ "$2" = address ] || exit 1
        printf '%s\n' '[{"ifname":"fctvpn123","flags":["UP"],"addr_info":[{"scope":"global"}]}]'
    "#,
    );
    assert_eq!(
        fixture.run(),
        json!({
            "state": "connected", "name": "Office", "ip": "10.0.0.1",
            "duration": "00:01:00", "sent": "123", "received": "456", "interface": "fctvpn123",
            "error": "", "config": defaults()
        })
    );
}

#[test]
fn missing_commands_return_unknown() {
    let fixture = Fixture::new();
    assert_eq!(fixture.run(), unknown("forticlient_missing"));
    fixture.script("forticlient", "printf 'Status: Connected\\n'");
    assert_eq!(fixture.run(), unknown("ip_missing"));
}

#[test]
fn failed_commands_return_unknown() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "printf 'Status: Connected\\n'; exit 1");
    fixture.script("ip", "printf '[]\\n'");
    assert_eq!(fixture.run(), unknown("forticlient_failed"));
    fixture.script("forticlient", "printf 'Status: Connected\\n'");
    fixture.script("ip", "printf '[]\\n'; exit 1");
    assert_eq!(fixture.run(), unknown("ip_failed"));
}

#[test]
fn invalid_interface_json_returns_unknown() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "printf 'Status: Connected\\n'");
    for value in ["not json", "{}", "null", r#"[{"flags":null}]"#] {
        fixture.script("ip", &format!("printf '%s\\n' '{value}'"));
        assert_eq!(fixture.run(), unknown("ip_invalid_json"));
    }
}

#[test]
fn forticlient_timeout_is_bounded() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "exec /bin/sleep 10");
    let start = Instant::now();
    assert_eq!(fixture.run(), unknown("forticlient_timeout"));
    assert!(start.elapsed() >= Duration::from_secs(3));
    assert!(start.elapsed() < Duration::from_secs(6));
}

#[test]
fn ip_timeout_is_bounded_even_after_stdout_closes() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "printf 'Status: Connected\\n'");
    fixture.script("ip", "exec 1>&-; exec /bin/sleep 10");
    let start = Instant::now();
    assert_eq!(fixture.run(), unknown("ip_timeout"));
    assert!(start.elapsed() >= Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn large_command_output_does_not_deadlock() {
    let fixture = Fixture::new();
    fixture.script(
        "forticlient",
        r#"
        /usr/bin/head -c 262144 /dev/zero
        printf '\nStatus: Disconnected\n'
    "#,
    );
    fixture.script("ip", "printf '[]\\n'");
    assert_eq!(fixture.run()["state"], "disconnected");
}

#[test]
fn config_preferences_are_returned_even_when_vpn_is_unavailable() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("config.toml"),
        "poll_interval_seconds = 12\nshow_rates = true\nshow_connection_name = true\n",
    )
    .unwrap();
    let result = fixture.run_with_args(&["--config", "config.toml"]);
    assert_eq!(result["error"], "forticlient_missing");
    assert_eq!(
        result["config"],
        json!({"poll_interval_seconds": 12, "show_rates": true, "show_connection_name": true})
    );
}

#[test]
fn partial_config_uses_defaults_and_reload_picks_up_edits() {
    let fixture = Fixture::new();
    let path = fixture.0.join("config.toml");
    fs::write(&path, "show_rates = true").unwrap();
    let result = fixture.run_with_args(&["--config", "config.toml"]);
    assert_eq!(result["config"]["poll_interval_seconds"], 5);
    assert_eq!(result["config"]["show_rates"], true);
    fs::write(&path, "poll_interval_seconds = 30").unwrap();
    let result = fixture.run_with_args(&["--config", "config.toml"]);
    assert_eq!(result["config"]["poll_interval_seconds"], 30);
    assert_eq!(result["config"]["show_rates"], false);
}

#[test]
fn invalid_config_is_reported_without_exposing_file_contents() {
    let fixture = Fixture::new();
    for source in [
        "broken = [",
        "secret = 'private'",
        "show_rates = 'yes'",
        "poll_interval_seconds = -1",
        "poll_interval_seconds = 1",
        "poll_interval_seconds = 301",
    ] {
        fs::write(fixture.0.join("config.toml"), source).unwrap();
        assert_eq!(
            fixture.run_with_args(&["--config", "config.toml"]),
            unknown("config_invalid")
        );
    }
    assert_eq!(
        fixture.run_with_args(&["--config", "missing.toml"]),
        unknown("config_unreadable")
    );
    assert_eq!(
        fixture.run_with_args(&["--config"]),
        unknown("invalid_arguments")
    );
}

#[test]
fn tunnel_ip_is_discovered_dynamically_when_cli_omits_it() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "printf 'Status: Connected\\n'");
    for address in ["192.0.2.17", "2001:db8::17"] {
        fixture.script("ip", &format!("printf '%s\\n' '[{{\"ifname\":\"fctvpn123\",\"flags\":[\"UP\"],\"addr_info\":[{{\"scope\":\"link\",\"local\":\"fe80::1\"}},{{\"scope\":\"global\",\"local\":\"{address}\"}}]}}]'"));
        let result = fixture.run();
        assert_eq!(result["state"], "connected");
        assert_eq!(result["ip"], address);
    }
}

#[test]
fn watch_streams_rates_refreshes_and_reloads_preferences() {
    let fixture = Fixture::new();
    fixture.script("forticlient", r#"
        count=0
        if [ -f count ]; then read -r count < count; fi
        count=$((count + 300))
        printf '%s\n' "$count" > count
        printf 'Status: Connected\nVPN name: Office\nSent bytes: %s\nRecv bytes: %s\n' "$count" "$count"
    "#);
    fixture.script("ip", r#"printf '%s\n' '[{"ifname":"fctvpn123","flags":["UP"],"addr_info":[{"scope":"global","local":"192.0.2.1"}]}]'"#);
    let config = fixture.0.join("config.toml");
    fs::write(&config, "poll_interval_seconds = 300\nshow_rates = true").unwrap();
    let mut watch = fixture.watch();
    let first = watch.next();
    assert_eq!(first["state"], "connected");
    assert!(first["rates"].is_null());
    assert!(first["display"]["download"].is_null());
    assert!(first["checked_at_ms"].as_u64().unwrap() > 0);

    let next = watch.refresh(); // Must not wait for the configured 300 seconds.
    assert_eq!(next["sent"], "600");
    assert!(next["rates"]["sent"].as_f64().unwrap() > 0.0);
    assert!(next["display"]["label"].as_str().unwrap().contains("/s"));
    fs::write(
        &config,
        "poll_interval_seconds = 300\nshow_connection_name = true",
    )
    .unwrap();
    let next = watch.refresh();
    assert_eq!(next["display"]["label"], "VPN ON · Office");
    assert!(next["display"]["download"].is_string());

    fixture.script("forticlient", "printf 'Status: Disconnected\\n'");
    let next = watch.refresh();
    assert_eq!(next["display"]["label"], "VPN OFF");
    assert!(next["rates"].is_null());
    fixture.script(
        "forticlient",
        "printf 'Status: Connected\\nVPN name: Office\\nSent bytes: 900\\nRecv bytes: 900\\n'",
    );
    assert!(watch.refresh()["rates"].is_null());
    assert_eq!(watch.refresh()["rates"]["sent"], 0.0);
}

#[test]
fn watch_polls_periodically_after_stdin_eof() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("config.toml"), "poll_interval_seconds = 2").unwrap();
    let mut watch = fixture.watch();
    assert_eq!(watch.next()["error"], "forticlient_missing");
    drop(watch.child.stdin.take());
    let start = Instant::now();
    assert_eq!(watch.next()["error"], "forticlient_missing");
    assert!(start.elapsed() >= Duration::from_millis(1800));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn watch_recovers_after_invalid_configuration_is_fixed() {
    let fixture = Fixture::new();
    let config = fixture.0.join("config.toml");
    fs::write(&config, "show_rates = 'invalid'").unwrap();
    let mut watch = fixture.watch();
    let result = watch.next();
    assert_eq!(result["error"], "config_invalid");
    assert!(result["display"]["error"]
        .as_str()
        .unwrap()
        .contains("config.toml"));
    fs::write(&config, "poll_interval_seconds = 2").unwrap();
    let result = watch.refresh();
    assert_eq!(result["error"], "forticlient_missing");
    assert_eq!(result["config"]["poll_interval_seconds"], 2);
}

#[test]
fn excessive_command_output_is_bounded() {
    let fixture = Fixture::new();
    fixture.script("forticlient", "exec /usr/bin/head -c 2097152 /dev/zero");
    assert_eq!(fixture.run(), unknown("forticlient_failed"));
}
