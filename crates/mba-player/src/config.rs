use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerConfig {
    pub bind: SocketAddr,
    pub mpd_addr: SocketAddr,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8090),
            mpd_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6600),
        }
    }
}

impl PlayerConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;

        serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse config file {}", path.display()))
    }
}
