use fortinet_vpn_status::{
    config::Config,
    monitor::Rates,
    presentation::{bytes, error_message, Display},
    Status,
};

#[test]
fn byte_units_and_invalid_values() {
    for (input, expected) in [
        (0.0, "0 B"),
        (512.0, "512 B"),
        (1024.0, "1.0 KiB"),
        (1048576.0, "1.0 MiB"),
        (1073741824.0, "1.0 GiB"),
        (-1.0, "—"),
        (f64::NAN, "—"),
        (f64::INFINITY, "—"),
    ] {
        assert_eq!(bytes(input), expected);
    }
}

#[test]
fn actionable_diagnostics() {
    assert!(error_message("forticlient_missing").contains("PATH"));
    assert!(error_message("ip_missing").contains("iproute2"));
    assert!(error_message("config_invalid").contains("config.toml"));
    assert_eq!(error_message(""), "");
    for code in [
        "forticlient_timeout",
        "forticlient_failed",
        "ip_timeout",
        "ip_failed",
        "ip_invalid_json",
        "tunnel_unavailable",
        "status_unrecognized",
        "config_unreadable",
        "invalid_arguments",
    ] {
        assert!(!error_message(code).is_empty());
    }
}

#[test]
fn label_options_and_tooltip_values() {
    let status = Status {
        state: "connected".into(),
        name: "Office".into(),
        sent: "1024".into(),
        received: "2048".into(),
        ..Status::default()
    };
    let rates = Rates {
        sent: 1024.0,
        received: 2048.0,
    };
    let display = Display::new(&status, &Config::default(), Some(&rates));
    assert_eq!(display.label, "VPN ON");
    assert_eq!(display.heading, "Connected");
    assert_eq!(display.sent, "1.0 KiB");
    assert_eq!(display.received, "2.0 KiB");
    assert_eq!(display.upload.as_deref(), Some("1.0 KiB/s"));
    let config = Config {
        show_rates: true,
        show_connection_name: true,
        ..Config::default()
    };
    assert_eq!(
        Display::new(&status, &config, Some(&rates)).label,
        "VPN ON · Office  ↓ 2.0 KiB/s  ↑ 1.0 KiB/s"
    );
    let display = Display::new(&status, &config, None);
    assert_eq!(display.label, "VPN ON · Office");
    assert!(display.download.is_none());
}

#[test]
fn unavailable_connection_never_displays_stale_speed() {
    let config = Config {
        show_rates: true,
        show_connection_name: true,
        ..Config::default()
    };
    for (state, label) in [
        ("disconnected", "VPN OFF"),
        ("connecting", "VPN …"),
        ("degraded", "VPN ?"),
        ("unknown", "VPN ?"),
    ] {
        let status = Status {
            state: state.into(),
            ..Status::default()
        };
        let display = Display::new(
            &status,
            &config,
            Some(&Rates {
                sent: 100.0,
                received: 100.0,
            }),
        );
        assert_eq!(display.label, label);
        assert!(display.download.is_none());
        assert_eq!(display.sent, "—");
    }
}
