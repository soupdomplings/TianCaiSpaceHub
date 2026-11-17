#![allow(dead_code)]

#[cfg(not(feature = "hub-embedded"))]
compile_error!(
    "This local EasyTier library requires the hub-embedded feature; standalone tools are not built."
);
#[cfg(any(
    feature = "faketcp",
    feature = "full",
    feature = "jemalloc",
    feature = "jemalloc-prof",
    feature = "kcp",
    feature = "macos-ne",
    feature = "magic-dns",
    feature = "mimalloc",
    feature = "openssl-crypto",
    feature = "quic",
    feature = "tracing",
    feature = "tun",
    feature = "websocket",
    feature = "wireguard",
    feature = "zstd"
))]
compile_error!(
    "This Hub library profile excludes standalone/TUN/alternative transports and allocators. See SOURCE.md."
);

#[cfg(not(feature = "hub-embedded"))]
use std::io;

#[cfg(not(feature = "hub-embedded"))]
use clap::Command;
#[cfg(not(feature = "hub-embedded"))]
use clap_complete::{Generator, Shell};

mod arch;
mod gateway;
pub mod instance;
mod peer_center;
mod vpn_portal;

pub mod common;
pub mod connector;
#[cfg(not(feature = "hub-embedded"))]
pub mod core;
pub mod instance_manager;
pub mod launcher;
pub mod peers;
pub mod proto;
pub mod rpc_service;
#[cfg(not(feature = "hub-embedded"))]
pub mod service_manager;
pub mod tunnel;
pub mod utils;
pub mod web_client;

#[cfg(test)]
mod tests;

pub const VERSION: &str = common::constants::EASYTIER_VERSION;
rust_i18n::i18n!("locales", fallback = "en");

#[cfg(not(feature = "hub-embedded"))]
#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum ShellType {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
    Nu,
}

#[cfg(not(feature = "hub-embedded"))]
impl ShellType {
    pub fn to_shell(&self) -> Option<Shell> {
        match self {
            ShellType::Bash => Some(Shell::Bash),
            ShellType::Elvish => Some(Shell::Elvish),
            ShellType::Fish => Some(Shell::Fish),
            ShellType::Powershell => Some(Shell::PowerShell),
            ShellType::Zsh => Some(Shell::Zsh),
            ShellType::Nu => None,
        }
    }
}

#[cfg(not(feature = "hub-embedded"))]
pub fn print_completions<G: Generator>(generator: G, cmd: &mut Command, bin_name: &str) {
    clap_complete::generate(generator, cmd, bin_name, &mut io::stdout());
}

#[cfg(not(feature = "hub-embedded"))]
pub fn print_nushell_completions(cmd: &mut Command, bin_name: &str) {
    clap_complete::generate(
        clap_complete_nushell::Nushell,
        cmd,
        bin_name,
        &mut io::stdout(),
    );
}
