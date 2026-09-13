use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub poll_interval_seconds: u64,
    pub show_rates: bool,
    pub show_connection_name: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_seconds: 5,
            show_rates: false,
            show_connection_name: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, &'static str> {
        let source = std::fs::read_to_string(path).map_err(|_| "config_unreadable")?;
        let config: Self = toml::from_str(&source).map_err(|_| "config_invalid")?;
        if !(2..=300).contains(&config.poll_interval_seconds) {
            return Err("config_invalid");
        }
        Ok(config)
    }
}
