use crate::{
    config::Config,
    monitor::{counter, Rates},
    Status,
};
use serde::Serialize;

#[derive(Serialize)]
pub struct Display {
    pub label: String,
    pub heading: &'static str,
    pub error: &'static str,
    pub sent: String,
    pub received: String,
    pub upload: Option<String>,
    pub download: Option<String>,
}

#[derive(Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub status: Status,
    pub config: Config,
    pub rates: Option<Rates>,
    pub display: Display,
    pub checked_at_ms: u64,
}

impl Report {
    pub fn new(status: Status, config: Config, rates: Option<Rates>, checked_at_ms: u64) -> Self {
        let display = Display::new(&status, &config, rates.as_ref());
        Self {
            status,
            config,
            rates,
            display,
            checked_at_ms,
        }
    }
}

impl Display {
    pub fn new(status: &Status, config: &Config, rates: Option<&Rates>) -> Self {
        let (label, heading) = match status.state.as_str() {
            "connected" => ("VPN ON", "Connected"),
            "disconnected" => ("VPN OFF", "Disconnected"),
            "connecting" => ("VPN …", "Connecting"),
            "degraded" => ("VPN ?", "Tunnel unavailable"),
            _ => ("VPN ?", "Status unavailable"),
        };
        let mut label = label.to_owned();
        let rates = rates.filter(|_| status.state == "connected");
        let upload = rates.map(|rates| format!("{}/s", bytes(rates.sent)));
        let download = rates.map(|rates| format!("{}/s", bytes(rates.received)));
        if status.state == "connected" {
            if config.show_connection_name {
                label.push_str(&format!(" · {}", status.name));
            }
            if config.show_rates {
                if let (Some(download), Some(upload)) = (&download, &upload) {
                    label.push_str(&format!("  ↓ {download}  ↑ {upload}"));
                }
            }
        }
        Self {
            label,
            heading,
            error: error_message(&status.error),
            upload,
            download,
            sent: counter(&status.sent).map_or_else(|| "—".into(), |n| bytes(n as f64)),
            received: counter(&status.received).map_or_else(|| "—".into(), |n| bytes(n as f64)),
        }
    }
}

pub fn bytes(value: f64) -> String {
    if !value.is_finite() || value < 0.0 {
        return "—".into();
    }
    for (divisor, unit) in [(1073741824.0, "GiB"), (1048576.0, "MiB"), (1024.0, "KiB")] {
        if value >= divisor {
            return format!("{:.1} {unit}", value / divisor);
        }
    }
    format!("{:.0} B", value.round())
}

pub fn error_message(code: &str) -> &'static str {
    match code {
        "forticlient_missing" => {
            "FortiClient was not found. Install it and make sure it is on PATH."
        }
        "forticlient_timeout" => "FortiClient did not respond within 3 seconds.",
        "forticlient_failed" => {
            "FortiClient status failed. Check the client and your user permissions."
        }
        "ip_missing" => "The ip command was not found. Install iproute2.",
        "ip_timeout" => "Reading network interfaces timed out after 2 seconds.",
        "ip_failed" => "Network interfaces could not be read.",
        "ip_invalid_json" => "The ip command returned an invalid interface list.",
        "tunnel_unavailable" => {
            "FortiClient reports connected, but no active addressed VPN tunnel was found."
        }
        "status_unrecognized" => "FortiClient returned an unrecognized connection status.",
        "config_unreadable" => {
            "Could not read config.toml. Check that the file exists and is readable."
        }
        "config_invalid" => {
            "Invalid config.toml. Check option names, types and the 2–300 second interval."
        }
        "invalid_arguments" => "Usage: fortinet-vpn-status [--watch] [--config PATH]",
        _ => "",
    }
}
