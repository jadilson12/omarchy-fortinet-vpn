use fortinet_vpn_status::{classify, Address, Interface};

fn tunnel() -> Interface {
    Interface {
        ifname: "fctvpn123".into(),
        flags: vec!["UP".into()],
        addr_info: vec![Address {
            scope: "global".into(),
            local: "10.0.0.2".into(),
        }],
    }
}

#[test]
fn connected_requires_tunnel() {
    assert_eq!(
        classify("Status: Connected", &[tunnel()]).state,
        "connected"
    );
    assert_eq!(classify("Status: Connected", &[]).state, "degraded");
}

#[test]
fn disconnected_overrides_remaining_interface() {
    for status in ["Disconnected", "Not connected"] {
        assert_eq!(
            classify(&format!("Status: {status}"), &[tunnel()]).state,
            "disconnected"
        );
    }
}

#[test]
fn connecting() {
    for status in ["Connecting", "Reconnecting"] {
        assert_eq!(
            classify(&format!("Status: {status}"), &[]).state,
            "connecting"
        );
    }
}

#[test]
fn unknown_is_not_connected() {
    assert_eq!(classify("unexpected output", &[tunnel()]).state, "unknown");
}

#[test]
fn down_or_unaddressed_tunnel_is_not_connected() {
    let mut down = tunnel();
    down.flags.clear();
    let mut unaddressed = tunnel();
    unaddressed.addr_info.clear();
    let mut local = tunnel();
    local.addr_info[0].scope = "link".into();
    for interface in [down, unaddressed, local, Interface::default()] {
        assert_eq!(
            classify("Status: Connected", &[interface]).state,
            "degraded"
        );
    }
}

#[test]
fn unrelated_interface_is_not_fortinet() {
    let mut link = tunnel();
    link.ifname = "wlan0".into();
    assert_eq!(classify("Status: Connected", &[link]).state, "degraded");
}

#[test]
fn details_exclude_username_and_preserve_colons() {
    let interfaces = [tunnel()];
    let result = classify(
        "  Status: CONNECTED\n  VPN name: Escritório: VPN\n  Username: private\n  IP: fd00::1\n  Duration: 00:01:00\n  Sent bytes: 123\n  Recv bytes: 456",
        &interfaces,
    );
    assert_eq!(result.state, "connected");
    assert_eq!(result.name, "Escritório: VPN");
    assert_eq!(result.ip, "fd00::1");
    assert_eq!(result.duration, "00:01:00");
    assert_eq!(result.sent, "123");
    assert_eq!(result.received, "456");
    assert_eq!(result.interface, "fctvpn123");
    assert!(!serde_json::to_string(&result).unwrap().contains("private"));
}

#[test]
fn defaults_and_first_valid_tunnel() {
    let interfaces = [Interface::default(), tunnel(), tunnel()];
    let result = classify("", &interfaces);
    assert_eq!(result.name, "Fortinet");
    assert_eq!(result.ip, "10.0.0.2");
    assert_eq!(result.duration, "");
    assert_eq!(result.sent, "0");
    assert_eq!(result.received, "0");
    assert_eq!(result.interface, "fctvpn123");
}
