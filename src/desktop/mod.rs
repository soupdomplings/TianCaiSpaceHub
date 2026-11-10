//! Shared desktop domain. Configuration operations live here, never in widgets.
pub mod activity;
pub mod clients;
pub mod components;
pub mod formats;
mod harness_models;
pub mod launcher;
pub mod mcp;
pub mod mcp_activity;
pub mod mcp_bridge;
pub mod model_operations;
pub mod models;
pub mod permissions;
pub mod resources;
pub mod runtime;
pub mod skill_market;
pub mod skills;
pub mod squad;
pub mod squad_browser;
pub mod squad_money;
pub mod squad_quote;
pub mod targets;
pub mod tiangong_resources;
pub mod tiangong_models;
pub mod transaction;
pub mod webstorm_native;

use std::path::{Path, PathBuf};

pub fn state_root(config_path: &Path) -> PathBuf {
    config_path.with_extension("desktop")
}
