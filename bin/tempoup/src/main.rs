#![cfg_attr(not(test), warn(unused_crate_dependencies))]

use clap::Parser;
use eyre::Result;

mod cli;
mod config;
mod download;
mod install;
mod macos;
mod platform;
mod release;
mod self_update;
mod verify;

use cli::Cli;
use config::Config;

fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| eyre::eyre!("failed to install rustls crypto provider"))?;

    let cli = Cli::parse();
    if cli.print_version {
        println!("{}", config::VERSION);
        return Ok(());
    }

    let config = Config::from_env()?;
    if cli.update {
        self_update::run(&config, cli.unsafe_skip_verify)
    } else {
        install::run(&config, cli.version.as_deref(), cli.unsafe_skip_verify)
    }
}

pub(crate) fn info(message: impl std::fmt::Display) {
    eprintln!("info: {message}");
}

pub(crate) fn warn(message: impl std::fmt::Display) {
    eprintln!("warn: {message}");
}
