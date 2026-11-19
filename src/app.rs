//! What every command is handed.

use std::path::PathBuf;

#[derive(Debug)]
pub struct App {
    /// Path of `planet.toml`.
    pub config: PathBuf,
}
