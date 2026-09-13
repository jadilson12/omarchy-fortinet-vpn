use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod config;
pub mod monitor;
pub mod presentation;

#[derive(Default, Deserialize)]
pub struct Interface {
    #[serde(default)]
    pub ifname: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub addr_info: Vec<Address>,
}

#[derive(Deserialize)]
pub struct Address {
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub local: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub state: String,
    pub name: String,
    pub ip: String,
    pub duration: String,
    pub sent: String,
    pub received: String,
    pub interface: String,
    pub error: String,
}

impl Status {
    pub fn unavailable(error: &str) -> Self {
        Self {
            state: "unknown".into(),
            name: "Fortinet".into(),
            error: error.into(),
            ..Self::default()
        }
    }
}

pub fn classify(output: &str, interfaces: &[Interface]) -> Status {
    let fields: HashMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect();
    let field = |key, default| fields.get(key).copied().unwrap_or(default);
    let tunnel = interfaces.iter().find(|interface| {
        interface.ifname.starts_with("fctvpn")
            && interface.flags.iter().any(|flag| flag == "UP")
            && interface
                .addr_info
                .iter()
                .any(|addr| addr.scope == "global")
    });
    let state = match field("Status", "").to_lowercase().as_str() {
        "connected" if tunnel.is_some() => "connected",
        "connected" => "degraded",
        "disconnected" | "not connected" => "disconnected",
        status if status.contains("connecting") => "connecting",
        _ => "unknown",
    };
    Status {
        state: state.into(),
        name: field("VPN name", "Fortinet").into(),
        ip: if field("IP", "").is_empty() {
            tunnel
                .and_then(|interface| {
                    interface
                        .addr_info
                        .iter()
                        .find(|addr| addr.scope == "global" && !addr.local.is_empty())
                })
                .map_or("", |addr| addr.local.as_str())
        } else {
            field("IP", "")
        }
        .into(),
        duration: field("Duration", "").into(),
        sent: field("Sent bytes", "0").into(),
        received: field("Recv bytes", "0").into(),
        interface: tunnel
            .map_or("", |interface| interface.ifname.as_str())
            .into(),
        error: match state {
            "degraded" => "tunnel_unavailable",
            "unknown" => "status_unrecognized",
            _ => "",
        }
        .into(),
    }
}
