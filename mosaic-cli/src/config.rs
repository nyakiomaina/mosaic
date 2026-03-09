use std::{path::PathBuf, str::FromStr};

use anyhow::{Context, Result, anyhow};
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::Cli;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// RPC URL for Solana cluster
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,

    /// Mosaic program ID
    #[serde(alias = "program_id")]
    pub mosaic_id: Option<String>,

    /// Default payer keypair path
    pub payer_keypair: Option<PathBuf>,

    /// Destination program ID
    pub destination_program: Option<String>,
}

fn default_rpc_url() -> String {
    "https://api.mainnet-beta.solana.com".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rpc_url: default_rpc_url(),
            mosaic_id: None,
            payer_keypair: None,
            destination_program: None,
        }
    }
}

pub fn load_config(config_path: Option<&PathBuf>) -> Result<Config> {
    let mut figment = Figment::new();
    if let Some(path) = config_path {
        figment = figment.merge(Toml::file(path));
    } else {
        figment = figment.merge(Toml::file("Mosaic.toml"));
    }
    figment = figment.merge(Env::prefixed("MOSAIC_"));

    Ok(figment.extract()?)
}

pub fn merge_cli_config(config: &mut Config, cli: &Cli) {
    if let Some(rpc_url) = &cli.rpc_url {
        config.rpc_url = rpc_url.clone();
    }
    if let Some(mosaic_id) = &cli.mosaic_id {
        config.mosaic_id = Some(mosaic_id.clone());
    }
}

pub fn get_mosaic_id(config: &Config) -> Result<Pubkey> {
    config
        .mosaic_id
        .as_ref()
        .ok_or_else(|| anyhow!("Mosaic program ID not configured"))
        .and_then(|s| Pubkey::from_str(s).context("Invalid mosaic program ID"))
}
