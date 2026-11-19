use usage_rs::{Args, RunWith};

use crate::app::App;
use crate::config::Config;

/// Validate the config without touching the network
///
/// For CI on changes to the feed list.
#[derive(Args, Debug)]
#[usage(effect = "read")]
pub struct Check {}

impl RunWith<&App> for Check {
    type Output = anyhow::Result<()>;

    fn run_with(self, app: &App) -> Self::Output {
        let config = Config::load(&app.config)?;
        println!("{}: ok, {} feeds", app.config.display(), config.feeds.len());
        Ok(())
    }
}
