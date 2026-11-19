// The derived async dispatch is generic over the context with no `Send` bound;
// clippy::nursery's lint fires on the generated impl and only a module-level
// allow reaches it.
#![allow(clippy::future_not_send)]

mod build;
mod check;

use std::path::PathBuf;

use usage_rs::{Cli, Subcommands};

pub use build::Build;
pub use check::Check;

/// Aggregate feeds into a static planet site
#[derive(Cli, Debug)]
#[usage(bin = "planet", version, unknown_flags = "error", args_override_self = false)]
pub struct Planet {
    /// Site settings and feed list
    #[usage(short = 'c', long, global, value_name = "FILE", default = "planet.toml")]
    pub config: PathBuf,

    #[usage(subcommand)]
    pub command: Commands,
}

#[derive(Subcommands, Debug)]
#[usage(run_async_with)]
pub enum Commands {
    Build(Build),
    #[usage(run)]
    Check(Check),
}

#[cfg(test)]
mod tests;
